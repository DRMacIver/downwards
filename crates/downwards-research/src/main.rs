use std::{
    collections::{BTreeMap, HashSet},
    env,
    error::Error,
    process::ExitCode,
};

use downwards_ai::{ComplexityBand, DifficultyReport, InconclusiveReason, Solution};
use downwards_core::{BoundarySide, Simulation};
use downwards_gen::{
    AbilityTier,
    experimental::{
        ChallengeIntent, ExperimentalCandidate, GenerationStrategy, generate_candidate,
    },
};
use downwards_lab::{
    CollisionTopologyDescriptor, SemanticActionTrace, StaticVisualDescriptor, TraversalTrace,
    collision_topology_distance, observe_solution, semantic_action_distance,
    static_visual_distance, traversal_distance,
};
use downwards_validation::{
    DoorReachabilityObjective, DoorValidationError, PickupFromDoorObjective,
    PickupFromDoorValidationError, validate_generated_door_reachability,
    validate_generated_pickup_from_door,
};

pub mod corpus;
mod curation;
pub mod structural;

const BEHAVIOR_OBSERVATION_CAP: usize = 256;

const USAGE: &str = "\
Downwards procedural-generation research harness

USAGE:
    downwards-research sweep <start-seed> <count> [strategy|all] [tier|all] [intent|all]
    downwards-research curate <start-seed> <seeds-per-stratum> <quota-per-band> <tier>
    downwards-research corpus pilot <start-seed> <seed-count> <output-directory>
    downwards-research corpus scan <start-seed> <seed-count>
    downwards-research corpus verify <artifact-directory>
    downwards-research corpus verify-shards <shard-root> <start-seed> <seed-count>
    downwards-research corpus inconclusive-audit <start-seed> <seed-count> <new-output-directory>
    downwards-research corpus socket-audit <shard-root> <start-seed> <seed-count> <minimum> <maximum>
    downwards-research corpus socket-coverage <shard-root> <start-seed> <seed-count>
    downwards-research corpus visual-audit <start-seed> <seed-count> <output-directory>
    downwards-research corpus deep-pilot <start-seed> <seed-count> <room-limit>
    downwards-research corpus deep-shard <seed> <new-output-directory>
    downwards-research corpus verify-deep-shard <artifact-directory>

STRATEGIES: cyclic | growth | rhythm
TIERS:      baseline | wall | dash | both
INTENTS:    gentle | standard | technical
";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let [command, tail @ ..] = arguments.as_slice() else {
        return Err(USAGE.into());
    };
    if command == "corpus" {
        return corpus::run_cli(tail);
    }
    if command == "curate" {
        let manifest = curation::curate(tail)?;
        print!("{manifest}");
        return Ok(());
    }
    let [start, count, tail @ ..] = tail else {
        return Err(USAGE.into());
    };
    if command != "sweep" || tail.len() > 3 {
        return Err(USAGE.into());
    }
    let start = start.parse::<u64>()?;
    let count = count.parse::<usize>()?;
    if count == 0 {
        return Err("count must be positive".into());
    }
    let strategies = parse_strategies(tail.first().map(String::as_str).unwrap_or("all"))?;
    let tiers = parse_tiers(tail.get(1).map(String::as_str).unwrap_or("all"))?;
    let intents = parse_intents(tail.get(2).map(String::as_str).unwrap_or("all"))?;

    println!("research-format=2 start={start} count={count}");
    for strategy in strategies {
        for tier in &tiers {
            for intent in &intents {
                let aggregate = sweep_group(start, count, strategy, *tier, *intent);
                aggregate.print();
            }
        }
    }
    Ok(())
}

fn sweep_group(
    start_seed: u64,
    count: usize,
    strategy: GenerationStrategy,
    tier: AbilityTier,
    intent: ChallengeIntent,
) -> Aggregate {
    let mut aggregate = Aggregate::new(strategy, tier, intent, count);
    for offset in 0..count {
        let seed = start_seed.wrapping_add(offset as u64);
        match generate_candidate(seed, tier.abilities(), strategy, intent) {
            Ok(candidate) => aggregate.observe_candidate(candidate),
            Err(_) => aggregate.reject("generation"),
        }
    }
    aggregate
}

#[derive(Clone)]
struct SolvedObservation {
    visual: StaticVisualDescriptor,
    collision: CollisionTopologyDescriptor,
    traversal: TraversalTrace,
    actions: SemanticActionTrace,
}

