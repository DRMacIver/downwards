//! Offline, deterministic catalogue curation for experimental room generators.
//!
//! This module is deliberately an adapter around production generation and
//! validation APIs.  It owns selection policy and the manifest format, but it
//! does not make either generator or solver policy part of a shipped crate.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    error::Error,
    fmt::{self, Write},
};

use downwards_ai::{
    ComplexityBand, DIFFICULTY_HEURISTIC_VERSION, DIRECT_PROBE_AUDIT_VERSION, DifficultyReport,
    DirectProbeAuditStatus, HazardReference, ReachedTarget, Replay, SOLVER_POLICY_VERSION,
    SearchTarget, Solution, SolverConfig, TargetSolution, analyze_solution,
    audit_direct_controller_probes,
};
use downwards_core::{AbilitySet, Action, BoundarySide, DoorSocket, Simulation, SimulationEvent};
use downwards_gen::{
    AbilityTier,
    experimental::{ChallengeIntent, EXPERIMENTAL_GENERATION_VERSION, GenerationStrategy},
    v6::{
        COMPOSITIONAL_GENERATION_VERSION, CompositionalCandidate, CompositionalKey,
        CompositionalProfile, generate_compositional,
    },
};
use downwards_lab::{
    ActionSpan, COLLISION_TOPOLOGY_DESCRIPTOR_VERSION, CollisionTopologyDescriptor,
    STATIC_VISUAL_DESCRIPTOR_VERSION, SemanticAction, SemanticActionTrace, SemanticEvent,
    StaticVisualDescriptor, TraversalGrid, TraversalTrace, collision_topology_distance,
    observe_solution, semantic_action_distance, static_visual_distance, traversal_distance,
};
use downwards_validation::{
    DoorReachabilityCertificate, DoorReachabilityObjective, DoorSourceSearchEffort,
    DoorTargetBatchValidationError, PickupFromDoorCertificate, ValidationConfig,
    WITNESS_FINGERPRINT_VERSION, fingerprint_door_witness,
    validate_all_generated_door_targets_with_config,
};

use crate::structural::{
    STRUCTURAL_DESCRIPTOR_VERSION, StructuralBypassDescriptor, StructuralPathCost,
    TerrainUtilityDescriptor, describe_port_path, describe_terrain_utility,
};

const MANIFEST_VERSION: u32 = 3;
const SELECTION_VERSION: u32 = 5;
const VISUAL_FINGERPRINT_VERSION: u32 = 1;
const CONFIG_FINGERPRINT_VERSION: u32 = 1;
const ROUTE_BAND_POLICY_VERSION: u32 = 2;
const EASIEST_ROUTE_AUDIT_VERSION: u32 = 2;
const REPRESENTATIVE_ACTION_ENCODING_VERSION: u32 = 1;
const DISTANCE_QUANTUM: f64 = 1_000_000_000.0;
const MIN_ROBUSTNESS_NUMERATOR: usize = 1;
const MIN_ROBUSTNESS_DENOMINATOR: usize = 4;
const SELECTION_NODE_BUDGET: usize = 250_000;
const BANDS: [ComplexityBand; 3] = [
    ComplexityBand::Gentle,
    ComplexityBand::Standard,
    ComplexityBand::Technical,
];

/// Run `curate <start-seed> <seeds-per-stratum> <quota-per-band> <tier>`.
pub fn curate(arguments: &[String]) -> Result<String, CurateError> {
    let [start_seed, seeds_per_stratum, quota_per_band, tier] = arguments else {
        return Err(CurateError::Arguments(
            "curate expects <start-seed> <seeds-per-stratum> <quota-per-band> <tier>".into(),
        ));
    };
    let request = CurateRequest {
        start_seed: parse_positive_or_zero(start_seed, "start seed")?,
        seeds_per_stratum: parse_positive(seeds_per_stratum, "seeds per stratum")?,
        quota_per_band: parse_positive(quota_per_band, "quota per band")?,
        tier: parse_tier(tier)?,
    };
    curate_with_adapter(request, &BatchCertificationAdapter)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CurateRequest {
    start_seed: u64,
    seeds_per_stratum: usize,
    quota_per_band: usize,
    tier: AbilityTier,
}

#[derive(Debug)]
pub enum CurateError {
    Arguments(String),
    Deficit(String),
    SelectionSearchExhausted { explored: usize, budget: usize },
    Internal(String),
}

impl fmt::Display for CurateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(message) | Self::Deficit(message) | Self::Internal(message) => {
                formatter.write_str(message)
            }
            Self::SelectionSearchExhausted { explored, budget } => write!(
                formatter,
                "curation selection exhausted its deterministic search budget: explored={explored} budget={budget}; quotas were not relaxed"
            ),
        }
    }
}

impl Error for CurateError {}

fn curate_with_adapter(
    request: CurateRequest,
    adapter: &impl CertificationAdapter,
) -> Result<String, CurateError> {
    let validation = ValidationConfig::for_loadout(request.tier.abilities());
    let mut diagnostics = CurationDiagnostics::default();
    let mut observed_visuals = HashSet::<StaticVisualDescriptor>::new();
    let mut certified_visuals = HashSet::<StaticVisualDescriptor>::new();
    let mut pool = Vec::new();

    for offset in 0..request.seeds_per_stratum {
        let seed = request.start_seed.wrapping_add(offset as u64);
        for strategy in GenerationStrategy::ALL {
            for intent in ChallengeIntent::ALL {
                diagnostics.attempted += 1;
                let key = CompositionalKey::new(
                    seed,
                    CompositionalProfile::new(request.tier.abilities(), strategy, intent),
                );
                let candidate = match generate_compositional(key) {
                    Ok(candidate) => candidate,
                    Err(_) => {
                        diagnostics.reject("generation");
                        continue;
                    }
                };
                diagnostics.constructed += 1;
                let visual = StaticVisualDescriptor::from_room(&candidate.generated.room);
                if !observed_visuals.insert(visual.clone()) {
                    diagnostics.exact_visual_duplicates += 1;
                } else {
                    diagnostics.unique_visuals += 1;
                }
                // A failed candidate must not suppress a later candidate with
                // the same static preview but a certifiable timer schedule.
                // Once one positive proof exists, retaining the canonical
                // first certified provenance gives exact visual deduplication.
                if certified_visuals.contains(&visual) {
                    continue;
                }
                match certify_candidate(candidate, visual, &validation, adapter) {
                    Ok(room) => {
                        certified_visuals.insert(room.visual.clone());
                        diagnostics.certified += 1;
                        pool.push(room);
                    }
                    Err(class) => diagnostics.reject(class),
                }
            }
        }
    }

    pool.sort_unstable_by_key(CertifiedRoom::stable_key);
    diagnostics.eligible_by_band = eligible_counts(&pool, request.tier);
    diagnostics.socket_coverable_pool = pool
        .iter()
        .filter(|room| room_sockets_coverable_in_pool(room, &pool))
        .count();

    let quotas = [request.quota_per_band; 3];
    let selected =
        select_catalogue(&pool, quotas, request.tier).map_err(|failure| match failure {
            SelectionFailure::Deficit {
                maximum_joint,
                reason,
            } => CurateError::Deficit(diagnostics.deficit_report(
                &request,
                maximum_joint,
                reason,
                &pool,
            )),
            SelectionFailure::SearchExhausted { explored } => {
                CurateError::SelectionSearchExhausted {
                    explored,
                    budget: SELECTION_NODE_BUDGET,
                }
            }
        })?;

    render_manifest(&request, &validation, &diagnostics, &pool, &selected)
}

trait CertificationAdapter {
    fn certify(
        &self,
        candidate: &CompositionalCandidate,
        config: &ValidationConfig,
    ) -> Result<CertificateParts, &'static str>;
}

struct BatchCertificationAdapter;

impl CertificationAdapter for BatchCertificationAdapter {
    fn certify(
        &self,
        candidate: &CompositionalCandidate,
        config: &ValidationConfig,
    ) -> Result<CertificateParts, &'static str> {
        let batch = validate_all_generated_door_targets_with_config(&candidate.generated, config)
            .map_err(classify_batch_error)?;
        let (door_pairs, pickups_from_doors, source_search_effort) = batch.into_parts();
        Ok(CertificateParts {
            door_pairs,
            pickups_from_doors,
            source_search_effort,
        })
    }
}

fn classify_batch_error(error: DoorTargetBatchValidationError) -> &'static str {
    match error {
        DoorTargetBatchValidationError::DoorTopology(_) => "certification:door-topology",
        DoorTargetBatchValidationError::Door { .. } => "certification:door-route",
        DoorTargetBatchValidationError::Pickup { .. } => "certification:pickup-route",
    }
}

struct CertificateParts {
    door_pairs: Vec<DoorReachabilityCertificate>,
    pickups_from_doors: Vec<PickupFromDoorCertificate>,
    source_search_effort: Vec<DoorSourceSearchEffort>,
}

#[derive(Clone)]
struct RouteRecord {
    source_door_id: String,
    target_door_id: String,
    witness_fingerprint: u64,
    canonical_witness_fingerprint: u64,
    difficulty: DifficultyReport,
    traversal: TraversalTrace,
    actions: SemanticActionTrace,
    demand: RouteDemand,
    structural: StructuralBypassDescriptor,
    audit: EasiestRouteAudit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteWitnessOrigin {
    Canonical,
    DirectProbe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EasiestRouteAudit {
    version: u32,
    direct_probe_version: u32,
    candidate_witnesses: usize,
    direct_probe_successes: usize,
    audited_loadouts: usize,
    expected_loadouts: usize,
    audited_loadout_mask: u8,
    expected_loadout_mask: u8,
    budget_limited_audits: usize,
    lower_loadout_successes: usize,
    positive_loadout_evidence: [Option<PositiveLoadoutEvidence>; 4],
    canonical_band: ComplexityBand,
    easiest_origin: RouteWitnessOrigin,
    easiest_discovery_loadout: AbilitySet,
}

impl EasiestRouteAudit {
    fn complete(self) -> bool {
        self.audited_loadouts == self.expected_loadouts
            && self.audited_loadouts == self.audited_loadout_mask.count_ones() as usize
            && self.expected_loadouts == self.expected_loadout_mask.count_ones() as usize
            && self.audited_loadout_mask == self.expected_loadout_mask
            && self.budget_limited_audits == 0
            && self.positive_loadout_mask() & !self.expected_loadout_mask == 0
    }

    fn has_positive_without_wall_jump(self) -> bool {
        self.positive_loadout_evidence
            .iter()
            .enumerate()
            .any(|(index, evidence)| evidence.is_some() && !loadout_for_index(index).wall_jump)
    }

    fn has_positive_without_dash(self) -> bool {
        self.positive_loadout_evidence
            .iter()
            .enumerate()
            .any(|(index, evidence)| evidence.is_some() && !loadout_for_index(index).dash)
    }

    fn positive_loadout_mask(self) -> u8 {
        self.positive_loadout_evidence
            .iter()
            .enumerate()
            .fold(0, |mask, (index, evidence)| {
                mask | if evidence.is_some() { 1 << index } else { 0 }
            })
    }
}

/// Canonical positive direct-controller evidence for one exact discovery
/// loadout. The fingerprint is selected independently of observation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PositiveLoadoutEvidence {
    successes: usize,
    canonical_witness_fingerprint: u64,
}

impl PositiveLoadoutEvidence {
    const fn first(witness_fingerprint: u64) -> Self {
        Self {
            successes: 1,
            canonical_witness_fingerprint: witness_fingerprint,
        }
    }

    fn observe(&mut self, witness_fingerprint: u64) {
        self.successes += 1;
        self.canonical_witness_fingerprint =
            self.canonical_witness_fingerprint.min(witness_fingerprint);
    }
}

struct AssessedWitness {
    witness_fingerprint: u64,
    difficulty: DifficultyReport,
    traversal: TraversalTrace,
    actions: SemanticActionTrace,
    demand: RouteDemand,
    origin: RouteWitnessOrigin,
    discovery_loadout: AbilitySet,
}

struct PendingWitness {
    target_solution: TargetSolution,
    known_difficulty: Option<DifficultyReport>,
    traversal: TraversalTrace,
    actions: SemanticActionTrace,
    optimistic_demand: RouteDemand,
    witness_fingerprint: u64,
    origin: RouteWitnessOrigin,
    discovery_loadout: AbilitySet,
}

struct PendingRouteAssessment {
    objective: DoorReachabilityObjective,
    canonical_witness_fingerprint: u64,
    canonical_band: ComplexityBand,
    witnesses: Vec<PendingWitness>,
    seen_actions: HashSet<Vec<Action>>,
    direct_probe_successes: usize,
    audited_loadouts: usize,
    audited_loadout_mask: u8,
    budget_limited_audits: usize,
    lower_loadout_successes: usize,
    positive_loadout_evidence: [Option<PositiveLoadoutEvidence>; 4],
}

#[derive(Clone, Copy)]
struct WitnessAssessmentContext<'a> {
    candidate: &'a CompositionalCandidate,
    objective: &'a DoorReachabilityObjective,
    initial: &'a Simulation,
}

/// Deterministic demand observations for one directed, replay-certified route.
///
/// These deliberately describe the selected controller and trajectory rather
/// than treating replay duration or repeated jump presses as difficulty.  In
/// particular, a monotone run/auto-jump controller can never classify as
/// Technical, however long its replay is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteDemand {
    band: ComplexityBand,
    demand_score: u8,
    pressure_signals: u8,
    run_only: bool,
    engaging_gentle: bool,
    monotone_simple_controller: bool,
    controller_decision: bool,
    nontrivial_traversal: bool,
    horizontal_reversals: usize,
    vertical_input_changes: usize,
    accepted_dash_direction_changes: usize,
    control_vocabulary: usize,
    action_spans: usize,
    ordinary_jumps: usize,
    visited_horizontal_span_cells: u16,
    visited_vertical_span_cells: u16,
    horizontal_travel_cells: u32,
    vertical_travel_cells: u32,
    unique_semantic_actions: usize,
    successful_wall_jumps: usize,
    successful_dashes: usize,
    robustness_numerator: usize,
    robustness_denominator: usize,
    hazard_clearance: u32,
}

impl RouteDemand {
    fn observe(
        report: &DifficultyReport,
        actions: &SemanticActionTrace,
        traversal: &TraversalTrace,
    ) -> Self {
        let robustness = robustness_fraction(report);
        Self::observe_with_diagnostics(
            report.successful_jumps,
            report.successful_wall_jumps,
            report.successful_dashes,
            robustness,
            hazard_clearance(report),
            actions,
            traversal,
        )
    }