struct Aggregate {
    strategy: GenerationStrategy,
    tier: AbilityTier,
    intent: ChallengeIntent,
    attempted: usize,
    constructed: usize,
    accepted: usize,
    door_routes_expected: usize,
    door_routes_certified: usize,
    all_door_pairs_certified: usize,
    pickup_routes_expected: usize,
    pickup_routes_certified: usize,
    all_pickup_routes_certified: usize,
    port_counts: BTreeMap<usize, usize>,
    exact_visuals: HashSet<StaticVisualDescriptor>,
    collision_topologies: HashSet<CollisionTopologyDescriptor>,
    route_signatures: HashSet<u64>,
    cycle_ranks: BTreeMap<u16, usize>,
    difficulty: DifficultyAggregate,
    directions: BTreeMap<Direction, DirectionAggregate>,
    observations: Vec<SolvedObservation>,
    rejections: BTreeMap<&'static str, usize>,
}

impl Aggregate {
    fn new(
        strategy: GenerationStrategy,
        tier: AbilityTier,
        intent: ChallengeIntent,
        attempted: usize,
    ) -> Self {
        Self {
            strategy,
            tier,
            intent,
            attempted,
            constructed: 0,
            accepted: 0,
            door_routes_expected: 0,
            door_routes_certified: 0,
            all_door_pairs_certified: 0,
            pickup_routes_expected: 0,
            pickup_routes_certified: 0,
            all_pickup_routes_certified: 0,
            port_counts: BTreeMap::new(),
            exact_visuals: HashSet::new(),
            collision_topologies: HashSet::new(),
            route_signatures: HashSet::new(),
            cycle_ranks: BTreeMap::new(),
            difficulty: DifficultyAggregate::default(),
            directions: BTreeMap::new(),
            observations: Vec::new(),
            rejections: BTreeMap::new(),
        }
    }

    fn observe_candidate(&mut self, candidate: ExperimentalCandidate) {
        self.constructed += 1;
        let visual = StaticVisualDescriptor::from_room(&candidate.generated.room);
        let collision = CollisionTopologyDescriptor::from_room(&candidate.generated.room);
        self.exact_visuals.insert(visual.clone());
        self.collision_topologies.insert(collision.clone());
        self.route_signatures
            .insert(candidate.route_summary.signature);
        *self
            .cycle_ranks
            .entry(candidate.route_summary.cycle_rank)
            .or_default() += 1;

        let mut doors = candidate
            .generated
            .room
            .doors()
            .iter()
            .map(|door| (door.id.clone(), door.side))
            .collect::<Vec<_>>();
        doors.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let door_count = doors.len();
        *self.port_counts.entry(door_count).or_default() += 1;
        let pair_count = door_count.saturating_mul(door_count.saturating_sub(1));
        self.door_routes_expected += pair_count;
        let mut all_door_pairs = door_count >= 2;
        if !all_door_pairs {
            self.reject("door:topology");
        }

        for (source_id, source_side) in &doors {
            for (target_id, target_side) in &doors {
                if source_id == target_id {
                    continue;
                }
                let direction = Direction::new(*source_side, *target_side);
                self.directions.entry(direction).or_default().expected += 1;
                let objective = DoorReachabilityObjective::for_generated(
                    &candidate.generated,
                    source_id,
                    target_id,
                );
                match validate_generated_door_reachability(candidate.generated.clone(), objective) {
                    Ok(certificate) => {
                        self.door_routes_certified += 1;
                        self.difficulty.observe(certificate.difficulty());
                        let direction_aggregate = self.directions.entry(direction).or_default();
                        direction_aggregate.certified += 1;
                        direction_aggregate
                            .difficulty
                            .observe(certificate.difficulty());
                        self.observe_behavior(
                            &candidate,
                            source_id,
                            target_id,
                            certificate.solution(),
                            &visual,
                            &collision,
                        );
                    }
                    Err(error) => {
                        all_door_pairs = false;
                        self.reject(classify_door_error(&error));
                    }
                }
            }
        }
        if all_door_pairs {
            self.all_door_pairs_certified += 1;
        }

        let pickup_ids = candidate
            .generated
            .room
            .pickups()
            .iter()
            .map(|pickup| pickup.id().to_owned())
            .collect::<Vec<_>>();
        self.pickup_routes_expected += door_count * pickup_ids.len();
        let mut all_pickup_routes = true;
        for (source_id, _) in &doors {
            for pickup_id in &pickup_ids {
                let objective = PickupFromDoorObjective::for_generated(
                    &candidate.generated,
                    source_id,
                    pickup_id,
                );
                match validate_generated_pickup_from_door(candidate.generated.clone(), objective) {
                    Ok(_) => self.pickup_routes_certified += 1,
                    Err(error) => {
                        all_pickup_routes = false;
                        self.reject(classify_pickup_error(&error));
                    }
                }
            }
        }
        if all_pickup_routes {
            self.all_pickup_routes_certified += 1;
        }
        if all_door_pairs && all_pickup_routes {
            self.accepted += 1;
        }
    }