    fn optimistic(actions: &SemanticActionTrace, traversal: &TraversalTrace) -> Self {
        Self::observe_with_diagnostics(
            actions.successful_jumps,
            actions.successful_wall_jumps,
            actions.successful_dashes,
            (1, 1),
            u32::MAX,
            actions,
            traversal,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn observe_with_diagnostics(
        successful_jumps: usize,
        successful_wall_jumps: usize,
        successful_dashes: usize,
        robustness: (usize, usize),
        hazard_clearance: u32,
        actions: &SemanticActionTrace,
        traversal: &TraversalTrace,
    ) -> Self {
        let horizontal_reversals = horizontal_reversals(actions);
        let unique_semantic_actions = actions
            .spans
            .iter()
            .map(|span| span.action)
            .collect::<BTreeSet<_>>()
            .len();
        let traversal_demand = traversal_demand(traversal);
        let visited_horizontal_span_cells = traversal_demand.horizontal_span;
        let visited_vertical_span_cells = traversal_demand.vertical_span;
        let horizontal_travel_cells = traversal_demand.horizontal_travel;
        let vertical_travel_cells = traversal_demand.vertical_travel;
        let horizontal_directions = actions
            .spans
            .iter()
            .filter_map(|span| (span.action.move_x != 0).then_some(span.action.move_x))
            .collect::<BTreeSet<_>>();
        let vertical_directions = actions
            .spans
            .iter()
            .filter_map(|span| (span.action.move_y != 0).then_some(span.action.move_y))
            .collect::<BTreeSet<_>>();
        let uses_vertical_input = actions.spans.iter().any(|span| span.action.move_y != 0);
        let uses_dash_input = actions.spans.iter().any(|span| span.action.dash_held);
        let uses_jump_input = actions.spans.iter().any(|span| span.action.jump_held);
        let uses_restart_input = actions.spans.iter().any(|span| span.action.restart);
        let vertical_input_changes = input_direction_changes(actions, |action| action.move_y);
        let accepted_dash_direction_changes = accepted_dash_direction_changes(actions);
        let ordinary_jumps = successful_jumps.saturating_sub(successful_wall_jumps);
        let accepted_dash_directions = actions
            .events
            .iter()
            .filter_map(|event| match event.event {
                SemanticEvent::Dash(direction) => Some(direction),
                _ => None,
            })
            .collect::<HashSet<_>>()
            .len()
            .max(usize::from(successful_dashes > 0));
        let control_vocabulary = horizontal_directions.len()
            + vertical_directions.len()
            + usize::from(ordinary_jumps > 0)
            + usize::from(successful_wall_jumps > 0)
            + accepted_dash_directions;
        let run_only = horizontal_reversals == 0
            && horizontal_directions.len() <= 1
            && !uses_vertical_input
            && !uses_dash_input
            && !uses_jump_input
            && !uses_restart_input
            && successful_jumps == 0
            && successful_dashes == 0;
        let monotone_simple_controller = horizontal_reversals == 0
            && horizontal_directions.len() <= 1
            && !uses_vertical_input
            && !uses_dash_input
            && !uses_restart_input
            && successful_dashes == 0;
        let nontrivial_traversal = successful_jumps + successful_dashes > 0
            && (visited_vertical_span_cells >= 2
                || vertical_travel_cells >= 3
                || horizontal_reversals > 0
                || successful_wall_jumps > 0
                || successful_dashes > 0);
        let (robustness_numerator, robustness_denominator) = robustness;
        let pressure_signals: u8 = [
            horizontal_reversals > 0,
            visited_vertical_span_cells >= 4 || vertical_travel_cells >= 8,
            unique_semantic_actions >= 5,
            fraction_at_most(robustness_numerator, robustness_denominator, 3, 4),
            hazard_clearance <= 4,
            successful_wall_jumps >= 2,
            successful_dashes >= 2,
        ]
        .into_iter()
        .map(u8::from)
        .sum();
        let controller_decision = horizontal_reversals > 0
            || vertical_directions.len() >= 2
            || vertical_input_changes >= 2
            || accepted_dash_direction_changes > 0;
        let nontrivial_travel = visited_horizontal_span_cells >= 6
            || visited_vertical_span_cells >= 3
            || horizontal_travel_cells.saturating_add(vertical_travel_cells) >= 8;
        let engaging_gentle = monotone_simple_controller
            && (1..=3).contains(&ordinary_jumps)
            && successful_wall_jumps == 0
            && successful_dashes == 0
            && pressure_signals <= 1
            && fraction_at_least(robustness_numerator, robustness_denominator, 3, 4)
            && hazard_clearance >= 8
            && nontrivial_travel;
        let band = if !monotone_simple_controller && controller_decision && pressure_signals >= 2 {
            ComplexityBand::Technical
        } else if run_only || !nontrivial_traversal || engaging_gentle {
            ComplexityBand::Gentle
        } else {
            ComplexityBand::Standard
        };
        let demand_score = u8::from(nontrivial_traversal)
            + u8::from(!monotone_simple_controller)
            + u8::from(controller_decision)
            + pressure_signals.saturating_mul(2)
            + u8::from(successful_wall_jumps > 0)
            + u8::from(successful_dashes > 0);
        Self {
            band,
            demand_score,
            pressure_signals,
            run_only,
            engaging_gentle,
            monotone_simple_controller,
            controller_decision,
            nontrivial_traversal,
            horizontal_reversals,
            vertical_input_changes,
            accepted_dash_direction_changes,
            control_vocabulary,
            action_spans: actions.spans.len(),
            ordinary_jumps,
            visited_horizontal_span_cells,
            visited_vertical_span_cells,
            horizontal_travel_cells,
            vertical_travel_cells,
            unique_semantic_actions,
            successful_wall_jumps,
            successful_dashes,
            robustness_numerator,
            robustness_denominator,
            hazard_clearance,
        }
    }

    fn representative_eligible(self, band: ComplexityBand, tier: AbilityTier) -> bool {
        if self.band != band {
            return false;
        }
        if band == ComplexityBand::Gentle && !self.engaging_gentle {
            return false;
        }
        if band == ComplexityBand::Technical && !self.controller_decision {
            return false;
        }
        match (band, tier) {
            (ComplexityBand::Technical, AbilityTier::WallJump) => self.successful_wall_jumps > 0,
            (ComplexityBand::Technical, AbilityTier::Dash) => self.successful_dashes > 0,
            (ComplexityBand::Technical, AbilityTier::WallJumpAndDash) => {
                self.successful_wall_jumps > 0 || self.successful_dashes > 0
            }
            _ => true,
        }
    }
}

impl RouteRecord {
    fn supports_wall_jump_requirement(&self) -> bool {
        self.audit.complete()
            && self.structural.unavoidable_abilities.wall_jump
            && !self.audit.has_positive_without_wall_jump()
    }

    fn supports_dash_requirement(&self) -> bool {
        self.audit.complete()
            && self.structural.unavoidable_abilities.dash
            && !self.audit.has_positive_without_dash()
    }

    fn representative_eligible(&self, band: ComplexityBand, tier: AbilityTier) -> bool {
        if !self.demand.representative_eligible(band, tier) || !self.audit.complete() {
            return false;
        }
        let witnessed_wall = self.demand.successful_wall_jumps > 0;
        let witnessed_dash = self.demand.successful_dashes > 0;
        let supported_wall_requirement = self.supports_wall_jump_requirement();
        let supported_dash_requirement = self.supports_dash_requirement();
        if band == ComplexityBand::Gentle {
            return !witnessed_wall
                && !witnessed_dash
                && !supported_wall_requirement
                && !supported_dash_requirement;
        }
        if band == ComplexityBand::Technical
            && self.structural.cost.edge_count == 1
            && !supported_wall_requirement
            && !supported_dash_requirement
        {
            return false;
        }
        match tier {
            AbilityTier::Baseline => true,
            AbilityTier::WallJump => witnessed_wall && supported_wall_requirement,
            AbilityTier::Dash => witnessed_dash && supported_dash_requirement,
            AbilityTier::WallJumpAndDash => {
                (witnessed_wall && supported_wall_requirement)
                    || (witnessed_dash && supported_dash_requirement)
            }
        }
    }
}

fn input_direction_changes(
    actions: &SemanticActionTrace,
    direction: impl Fn(SemanticAction) -> i8,
) -> usize {
    let Some(first) = actions.spans.first() else {
        return 0;
    };
    let mut previous = direction(first.action);
    let mut changes = 0;
    for span in &actions.spans[1..] {
        let current = direction(span.action);
        if current != previous && (current != 0 || previous != 0) {
            changes += 1;
        }
        previous = current;
    }
    changes
}

fn accepted_dash_direction_changes(actions: &SemanticActionTrace) -> usize {
    let mut previous = None;
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

fn horizontal_reversals(actions: &SemanticActionTrace) -> usize {
    let mut previous = 0;
    let mut reversals = 0;
    for (index, span) in actions.spans.iter().enumerate() {
        let direction = span.action.move_x;
        if direction == 0 {
            continue;
        }
        let one_tick_bounce = span.ticks == 1
            && previous != 0
            && previous != direction
            && actions.spans[index + 1..]
                .iter()
                .find_map(|next| (next.action.move_x != 0).then_some(next.action.move_x))
                == Some(previous);
        if one_tick_bounce {
            continue;
        }
        if previous != 0 && previous != direction {
            reversals += 1;
        }
        previous = direction;
    }
    reversals
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TraversalDemand {
    horizontal_span: u16,
    vertical_span: u16,
    horizontal_travel: u32,
    vertical_travel: u32,
}

fn traversal_demand(traversal: &TraversalTrace) -> TraversalDemand {
    let Some(first) = traversal.spans.first().map(|span| span.cell) else {
        return TraversalDemand::default();
    };
    let (mut minimum_x, mut maximum_x) = (first.x, first.x);
    let (mut minimum_y, mut maximum_y) = (first.y, first.y);
    let mut previous = first;
    let mut horizontal_travel = 0_u32;
    let mut vertical_travel = 0_u32;
    for span in &traversal.spans[1..] {
        minimum_x = minimum_x.min(span.cell.x);
        maximum_x = maximum_x.max(span.cell.x);
        minimum_y = minimum_y.min(span.cell.y);
        maximum_y = maximum_y.max(span.cell.y);
        horizontal_travel =
            horizontal_travel.saturating_add(u32::from(previous.x.abs_diff(span.cell.x)));
        vertical_travel =
            vertical_travel.saturating_add(u32::from(previous.y.abs_diff(span.cell.y)));
        previous = span.cell;
    }
    TraversalDemand {
        horizontal_span: maximum_x - minimum_x,
        vertical_span: maximum_y - minimum_y,
        horizontal_travel,
        vertical_travel,
    }
}

const fn fraction_at_most(
    numerator: usize,
    denominator: usize,
    limit_numerator: usize,
    limit_denominator: usize,
) -> bool {
    (numerator as u128) * (limit_denominator as u128)
        <= (limit_numerator as u128) * (denominator as u128)
}

const fn fraction_at_least(
    numerator: usize,
    denominator: usize,
    limit_numerator: usize,
    limit_denominator: usize,
) -> bool {
    (numerator as u128) * (limit_denominator as u128)
        >= (limit_numerator as u128) * (denominator as u128)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PickupRecord {
    source_door_id: String,
    pickup_id: String,
    witness_fingerprint: u64,
    action_ticks: usize,
    action_spans: Vec<ActionSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchEffortRecord {
    source_door_id: String,
    expanded_nodes: usize,
    generated_nodes: usize,
    simulated_ticks: usize,
    deepest_path_ticks: usize,
}

struct CertifiedRoom {
    candidate: CompositionalCandidate,
    visual: StaticVisualDescriptor,
    collision: CollisionTopologyDescriptor,
    visual_fingerprint: u64,
    sockets: Vec<DoorSocket>,
    routes: Vec<RouteRecord>,
    pickups: Vec<PickupRecord>,
    source_search_effort: Vec<SearchEffortRecord>,
    terrain_utility: TerrainUtilityDescriptor,
    representative_routes: [Option<usize>; 3],
}

impl CertifiedRoom {
    fn stable_key(&self) -> (GenerationStrategy, ChallengeIntent, u64, u64) {
        (
            self.candidate.key.profile.strategy,
            self.candidate.key.profile.intent,
            self.candidate.generated.metadata.seed,
            self.visual_fingerprint,
        )
    }

    fn qd_stratum(&self) -> QdStratum {
        let summary = self.candidate.route_summary;
        QdStratum {
            strategy: self.candidate.key.profile.strategy,
            intent: self.candidate.key.profile.intent,
            port_count: self.sockets.len().try_into().unwrap_or(u16::MAX),
            cycle_bin: match summary.cycle_rank {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 3,
            },
            vertical_span_bin: match summary.vertical_span_rows {
                0..=3 => 0,
                4..=7 => 1,
                _ => 2,
            },
        }
    }

    fn has_vertical_port(&self) -> bool {
        self.sockets
            .iter()
            .any(|socket| matches!(socket.side, BoundarySide::Ceiling | BoundarySide::Floor))
    }

    fn representative_route(&self, band: ComplexityBand, tier: AbilityTier) -> Option<usize> {
        debug_assert_eq!(self.candidate.generated.metadata.ability_tier, tier);
        self.representative_routes[band_index(band)]
    }
}

fn certify_candidate(
    candidate: CompositionalCandidate,
    visual: StaticVisualDescriptor,
    validation: &ValidationConfig,
    adapter: &impl CertificationAdapter,
) -> Result<CertifiedRoom, &'static str> {
    let door_count = candidate.generated.room.doors().len();
    if door_count < 2 {
        return Err("contract:fewer-than-two-doors");
    }
    let pickup_count = candidate.generated.room.pickups().len();
    let parts = adapter.certify(&candidate, validation)?;
    if parts.door_pairs.len() != door_count * (door_count - 1) {
        return Err("contract:incomplete-door-matrix");
    }
    if parts.pickups_from_doors.len() != door_count * pickup_count {
        return Err("contract:incomplete-pickup-matrix");
    }

    let routes = assess_easiest_routes(&candidate, parts.door_pairs, validation)?;
    let certified_traversals = routes
        .iter()
        .map(|route| &route.traversal)
        .collect::<Vec<_>>();
    let terrain_utility = describe_terrain_utility(
        &candidate.generated.room,
        &candidate.route_plan,
        &certified_traversals,
    )
    .map_err(|_| "structural:terrain-utility")?;

    let mut pickups = parts
        .pickups_from_doors
        .into_iter()
        .map(|certificate| PickupRecord {
            source_door_id: certificate.objective().source_door_id.clone(),
            pickup_id: certificate.objective().required_pickup_id.clone(),
            witness_fingerprint: certificate.witness_fingerprint().as_u64(),
            action_ticks: certificate.solution().replay.frames.len(),
            action_spans: replay_action_spans(certificate.solution()),
        })
        .collect::<Vec<_>>();
    pickups.sort_unstable_by(|left, right| {
        (&left.source_door_id, &left.pickup_id).cmp(&(&right.source_door_id, &right.pickup_id))
    });

    let mut source_search_effort = parts
        .source_search_effort
        .into_iter()
        .map(|effort| SearchEffortRecord {
            source_door_id: effort.source_door_id,
            expanded_nodes: effort.stats.expanded_nodes,
            generated_nodes: effort.stats.generated_nodes,
            simulated_ticks: effort.stats.simulated_ticks,
            deepest_path_ticks: effort.stats.deepest_path_ticks,
        })
        .collect::<Vec<_>>();
    source_search_effort
        .sort_unstable_by(|left, right| left.source_door_id.cmp(&right.source_door_id));

    let mut sockets = candidate
        .generated
        .room
        .doors()
        .iter()
        .map(|door| door.socket())
        .collect::<Vec<_>>();
    sockets.sort_unstable();
    let visual_fingerprint = fingerprint_visual(&visual);
    let tier = candidate.generated.metadata.ability_tier;
    let representative_routes = BANDS.map(|band| {
        routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.representative_eligible(band, tier))
            .min_by(|(_, left), (_, right)| compare_representative_routes(left, right, tier, band))
            .map(|(index, _)| index)
    });
    Ok(CertifiedRoom {
        collision: CollisionTopologyDescriptor::from_room(&candidate.generated.room),
        candidate,
        visual,
        visual_fingerprint,
        sockets,
        routes,
        pickups,
        source_search_effort,
        terrain_utility,
        representative_routes,
    })
}

fn assess_easiest_routes(
    candidate: &CompositionalCandidate,
    certificates: Vec<DoorReachabilityCertificate>,
    validation: &ValidationConfig,
) -> Result<Vec<RouteRecord>, &'static str> {
    let intended_loadout = candidate.generated.metadata.intended_abilities;
    let expected_audit_loadouts = audit_loadouts(intended_loadout);
    let expected_loadouts = expected_audit_loadouts.len();
    let expected_loadout_mask = loadout_mask(&expected_audit_loadouts);
    let mut pending = Vec::with_capacity(certificates.len());
    for certificate in certificates {
        let objective = certificate.objective().clone();
        let initial = Simulation::enter_via_door(
            candidate.generated.room.clone(),
            intended_loadout,
            &objective.source_door_id,
        )
        .map_err(|_| "observation:door-entry")?;
        let canonical_fingerprint = certificate.witness_fingerprint().as_u64();
        let canonical = prepare_witness(
            WitnessAssessmentContext {
                candidate,
                objective: &objective,
                initial: &initial,
            },
            certificate.solution().clone(),
            Some(certificate.difficulty().clone()),
            RouteWitnessOrigin::Canonical,
            intended_loadout,
        )?;
        let canonical_band = RouteDemand::observe(
            canonical
                .known_difficulty
                .as_ref()
                .expect("canonical witness carries its certified difficulty"),
            &canonical.actions,
            &canonical.traversal,
        )
        .band;
        let canonical_actions = canonical
            .actions
            .spans
            .iter()
            .flat_map(|span| {
                std::iter::repeat_n(
                    Action {
                        move_x: span.action.move_x,
                        move_y: span.action.move_y,
                        jump: span.action.jump_held,
                        dash: span.action.dash_held,
                        restart: span.action.restart,
                    },
                    span.ticks,
                )
            })
            .collect::<Vec<_>>();
        pending.push(PendingRouteAssessment {
            objective,
            canonical_witness_fingerprint: canonical_fingerprint,
            canonical_band,
            witnesses: vec![canonical],
            seen_actions: HashSet::from([canonical_actions]),
            direct_probe_successes: 0,
            audited_loadouts: 0,
            audited_loadout_mask: 0,
            budget_limited_audits: 0,
            lower_loadout_successes: 0,
            positive_loadout_evidence: [None; 4],
        });
    }

    let mut routes_by_source = BTreeMap::<String, Vec<usize>>::new();
    for (route_index, route) in pending.iter().enumerate() {
        routes_by_source
            .entry(route.objective.source_door_id.clone())
            .or_default()
            .push(route_index);
    }

    for (source_door_id, route_indices) in routes_by_source {
        let targets = route_indices
            .iter()
            .map(|&route_index| SearchTarget::door(&pending[route_index].objective.target_door_id))
            .collect::<Vec<_>>();
        let authoritative_initial = Simulation::enter_via_door(
            candidate.generated.room.clone(),
            intended_loadout,
            &source_door_id,
        )
        .map_err(|_| "audit:door-entry")?;

        for &discovery_loadout in &expected_audit_loadouts {
            let discovery_initial = Simulation::enter_via_door(
                candidate.generated.room.clone(),
                discovery_loadout,
                &source_door_id,
            )
            .map_err(|_| "audit:door-entry")?;
            let mut audit_config = validation.solver.clone();
            audit_config.macros = SolverConfig::for_abilities(discovery_loadout).macros;
            let audit = audit_direct_controller_probes(&discovery_initial, &targets, &audit_config)
                .map_err(|_| "audit:direct-controller")?;
            let budget_limited = !matches!(audit.status, DirectProbeAuditStatus::Complete);
            for &route_index in &route_indices {
                pending[route_index].audited_loadouts += 1;
                pending[route_index].audited_loadout_mask |= loadout_bit(discovery_loadout);
                pending[route_index].budget_limited_audits += usize::from(budget_limited);
            }

            for witness in audit.witnesses {
                let Some(&route_index) = route_indices.get(witness.target_index) else {
                    return Err("audit:target-index");
                };
                let target_door_id = pending[route_index].objective.target_door_id.clone();
                if witness.reached != ReachedTarget::Door(target_door_id.clone()) {
                    return Err("audit:wrong-target");
                }
                let discovery_solution = TargetSolution {
                    target: witness.target,
                    reached: witness.reached,
                    replay: witness.replay,
                    stats: witness.stats_at_first_discovery,
                };
                let mut discovery_objective = pending[route_index].objective.clone();
                discovery_objective.loadout = discovery_loadout;
                let discovery_fingerprint = fingerprint_door_witness(
                    &candidate.generated,
                    &discovery_objective,
                    &discovery_solution,
                )
                .as_u64();
                // This is already exact positive evidence in the simulation
                // configured for `discovery_loadout`. Retain it before trying
                // the same buttons under the intended loadout: extra physics
                // can legitimately change that trajectory.
                pending[route_index].direct_probe_successes += 1;
                pending[route_index].lower_loadout_successes +=
                    usize::from(discovery_loadout != intended_loadout);
                record_positive_loadout_evidence(
                    &mut pending[route_index].positive_loadout_evidence,
                    discovery_loadout,
                    discovery_fingerprint,
                );
                let Some(replay) = replay_to_target_door(
                    &authoritative_initial,
                    discovery_solution.replay.actions(),
                    &target_door_id,
                ) else {
                    continue;
                };
                let actions = replay.actions().collect::<Vec<_>>();
                if !pending[route_index].seen_actions.insert(actions) {
                    continue;
                }
                let target_solution = TargetSolution {
                    target: SearchTarget::door(&target_door_id),
                    reached: ReachedTarget::Door(target_door_id),
                    replay,
                    stats: discovery_solution.stats,
                };
                let assessed = prepare_witness(
                    WitnessAssessmentContext {
                        candidate,
                        objective: &pending[route_index].objective,
                        initial: &authoritative_initial,
                    },
                    target_solution,
                    None,
                    RouteWitnessOrigin::DirectProbe,
                    discovery_loadout,
                )?;
                pending[route_index].witnesses.push(assessed);
            }
        }
    }

    let mut routes = Vec::with_capacity(pending.len());
    for assessment in pending {
        let candidate_witnesses = assessment.witnesses.len();
        let initial = Simulation::enter_via_door(
            candidate.generated.room.clone(),
            intended_loadout,
            &assessment.objective.source_door_id,
        )
        .map_err(|_| "audit:door-entry")?;
        let easiest = select_easiest_witness(assessment.witnesses, &initial, validation)?;
        if easiest.difficulty.deaths != 0 {
            return Err("fairness:route-death");
        }
        let robustness = &easiest.difficulty.temporal_robustness;
        if robustness.attempted_perturbations > 0
            && robustness.successful_perturbations * MIN_ROBUSTNESS_DENOMINATOR
                < robustness.attempted_perturbations * MIN_ROBUSTNESS_NUMERATOR
        {
            return Err("fairness:temporal-robustness");
        }
        let structural = describe_port_path(
            &candidate.route_plan,
            &candidate.boundary_ports,
            &assessment.objective.source_door_id,
            &assessment.objective.target_door_id,
        )
        .map_err(|_| "structural:ordered-port-path")?;
        routes.push(RouteRecord {
            source_door_id: assessment.objective.source_door_id,
            target_door_id: assessment.objective.target_door_id,
            witness_fingerprint: easiest.witness_fingerprint,
            canonical_witness_fingerprint: assessment.canonical_witness_fingerprint,
            difficulty: easiest.difficulty,
            traversal: easiest.traversal,
            actions: easiest.actions,
            demand: easiest.demand,
            structural,
            audit: EasiestRouteAudit {
                version: EASIEST_ROUTE_AUDIT_VERSION,
                direct_probe_version: DIRECT_PROBE_AUDIT_VERSION,
                candidate_witnesses,
                direct_probe_successes: assessment.direct_probe_successes,
                audited_loadouts: assessment.audited_loadouts,
                expected_loadouts,
                audited_loadout_mask: assessment.audited_loadout_mask,
                expected_loadout_mask,
                budget_limited_audits: assessment.budget_limited_audits,
                lower_loadout_successes: assessment.lower_loadout_successes,
                positive_loadout_evidence: assessment.positive_loadout_evidence,
                canonical_band: assessment.canonical_band,
                easiest_origin: easiest.origin,
                easiest_discovery_loadout: easiest.discovery_loadout,
            },
        });
    }
    routes.sort_unstable_by(|left, right| {
        (&left.source_door_id, &left.target_door_id)
            .cmp(&(&right.source_door_id, &right.target_door_id))
    });
    Ok(routes)
}

fn prepare_witness(
    context: WitnessAssessmentContext<'_>,
    target_solution: TargetSolution,
    known_difficulty: Option<DifficultyReport>,
    origin: RouteWitnessOrigin,
    discovery_loadout: AbilitySet,
) -> Result<PendingWitness, &'static str> {
    let WitnessAssessmentContext {
        candidate,
        objective,
        initial,
    } = context;
    let solution = Solution {
        exit_id: objective.target_door_id.clone(),
        replay: target_solution.replay.clone(),
        stats: target_solution.stats,
    };
    let observation = observe_solution(initial, &solution, TraversalGrid::default())
        .map_err(|_| "observation:door-replay")?;
    let optimistic_demand = RouteDemand::optimistic(&observation.actions, &observation.traversal);
    let witness_fingerprint =
        fingerprint_door_witness(&candidate.generated, objective, &target_solution).as_u64();
    Ok(PendingWitness {
        target_solution,
        known_difficulty,
        traversal: observation.traversal,
        actions: observation.actions,
        optimistic_demand,
        witness_fingerprint,
        origin,
        discovery_loadout,
    })
}

fn select_easiest_witness(
    witnesses: Vec<PendingWitness>,
    initial: &Simulation,
    validation: &ValidationConfig,
) -> Result<AssessedWitness, &'static str> {
    let mut remaining = witnesses;
    let mut assessed = Vec::new();
    for optimistic_band in BANDS {
        let mut deferred = Vec::new();
        for witness in remaining {
            if witness.optimistic_demand.band == optimistic_band {
                let optimistic_index = band_index(witness.optimistic_demand.band);
                let witness = finalize_witness(witness, initial, validation)?;
                debug_assert!(band_index(witness.demand.band) >= optimistic_index);
                assessed.push(witness);
            } else {
                deferred.push(witness);
            }
        }
        remaining = deferred;
        if assessed
            .iter()
            .any(|witness| witness.demand.band == optimistic_band)
        {
            return assessed
                .into_iter()
                .filter(|witness| witness.demand.band == optimistic_band)
                .min_by(compare_easiest_witness)
                .ok_or("audit:no-easiest-witness");
        }
    }
    Err("audit:no-easiest-witness")
}

fn finalize_witness(
    pending: PendingWitness,
    initial: &Simulation,
    validation: &ValidationConfig,
) -> Result<AssessedWitness, &'static str> {
    let solution = Solution {
        exit_id: match &pending.target_solution.reached {
            ReachedTarget::Door(id) | ReachedTarget::Exit(id) => id.clone(),
            ReachedTarget::Pickup(_) => return Err("audit:non-door-witness"),
        },
        replay: pending.target_solution.replay,
        stats: pending.target_solution.stats,
    };
    let difficulty = match pending.known_difficulty {
        Some(difficulty) => difficulty,
        None => analyze_solution(initial, &solution, &validation.difficulty)
            .map_err(|_| "audit:difficulty")?,
    };
    let demand = RouteDemand::observe(&difficulty, &pending.actions, &pending.traversal);
    Ok(AssessedWitness {
        witness_fingerprint: pending.witness_fingerprint,
        difficulty,
        traversal: pending.traversal,
        actions: pending.actions,
        demand,
        origin: pending.origin,
        discovery_loadout: pending.discovery_loadout,
    })
}