    fn observe_behavior(
        &mut self,
        candidate: &ExperimentalCandidate,
        source_door_id: &str,
        target_door_id: &str,
        target_solution: &downwards_ai::TargetSolution,
        visual: &StaticVisualDescriptor,
        collision: &CollisionTopologyDescriptor,
    ) {
        // Pairwise behavior distance is quadratic. Taking the first witnesses
        // in seed and canonical door-pair order makes the cap reproducible.
        if self.observations.len() >= BEHAVIOR_OBSERVATION_CAP {
            return;
        }
        let initial = match Simulation::enter_via_door(
            candidate.generated.room.clone(),
            candidate.generated.metadata.intended_abilities,
            source_door_id,
        ) {
            Ok(initial) => initial,
            Err(_) => {
                self.reject("observation:entry");
                return;
            }
        };
        let solution = Solution {
            exit_id: target_door_id.to_owned(),
            replay: target_solution.replay.clone(),
            stats: target_solution.stats,
        };
        match observe_solution(&initial, &solution, downwards_lab::TraversalGrid::default()) {
            Ok(observation) => self.observations.push(SolvedObservation {
                visual: visual.clone(),
                collision: collision.clone(),
                traversal: observation.traversal,
                actions: observation.actions,
            }),
            Err(_) => self.reject("observation:replay"),
        }
    }

    fn reject(&mut self, class: &'static str) {
        *self.rejections.entry(class).or_default() += 1;
    }

    fn print(&self) {
        let distances = PairwiseSummary::from_observations(&self.observations);
        println!(
            "\nstrategy={} tier={:?} intent={:?} attempted={}",
            self.strategy.slug(),
            self.tier,
            self.intent,
            self.attempted,
        );
        println!(
            "construct={}/{} accepted={}/{} accepted-rate={} ports={:?}",
            self.constructed,
            self.attempted,
            self.accepted,
            self.constructed,
            ratio(self.accepted, self.constructed),
            self.port_counts,
        );
        println!(
            "door-routes={}/{} route-rate={} all-pairs-rooms={}/{} all-pairs-rate={} pickup-routes={}/{} pickup-route-rate={} all-pickups-rooms={}/{} all-pickups-rate={}",
            self.door_routes_certified,
            self.door_routes_expected,
            ratio(self.door_routes_certified, self.door_routes_expected),
            self.all_door_pairs_certified,
            self.constructed,
            ratio(self.all_door_pairs_certified, self.constructed),
            self.pickup_routes_certified,
            self.pickup_routes_expected,
            ratio(self.pickup_routes_certified, self.pickup_routes_expected),
            self.all_pickup_routes_certified,
            self.constructed,
            ratio(self.all_pickup_routes_certified, self.constructed),
        );
        println!(
            "visuals={} collision-topologies={} route-signatures={} cycles={:?}",
            self.exact_visuals.len(),
            self.collision_topologies.len(),
            self.route_signatures.len(),
            self.cycle_ranks,
        );
        println!("door-route-difficulty={}", self.difficulty.summary(),);
        println!(
            "behavior-witnesses={}/{} pairwise-comparisons={} visual-distance={} collision-distance={} traversal-distance={} action-distance={}",
            self.observations.len(),
            BEHAVIOR_OBSERVATION_CAP,
            distances.pairs,
            distribution_f64(&distances.visual),
            distribution_f64(&distances.collision),
            distribution_f64(&distances.traversal),
            distribution_f64(&distances.actions),
        );
        for (direction, aggregate) in &self.directions {
            println!(
                "direction={} routes={}/{} rate={} difficulty={}",
                direction,
                aggregate.certified,
                aggregate.expected,
                ratio(aggregate.certified, aggregate.expected),
                aggregate.difficulty.summary(),
            );
        }
        if !self.rejections.is_empty() {
            println!("rejections={:?}", self.rejections);
        }
    }
}

#[derive(Default)]
struct DifficultyAggregate {
    reports: usize,
    bands: [usize; 3],
    completion_ticks: Vec<usize>,
    robustness: Vec<f64>,
    wall_witnesses: usize,
    dash_witnesses: usize,
}

impl DifficultyAggregate {
    fn observe(&mut self, report: &DifficultyReport) {
        self.reports += 1;
        self.bands[band_index(report.provisional_complexity.band)] += 1;
        self.completion_ticks.push(report.completion_ticks);
        if let Some(ratio) = report.temporal_robustness.successful_perturbation_ratio {
            self.robustness.push(ratio);
        }
        self.wall_witnesses += usize::from(report.successful_wall_jumps > 0);
        self.dash_witnesses += usize::from(report.successful_dashes > 0);
    }