fn replay_to_target_door(
    initial: &Simulation,
    actions: impl IntoIterator<Item = Action>,
    target_door_id: &str,
) -> Option<Replay> {
    let mut simulation = initial.clone();
    let mut accepted = Vec::new();
    for action in actions {
        let report = simulation.step(action);
        accepted.push(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(_)))
        {
            return None;
        }
        if let Some(reached) = simulation.reached_exit() {
            return (reached == target_door_id).then(|| Replay::record(initial, accepted));
        }
    }
    None
}

const AUDIT_LOADOUT_UNIVERSE: [AbilitySet; 4] = [
    AbilitySet::NONE,
    AbilitySet::new(true, false),
    AbilitySet::new(false, true),
    AbilitySet::ALL,
];

fn audit_loadouts(intended: AbilitySet) -> Vec<AbilitySet> {
    AUDIT_LOADOUT_UNIVERSE
        .into_iter()
        .filter(|loadout| {
            (!loadout.wall_jump || intended.wall_jump) && (!loadout.dash || intended.dash)
        })
        .collect()
}

const fn loadout_index(loadout: AbilitySet) -> usize {
    match (loadout.wall_jump, loadout.dash) {
        (false, false) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (true, true) => 3,
    }
}

const fn loadout_for_index(index: usize) -> AbilitySet {
    AUDIT_LOADOUT_UNIVERSE[index]
}

const fn loadout_bit(loadout: AbilitySet) -> u8 {
    1 << loadout_index(loadout)
}

fn loadout_mask(loadouts: &[AbilitySet]) -> u8 {
    loadouts
        .iter()
        .fold(0, |mask, &loadout| mask | loadout_bit(loadout))
}

fn record_positive_loadout_evidence(
    evidence: &mut [Option<PositiveLoadoutEvidence>; 4],
    discovery_loadout: AbilitySet,
    witness_fingerprint: u64,
) {
    let slot = &mut evidence[loadout_index(discovery_loadout)];
    match slot {
        Some(existing) => existing.observe(witness_fingerprint),
        None => *slot = Some(PositiveLoadoutEvidence::first(witness_fingerprint)),
    }
}

fn compare_easiest_witness(left: &AssessedWitness, right: &AssessedWitness) -> std::cmp::Ordering {
    band_index(left.demand.band)
        .cmp(&band_index(right.demand.band))
        .then_with(|| right.demand.run_only.cmp(&left.demand.run_only))
        .then_with(|| {
            right
                .demand
                .monotone_simple_controller
                .cmp(&left.demand.monotone_simple_controller)
        })
        .then_with(|| {
            left.demand
                .controller_decision
                .cmp(&right.demand.controller_decision)
        })
        .then_with(|| {
            left.demand
                .pressure_signals
                .cmp(&right.demand.pressure_signals)
        })
        .then_with(|| {
            (left.demand.successful_wall_jumps + left.demand.successful_dashes)
                .cmp(&(right.demand.successful_wall_jumps + right.demand.successful_dashes))
        })
        .then_with(|| {
            left.demand
                .horizontal_reversals
                .cmp(&right.demand.horizontal_reversals)
        })
        .then_with(|| {
            left.demand
                .vertical_input_changes
                .cmp(&right.demand.vertical_input_changes)
        })
        .then_with(|| {
            left.demand
                .accepted_dash_direction_changes
                .cmp(&right.demand.accepted_dash_direction_changes)
        })
        .then_with(|| {
            left.demand
                .control_vocabulary
                .cmp(&right.demand.control_vocabulary)
        })
        .then_with(|| {
            left.demand
                .unique_semantic_actions
                .cmp(&right.demand.unique_semantic_actions)
        })
        .then_with(|| left.demand.ordinary_jumps.cmp(&right.demand.ordinary_jumps))
        .then_with(|| left.demand.action_spans.cmp(&right.demand.action_spans))
        .then_with(|| {
            left.difficulty
                .meaningful_input_transitions
                .cmp(&right.difficulty.meaningful_input_transitions)
        })
        .then_with(|| compare_robustness_descending(left.demand, right.demand))
        .then_with(|| {
            right
                .demand
                .hazard_clearance
                .cmp(&left.demand.hazard_clearance)
        })
        .then_with(|| {
            left.difficulty
                .completion_ticks
                .cmp(&right.difficulty.completion_ticks)
        })
        .then_with(|| witness_origin_index(left.origin).cmp(&witness_origin_index(right.origin)))
        .then_with(|| left.witness_fingerprint.cmp(&right.witness_fingerprint))
}

fn compare_robustness_descending(left: RouteDemand, right: RouteDemand) -> std::cmp::Ordering {
    let left_cross = left.robustness_numerator as u128 * right.robustness_denominator as u128;
    let right_cross = right.robustness_numerator as u128 * left.robustness_denominator as u128;
    right_cross.cmp(&left_cross)
}

const fn witness_origin_index(origin: RouteWitnessOrigin) -> u8 {
    match origin {
        RouteWitnessOrigin::Canonical => 0,
        RouteWitnessOrigin::DirectProbe => 1,
    }
}

fn representative_ability_score(route: &RouteRecord, tier: AbilityTier) -> u8 {
    match tier {
        AbilityTier::Baseline => 0,
        AbilityTier::WallJump => u8::from(route_uses_and_requires_wall(route)),
        AbilityTier::Dash => u8::from(route_uses_and_requires_dash(route)),
        AbilityTier::WallJumpAndDash => {
            u8::from(route_uses_and_requires_wall(route))
                + u8::from(route_uses_and_requires_dash(route))
        }
    }
}