    fn summary(&self) -> String {
        format!(
            "reports:{} bands=gentle:{} standard:{} technical:{} witnesses=wall:{} dash:{} completion-ticks={} robustness={}",
            self.reports,
            self.bands[0],
            self.bands[1],
            self.bands[2],
            self.wall_witnesses,
            self.dash_witnesses,
            distribution(&self.completion_ticks),
            distribution_f64(&self.robustness),
        )
    }
}

#[derive(Default)]
struct DirectionAggregate {
    expected: usize,
    certified: usize,
    difficulty: DifficultyAggregate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Left,
    Right,
    Ceiling,
    Floor,
}

impl From<BoundarySide> for Side {
    fn from(side: BoundarySide) -> Self {
        match side {
            BoundarySide::Left => Self::Left,
            BoundarySide::Right => Self::Right,
            BoundarySide::Ceiling => Self::Ceiling,
            BoundarySide::Floor => Self::Floor,
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Ceiling => "ceiling",
            Self::Floor => "floor",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Direction {
    source: Side,
    target: Side,
}

impl Direction {
    fn new(source: BoundarySide, target: BoundarySide) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
        }
    }
}

impl std::fmt::Display for Direction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}->{}", self.source, self.target)
    }
}

#[derive(Default)]
struct PairwiseSummary {
    pairs: usize,
    visual: Vec<f64>,
    collision: Vec<f64>,
    traversal: Vec<f64>,
    actions: Vec<f64>,
}

impl PairwiseSummary {
    fn from_observations(observations: &[SolvedObservation]) -> Self {
        let mut result = Self::default();
        for left in 0..observations.len() {
            for right in left + 1..observations.len() {
                let left = &observations[left];
                let right = &observations[right];
                result.pairs += 1;
                result
                    .visual
                    .push(static_visual_distance(&left.visual, &right.visual).combined);
                result
                    .collision
                    .push(collision_topology_distance(&left.collision, &right.collision).combined);
                result
                    .traversal
                    .push(traversal_distance(&left.traversal, &right.traversal).combined);
                result
                    .actions
                    .push(semantic_action_distance(&left.actions, &right.actions).combined);
            }
        }
        result
    }
}

fn distribution(values: &[usize]) -> String {
    let mut values = values.to_vec();
    values.sort_unstable();
    if values.is_empty() {
        return "n/a".to_owned();
    }
    format!(
        "min:{} p50:{} p90:{} max:{} mean:{:.2}",
        values[0],
        percentile(&values, 50),
        percentile(&values, 90),
        values[values.len() - 1],
        values.iter().sum::<usize>() as f64 / values.len() as f64,
    )
}

fn distribution_f64(values: &[f64]) -> String {
    let mut values = values.to_vec();
    values.sort_unstable_by(f64::total_cmp);
    if values.is_empty() {
        return "n/a".to_owned();
    }
    format!(
        "min:{:.3} p50:{:.3} p90:{:.3} max:{:.3} mean:{:.3}",
        values[0],
        percentile(&values, 50),
        percentile(&values, 90),
        values[values.len() - 1],
        values.iter().sum::<f64>() / values.len() as f64,
    )
}

fn percentile<T: Copy>(sorted: &[T], percentile: usize) -> T {
    sorted[(sorted.len() - 1) * percentile / 100]
}

fn ratio(successes: usize, total: usize) -> String {
    if total == 0 {
        return "n/a".to_owned();
    }
    format!("{:.1}%", successes as f64 * 100.0 / total as f64)
}

const fn classify_door_error(error: &DoorValidationError) -> &'static str {
    match error {
        DoorValidationError::FewerThanTwoDoors { .. }
        | DoorValidationError::NonCanonicalDoorId { .. }
        | DoorValidationError::DuplicateDoorId { .. }
        | DoorValidationError::EmptySourceDoorId
        | DoorValidationError::EmptyTargetDoorId
        | DoorValidationError::SameSourceAndTarget { .. }
        | DoorValidationError::SourceDoorNotDefined { .. }
        | DoorValidationError::TargetDoorNotDefined { .. }
        | DoorValidationError::MetadataMismatch { .. }
        | DoorValidationError::ObjectiveLoadoutMismatch { .. }
        | DoorValidationError::MetadataAbilityTierMismatch { .. } => "door:contract",
        DoorValidationError::EntryRejected(_) => "door:entry",
        DoorValidationError::SolverConfiguration(_) => "door:solver-config",
        DoorValidationError::SearchTargetRejected(_) => "door:target",
        DoorValidationError::Inconclusive { reason, .. } => door_inconclusive_class(*reason),
        DoorValidationError::WrongReachedTarget { .. } => "door:wrong-target",
        DoorValidationError::ReplayDiverged(_)
        | DoorValidationError::ReplayDidNotReachTargetDoor { .. } => "door:replay",
        DoorValidationError::DifficultyAnalysis(_) => "door:difficulty",
        DoorValidationError::ComplexityBandMismatch { .. } => "door:band",
    }
}