fn compare_representative_routes(
    left: &RouteRecord,
    right: &RouteRecord,
    tier: AbilityTier,
    band: ComplexityBand,
) -> std::cmp::Ordering {
    compare_route_preference(
        route_preference(left, tier, band),
        route_preference(right, tier, band),
    )
    .then_with(|| {
        (&left.source_door_id, &left.target_door_id)
            .cmp(&(&right.source_door_id, &right.target_door_id))
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteQuality {
    robustness_numerator: usize,
    robustness_denominator: usize,
    hazard_clearance: u32,
    simulated_ticks: usize,
    expanded_nodes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RoutePreference {
    band: ComplexityBand,
    ability_score: u8,
    demand: RouteDemand,
    structural: StructuralRoutePreference,
    operational: RouteQuality,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StructuralRoutePreference {
    baseline_one_edge: bool,
    cost: StructuralPathCost,
}

fn route_preference(
    route: &RouteRecord,
    tier: AbilityTier,
    band: ComplexityBand,
) -> RoutePreference {
    RoutePreference {
        band,
        ability_score: representative_ability_score(route, tier),
        demand: route.demand,
        structural: StructuralRoutePreference {
            baseline_one_edge: route.structural.cost.edge_count == 1
                && !route.supports_wall_jump_requirement()
                && !route.supports_dash_requirement(),
            cost: route.structural.cost,
        },
        operational: route_quality(&route.difficulty),
    }
}

fn compare_route_preference(left: RoutePreference, right: RoutePreference) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let band_order = band_index(left.band).cmp(&band_index(right.band));
    if band_order != Ordering::Equal {
        return band_order;
    }
    match left.band {
        ComplexityBand::Gentle => right
            .demand
            .engaging_gentle
            .cmp(&left.demand.engaging_gentle)
            .then_with(|| {
                left.demand
                    .pressure_signals
                    .cmp(&right.demand.pressure_signals)
            })
            .then_with(|| compare_route_quality_values(left.operational, right.operational))
            .then_with(|| left.demand.ordinary_jumps.cmp(&right.demand.ordinary_jumps))
            .then_with(|| {
                right
                    .demand
                    .visited_horizontal_span_cells
                    .cmp(&left.demand.visited_horizontal_span_cells)
            })
            .then_with(|| left.demand.demand_score.cmp(&right.demand.demand_score)),
        ComplexityBand::Standard => right
            .ability_score
            .cmp(&left.ability_score)
            .then_with(|| {
                right
                    .demand
                    .pressure_signals
                    .cmp(&left.demand.pressure_signals)
            })
            .then_with(|| {
                right
                    .demand
                    .horizontal_reversals
                    .cmp(&left.demand.horizontal_reversals)
            })
            .then_with(|| compare_structural_pressure(left.structural, right.structural))
            .then_with(|| compare_route_quality_values(left.operational, right.operational)),
        ComplexityBand::Technical => right
            .demand
            .controller_decision
            .cmp(&left.demand.controller_decision)
            .then_with(|| {
                right
                    .demand
                    .pressure_signals
                    .cmp(&left.demand.pressure_signals)
            })
            .then_with(|| right.ability_score.cmp(&left.ability_score))
            .then_with(|| {
                right
                    .demand
                    .horizontal_reversals
                    .cmp(&left.demand.horizontal_reversals)
            })
            .then_with(|| {
                right
                    .demand
                    .visited_vertical_span_cells
                    .cmp(&left.demand.visited_vertical_span_cells)
            })
            .then_with(|| {
                right
                    .demand
                    .vertical_travel_cells
                    .cmp(&left.demand.vertical_travel_cells)
            })
            .then_with(|| {
                right
                    .demand
                    .vertical_input_changes
                    .cmp(&left.demand.vertical_input_changes)
            })
            .then_with(|| {
                right
                    .demand
                    .accepted_dash_direction_changes
                    .cmp(&left.demand.accepted_dash_direction_changes)
            })
            .then_with(|| {
                right
                    .demand
                    .control_vocabulary
                    .cmp(&left.demand.control_vocabulary)
            })
            .then_with(|| {
                right
                    .demand
                    .unique_semantic_actions
                    .cmp(&left.demand.unique_semantic_actions)
            })
            .then_with(|| compare_structural_pressure(left.structural, right.structural))
            .then_with(|| compare_robustness_near_half(left.operational, right.operational))
            .then_with(|| {
                left.operational
                    .hazard_clearance
                    .cmp(&right.operational.hazard_clearance)
            })
            .then_with(|| {
                left.operational
                    .simulated_ticks
                    .cmp(&right.operational.simulated_ticks)
            })
            .then_with(|| {
                left.operational
                    .expanded_nodes
                    .cmp(&right.operational.expanded_nodes)
            }),
    }
}

fn compare_structural_pressure(
    left: StructuralRoutePreference,
    right: StructuralRoutePreference,
) -> std::cmp::Ordering {
    left.baseline_one_edge
        .cmp(&right.baseline_one_edge)
        .then_with(|| {
            right
                .cost
                .required_ability_edges
                .cmp(&left.cost.required_ability_edges)
        })
        .then_with(|| right.cost.edge_count.cmp(&left.cost.edge_count))
        .then_with(|| right.cost.decision_nodes.cmp(&left.cost.decision_nodes))
        .then_with(|| {
            right
                .cost
                .vertical_transitions
                .cmp(&left.cost.vertical_transitions)
        })
        .then_with(|| right.cost.verb_variety.cmp(&left.cost.verb_variety))
}

fn compare_robustness_near_half(left: RouteQuality, right: RouteQuality) -> std::cmp::Ordering {
    let left_delta =
        (2_u128 * left.robustness_numerator as u128).abs_diff(left.robustness_denominator as u128);
    let right_delta = (2_u128 * right.robustness_numerator as u128)
        .abs_diff(right.robustness_denominator as u128);
    (left_delta * right.robustness_denominator as u128)
        .cmp(&(right_delta * left.robustness_denominator as u128))
}

fn route_quality(report: &DifficultyReport) -> RouteQuality {
    let robustness = robustness_fraction(report);
    RouteQuality {
        robustness_numerator: robustness.0,
        robustness_denominator: robustness.1,
        hazard_clearance: hazard_clearance(report),
        simulated_ticks: report.search_effort.simulated_ticks,
        expanded_nodes: report.search_effort.expanded_nodes,
    }
}

fn compare_route_quality_values(left: RouteQuality, right: RouteQuality) -> std::cmp::Ordering {
    let left_robustness = (left.robustness_numerator, left.robustness_denominator);
    let right_robustness = (right.robustness_numerator, right.robustness_denominator);
    let left_cross = left_robustness.0 as u128 * right_robustness.1 as u128;
    let right_cross = right_robustness.0 as u128 * left_robustness.1 as u128;
    right_cross
        .cmp(&left_cross)
        .then_with(|| right.hazard_clearance.cmp(&left.hazard_clearance))
        .then_with(|| left.simulated_ticks.cmp(&right.simulated_ticks))
        .then_with(|| left.expanded_nodes.cmp(&right.expanded_nodes))
}

fn robustness_fraction(report: &DifficultyReport) -> (usize, usize) {
    let robustness = &report.temporal_robustness;
    if robustness.attempted_perturbations == 0 {
        (1, 1)
    } else {
        (
            robustness.successful_perturbations,
            robustness.attempted_perturbations,
        )
    }
}

fn hazard_clearance(report: &DifficultyReport) -> u32 {
    report
        .minimum_hazard_clearance
        .map_or(u32::MAX, |clearance| clearance.pixels)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct QdStratum {
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
    port_count: u16,
    cycle_bin: u8,
    vertical_span_bin: u8,
}

#[derive(Default)]
struct CurationDiagnostics {
    attempted: usize,
    constructed: usize,
    unique_visuals: usize,
    exact_visual_duplicates: usize,
    certified: usize,
    eligible_by_band: [usize; 3],
    socket_coverable_pool: usize,
    rejections: BTreeMap<&'static str, usize>,
}

impl CurationDiagnostics {
    fn reject(&mut self, class: &'static str) {
        *self.rejections.entry(class).or_default() += 1;
    }

    fn deficit_report(
        &self,
        request: &CurateRequest,
        maximum_joint: usize,
        reason: &str,
        pool: &[CertifiedRoom],
    ) -> String {
        let required = request.quota_per_band * BANDS.len();
        let mut output = format!(
            "curation quota deficit; quotas were not relaxed\nreason={reason}\nrequested-per-band={} required-total={} maximum-joint-distinct={}\nattempted={} constructed={} unique-visuals={} certified={} socket-coverable-pool={}\n",
            request.quota_per_band,
            required,
            maximum_joint,
            self.attempted,
            self.constructed,
            self.unique_visuals,
            self.certified,
            self.socket_coverable_pool,
        );
        for band in BANDS {
            let eligible = self.eligible_by_band[band_index(band)];
            let demand_routes = pool
                .iter()
                .flat_map(|room| room.routes.iter())
                .filter(|route| route.demand.band == band)
                .count();
            let deficit = request.quota_per_band.saturating_sub(eligible);
            writeln!(
                output,
                "band={} demand-routes={} representative-eligible-distinct={} requested={} raw-deficit={}",
                band_slug(band),
                demand_routes,
                eligible,
                request.quota_per_band,
                deficit,
            )
            .expect("writing to String cannot fail");
        }
        let vertical_eligible = pool.iter().filter(|room| room.has_vertical_port()).count();
        writeln!(output, "vertical-port-eligible-rooms={vertical_eligible}")
            .expect("writing to String cannot fail");
        let routes = pool.iter().flat_map(|room| room.routes.iter());
        let complete_audits = routes
            .clone()
            .filter(|route| route.audit.complete())
            .count();
        let grounded_wall = routes
            .clone()
            .filter(|route| route_uses_and_requires_wall(route))
            .count();
        let grounded_dash = routes
            .clone()
            .filter(|route| route_uses_and_requires_dash(route))
            .count();
        let grounded_both = routes
            .filter(|route| {
                route_uses_and_requires_wall(route) && route_uses_and_requires_dash(route)
            })
            .count();
        let wall_bypass_contradictions = pool
            .iter()
            .flat_map(|room| room.routes.iter())
            .filter(|route| {
                route.structural.unavoidable_abilities.wall_jump
                    && route.audit.has_positive_without_wall_jump()
            })
            .count();
        let dash_bypass_contradictions = pool
            .iter()
            .flat_map(|room| room.routes.iter())
            .filter(|route| {
                route.structural.unavoidable_abilities.dash
                    && route.audit.has_positive_without_dash()
            })
            .count();
        writeln!(
            output,
            "route-audit complete={} witnessed-unavoidable-unbypassed-wall={} witnessed-unavoidable-unbypassed-dash={} witnessed-unavoidable-unbypassed-both={} structural-wall-with-positive-bypass={} structural-dash-with-positive-bypass={}",
            complete_audits,
            grounded_wall,
            grounded_dash,
            grounded_both,
            wall_bypass_contradictions,
            dash_bypass_contradictions,
        )
        .expect("writing to String cannot fail");
        writeln!(output, "rejections={:?}", self.rejections)
            .expect("writing to String cannot fail");
        output
    }
}

fn eligible_counts(pool: &[CertifiedRoom], tier: AbilityTier) -> [usize; 3] {
    let mut counts = [0; 3];
    for room in pool {
        for band in BANDS {
            counts[band_index(band)] +=
                usize::from(room.representative_route(band, tier).is_some());
        }
    }
    counts
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectedRoom {
    room_index: usize,
    band: ComplexityBand,
    representative_route: usize,
}

enum SelectionFailure {
    Deficit {
        maximum_joint: usize,
        reason: &'static str,
    },
    SearchExhausted {
        explored: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchConstraint {
    Socket(DoorSocket),
    WallAbility,
    DashAbility,
    Band(ComplexityBand),
}

impl BranchConstraint {
    const fn stable_key(self) -> (u8, u8, i32, i32) {
        match self {
            Self::Socket(socket) => (0, socket.side as u8, socket.offset, socket.span),
            Self::WallAbility => (1, 0, 0, 0),
            Self::DashAbility => (1, 1, 0, 0),
            Self::Band(band) => (2, band_index(band) as u8, 0, 0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BranchOption {
    room_index: usize,
    band: ComplexityBand,
    representative_route: usize,
    rank: SelectionRank,
}

fn choose_mrv_constraint(
    constraints: &[BranchConstraint],
    option_counts: &[usize],
) -> Option<BranchConstraint> {
    assert_eq!(constraints.len(), option_counts.len());
    constraints
        .iter()
        .copied()
        .zip(option_counts.iter().copied())
        .min_by_key(|(constraint, count)| (*count, constraint.stable_key()))
        .map(|(constraint, _)| constraint)
}

fn select_catalogue(
    pool: &[CertifiedRoom],
    quotas: [usize; 3],
    tier: AbilityTier,
) -> Result<Vec<SelectedRoom>, SelectionFailure> {
    let required = quotas.iter().sum::<usize>();
    let maximum_joint = maximum_joint_assignments(pool, &vec![false; pool.len()], quotas, tier);
    if maximum_joint < required {
        return Err(SelectionFailure::Deficit {
            maximum_joint,
            reason: "distinct-room band assignment",
        });
    }
    if !pool
        .iter()
        .all(|room| room_sockets_coverable_in_pool(room, pool))
    {
        // Rooms without any possible mate are still allowed in the certified
        // pool, but the selector will never place them in a valid leaf.
    }

    let mut search = SelectionSearch {
        pool,
        tier,
        socket_ranks: precompute_socket_ranks(pool),
        distance_cache: RefCell::new(HashMap::new()),
        selected_mask: vec![false; pool.len()],
        selected: Vec::with_capacity(required),
        remaining: quotas,
        explored: 0,
        exhausted: false,
    };
    if search.recurse() {
        return Ok(search.selected);
    }
    if search.exhausted {
        Err(SelectionFailure::SearchExhausted {
            explored: search.explored,
        })
    } else {
        Err(SelectionFailure::Deficit {
            maximum_joint,
            reason: "socket-mate and catalogue-level ability coverage",
        })
    }
}

struct SelectionSearch<'a> {
    pool: &'a [CertifiedRoom],
    tier: AbilityTier,
    socket_ranks: Vec<SocketRank>,
    distance_cache: RefCell<HashMap<DistanceKey, u64>>,
    selected_mask: Vec<bool>,
    selected: Vec<SelectedRoom>,
    remaining: [usize; 3],
    explored: usize,
    exhausted: bool,
}

impl SelectionSearch<'_> {
    fn recurse(&mut self) -> bool {
        if self.remaining.iter().all(|remaining| *remaining == 0) {
            return selected_sockets_are_closed(self.pool, &self.selected)
                && selected_covers_required_abilities(self.pool, &self.selected, self.tier);
        }
        if self.explored >= SELECTION_NODE_BUDGET {
            self.exhausted = true;
            return false;
        }
        self.explored += 1;

        let constraint = self.next_branch_constraint();
        let mut candidates = self.branch_options(constraint);
        candidates.sort_unstable_by(|left, right| {
            compare_selection_rank(left.rank, right.rank)
                .then_with(|| band_index(left.band).cmp(&band_index(right.band)))
                .then_with(|| {
                    self.pool[left.room_index]
                        .stable_key()
                        .cmp(&self.pool[right.room_index].stable_key())
                })
        });

        for candidate in candidates {
            let band_slot = band_index(candidate.band);
            self.remaining[band_slot] -= 1;
            self.selected_mask[candidate.room_index] = true;
            self.selected.push(SelectedRoom {
                room_index: candidate.room_index,
                band: candidate.band,
                representative_route: candidate.representative_route,
            });
            let band_feasible = maximum_joint_assignments(
                self.pool,
                &self.selected_mask,
                self.remaining,
                self.tier,
            ) == self.remaining.iter().sum::<usize>();
            if band_feasible
                && self.partial_socket_closure_is_possible()
                && self.partial_ability_coverage_is_possible()
                && self.recurse()
            {
                return true;
            }
            self.selected.pop();
            self.selected_mask[candidate.room_index] = false;
            self.remaining[band_slot] += 1;
            if self.exhausted {
                break;
            }
        }
        false
    }

    fn next_branch_constraint(&self) -> BranchConstraint {
        let selected_sockets = self
            .selected
            .iter()
            .flat_map(|selected| self.pool[selected.room_index].sockets.iter().copied())
            .collect::<Vec<_>>();
        let mut constraints = selected_sockets
            .iter()
            .copied()
            .filter(|socket| !selected_sockets.iter().any(|other| socket.matches(*other)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(BranchConstraint::Socket)
            .collect::<Vec<_>>();
        let (wall_covered, dash_covered) = selected_ability_coverage(self.pool, &self.selected);
        if tier_requires_wall(self.tier) && !wall_covered {
            constraints.push(BranchConstraint::WallAbility);
        }
        if tier_requires_dash(self.tier) && !dash_covered {
            constraints.push(BranchConstraint::DashAbility);
        }
        let option_counts = constraints
            .iter()
            .map(|constraint| self.constraint_option_count(*constraint))
            .collect::<Vec<_>>();
        choose_mrv_constraint(&constraints, &option_counts)
            .unwrap_or_else(|| BranchConstraint::Band(self.next_band()))
    }

    fn constraint_option_count(&self, constraint: BranchConstraint) -> usize {
        self.pool
            .iter()
            .enumerate()
            .flat_map(|(room_index, _)| BANDS.into_iter().map(move |band| (room_index, band)))
            .filter(|(room_index, band)| {
                self.representative_for_option(*room_index, *band, constraint)
                    .is_some()
            })
            .count()
    }

    fn branch_options(&self, constraint: BranchConstraint) -> Vec<BranchOption> {
        let mut result = Vec::new();
        for room_index in 0..self.pool.len() {
            for band in BANDS {
                let Some(representative_route) =
                    self.representative_for_option(room_index, band, constraint)
                else {
                    continue;
                };
                result.push(BranchOption {
                    room_index,
                    band,
                    representative_route,
                    rank: self.rank(room_index, representative_route),
                });
            }
        }
        result
    }

    fn representative_for_option(
        &self,
        room_index: usize,
        band: ComplexityBand,
        constraint: BranchConstraint,
    ) -> Option<usize> {
        if self.selected_mask[room_index]
            || self.remaining[band_index(band)] == 0
            || matches!(constraint, BranchConstraint::Band(required) if required != band)
        {
            return None;
        }
        let room = &self.pool[room_index];
        let representative_route = room.representative_route(band, self.tier)?;
        let route = &room.routes[representative_route];
        let satisfies = match constraint {
            BranchConstraint::Socket(socket) => {
                room.sockets.iter().any(|other| socket.matches(*other))
            }
            BranchConstraint::WallAbility => route_uses_and_requires_wall(route),
            BranchConstraint::DashAbility => route_uses_and_requires_dash(route),
            BranchConstraint::Band(_) => true,
        };
        satisfies.then_some(representative_route)
    }

    fn next_band(&self) -> ComplexityBand {
        BANDS
            .into_iter()
            .filter(|band| self.remaining[band_index(*band)] > 0)
            .min_by_key(|band| {
                let eligible = self
                    .pool
                    .iter()
                    .enumerate()
                    .filter(|(index, room)| {
                        !self.selected_mask[*index]
                            && room.representative_route(*band, self.tier).is_some()
                    })
                    .count();
                (
                    eligible.saturating_sub(self.remaining[band_index(*band)]),
                    band_index(*band),
                )
            })
            .expect("at least one quota remains")
    }

    fn rank(&self, room_index: usize, representative_route: usize) -> SelectionRank {
        let room = &self.pool[room_index];
        let representative = &room.routes[representative_route];
        let qd = room.qd_stratum();
        let strategy_coverage = self
            .selected
            .iter()
            .filter(|selected| {
                self.pool[selected.room_index]
                    .candidate
                    .key
                    .profile
                    .strategy
                    == room.candidate.key.profile.strategy
            })
            .count();
        let intent_coverage = self
            .selected
            .iter()
            .filter(|selected| {
                self.pool[selected.room_index].candidate.key.profile.intent
                    == room.candidate.key.profile.intent
            })
            .count();
        let qd_coverage = self
            .selected
            .iter()
            .filter(|selected| self.pool[selected.room_index].qd_stratum() == qd)
            .count();
        let nearest_distance = self
            .selected
            .iter()
            .map(|selected| {
                self.cached_room_distance(
                    room_index,
                    representative_route,
                    selected.room_index,
                    selected.representative_route,
                )
            })
            .min()
            .unwrap_or_default();
        let socket_match_gain = room
            .sockets
            .iter()
            .filter(|socket| {
                self.selected.iter().any(|selected| {
                    self.pool[selected.room_index]
                        .sockets
                        .iter()
                        .any(|other| socket.matches(*other))
                })
            })
            .count();
        let socket_rank = self.socket_ranks[room_index];
        let (wall_covered, dash_covered) = selected_ability_coverage(self.pool, &self.selected);
        let ability_coverage_gain = usize::from(
            !wall_covered
                && route_uses_and_requires_wall(representative)
                && tier_requires_wall(self.tier),
        ) + usize::from(
            !dash_covered
                && route_uses_and_requires_dash(representative)
                && tier_requires_dash(self.tier),
        );
        let (terrain_uncorroborated_components, terrain_uncorroborated_tiles) =
            terrain_selection_penalty(&room.terrain_utility);
        SelectionRank {
            strategy_coverage,
            ability_coverage_gain,
            intent_mismatch: intent_mismatch(
                room.candidate.key.profile.intent,
                representative.demand.band,
            ),
            vertical_port: room.has_vertical_port(),
            terrain_uncorroborated_components,
            terrain_uncorroborated_tiles,
            intent_coverage,
            qd_coverage,
            unmatched_own_sockets: socket_rank.unmatched_own_sockets,
            bottleneck_mate_inventory: socket_rank.bottleneck_mate_inventory,
            socket_match_gain,
            route_preference: route_preference(
                representative,
                self.tier,
                representative.demand.band,
            ),
            nearest_distance,
        }
    }

    fn cached_room_distance(
        &self,
        left_room: usize,
        left_route: usize,
        right_room: usize,
        right_route: usize,
    ) -> u64 {
        let left = (left_room, left_route);
        let right = (right_room, right_route);
        let key = if left <= right {
            DistanceKey { left, right }
        } else {
            DistanceKey {
                left: right,
                right: left,
            }
        };
        if let Some(distance) = self.distance_cache.borrow().get(&key) {
            return *distance;
        }
        let distance = room_distance(
            &self.pool[key.left.0],
            key.left.1,
            &self.pool[key.right.0],
            key.right.1,
        );
        self.distance_cache.borrow_mut().insert(key, distance);
        distance
    }

    fn partial_socket_closure_is_possible(&self) -> bool {
        let selected_sockets = self
            .selected
            .iter()
            .flat_map(|selected| self.pool[selected.room_index].sockets.iter().copied())
            .collect::<Vec<_>>();
        for socket in &selected_sockets {
            if selected_sockets.iter().any(|other| socket.matches(*other)) {
                continue;
            }
            let has_future_mate = self.pool.iter().enumerate().any(|(index, room)| {
                !self.selected_mask[index]
                    && room.sockets.iter().any(|other| socket.matches(*other))
                    && BANDS.into_iter().any(|band| {
                        self.remaining[band_index(band)] > 0
                            && room.representative_route(band, self.tier).is_some()
                    })
            });
            if !has_future_mate {
                return false;
            }
        }
        true
    }

    fn partial_ability_coverage_is_possible(&self) -> bool {
        let (wall_covered, dash_covered) = selected_ability_coverage(self.pool, &self.selected);
        for (needed, wall) in [
            (tier_requires_wall(self.tier) && !wall_covered, true),
            (tier_requires_dash(self.tier) && !dash_covered, false),
        ] {
            if !needed {
                continue;
            }
            let possible = self.pool.iter().enumerate().any(|(index, room)| {
                !self.selected_mask[index]
                    && BANDS.into_iter().any(|band| {
                        if self.remaining[band_index(band)] == 0 {
                            return false;
                        }
                        room.representative_route(band, self.tier)
                            .is_some_and(|route_index| {
                                let route = &room.routes[route_index];
                                if wall {
                                    route_uses_and_requires_wall(route)
                                } else {
                                    route_uses_and_requires_dash(route)
                                }
                            })
                    })
            });
            if !possible {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DistanceKey {
    left: (usize, usize),
    right: (usize, usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SocketRank {
    unmatched_own_sockets: usize,
    bottleneck_mate_inventory: usize,
}

fn precompute_socket_ranks(pool: &[CertifiedRoom]) -> Vec<SocketRank> {
    pool.iter()
        .map(|room| SocketRank {
            unmatched_own_sockets: room
                .sockets
                .iter()
                .filter(|socket| !room.sockets.iter().any(|other| socket.matches(*other)))
                .count(),
            bottleneck_mate_inventory: room
                .sockets
                .iter()
                .map(|socket| {
                    pool.iter()
                        .filter(|candidate| {
                            candidate.sockets.iter().any(|other| socket.matches(*other))
                        })
                        .count()
                })
                .min()
                .unwrap_or_default(),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectionRank {
    strategy_coverage: usize,
    ability_coverage_gain: usize,
    intent_mismatch: u8,
    vertical_port: bool,
    terrain_uncorroborated_components: usize,
    terrain_uncorroborated_tiles: usize,
    intent_coverage: usize,
    qd_coverage: usize,
    unmatched_own_sockets: usize,
    bottleneck_mate_inventory: usize,
    socket_match_gain: usize,
    route_preference: RoutePreference,
    nearest_distance: u64,
}

fn compare_selection_rank(left: SelectionRank, right: SelectionRank) -> std::cmp::Ordering {
    left.intent_mismatch
        .cmp(&right.intent_mismatch)
        .then_with(|| left.unmatched_own_sockets.cmp(&right.unmatched_own_sockets))
        .then_with(|| {
            right
                .bottleneck_mate_inventory
                .cmp(&left.bottleneck_mate_inventory)
        })
        .then_with(|| right.socket_match_gain.cmp(&left.socket_match_gain))
        .then_with(|| right.ability_coverage_gain.cmp(&left.ability_coverage_gain))
        .then_with(|| left.strategy_coverage.cmp(&right.strategy_coverage))
        .then_with(|| right.vertical_port.cmp(&left.vertical_port))
        .then_with(|| {
            left.terrain_uncorroborated_components
                .cmp(&right.terrain_uncorroborated_components)
        })
        .then_with(|| {
            left.terrain_uncorroborated_tiles
                .cmp(&right.terrain_uncorroborated_tiles)
        })
        .then_with(|| left.intent_coverage.cmp(&right.intent_coverage))
        .then_with(|| left.qd_coverage.cmp(&right.qd_coverage))
        .then_with(|| compare_route_preference(left.route_preference, right.route_preference))
        .then_with(|| right.nearest_distance.cmp(&left.nearest_distance))
}

const fn terrain_selection_penalty(terrain: &TerrainUtilityDescriptor) -> (usize, usize) {
    (
        terrain.components_without_positive_corroboration,
        terrain.tiles_without_positive_corroboration,
    )
}

const fn intent_mismatch(intent: ChallengeIntent, band: ComplexityBand) -> u8 {
    match (intent, band) {
        (ChallengeIntent::Gentle, ComplexityBand::Gentle)
        | (ChallengeIntent::Standard, ComplexityBand::Standard)
        | (ChallengeIntent::Technical, ComplexityBand::Technical) => 0,
        (ChallengeIntent::Gentle, ComplexityBand::Standard)
        | (ChallengeIntent::Standard, ComplexityBand::Gentle)
        | (ChallengeIntent::Standard, ComplexityBand::Technical)
        | (ChallengeIntent::Technical, ComplexityBand::Standard) => 1,
        (ChallengeIntent::Gentle, ComplexityBand::Technical)
        | (ChallengeIntent::Technical, ComplexityBand::Gentle) => 2,
    }
}

fn room_distance(
    left: &CertifiedRoom,
    left_route: usize,
    right: &CertifiedRoom,
    right_route: usize,
) -> u64 {
    let left_route = &left.routes[left_route];
    let right_route = &right.routes[right_route];
    let distance = (static_visual_distance(&left.visual, &right.visual).combined
        + collision_topology_distance(&left.collision, &right.collision).combined
        + traversal_distance(&left_route.traversal, &right_route.traversal).combined
        + semantic_action_distance(&left_route.actions, &right_route.actions).combined)
        / 4.0;
    (distance * DISTANCE_QUANTUM).round() as u64
}

fn maximum_joint_assignments(
    pool: &[CertifiedRoom],
    unavailable: &[bool],
    quotas: [usize; 3],
    tier: AbilityTier,
) -> usize {
    let eligibility = pool
        .iter()
        .map(|room| BANDS.map(|band| room.representative_route(band, tier).is_some()))
        .collect::<Vec<_>>();
    maximum_matching_for_eligibility(&eligibility, unavailable, quotas)
}

fn maximum_matching_for_eligibility(
    eligibility: &[[bool; 3]],
    unavailable: &[bool],
    quotas: [usize; 3],
) -> usize {
    debug_assert_eq!(eligibility.len(), unavailable.len());
    let mut slots = Vec::new();
    for band in BANDS {
        slots.extend(std::iter::repeat_n(band, quotas[band_index(band)]));
    }
    slots.sort_unstable_by_key(|band| {
        eligibility
            .iter()
            .enumerate()
            .filter(|(index, bands)| !unavailable[*index] && bands[band_index(*band)])
            .count()
    });
    let mut room_to_slot = vec![None; eligibility.len()];
    let mut matched = 0;
    for slot_index in 0..slots.len() {
        let mut seen_rooms = vec![false; eligibility.len()];
        if augment_slot(
            slot_index,
            &slots,
            eligibility,
            unavailable,
            &mut room_to_slot,
            &mut seen_rooms,
        ) {
            matched += 1;
        }
    }
    matched
}

fn augment_slot(
    slot_index: usize,
    slots: &[ComplexityBand],
    eligibility: &[[bool; 3]],
    unavailable: &[bool],
    room_to_slot: &mut [Option<usize>],
    seen_rooms: &mut [bool],
) -> bool {
    let band = slots[slot_index];
    for (room_index, bands) in eligibility.iter().enumerate() {
        if unavailable[room_index] || seen_rooms[room_index] || !bands[band_index(band)] {
            continue;
        }
        seen_rooms[room_index] = true;
        if room_to_slot[room_index].is_none_or(|previous_slot| {
            augment_slot(
                previous_slot,
                slots,
                eligibility,
                unavailable,
                room_to_slot,
                seen_rooms,
            )
        }) {
            room_to_slot[room_index] = Some(slot_index);
            return true;
        }
    }
    false
}

fn room_sockets_coverable_in_pool(room: &CertifiedRoom, pool: &[CertifiedRoom]) -> bool {
    room.sockets.iter().all(|socket| {
        pool.iter()
            .flat_map(|candidate| candidate.sockets.iter())
            .any(|other| socket.matches(*other))
    })
}

fn selected_sockets_are_closed(pool: &[CertifiedRoom], selected: &[SelectedRoom]) -> bool {
    let sockets = selected
        .iter()
        .flat_map(|selected| pool[selected.room_index].sockets.iter().copied())
        .collect::<Vec<_>>();
    sockets
        .iter()
        .all(|socket| sockets.iter().any(|other| socket.matches(*other)))
}

const fn tier_requires_wall(tier: AbilityTier) -> bool {
    matches!(tier, AbilityTier::WallJump | AbilityTier::WallJumpAndDash)
}

const fn tier_requires_dash(tier: AbilityTier) -> bool {
    matches!(tier, AbilityTier::Dash | AbilityTier::WallJumpAndDash)
}

fn selected_ability_coverage(pool: &[CertifiedRoom], selected: &[SelectedRoom]) -> (bool, bool) {
    let wall = selected.iter().any(|selected| {
        route_uses_and_requires_wall(
            &pool[selected.room_index].routes[selected.representative_route],
        )
    });
    let dash = selected.iter().any(|selected| {
        route_uses_and_requires_dash(
            &pool[selected.room_index].routes[selected.representative_route],
        )
    });
    (wall, dash)
}

fn route_uses_and_requires_wall(route: &RouteRecord) -> bool {
    route.difficulty.successful_wall_jumps > 0 && route.supports_wall_jump_requirement()
}

fn route_uses_and_requires_dash(route: &RouteRecord) -> bool {
    route.difficulty.successful_dashes > 0 && route.supports_dash_requirement()
}

fn selected_covers_required_abilities(
    pool: &[CertifiedRoom],
    selected: &[SelectedRoom],
    tier: AbilityTier,
) -> bool {
    let (wall, dash) = selected_ability_coverage(pool, selected);
    (!tier_requires_wall(tier) || wall) && (!tier_requires_dash(tier) || dash)
}

fn render_manifest(
    request: &CurateRequest,
    validation: &ValidationConfig,
    diagnostics: &CurationDiagnostics,
    pool: &[CertifiedRoom],
    selected: &[SelectedRoom],
) -> Result<String, CurateError> {
    if !selected_sockets_are_closed(pool, selected) {
        return Err(CurateError::Internal(
            "selected catalogue violated the socket-mate postcondition".into(),
        ));
    }
    let mut selected = selected.to_vec();
    selected.sort_unstable_by(|left, right| {
        band_index(left.band)
            .cmp(&band_index(right.band))
            .then_with(|| {
                pool[left.room_index]
                    .stable_key()
                    .cmp(&pool[right.room_index].stable_key())
            })
    });

    let solver_identity = solver_config_identity(validation);
    let difficulty_identity = difficulty_config_identity(validation);
    let abilities = request.tier.abilities();
    let selected_rooms_with_vertical = selected
        .iter()
        .filter(|selected| pool[selected.room_index].has_vertical_port())
        .count();
    let selected_sockets = selected
        .iter()
        .flat_map(|selected| pool[selected.room_index].sockets.iter().copied())
        .collect::<Vec<_>>();
    let vertical_sockets = selected_sockets
        .iter()
        .filter(|socket| matches!(socket.side, BoundarySide::Ceiling | BoundarySide::Floor))
        .count();
    let mut socket_inventory = BTreeMap::<DoorSocket, usize>::new();
    for socket in &selected_sockets {
        *socket_inventory.entry(*socket).or_default() += 1;
    }
    let qd_coverage = selected
        .iter()
        .map(|selected| pool[selected.room_index].qd_stratum())
        .collect::<BTreeSet<_>>()
        .len();
    let representative_wall_witnesses = selected
        .iter()
        .filter(|selected| {
            route_uses_and_requires_wall(
                &pool[selected.room_index].routes[selected.representative_route],
            )
        })
        .count();
    let representative_dash_witnesses = selected
        .iter()
        .filter(|selected| {
            route_uses_and_requires_dash(
                &pool[selected.room_index].routes[selected.representative_route],
            )
        })
        .count();
    let representative_both_witnesses = selected
        .iter()
        .filter(|selected| {
            let route = &pool[selected.room_index].routes[selected.representative_route];
            route_uses_and_requires_wall(route) && route_uses_and_requires_dash(route)
        })
        .count();
    let mut strategy_coverage = BTreeMap::<GenerationStrategy, usize>::new();
    let mut intent_coverage = BTreeMap::<ChallengeIntent, usize>::new();
    let mut band_intent_matches = 0_usize;
    let mut representative_pressure_signals = 0_usize;
    let mut representative_monotone_simple = 0_usize;
    for selected in &selected {
        let profile = pool[selected.room_index].candidate.key.profile;
        *strategy_coverage.entry(profile.strategy).or_default() += 1;
        *intent_coverage.entry(profile.intent).or_default() += 1;
        band_intent_matches += usize::from(intent_mismatch(profile.intent, selected.band) == 0);
        let demand = pool[selected.room_index].routes[selected.representative_route].demand;
        representative_pressure_signals += usize::from(demand.pressure_signals);
        representative_monotone_simple += usize::from(demand.monotone_simple_controller);
    }

    let mut output = String::new();
    writeln!(output, "downwards-curation-manifest-v{MANIFEST_VERSION}").unwrap();
    writeln!(output, "manifest-version={MANIFEST_VERSION}").unwrap();
    writeln!(
        output,
        "compositional-generation-version={COMPOSITIONAL_GENERATION_VERSION} experimental-backing-version={EXPERIMENTAL_GENERATION_VERSION}"
    )
    .unwrap();
    writeln!(
        output,
        "descriptor-versions static-visual={STATIC_VISUAL_DESCRIPTOR_VERSION} collision-topology={COLLISION_TOPOLOGY_DESCRIPTOR_VERSION} visual-fingerprint={VISUAL_FINGERPRINT_VERSION} witness={WITNESS_FINGERPRINT_VERSION} representative-actions={REPRESENTATIVE_ACTION_ENCODING_VERSION}"
    )
    .unwrap();
    writeln!(output, "selection-version={SELECTION_VERSION}").unwrap();
    writeln!(
        output,
        "request start-seed={} seeds-per-stratum={} strategies={} intents={} quota-per-band={} tier={} wall-jump={} dash={}",
        request.start_seed,
        request.seeds_per_stratum,
        GenerationStrategy::ALL.len(),
        ChallengeIntent::ALL.len(),
        request.quota_per_band,
        tier_slug(request.tier),
        u8::from(abilities.wall_jump),
        u8::from(abilities.dash),
    )
    .unwrap();
    writeln!(
        output,
        "solver-config id={solver_identity} policy-version={SOLVER_POLICY_VERSION} max-expanded-nodes={} max-simulated-ticks={} max-ticks-per-path={} beam-width={} position-quantum={} velocity-quantum={} probe-direct-routes={} baseline-preview-max-expanded-nodes={} baseline-preview-max-simulated-ticks={} macros={}",
        validation.solver.max_expanded_nodes,
        validation.solver.max_simulated_ticks,
        validation.solver.max_ticks_per_path,
        validation.solver.beam_width,
        validation.solver.position_quantum,
        validation.solver.velocity_quantum,
        u8::from(validation.solver.probe_direct_routes),
        validation.solver.baseline_preview_max_expanded_nodes,
        validation.solver.baseline_preview_max_simulated_ticks,
        validation.solver.macros.len(),
    )
    .unwrap();
    writeln!(
        output,
        "difficulty-config id={difficulty_identity} heuristic-version={DIFFICULTY_HEURISTIC_VERSION} perturbation-grace-ticks={} interpretation=heuristic-not-human-difficulty",
        validation.difficulty.perturbation_grace_ticks,
    )
    .unwrap();
    writeln!(
        output,
        "route-band-policy version={ROUTE_BAND_POLICY_VERSION} classifier=route-demand gentle-matrix=run-only-or-nontrivial-easy-engaging gentle-representative=monotone-simple,ordinary-jumps-1-to-3,no-wall-or-dash,pressure-at-most-1,robustness-at-least-3/4,hazard-clearance-at-least-8,nontrivial-travel standard=non-run-only-and-nontrivial technical=non-monotone-simple,explicit-controller-decision,pressure-signals-at-least-2 controller-decision=debounced-horizontal-reversal,vertical-both-signs-or-two-post-initial-changes,accepted-dash-direction-change pressure-signals=horizontal-reversal,vertical-demand,unique-actions-5,robustness-at-most-3/4,hazard-clearance-at-most-4,wall-chain-2,dash-chain-2 ordered-source-target=true"
    )
    .unwrap();
    writeln!(
        output,
        "easiest-route-policy version={EASIEST_ROUTE_AUDIT_VERSION} direct-probe-audit-version={DIRECT_PROBE_AUDIT_VERSION} challenge-unit=pinned-directed-source-target candidates=canonical-witness-plus-intended-physics-compatible-direct-controller-successes loadouts=all-subsets-of-intended exact-positive-discovery-evidence=retained-before-intended-loadout-replay canonical-discovery-evidence=minimum-witness-fingerprint-per-loadout selection=minimum-observed-route-demand semantic-action-dedup=true ability-requirement=structurally-unavoidable-and-no-positive-success-without-ability every-representative-requires-exact-expected-loadout-mask-and-no-budget-limited-audit=true unrelated-door-pairs=reachability-only non-success-is-not-unreachability-proof=true beam-optimality-claimed=false"
    )
    .unwrap();
    writeln!(
        output,
        "fairness-policy deaths-per-door-route=0 robustness-success-floor={MIN_ROBUSTNESS_NUMERATOR}/{MIN_ROBUSTNESS_DENOMINATOR} no-applicable-perturbations=pass all-door-pairs=required all-pickups-from-all-doors=required"
    )
    .unwrap();
    writeln!(
        output,
        "selection-policy distinct-static-visuals=true distinct-rooms-across-bands=true branching=mrv-unmatched-socket-or-ability rank-priority=intent-alignment,socket-self-closure,socket-mate-inventory,socket-match,ability,strategy,vertical,terrain-uncorroborated-components,terrain-uncorroborated-tiles,intent-coverage,qd,band-aware-route-demand,distance qd=strategy,intent,port-count,cycle-rank-bin,vertical-span-bin route-ranking=gentle-easy-engaging-safe;standard-nontrivial,structural-pressure,safe;technical-controller-decision,pressure,ability,reversal,vertical-demand,control-vocabulary,structural-pressure,robustness-near-1/2,small-clearance,solver-headroom terrain-evidence=route-plan-attribution-or-certified-traversal-proximity absence-from-one-witness-is-not-unreachability=true terrain-hard-threshold=none distance=equal-mean(static-visual,collision-topology,traversal,semantic-action) solver-effort-as-difficulty=false solver-effort-as-operational-quality=true vertical-port-preference=true socket-mate-closure=required representative-ability-policy={} node-budget={SELECTION_NODE_BUDGET}",
        representative_gate_slug(request.tier),
    )
    .unwrap();
    writeln!(
        output,
        "strategy-coverage distinct={} cyclic-graph={} reachability-growth={} rhythm-weave={}",
        strategy_coverage.len(),
        strategy_coverage
            .get(&GenerationStrategy::CyclicGraph)
            .copied()
            .unwrap_or_default(),
        strategy_coverage
            .get(&GenerationStrategy::ReachabilityGrowth)
            .copied()
            .unwrap_or_default(),
        strategy_coverage
            .get(&GenerationStrategy::RhythmWeave)
            .copied()
            .unwrap_or_default(),
    )
    .unwrap();
    writeln!(
        output,
        "intent-coverage distinct={} gentle={} standard={} technical={}",
        intent_coverage.len(),
        intent_coverage
            .get(&ChallengeIntent::Gentle)
            .copied()
            .unwrap_or_default(),
        intent_coverage
            .get(&ChallengeIntent::Standard)
            .copied()
            .unwrap_or_default(),
        intent_coverage
            .get(&ChallengeIntent::Technical)
            .copied()
            .unwrap_or_default(),
    )
    .unwrap();
    writeln!(
        output,
        "route-demand-coverage exact-band-intent-matches={}/{} representative-pressure-signals={} representative-monotone-simple={} technical-monotone-simple=0",
        band_intent_matches,
        selected.len(),
        representative_pressure_signals,
        representative_monotone_simple,
    )
    .unwrap();
    writeln!(
        output,
        "pool attempted={} constructed={} raw-unique-visuals={} exact-visual-duplicates={} certified-unique-visuals={} socket-coverable-rooms={} rejections={}",
        diagnostics.attempted,
        diagnostics.constructed,
        diagnostics.unique_visuals,
        diagnostics.exact_visual_duplicates,
        diagnostics.certified,
        diagnostics.socket_coverable_pool,
        map_summary(&diagnostics.rejections),
    )
    .unwrap();
    writeln!(
        output,
        "coverage selected-rooms={} qd-strata={} rooms-with-vertical-port={} vertical-port-rooms-ratio={}/{} sockets={} vertical-sockets={} socket-signatures={} all-sockets-have-mate=true",
        selected.len(),
        qd_coverage,
        selected_rooms_with_vertical,
        selected_rooms_with_vertical,
        selected.len(),
        selected_sockets.len(),
        vertical_sockets,
        socket_inventory.len(),
    )
    .unwrap();
    writeln!(
        output,
        "representative-ability-coverage wall-witnesses={} dash-witnesses={} both-on-one-route={} required-wall={} required-dash={} satisfied=true",
        representative_wall_witnesses,
        representative_dash_witnesses,
        representative_both_witnesses,
        u8::from(tier_requires_wall(request.tier)),
        u8::from(tier_requires_dash(request.tier)),
    )
    .unwrap();
    for (socket, count) in &socket_inventory {
        writeln!(
            output,
            "socket-inventory side={} offset={} span={} count={} mate-count={}",
            side_slug(socket.side),
            socket.offset,
            socket.span,
            count,
            socket_inventory
                .get(&socket.mate())
                .copied()
                .unwrap_or_default(),
        )
        .unwrap();
    }

    for (catalogue_index, selected) in selected.iter().enumerate() {
        render_room(
            &mut output,
            catalogue_index,
            selected,
            &pool[selected.room_index],
            request.tier,
        );
    }
    append_manifest_fingerprint(output)
}

fn render_room(
    output: &mut String,
    catalogue_index: usize,
    selected: &SelectedRoom,
    room: &CertifiedRoom,
    tier: AbilityTier,
) {
    let metadata = &room.candidate.generated.metadata;
    let summary = room.candidate.route_summary;
    let qd = room.qd_stratum();
    let representative = &room.routes[selected.representative_route];
    let catalogue_id = format!(
        "prototype-v{MANIFEST_VERSION}-{}-{}-{catalogue_index:03}",
        tier_slug(tier),
        band_slug(selected.band),
    );
    writeln!(
        output,
        "room-begin index={catalogue_index} id={} catalogue-band={} strategy={} intent={} seed={} tier={} wall-jump={} dash={} metadata-generation-version={} visual-fingerprint=downwards-static-v{VISUAL_FINGERPRINT_VERSION}-{:016x}",
        quoted(&catalogue_id),
        band_slug(selected.band),
        room.candidate.key.profile.strategy.slug(),
        room.candidate.key.profile.intent.slug(),
        metadata.seed,
        tier_slug(metadata.ability_tier),
        u8::from(metadata.intended_abilities.wall_jump),
        u8::from(metadata.intended_abilities.dash),
        metadata.generation_version,
        room.visual_fingerprint,
    )
    .unwrap();
    writeln!(
        output,
        "route-plan signature={:016x} nodes={} edges={} ports={} cycles={} branch-nodes={} vertical-span-rows={} wall-edges={} dash-edges={}",
        summary.signature,
        summary.node_count,
        summary.edge_count,
        summary.port_count,
        summary.cycle_rank,
        summary.branch_nodes,
        summary.vertical_span_rows,
        summary.wall_edges,
        summary.dash_edges,
    )
    .unwrap();
    writeln!(
        output,
        "qd-stratum strategy={} intent={} port-count={} cycle-bin={} vertical-span-bin={}",
        qd.strategy.slug(),
        qd.intent.slug(),
        qd.port_count,
        qd.cycle_bin,
        qd.vertical_span_bin,
    )
    .unwrap();
    writeln!(
        output,
        "terrain-utility descriptor-version={STRUCTURAL_DESCRIPTOR_VERSION} components={} interior-tiles={} route-support-tiles={} recovery-support-tiles={} ability-gate-envelope-tiles={} static-attributed-tiles={} certified-traversal-near-tiles={} positively-corroborated-tiles={} components-without-static-attribution={} tiles-without-static-attribution={} components-without-positive-corroboration={} tiles-without-positive-corroboration={} absence-is-not-negative-evidence=true",
        room.terrain_utility.components.len(),
        room.terrain_utility.interior_terrain_tiles,
        room.terrain_utility.route_support_tiles,
        room.terrain_utility.recovery_support_tiles,
        room.terrain_utility.ability_gate_envelope_tiles,
        room.terrain_utility.static_attributed_tiles,
        room.terrain_utility.certified_traversal_near_tiles,
        room.terrain_utility.positively_corroborated_tiles,
        room.terrain_utility.components_without_static_attribution,
        room.terrain_utility.tiles_without_static_attribution,
        room.terrain_utility.components_without_positive_corroboration,
        room.terrain_utility.tiles_without_positive_corroboration,
    )
    .unwrap();
    writeln!(
        output,
        "socket-signature {}",
        room.sockets
            .iter()
            .map(|socket| format!(
                "{}:{}:{}",
                side_slug(socket.side),
                socket.offset,
                socket.span
            ))
            .collect::<Vec<_>>()
            .join(","),
    )
    .unwrap();
    let mut doors = room
        .candidate
        .generated
        .room
        .doors()
        .iter()
        .collect::<Vec<_>>();
    doors.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for door in doors {
        let socket = door.socket();
        writeln!(
            output,
            "door id={} side={} offset={} span={} arrival-x={} arrival-y={}",
            quoted(&door.id),
            side_slug(socket.side),
            socket.offset,
            socket.span,
            door.arrival.x,
            door.arrival.y,
        )
        .unwrap();
    }
    writeln!(
        output,
        "representative-route catalogue-band={} challenge-unit=pinned-directed-source-target source={} target={} witness=downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x} easiest-known=true successful-wall-jumps={} successful-dashes={} structural-unavoidable-wall={} structural-unavoidable-dash={} positive-discovery-loadout-mask={:x} positive-bypass-without-wall={} positive-bypass-without-dash={} supported-required-wall={} supported-required-dash={}",
        band_slug(selected.band),
        quoted(&representative.source_door_id),
        quoted(&representative.target_door_id),
        representative.witness_fingerprint,
        representative.difficulty.successful_wall_jumps,
        representative.difficulty.successful_dashes,
        u8::from(representative.structural.unavoidable_abilities.wall_jump),
        u8::from(representative.structural.unavoidable_abilities.dash),
        representative.audit.positive_loadout_mask(),
        u8::from(representative.audit.has_positive_without_wall_jump()),
        u8::from(representative.audit.has_positive_without_dash()),
        u8::from(representative.supports_wall_jump_requirement()),
        u8::from(representative.supports_dash_requirement()),
    )
    .unwrap();
    render_representative_actions(
        output,
        "representative-actions",
        representative.actions.total_ticks,
        &representative.actions.spans,
    );
    writeln!(output, "route-matrix count={}", room.routes.len()).unwrap();
    for (route_index, route) in room.routes.iter().enumerate() {
        render_route(output, route_index, route);
    }
    writeln!(output, "pickup-matrix count={}", room.pickups.len()).unwrap();
    for pickup in &room.pickups {
        writeln!(
            output,
            "pickup-route source={} pickup={} witness=downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x}",
            quoted(&pickup.source_door_id),
            quoted(&pickup.pickup_id),
            pickup.witness_fingerprint,
        )
        .unwrap();
    }
    if let Some(pickup) = room
        .pickups
        .iter()
        .find(|pickup| pickup.source_door_id == representative.source_door_id)
    {
        writeln!(
            output,
            "representative-pickup source={} pickup={} witness=downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x}",
            quoted(&pickup.source_door_id),
            quoted(&pickup.pickup_id),
            pickup.witness_fingerprint,
        )
        .unwrap();
        render_representative_actions(
            output,
            "representative-pickup-actions",
            pickup.action_ticks,
            &pickup.action_spans,
        );
    }
    writeln!(
        output,
        "source-search-matrix count={}",
        room.source_search_effort.len()
    )
    .unwrap();
    for effort in &room.source_search_effort {
        writeln!(
            output,
            "source-search source={} expanded={} generated={} simulated-ticks={} deepest-path-ticks={}",
            quoted(&effort.source_door_id),
            effort.expanded_nodes,
            effort.generated_nodes,
            effort.simulated_ticks,
            effort.deepest_path_ticks,
        )
        .unwrap();
    }
    writeln!(output, "room-end").unwrap();
}

fn replay_action_spans(solution: &downwards_ai::TargetSolution) -> Vec<ActionSpan> {
    let mut spans = Vec::<ActionSpan>::new();
    for frame in &solution.replay.frames {
        let action = SemanticAction::from(frame.action);
        if let Some(last) = spans.last_mut()
            && last.action == action
        {
            last.ticks += 1;
        } else {
            spans.push(ActionSpan { action, ticks: 1 });
        }
    }
    spans
}

fn render_representative_actions(
    output: &mut String,
    label: &str,
    total_ticks: usize,
    spans: &[ActionSpan],
) {
    let data = spans
        .iter()
        .map(|span| {
            let action = span.action;
            format!(
                "{}:{}:{}:{}:{}*{}",
                action.move_x,
                action.move_y,
                u8::from(action.jump_held),
                u8::from(action.dash_held),
                u8::from(action.restart),
                span.ticks,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    writeln!(
        output,
        "{label} encoding=semantic-rle-v{REPRESENTATIVE_ACTION_ENCODING_VERSION} fields=move-x:move-y:jump:dash:restart*ticks total-ticks={total_ticks} spans={} data={}",
        spans.len(),
        quoted(&data),
    )
    .unwrap();
}

fn positive_loadout_evidence_slug(audit: EasiestRouteAudit) -> String {
    let rendered = audit
        .positive_loadout_evidence
        .iter()
        .enumerate()
        .filter_map(|(index, evidence)| {
            evidence.map(|evidence| {
                format!(
                    "{}:{}:downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x}",
                    ability_set_slug(loadout_for_index(index)),
                    evidence.successes,
                    evidence.canonical_witness_fingerprint,
                )
            })
        })
        .collect::<Vec<_>>()
        .join(",");
    if rendered.is_empty() {
        "none".to_owned()
    } else {
        rendered
    }
}

fn render_route(output: &mut String, route_index: usize, route: &RouteRecord) {
    let report = &route.difficulty;
    let demand = route.demand;
    let complexity = report.provisional_complexity;
    let components = complexity.components;
    let robustness = &report.temporal_robustness;
    let clearance = match report.minimum_hazard_clearance {
        None => "none".to_owned(),
        Some(clearance) => match clearance.hazard {
            HazardReference::StaticTile { tile_x, tile_y } => format!(
                "static:{tile_x}:{tile_y}:pixels={}:tick={}",
                clearance.pixels, clearance.replay_tick
            ),
            HazardReference::TimedHazard { hazard_index } => format!(
                "timed:{hazard_index}:pixels={}:tick={}",
                clearance.pixels, clearance.replay_tick
            ),
        },
    };
    let positive_loadout_evidence = positive_loadout_evidence_slug(route.audit);
    writeln!(
        output,
        "route index={route_index} source={} target={} band={} diagnostic-ai-band={} witness=downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x} canonical-witness=downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x} easiest-route-audit-version={} direct-probe-audit-version={} easiest-known=true audit-status={} candidate-witnesses={} raw-direct-probe-successes={} audited-loadouts={}/{} audited-loadout-mask={:x}/{:x} budget-limited-audits={} lower-loadout-successes={} positive-discovery-loadout-mask={:x} positive-discovery-loadout-evidence={} positive-bypass-without-wall={} positive-bypass-without-dash={} canonical-band={} easiest-origin={} easiest-discovery-loadout={} completion-ticks={} input-transitions={} jump-presses={} dash-presses={} successful-jumps={} successful-wall-jumps={} successful-dashes={} deaths={} hazard-clearance={} robustness={}/{} robustness-not-applicable={} demand-score={} pressure-signals={} run-only={} engaging-gentle={} monotone-simple-controller={} controller-decision={} nontrivial-traversal={} horizontal-reversals={} vertical-input-changes={} accepted-dash-direction-changes={} control-vocabulary={} action-spans={} ordinary-jumps={} visited-horizontal-span-cells={} visited-vertical-span-cells={} horizontal-travel-cells={} vertical-travel-cells={} unique-semantic-actions={} structural-descriptor-version={} structural-cost={}:{}:{}:{}:{} structural-selected-path-wall={} structural-selected-path-dash={} structural-selected-path-wall-edges={} structural-selected-path-dash-edges={} structural-unavoidable-wall={} structural-unavoidable-dash={} supported-required-wall={} supported-required-dash={} structural-critical-edges={} structural-direct-run-bypass={} structural-direct-drop-bypass={} structural-one-edge-bypass={} diagnostic-ai-complexity-score={} components=completion:{},input:{},verbs:{},deaths:{},fragility:{},solver-effort:{} search=expanded:{},generated:{},simulated-ticks:{},deepest-path-ticks:{}",
        quoted(&route.source_door_id),
        quoted(&route.target_door_id),
        band_slug(demand.band),
        band_slug(complexity.band),
        route.witness_fingerprint,
        route.canonical_witness_fingerprint,
        route.audit.version,
        route.audit.direct_probe_version,
        audit_status_slug(route.audit),
        route.audit.candidate_witnesses,
        route.audit.direct_probe_successes,
        route.audit.audited_loadouts,
        route.audit.expected_loadouts,
        route.audit.audited_loadout_mask,
        route.audit.expected_loadout_mask,
        route.audit.budget_limited_audits,
        route.audit.lower_loadout_successes,
        route.audit.positive_loadout_mask(),
        quoted(&positive_loadout_evidence),
        u8::from(route.audit.has_positive_without_wall_jump()),
        u8::from(route.audit.has_positive_without_dash()),
        band_slug(route.audit.canonical_band),
        witness_origin_slug(route.audit.easiest_origin),
        ability_set_slug(route.audit.easiest_discovery_loadout),
        report.completion_ticks,
        report.meaningful_input_transitions,
        report.jump_presses,
        report.dash_presses,
        report.successful_jumps,
        report.successful_wall_jumps,
        report.successful_dashes,
        report.deaths,
        clearance,
        robustness.successful_perturbations,
        robustness.attempted_perturbations,
        robustness.not_applicable_perturbations,
        demand.demand_score,
        demand.pressure_signals,
        u8::from(demand.run_only),
        u8::from(demand.engaging_gentle),
        u8::from(demand.monotone_simple_controller),
        u8::from(demand.controller_decision),
        u8::from(demand.nontrivial_traversal),
        demand.horizontal_reversals,
        demand.vertical_input_changes,
        demand.accepted_dash_direction_changes,
        demand.control_vocabulary,
        demand.action_spans,
        demand.ordinary_jumps,
        demand.visited_horizontal_span_cells,
        demand.visited_vertical_span_cells,
        demand.horizontal_travel_cells,
        demand.vertical_travel_cells,
        demand.unique_semantic_actions,
        STRUCTURAL_DESCRIPTOR_VERSION,
        route.structural.cost.required_ability_edges,
        route.structural.cost.edge_count,
        route.structural.cost.decision_nodes,
        route.structural.cost.vertical_transitions,
        route.structural.cost.verb_variety,
        u8::from(route.structural.selected_path_abilities.wall_jump),
        u8::from(route.structural.selected_path_abilities.dash),
        route.structural.selected_path_wall_jump_edges,
        route.structural.selected_path_dash_edges,
        u8::from(route.structural.unavoidable_abilities.wall_jump),
        u8::from(route.structural.unavoidable_abilities.dash),
        u8::from(route.supports_wall_jump_requirement()),
        u8::from(route.supports_dash_requirement()),
        route.structural.critical_edges,
        u8::from(route.structural.direct_run_bypass),
        u8::from(route.structural.direct_drop_bypass),
        u8::from(route.structural.one_edge_bypass),
        complexity.component_score,
        components.completion,
        components.input_transitions,
        components.traversal_verbs,
        components.deaths,
        components.temporal_fragility,
        components.solver_effort,
        report.search_effort.expanded_nodes,
        report.search_effort.generated_nodes,
        report.search_effort.simulated_ticks,
        report.search_effort.deepest_path_ticks,
    )
    .unwrap();
}

fn solver_config_identity(config: &ValidationConfig) -> String {
    let solver = &config.solver;
    let mut hash = StableHash::domain(b"downwards-curation-solver-config");
    hash.u32(CONFIG_FINGERPRINT_VERSION);
    hash.u32(SOLVER_POLICY_VERSION);
    hash.usize(solver.max_expanded_nodes);
    hash.usize(solver.max_simulated_ticks);
    hash.usize(solver.max_ticks_per_path);
    hash.usize(solver.beam_width);
    hash.i32(solver.position_quantum);
    hash.i32(solver.velocity_quantum);
    hash.bool(solver.probe_direct_routes);
    hash.usize(solver.baseline_preview_max_expanded_nodes);
    hash.usize(solver.baseline_preview_max_simulated_ticks);
    hash.usize(solver.macros.len());
    for action_macro in &solver.macros {
        hash.string(&action_macro.name);
        hash.usize(action_macro.actions.len());
        for action in &action_macro.actions {
            hash.i8(action.move_x);
            hash.i8(action.move_y);
            hash.bool(action.jump);
            hash.bool(action.dash);
            hash.bool(action.restart);
        }
    }
    format!(
        "downwards-solver-config-v{CONFIG_FINGERPRINT_VERSION}-{:016x}",
        hash.finish()
    )
}

fn difficulty_config_identity(config: &ValidationConfig) -> String {
    let mut hash = StableHash::domain(b"downwards-curation-difficulty-config");
    hash.u32(CONFIG_FINGERPRINT_VERSION);
    hash.u32(DIFFICULTY_HEURISTIC_VERSION);
    hash.u32(ROUTE_BAND_POLICY_VERSION);
    hash.usize(config.difficulty.perturbation_grace_ticks);
    format!(
        "downwards-difficulty-config-v{CONFIG_FINGERPRINT_VERSION}-{:016x}",
        hash.finish()
    )
}

fn fingerprint_visual(visual: &StaticVisualDescriptor) -> u64 {
    let mut hash = StableHash::domain(b"downwards-curation-static-visual");
    hash.u32(VISUAL_FINGERPRINT_VERSION);
    hash.u32(visual.version);
    hash.u16(visual.width);
    hash.u16(visual.height);
    hash.i32(visual.tile_size);
    hash.i32(visual.spawn.x);
    hash.i32(visual.spawn.y);
    hash.usize(visual.tiles.len());
    for tile in &visual.tiles {
        hash.u8(*tile as u8);
    }
    hash.usize(visual.exits.len());
    for bounds in &visual.exits {
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.usize(visual.doors.len());
    for door in &visual.doors {
        hash.u8(door.side as u8);
        let bounds = door.trigger_bounds;
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.usize(visual.pickups.len());
    for bounds in &visual.pickups {
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.usize(visual.timed_hazards.len());
    for bounds in &visual.timed_hazards {
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.finish()
}

fn append_manifest_fingerprint(mut body: String) -> Result<String, CurateError> {
    let mut hash = StableHash::domain(b"downwards-curation-manifest");
    hash.u32(MANIFEST_VERSION);
    hash.bytes(body.as_bytes());
    writeln!(
        body,
        "manifest-fingerprint=downwards-curation-manifest-v{MANIFEST_VERSION}-{:016x}",
        hash.finish()
    )
    .map_err(|_| CurateError::Internal("could not render manifest fingerprint".into()))?;
    Ok(body)
}

struct StableHash(u64);

impl StableHash {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn domain(domain: &[u8]) -> Self {
        let mut hash = Self(Self::OFFSET);
        hash.bytes(domain);
        hash
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.usize(bytes.len());
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn i8(&mut self, value: i8) {
        self.u8(value as u8);
    }

    fn u8(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn u16(&mut self, value: u16) {
        self.raw(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.raw(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.raw(&value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.raw(&(value as u64).to_le_bytes());
    }

    fn rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.i32(x);
        self.i32(y);
        self.i32(width);
        self.i32(height);
    }

    fn raw(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.u8(*byte);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

fn quoted(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control.is_control() => {
                write!(output, "\\u{{{:x}}}", u32::from(control)).unwrap();
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn map_summary(values: &BTreeMap<&'static str, usize>) -> String {
    if values.is_empty() {
        return "none".into();
    }
    values
        .iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_positive(value: &str, label: &str) -> Result<usize, CurateError> {
    let value = value
        .parse::<usize>()
        .map_err(|error| CurateError::Arguments(format!("invalid {label} {value:?}: {error}")))?;
    if value == 0 {
        return Err(CurateError::Arguments(format!("{label} must be positive")));
    }
    Ok(value)
}

fn parse_positive_or_zero(value: &str, label: &str) -> Result<u64, CurateError> {
    value
        .parse::<u64>()
        .map_err(|error| CurateError::Arguments(format!("invalid {label} {value:?}: {error}")))
}

fn parse_tier(value: &str) -> Result<AbilityTier, CurateError> {
    match value {
        "baseline" => Ok(AbilityTier::Baseline),
        "wall" => Ok(AbilityTier::WallJump),
        "dash" => Ok(AbilityTier::Dash),
        "both" => Ok(AbilityTier::WallJumpAndDash),
        _ => Err(CurateError::Arguments(format!(
            "unknown tier {value:?}; expected baseline, wall, dash, or both"
        ))),
    }
}

const fn band_index(band: ComplexityBand) -> usize {
    match band {
        ComplexityBand::Gentle => 0,
        ComplexityBand::Standard => 1,
        ComplexityBand::Technical => 2,
    }
}

const fn band_slug(band: ComplexityBand) -> &'static str {
    match band {
        ComplexityBand::Gentle => "gentle",
        ComplexityBand::Standard => "standard",
        ComplexityBand::Technical => "technical",
    }
}

const fn tier_slug(tier: AbilityTier) -> &'static str {
    match tier {
        AbilityTier::Baseline => "baseline",
        AbilityTier::WallJump => "wall",
        AbilityTier::Dash => "dash",
        AbilityTier::WallJumpAndDash => "both",
    }
}

const fn ability_set_slug(abilities: AbilitySet) -> &'static str {
    match (abilities.wall_jump, abilities.dash) {
        (false, false) => "baseline",
        (true, false) => "wall",
        (false, true) => "dash",
        (true, true) => "both",
    }
}

const fn witness_origin_slug(origin: RouteWitnessOrigin) -> &'static str {
    match origin {
        RouteWitnessOrigin::Canonical => "canonical",
        RouteWitnessOrigin::DirectProbe => "direct-probe",
    }
}

fn audit_status_slug(audit: EasiestRouteAudit) -> &'static str {
    if audit.complete() {
        "complete"
    } else if audit.budget_limited_audits > 0 {
        "budget-limited"
    } else {
        "incomplete-loadout-coverage"
    }
}

const fn representative_gate_slug(tier: AbilityTier) -> &'static str {
    match tier {
        AbilityTier::Baseline => "all-bands-complete-exact-loadout-mask-audit",
        AbilityTier::WallJump => {
            "gentle-no-supported-advanced-requirement;standard-and-technical-each-use-wall-and-have-wall-unavoidable-with-no-positive-no-wall-bypass;technical-no-one-edge-baseline;all-bands-complete-exact-loadout-mask-audit"
        }
        AbilityTier::Dash => {
            "gentle-no-supported-advanced-requirement;standard-and-technical-each-use-dash-and-have-dash-unavoidable-with-no-positive-no-dash-bypass;technical-no-one-edge-baseline;all-bands-complete-exact-loadout-mask-audit"
        }
        AbilityTier::WallJumpAndDash => {
            "gentle-no-supported-advanced-requirement;standard-and-technical-each-use-wall-or-dash-with-corresponding-unavoidable-edge-and-no-positive-missing-ability-bypass;technical-no-one-edge-baseline;catalogue-has-supported-wall-and-dash;prefer-both-on-one-route;all-bands-complete-exact-loadout-mask-audit"
        }
    }
}

const fn side_slug(side: BoundarySide) -> &'static str {
    match side {
        BoundarySide::Left => "left",
        BoundarySide::Right => "right",
        BoundarySide::Ceiling => "ceiling",
        BoundarySide::Floor => "floor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_demand(band: ComplexityBand) -> RouteDemand {
        RouteDemand {
            band,
            demand_score: 4,
            pressure_signals: u8::from(band == ComplexityBand::Technical) * 2,
            run_only: false,
            engaging_gentle: band == ComplexityBand::Gentle,
            monotone_simple_controller: band != ComplexityBand::Technical,
            controller_decision: band == ComplexityBand::Technical,
            nontrivial_traversal: true,
            horizontal_reversals: usize::from(band == ComplexityBand::Technical),
            vertical_input_changes: 0,
            accepted_dash_direction_changes: 0,
            control_vocabulary: 2,
            action_spans: 3,
            ordinary_jumps: 1,
            visited_horizontal_span_cells: 8,
            visited_vertical_span_cells: 2,
            horizontal_travel_cells: 8,
            vertical_travel_cells: 4,
            unique_semantic_actions: 2,
            successful_wall_jumps: 0,
            successful_dashes: 0,
            robustness_numerator: 1,
            robustness_denominator: 1,
            hazard_clearance: 10,
        }
    }

    fn test_preference(band: ComplexityBand, quality: RouteQuality) -> RoutePreference {
        RoutePreference {
            band,
            ability_score: 0,
            demand: test_demand(band),
            structural: StructuralRoutePreference::default(),
            operational: quality,
        }
    }

    fn neutral_selection_rank() -> SelectionRank {
        let quality = RouteQuality {
            robustness_numerator: 1,
            robustness_denominator: 1,
            hazard_clearance: 10,
            simulated_ticks: 100,
            expanded_nodes: 10,
        };
        SelectionRank {
            strategy_coverage: 0,
            ability_coverage_gain: 0,
            intent_mismatch: 0,
            vertical_port: false,
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            intent_coverage: 0,
            qd_coverage: 0,
            unmatched_own_sockets: 0,
            bottleneck_mate_inventory: 0,
            socket_match_gain: 0,
            route_preference: test_preference(ComplexityBand::Standard, quality),
            nearest_distance: 0,
        }
    }

    fn test_audit(complete: bool, canonical_band: ComplexityBand) -> EasiestRouteAudit {
        EasiestRouteAudit {
            version: EASIEST_ROUTE_AUDIT_VERSION,
            direct_probe_version: DIRECT_PROBE_AUDIT_VERSION,
            candidate_witnesses: 2,
            direct_probe_successes: 1,
            audited_loadouts: 1,
            expected_loadouts: 1,
            audited_loadout_mask: loadout_bit(AbilitySet::NONE),
            expected_loadout_mask: loadout_bit(AbilitySet::NONE),
            budget_limited_audits: usize::from(!complete),
            lower_loadout_successes: 0,
            positive_loadout_evidence: [None; 4],
            canonical_band,
            easiest_origin: RouteWitnessOrigin::Canonical,
            easiest_discovery_loadout: AbilitySet::NONE,
        }
    }

    fn test_structural(unavoidable_abilities: AbilitySet) -> StructuralBypassDescriptor {
        let required_ability_edges =
            usize::from(unavoidable_abilities.wall_jump) + usize::from(unavoidable_abilities.dash);
        StructuralBypassDescriptor {
            source_door_id: "source".to_owned(),
            target_door_id: "target".to_owned(),
            source_node_id: 1,
            target_node_id: 2,
            node_path: vec![1, 3, 2],
            steps: Vec::new(),
            cost: StructuralPathCost {
                required_ability_edges,
                edge_count: 2,
                decision_nodes: 1,
                vertical_transitions: 1,
                verb_variety: 2,
            },
            selected_path_abilities: unavoidable_abilities,
            selected_path_wall_jump_edges: usize::from(unavoidable_abilities.wall_jump),
            selected_path_dash_edges: usize::from(unavoidable_abilities.dash),
            unavoidable_abilities,
            critical_edges: 1,
            direct_run_bypass: false,
            direct_drop_bypass: false,
            one_edge_bypass: false,
        }
    }

    #[test]
    fn band_matching_never_reuses_one_room_for_two_quotas() {
        let eligibility = vec![
            [true, true, false],
            [false, false, true],
            [false, false, true],
        ];
        let unavailable = vec![false; eligibility.len()];
        assert_eq!(
            maximum_matching_for_eligibility(&eligibility, &unavailable, [1, 1, 1]),
            2
        );

        let eligibility = vec![
            [true, true, false],
            [true, false, false],
            [false, false, true],
        ];
        assert_eq!(
            maximum_matching_for_eligibility(&eligibility, &unavailable, [1, 1, 1]),
            3
        );
    }

    #[test]
    fn selection_rank_is_deterministic_and_uses_all_policy_layers() {
        let quality = RouteQuality {
            robustness_numerator: 1,
            robustness_denominator: 1,
            hazard_clearance: 10,
            simulated_ticks: 100,
            expanded_nodes: 10,
        };
        let route_preference = test_preference(ComplexityBand::Standard, quality);
        let ordinary = SelectionRank {
            strategy_coverage: 1,
            ability_coverage_gain: 0,
            intent_mismatch: 0,
            vertical_port: false,
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            intent_coverage: 0,
            qd_coverage: 0,
            unmatched_own_sockets: 0,
            bottleneck_mate_inventory: 10,
            socket_match_gain: 3,
            route_preference,
            nearest_distance: 900,
        };
        let vertical = SelectionRank {
            strategy_coverage: 1,
            ability_coverage_gain: 0,
            intent_mismatch: 0,
            vertical_port: true,
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            intent_coverage: 9,
            qd_coverage: 9,
            unmatched_own_sockets: 0,
            bottleneck_mate_inventory: 10,
            socket_match_gain: 0,
            route_preference,
            nearest_distance: 0,
        };
        assert_eq!(
            compare_selection_rank(ordinary, vertical),
            std::cmp::Ordering::Less
        );

        let mismatched_intent = SelectionRank {
            intent_mismatch: 1,
            ..ordinary
        };
        let exact_intent_with_worse_socket_rank = SelectionRank {
            unmatched_own_sockets: 5,
            bottleneck_mate_inventory: 0,
            ..ordinary
        };
        assert_eq!(
            compare_selection_rank(exact_intent_with_worse_socket_rank, mismatched_intent),
            std::cmp::Ordering::Less
        );

        let uncovered_strategy = SelectionRank {
            strategy_coverage: 0,
            ability_coverage_gain: 0,
            intent_mismatch: 0,
            vertical_port: false,
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            intent_coverage: 9,
            qd_coverage: 9,
            unmatched_own_sockets: 0,
            bottleneck_mate_inventory: 10,
            socket_match_gain: 0,
            route_preference,
            nearest_distance: 0,
        };
        assert_eq!(
            compare_selection_rank(uncovered_strategy, vertical),
            std::cmp::Ordering::Less
        );

        let sparse_qd = SelectionRank {
            strategy_coverage: 1,
            ability_coverage_gain: 0,
            intent_mismatch: 0,
            vertical_port: true,
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            intent_coverage: 0,
            qd_coverage: 0,
            unmatched_own_sockets: 0,
            bottleneck_mate_inventory: 10,
            socket_match_gain: 0,
            route_preference,
            nearest_distance: 1,
        };
        assert_eq!(
            compare_selection_rank(sparse_qd, vertical),
            std::cmp::Ordering::Less
        );

        let farther = SelectionRank {
            strategy_coverage: 1,
            ability_coverage_gain: 0,
            intent_mismatch: 0,
            vertical_port: true,
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            intent_coverage: 0,
            qd_coverage: 0,
            unmatched_own_sockets: 0,
            bottleneck_mate_inventory: 10,
            socket_match_gain: 0,
            route_preference,
            nearest_distance: 2,
        };
        assert_eq!(
            compare_selection_rank(farther, sparse_qd),
            std::cmp::Ordering::Less
        );

        let inert_terrain = SelectionRank {
            terrain_uncorroborated_components: 1,
            terrain_uncorroborated_tiles: 12,
            ..ordinary
        };
        let corroborated_terrain = SelectionRank {
            terrain_uncorroborated_components: 0,
            terrain_uncorroborated_tiles: 0,
            ..ordinary
        };
        assert_eq!(
            compare_selection_rank(corroborated_terrain, inert_terrain),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn terrain_rank_penalizes_each_absolute_uncorroborated_measure() {
        let corroborated = neutral_selection_rank();
        let extra_uncorroborated_component = SelectionRank {
            terrain_uncorroborated_components: 1,
            ..corroborated
        };
        let extra_uncorroborated_tile = SelectionRank {
            terrain_uncorroborated_tiles: 1,
            ..corroborated
        };

        assert_eq!(
            compare_selection_rank(corroborated, extra_uncorroborated_component),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_selection_rank(corroborated, extra_uncorroborated_tile),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn terrain_rank_does_not_reward_more_fully_corroborated_tiles() {
        let small = TerrainUtilityDescriptor {
            interior_terrain_tiles: 12,
            positively_corroborated_tiles: 12,
            ..TerrainUtilityDescriptor::default()
        };
        let large = TerrainUtilityDescriptor {
            interior_terrain_tiles: 24,
            positively_corroborated_tiles: 24,
            ..TerrainUtilityDescriptor::default()
        };
        let (small_components, small_tiles) = terrain_selection_penalty(&small);
        let (large_components, large_tiles) = terrain_selection_penalty(&large);
        let small_rank = SelectionRank {
            terrain_uncorroborated_components: small_components,
            terrain_uncorroborated_tiles: small_tiles,
            ..neutral_selection_rank()
        };
        let large_rank = SelectionRank {
            terrain_uncorroborated_components: large_components,
            terrain_uncorroborated_tiles: large_tiles,
            ..neutral_selection_rank()
        };

        assert_eq!(
            compare_selection_rank(small_rank, large_rank),
            std::cmp::Ordering::Equal
        );
    }

    fn difficulty_report(
        successful_jumps: usize,
        successful_wall_jumps: usize,
        successful_dashes: usize,
        robustness: (usize, usize),
        clearance: u32,
    ) -> DifficultyReport {
        use downwards_ai::{
            ComplexityComponents, DifficultyInterpretation, MinimumHazardClearance,
            ProvisionalComplexity, SearchStats, TemporalRobustness,
        };

        DifficultyReport {
            interpretation: DifficultyInterpretation::HeuristicNotHumanDifficulty,
            exit_id: "target".to_owned(),
            completion_ticks: 120,
            meaningful_input_transitions: 8,
            jump_presses: successful_jumps,
            dash_presses: successful_dashes,
            successful_jumps,
            successful_wall_jumps,
            successful_dashes,
            search_effort: SearchStats::default(),
            deaths: 0,
            minimum_hazard_clearance: Some(MinimumHazardClearance {
                pixels: clearance,
                replay_tick: 1,
                hazard: HazardReference::StaticTile {
                    tile_x: 1,
                    tile_y: 1,
                },
            }),
            temporal_robustness: TemporalRobustness {
                attempted_perturbations: robustness.1,
                successful_perturbations: robustness.0,
                successful_perturbation_ratio: Some(robustness.0 as f64 / robustness.1 as f64),
                not_applicable_perturbations: 0,
                earliest_divergence: None,
                earliest_failure: None,
                trials: Vec::new(),
            },
            provisional_complexity: ProvisionalComplexity {
                interpretation: DifficultyInterpretation::HeuristicNotHumanDifficulty,
                band: ComplexityBand::Gentle,
                component_score: 0,
                components: ComplexityComponents {
                    completion: 0,
                    input_transitions: 0,
                    traversal_verbs: 0,
                    deaths: 0,
                    temporal_fragility: 0,
                    solver_effort: 0,
                },
            },
        }
    }

    fn action_trace(actions: Vec<SemanticAction>, jumps: usize) -> SemanticActionTrace {
        SemanticActionTrace {
            total_ticks: actions.len(),
            spans: actions
                .into_iter()
                .map(|action| ActionSpan { action, ticks: 1 })
                .collect(),
            events: Box::new([]),
            jump_presses: jumps,
            dash_presses: 0,
            restart_presses: 0,
            successful_jumps: jumps,
            successful_wall_jumps: 0,
            successful_dashes: 0,
            deaths: 0,
            pickups_collected: 0,
        }
    }

    fn traversal_trace(cells: &[(u16, u16)]) -> TraversalTrace {
        use downwards_lab::{TraversalCell, TraversalSpan};

        let spans = cells
            .iter()
            .map(|&(x, y)| TraversalSpan {
                cell: TraversalCell { x, y },
                samples: 1,
            })
            .collect::<Vec<_>>();
        let visited_cells = cells
            .iter()
            .map(|&(x, y)| TraversalCell { x, y })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        TraversalTrace {
            grid: TraversalGrid::new(16, 9).unwrap(),
            sample_count: spans.len(),
            spans: spans.into_boxed_slice(),
            visited_cells,
        }
    }

    #[test]
    fn monotone_repeated_auto_jump_cannot_be_technical() {
        let run = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let jump = SemanticAction {
            jump_held: true,
            ..run
        };
        let actions = action_trace(vec![run, jump, run, jump, run, jump, run, jump], 4);
        let traversal =
            traversal_trace(&[(1, 8), (3, 6), (5, 4), (7, 2), (9, 4), (11, 6), (13, 8)]);
        let report = difficulty_report(4, 2, 0, (1, 2), 0);
        let demand = RouteDemand::observe(&report, &actions, &traversal);

        assert!(demand.monotone_simple_controller);
        assert!(!demand.controller_decision);
        assert!(demand.pressure_signals >= 2);
        assert_eq!(demand.band, ComplexityBand::Standard);
    }

    #[test]
    fn one_direction_input_noise_cannot_fake_a_technical_decision() {
        let run = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let actions = action_trace(
            vec![
                run,
                SemanticAction {
                    jump_held: true,
                    ..run
                },
                SemanticAction::default(),
                SemanticAction {
                    jump_held: true,
                    ..SemanticAction::default()
                },
                SemanticAction {
                    dash_held: true,
                    ..run
                },
            ],
            4,
        );
        let report = difficulty_report(4, 0, 0, (1, 2), 0);
        let traversal =
            traversal_trace(&[(1, 8), (3, 6), (5, 4), (7, 2), (9, 4), (11, 6), (13, 8)]);
        let demand = RouteDemand::observe(&report, &actions, &traversal);

        assert_eq!(demand.unique_semantic_actions, 5);
        assert!(!demand.controller_decision);
        assert_eq!(demand.band, ComplexityBand::Standard);
    }

    #[test]
    fn held_drop_input_is_not_a_vertical_controller_decision() {
        let drop = SemanticAction {
            move_x: 1,
            move_y: 1,
            ..SemanticAction::default()
        };
        let jump_drop = SemanticAction {
            jump_held: true,
            ..drop
        };
        let actions = action_trace(vec![drop, jump_drop, drop, jump_drop, drop, jump_drop], 3);
        let report = difficulty_report(3, 0, 0, (1, 2), 0);
        let traversal = traversal_trace(&[(1, 1), (3, 3), (5, 5), (7, 7), (10, 8)]);
        let demand = RouteDemand::observe(&report, &actions, &traversal);

        assert_eq!(demand.vertical_input_changes, 0);
        assert!(!demand.controller_decision);
        assert_eq!(demand.band, ComplexityBand::Standard);
    }

    #[test]
    fn one_tick_opposite_input_bounce_is_not_a_reversal() {
        let right = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let left = SemanticAction {
            move_x: -1,
            ..SemanticAction::default()
        };
        let actions = action_trace(vec![right, left, right], 0);

        assert_eq!(horizontal_reversals(&actions), 0);
    }

    #[test]
    fn run_only_bypass_wins_over_a_technical_canonical_witness() {
        let make_witness = |demand, fingerprint| AssessedWitness {
            witness_fingerprint: fingerprint,
            difficulty: difficulty_report(1, 0, 0, (1, 1), 20),
            traversal: traversal_trace(&[(1, 8), (12, 8)]),
            actions: action_trace(Vec::new(), 0),
            demand,
            origin: if fingerprint == 1 {
                RouteWitnessOrigin::Canonical
            } else {
                RouteWitnessOrigin::DirectProbe
            },
            discovery_loadout: AbilitySet::NONE,
        };
        let hard = make_witness(test_demand(ComplexityBand::Technical), 1);
        let mut bypass_demand = test_demand(ComplexityBand::Gentle);
        bypass_demand.run_only = true;
        bypass_demand.engaging_gentle = false;
        bypass_demand.nontrivial_traversal = false;
        bypass_demand.ordinary_jumps = 0;
        let bypass = make_witness(bypass_demand, 2);
        let witnesses = [hard, bypass];
        let easiest = witnesses
            .iter()
            .min_by(|left, right| compare_easiest_witness(left, right))
            .unwrap();

        assert_eq!(easiest.witness_fingerprint, 2);
        assert_eq!(easiest.demand.band, ComplexityBand::Gentle);
        assert!(!easiest.demand.engaging_gentle);
    }

    #[test]
    fn incomplete_easy_route_audit_cannot_promote_any_band() {
        let make_route = |band| RouteRecord {
            source_door_id: "source".to_owned(),
            target_door_id: "target".to_owned(),
            witness_fingerprint: 1,
            canonical_witness_fingerprint: 1,
            difficulty: difficulty_report(1, 0, 0, (1, 1), 20),
            traversal: traversal_trace(&[(1, 8), (12, 8)]),
            actions: action_trace(Vec::new(), 0),
            demand: test_demand(band),
            structural: test_structural(AbilitySet::NONE),
            audit: test_audit(false, band),
        };

        assert!(
            !make_route(ComplexityBand::Gentle)
                .representative_eligible(ComplexityBand::Gentle, AbilityTier::Baseline)
        );
        assert!(
            !make_route(ComplexityBand::Standard)
                .representative_eligible(ComplexityBand::Standard, AbilityTier::Baseline)
        );
        assert!(
            !make_route(ComplexityBand::Technical)
                .representative_eligible(ComplexityBand::Technical, AbilityTier::Baseline)
        );

        let mut missing_loadout = test_audit(true, ComplexityBand::Standard);
        missing_loadout.expected_loadouts = 2;
        assert!(!missing_loadout.complete());

        let mut wrong_loadout = test_audit(true, ComplexityBand::Standard);
        wrong_loadout.audited_loadout_mask = loadout_bit(AbilitySet::new(true, false));
        assert!(!wrong_loadout.complete());
    }

    #[test]
    fn positive_loadout_evidence_is_a_deterministic_set() {
        let mut evidence = [None; 4];
        record_positive_loadout_evidence(&mut evidence, AbilitySet::NONE, 9);
        record_positive_loadout_evidence(&mut evidence, AbilitySet::NONE, 3);
        record_positive_loadout_evidence(&mut evidence, AbilitySet::new(false, true), 7);
        record_positive_loadout_evidence(&mut evidence, AbilitySet::NONE, 5);
        let mut audit = test_audit(true, ComplexityBand::Standard);
        audit.positive_loadout_evidence = evidence;

        assert_eq!(audit.positive_loadout_mask(), 0b0101);
        assert_eq!(
            evidence[loadout_index(AbilitySet::NONE)],
            Some(PositiveLoadoutEvidence {
                successes: 3,
                canonical_witness_fingerprint: 3,
            })
        );
        assert!(audit.has_positive_without_wall_jump());
        assert!(audit.has_positive_without_dash());
    }

    #[test]
    fn route_demand_separates_run_only_and_structurally_complex_routes() {
        let run = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let easy = RouteDemand::observe(
            &difficulty_report(0, 0, 0, (1, 1), 20),
            &action_trace(vec![run], 0),
            &traversal_trace(&[(1, 8), (12, 8)]),
        );
        assert!(easy.run_only);
        assert_eq!(easy.band, ComplexityBand::Gentle);

        let reverse_jump = SemanticAction {
            move_x: -1,
            jump_held: true,
            ..SemanticAction::default()
        };
        let complex = RouteDemand::observe(
            &difficulty_report(3, 0, 0, (1, 2), 2),
            &action_trace(vec![run, reverse_jump, run, reverse_jump], 3),
            &traversal_trace(&[(1, 8), (6, 5), (10, 2), (7, 5), (12, 8)]),
        );
        assert!(!complex.monotone_simple_controller);
        assert!(complex.pressure_signals >= 2);
        assert_eq!(complex.band, ComplexityBand::Technical);
    }

    #[test]
    fn readable_single_jump_is_an_engaging_gentle_route() {
        let run = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let jump = SemanticAction {
            jump_held: true,
            ..run
        };
        let demand = RouteDemand::observe(
            &difficulty_report(1, 0, 0, (1, 1), 20),
            &action_trace(vec![run, jump, run], 1),
            &traversal_trace(&[(1, 8), (5, 6), (9, 8)]),
        );

        assert!(demand.nontrivial_traversal);
        assert!(demand.monotone_simple_controller);
        assert!(demand.engaging_gentle);
        assert_eq!(demand.band, ComplexityBand::Gentle);
        assert!(demand.representative_eligible(ComplexityBand::Gentle, AbilityTier::Baseline));
    }

    #[test]
    fn advanced_technical_representatives_must_use_the_corresponding_ability() {
        let mut demand = test_demand(ComplexityBand::Technical);
        assert!(!demand.representative_eligible(ComplexityBand::Technical, AbilityTier::WallJump));
        assert!(!demand.representative_eligible(ComplexityBand::Technical, AbilityTier::Dash));
        assert!(
            !demand
                .representative_eligible(ComplexityBand::Technical, AbilityTier::WallJumpAndDash)
        );

        demand.successful_wall_jumps = 1;
        assert!(demand.representative_eligible(ComplexityBand::Technical, AbilityTier::WallJump));
        assert!(
            demand.representative_eligible(ComplexityBand::Technical, AbilityTier::WallJumpAndDash)
        );
        assert!(!demand.representative_eligible(ComplexityBand::Technical, AbilityTier::Dash));
    }

    #[test]
    fn advanced_representatives_must_use_an_unavoidable_unbypassed_ability() {
        let make_route = |band, demand, difficulty, structural| RouteRecord {
            source_door_id: "source".to_owned(),
            target_door_id: "target".to_owned(),
            witness_fingerprint: 1,
            canonical_witness_fingerprint: 1,
            difficulty,
            traversal: traversal_trace(&[(1, 8), (12, 2)]),
            actions: action_trace(Vec::new(), 0),
            demand,
            structural,
            audit: test_audit(true, band),
        };

        let mut wall_demand = test_demand(ComplexityBand::Technical);
        wall_demand.successful_wall_jumps = 1;
        let no_structural_gate = make_route(
            ComplexityBand::Technical,
            wall_demand,
            difficulty_report(1, 1, 0, (1, 1), 10),
            test_structural(AbilitySet::NONE),
        );
        assert!(
            !no_structural_gate
                .representative_eligible(ComplexityBand::Technical, AbilityTier::WallJump)
        );

        let structural_wall = make_route(
            ComplexityBand::Technical,
            wall_demand,
            difficulty_report(1, 1, 0, (1, 1), 10),
            test_structural(AbilitySet::new(true, false)),
        );
        assert!(
            structural_wall
                .representative_eligible(ComplexityBand::Technical, AbilityTier::WallJump)
        );

        let mut dash_demand = test_demand(ComplexityBand::Standard);
        dash_demand.successful_dashes = 1;
        let structural_dash = make_route(
            ComplexityBand::Standard,
            dash_demand,
            difficulty_report(0, 0, 1, (1, 1), 10),
            test_structural(AbilitySet::new(false, true)),
        );
        assert!(
            structural_dash.representative_eligible(ComplexityBand::Standard, AbilityTier::Dash)
        );

        let mut baseline_one_edge = make_route(
            ComplexityBand::Technical,
            test_demand(ComplexityBand::Technical),
            difficulty_report(3, 0, 0, (1, 2), 2),
            test_structural(AbilitySet::NONE),
        );
        baseline_one_edge.structural.cost.edge_count = 1;
        assert!(
            !baseline_one_edge
                .representative_eligible(ComplexityBand::Technical, AbilityTier::Baseline)
        );
    }

    #[test]
    fn exact_smaller_loadout_successes_veto_ability_requirement_claims() {
        let make_route = |positive_loadout| {
            let band = ComplexityBand::Standard;
            let mut demand = test_demand(band);
            demand.successful_wall_jumps = 1;
            demand.successful_dashes = 1;
            let mut audit = test_audit(true, band);
            audit.audited_loadouts = 4;
            audit.expected_loadouts = 4;
            audit.audited_loadout_mask = 0b1111;
            audit.expected_loadout_mask = 0b1111;
            record_positive_loadout_evidence(
                &mut audit.positive_loadout_evidence,
                positive_loadout,
                17,
            );
            audit.direct_probe_successes = 1;
            audit.lower_loadout_successes = 1;
            RouteRecord {
                source_door_id: "source".to_owned(),
                target_door_id: "target".to_owned(),
                witness_fingerprint: 1,
                canonical_witness_fingerprint: 1,
                difficulty: difficulty_report(2, 1, 1, (1, 1), 10),
                traversal: traversal_trace(&[(1, 8), (12, 2)]),
                actions: action_trace(Vec::new(), 0),
                demand,
                structural: test_structural(AbilitySet::ALL),
                audit,
            }
        };

        let baseline_bypass = make_route(AbilitySet::NONE);
        assert!(!baseline_bypass.supports_wall_jump_requirement());
        assert!(!baseline_bypass.supports_dash_requirement());
        assert!(!route_uses_and_requires_wall(&baseline_bypass));
        assert!(!route_uses_and_requires_dash(&baseline_bypass));
        assert!(
            !baseline_bypass
                .representative_eligible(ComplexityBand::Standard, AbilityTier::WallJumpAndDash)
        );

        let wall_only_bypass = make_route(AbilitySet::new(true, false));
        assert!(wall_only_bypass.supports_wall_jump_requirement());
        assert!(!wall_only_bypass.supports_dash_requirement());
        assert!(route_uses_and_requires_wall(&wall_only_bypass));
        assert!(!route_uses_and_requires_dash(&wall_only_bypass));

        let dash_only_bypass = make_route(AbilitySet::new(false, true));
        assert!(!dash_only_bypass.supports_wall_jump_requirement());
        assert!(dash_only_bypass.supports_dash_requirement());
        assert!(!route_uses_and_requires_wall(&dash_only_bypass));
        assert!(route_uses_and_requires_dash(&dash_only_bypass));
    }

    #[test]
    fn band_intent_alignment_prefers_exact_then_adjacent_profiles() {
        assert_eq!(
            intent_mismatch(ChallengeIntent::Technical, ComplexityBand::Technical),
            0
        );
        assert_eq!(
            intent_mismatch(ChallengeIntent::Standard, ComplexityBand::Technical),
            1
        );
        assert_eq!(
            intent_mismatch(ChallengeIntent::Gentle, ComplexityBand::Technical),
            2
        );
    }

    #[test]
    fn technical_ranking_prefers_route_specific_pressure() {
        let operationally_safe = RouteQuality {
            robustness_numerator: 1,
            robustness_denominator: 1,
            hazard_clearance: 30,
            simulated_ticks: 100,
            expanded_nodes: 10,
        };
        let pressured = RouteQuality {
            robustness_numerator: 1,
            robustness_denominator: 2,
            hazard_clearance: 2,
            simulated_ticks: 200,
            expanded_nodes: 20,
        };
        let mut safe_preference = test_preference(ComplexityBand::Technical, operationally_safe);
        safe_preference.demand.pressure_signals = 2;
        let mut pressure_preference = test_preference(ComplexityBand::Technical, pressured);
        pressure_preference.demand.pressure_signals = 4;
        pressure_preference.demand.horizontal_reversals = 2;
        pressure_preference.demand.visited_vertical_span_cells = 6;

        assert_eq!(
            compare_route_preference(pressure_preference, safe_preference),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn route_manifest_diagnostics_serialize_demand_evidence() {
        let run = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let jump = SemanticAction {
            jump_held: true,
            ..run
        };
        let actions = action_trace(vec![run, jump, run], 1);
        let traversal = traversal_trace(&[(1, 8), (5, 6), (9, 8)]);
        let difficulty = difficulty_report(1, 0, 0, (3, 4), 4);
        let demand = RouteDemand::observe(&difficulty, &actions, &traversal);
        let mut route = RouteRecord {
            source_door_id: "source".to_owned(),
            target_door_id: "target".to_owned(),
            witness_fingerprint: 7,
            canonical_witness_fingerprint: 7,
            difficulty,
            traversal,
            actions,
            demand,
            structural: test_structural(AbilitySet::NONE),
            audit: EasiestRouteAudit {
                version: EASIEST_ROUTE_AUDIT_VERSION,
                direct_probe_version: DIRECT_PROBE_AUDIT_VERSION,
                candidate_witnesses: 1,
                direct_probe_successes: 1,
                audited_loadouts: 1,
                expected_loadouts: 1,
                audited_loadout_mask: loadout_bit(AbilitySet::NONE),
                expected_loadout_mask: loadout_bit(AbilitySet::NONE),
                budget_limited_audits: 0,
                lower_loadout_successes: 0,
                positive_loadout_evidence: [None; 4],
                canonical_band: demand.band,
                easiest_origin: RouteWitnessOrigin::Canonical,
                easiest_discovery_loadout: AbilitySet::NONE,
            },
        };
        record_positive_loadout_evidence(
            &mut route.audit.positive_loadout_evidence,
            AbilitySet::NONE,
            11,
        );
        let mut rendered = String::new();
        render_route(&mut rendered, 0, &route);

        assert!(rendered.contains("band=gentle"));
        assert!(rendered.contains("easiest-known=true"));
        assert!(rendered.contains("audit-status=complete"));
        assert!(rendered.contains("candidate-witnesses=1"));
        assert!(rendered.contains("easiest-origin=canonical"));
        assert!(rendered.contains("demand-score="));
        assert!(rendered.contains("pressure-signals="));
        assert!(rendered.contains("monotone-simple-controller=1"));
        assert!(rendered.contains("horizontal-reversals=0"));
        assert!(rendered.contains("visited-vertical-span-cells=2"));
        assert!(rendered.contains("vertical-travel-cells=4"));
        assert!(rendered.contains("unique-semantic-actions=2"));
        assert!(rendered.contains("positive-discovery-loadout-mask=1"));
        assert!(rendered.contains(&format!(
            "positive-discovery-loadout-evidence=\"baseline:1:downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-000000000000000b\""
        )));
        assert!(rendered.contains("structural-descriptor-version=2"));
        assert!(rendered.contains("structural-cost=0:2:1:1:2"));
        assert!(rendered.contains("structural-selected-path-wall=0"));
        assert!(rendered.contains("structural-unavoidable-wall=0"));
        assert!(rendered.contains("supported-required-wall=0"));
    }

    #[test]
    fn mrv_closes_the_rare_socket_before_a_rank_first_wide_branch() {
        let common_socket = BranchConstraint::Socket(DoorSocket {
            side: BoundarySide::Left,
            offset: 24,
            span: 16,
        });
        let rare_socket = BranchConstraint::Socket(DoorSocket {
            side: BoundarySide::Floor,
            offset: 96,
            span: 16,
        });
        let constraints = [BranchConstraint::WallAbility, common_socket, rare_socket];

        // A rank-first search could inspect the 200 broad socket branches (or
        // 50 ability branches) before ever choosing the sole room that closes
        // the rare floor socket. MRV makes that required closure the branch.
        assert_eq!(
            choose_mrv_constraint(&constraints, &[50, 200, 1]),
            Some(rare_socket)
        );
    }

    #[test]
    fn manifest_bytes_and_fingerprint_are_repeatable() {
        let body = "downwards-curation-manifest-v1\nroom-begin seed=7\nroom-end\n".to_owned();
        let first = append_manifest_fingerprint(body.clone()).unwrap();
        let second = append_manifest_fingerprint(body).unwrap();
        assert_eq!(first, second);
        assert!(first.ends_with('\n'));
        assert_eq!(first.lines().count(), 4);
    }

    #[test]
    fn manifest_quoting_is_unambiguous() {
        assert_eq!(quoted("door \\\"a\\\"\n"), "\"door \\\\\\\"a\\\\\\\"\\n\"");
    }
}