const fn classify_pickup_error(error: &PickupFromDoorValidationError) -> &'static str {
    match error {
        PickupFromDoorValidationError::DoorTopology(_) => "pickup:door-contract",
        PickupFromDoorValidationError::PickupObjective(_) => "pickup:objective",
        PickupFromDoorValidationError::EntryRejected(_) => "pickup:entry",
        PickupFromDoorValidationError::SolverConfiguration(_) => "pickup:solver-config",
        PickupFromDoorValidationError::SearchTargetRejected(_) => "pickup:target",
        PickupFromDoorValidationError::Inconclusive { reason, .. } => {
            pickup_inconclusive_class(*reason)
        }
        PickupFromDoorValidationError::WrongReachedTarget { .. } => "pickup:wrong-target",
        PickupFromDoorValidationError::ReplayDiverged(_)
        | PickupFromDoorValidationError::ReplayDidNotCollectRequiredPickup { .. } => {
            "pickup:replay"
        }
    }
}

const fn door_inconclusive_class(reason: InconclusiveReason) -> &'static str {
    match reason {
        InconclusiveReason::NoExitsDefined => "door:inconclusive:no-exits",
        InconclusiveReason::ExpandedNodeBudget => "door:inconclusive:node-budget",
        InconclusiveReason::SimulatedTickBudget => "door:inconclusive:tick-budget",
        InconclusiveReason::PathHorizon => "door:inconclusive:path-horizon",
        InconclusiveReason::FrontierExhausted => "door:inconclusive:frontier-exhausted",
    }
}

const fn pickup_inconclusive_class(reason: InconclusiveReason) -> &'static str {
    match reason {
        InconclusiveReason::NoExitsDefined => "pickup:inconclusive:no-exits",
        InconclusiveReason::ExpandedNodeBudget => "pickup:inconclusive:node-budget",
        InconclusiveReason::SimulatedTickBudget => "pickup:inconclusive:tick-budget",
        InconclusiveReason::PathHorizon => "pickup:inconclusive:path-horizon",
        InconclusiveReason::FrontierExhausted => "pickup:inconclusive:frontier-exhausted",
    }
}

const fn band_index(band: ComplexityBand) -> usize {
    match band {
        ComplexityBand::Gentle => 0,
        ComplexityBand::Standard => 1,
        ComplexityBand::Technical => 2,
    }
}

fn parse_strategies(value: &str) -> Result<Vec<GenerationStrategy>, String> {
    match value {
        "all" => Ok(GenerationStrategy::ALL.to_vec()),
        "cyclic" => Ok(vec![GenerationStrategy::CyclicGraph]),
        "growth" => Ok(vec![GenerationStrategy::ReachabilityGrowth]),
        "rhythm" => Ok(vec![GenerationStrategy::RhythmWeave]),
        _ => Err(format!("unknown strategy {value:?}\n\n{USAGE}")),
    }
}

fn parse_tiers(value: &str) -> Result<Vec<AbilityTier>, String> {
    match value {
        "all" => Ok(vec![
            AbilityTier::Baseline,
            AbilityTier::WallJump,
            AbilityTier::Dash,
            AbilityTier::WallJumpAndDash,
        ]),
        "baseline" => Ok(vec![AbilityTier::Baseline]),
        "wall" => Ok(vec![AbilityTier::WallJump]),
        "dash" => Ok(vec![AbilityTier::Dash]),
        "both" => Ok(vec![AbilityTier::WallJumpAndDash]),
        _ => Err(format!("unknown tier {value:?}\n\n{USAGE}")),
    }
}

fn parse_intents(value: &str) -> Result<Vec<ChallengeIntent>, String> {
    match value {
        "all" => Ok(ChallengeIntent::ALL.to_vec()),
        "gentle" => Ok(vec![ChallengeIntent::Gentle]),
        "standard" => Ok(vec![ChallengeIntent::Standard]),
        "technical" => Ok(vec![ChallengeIntent::Technical]),
        _ => Err(format!("unknown intent {value:?}\n\n{USAGE}")),
    }
}
