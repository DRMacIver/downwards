//! Strict runtime loading for offline-curated Downwards room catalogues.
//!
//! A catalogue is data, not authority: loading verifies its checksum, stable
//! ordering, an explicitly supported policy/configuration identity, v6 regeneration key,
//! boundary sockets, visual fingerprint, and the curator's compact
//! representative replays. Complete route and pickup matrices are checked for
//! canonical coverage and regenerated object identity. Only their selected
//! representative rows carry action streams, so non-representative metrics
//! remain checksum-bound offline evidence rather than runtime replay claims.

#![forbid(unsafe_code)]

mod corpus_playtest;

pub use corpus_playtest::*;

use std::{collections::HashSet, error::Error, fmt};

use downwards_ai::{
    DIFFICULTY_HEURISTIC_VERSION, DifficultyConfig, SOLVER_POLICY_VERSION, SolverConfig,
};
use downwards_core::{
    AbilitySet, Action, BoundarySide, DoorSocket, JumpKind, Rect, Room, Simulation,
    SimulationEvent, Tile,
};
use downwards_gen::{
    AbilityTier, COMPOSITIONAL_GENERATION_VERSION, CompositionalKey, CompositionalProfile,
    experimental::{ChallengeIntent, EXPERIMENTAL_GENERATION_VERSION, GenerationStrategy},
    generate_compositional,
};
use downwards_validation::WITNESS_FINGERPRINT_VERSION;

const MANIFEST_VERSION: u32 = 3;
const SELECTION_VERSION: u32 = 5;
const VISUAL_DESCRIPTOR_VERSION: u32 = 2;
const COLLISION_TOPOLOGY_DESCRIPTOR_VERSION: u32 = 1;
const VISUAL_FINGERPRINT_VERSION: u32 = 1;
const CONFIG_FINGERPRINT_VERSION: u32 = 1;
// The checked-in v6 catalogue is a historical, replay-backed fixture. Solver
// policy v3 adds a direct probe but does not change simulation or the numeric
// macro configuration. Supporting v2 here is explicit compatibility, not an
// assertion that its checksum-bound non-representative metrics were produced
// by the current solver. Every stored representative is still regenerated and
// replayed exactly below.
const OLDEST_SUPPORTED_SOLVER_POLICY_VERSION: u32 = 2;
const ROUTE_BAND_POLICY_VERSION: u32 = 2;
const ACTION_ENCODING: &str = "semantic-rle-v1";
const ACTION_FIELDS: &str = "move-x:move-y:jump:dash:restart*ticks";
const ROUTE_BAND_POLICY: &str = "route-band-policy version=2 classifier=route-demand gentle-matrix=run-only-or-nontrivial-easy-engaging gentle-representative=monotone-simple,ordinary-jumps-1-to-3,no-wall-or-dash,pressure-at-most-1,robustness-at-least-3/4,hazard-clearance-at-least-8,nontrivial-travel standard=non-run-only-and-nontrivial technical=non-monotone-simple,explicit-controller-decision,pressure-signals-at-least-2 controller-decision=debounced-horizontal-reversal,vertical-both-signs-or-two-post-initial-changes,accepted-dash-direction-change pressure-signals=horizontal-reversal,vertical-demand,unique-actions-5,robustness-at-most-3/4,hazard-clearance-at-most-4,wall-chain-2,dash-chain-2 ordered-source-target=true";
const EASIEST_ROUTE_POLICY: &str = "easiest-route-policy version=2 direct-probe-audit-version=2 challenge-unit=pinned-directed-source-target candidates=canonical-witness-plus-intended-physics-compatible-direct-controller-successes loadouts=all-subsets-of-intended exact-positive-discovery-evidence=retained-before-intended-loadout-replay canonical-discovery-evidence=minimum-witness-fingerprint-per-loadout selection=minimum-observed-route-demand semantic-action-dedup=true ability-requirement=structurally-unavoidable-and-no-positive-success-without-ability every-representative-requires-exact-expected-loadout-mask-and-no-budget-limited-audit=true unrelated-door-pairs=reachability-only non-success-is-not-unreachability-proof=true beam-optimality-claimed=false";
const FAIRNESS_POLICY: &str = "fairness-policy deaths-per-door-route=0 robustness-success-floor=1/4 no-applicable-perturbations=pass all-door-pairs=required all-pickups-from-all-doors=required";
const SELECTION_POLICY_PREFIX: &str = "selection-policy distinct-static-visuals=true distinct-rooms-across-bands=true branching=mrv-unmatched-socket-or-ability rank-priority=intent-alignment,socket-self-closure,socket-mate-inventory,socket-match,ability,strategy,vertical,terrain-uncorroborated-components,terrain-uncorroborated-tiles,intent-coverage,qd,band-aware-route-demand,distance qd=strategy,intent,port-count,cycle-rank-bin,vertical-span-bin route-ranking=gentle-easy-engaging-safe;standard-nontrivial,structural-pressure,safe;technical-controller-decision,pressure,ability,reversal,vertical-demand,control-vocabulary,structural-pressure,robustness-near-1/2,small-clearance,solver-headroom terrain-evidence=route-plan-attribution-or-certified-traversal-proximity absence-from-one-witness-is-not-unreachability=true terrain-hard-threshold=none distance=equal-mean(static-visual,collision-topology,traversal,semantic-action) solver-effort-as-difficulty=false solver-effort-as-operational-quality=true vertical-port-preference=true socket-mate-closure=required representative-ability-policy=";
const SELECTION_POLICY_SUFFIX: &str = " node-budget=250000";

/// Curator-assigned difficulty band for a particular ordered door route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CatalogueBand {
    Gentle,
    Standard,
    Technical,
}

impl CatalogueBand {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Standard => "standard",
            Self::Technical => "technical",
        }
    }
}

/// One run in a compact representative input replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpan {
    pub action: Action,
    pub ticks: usize,
}

/// Run-length encoded actions selected by the offline curator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepresentativeActions {
    total_ticks: usize,
    spans: Box<[ActionSpan]>,
}

impl RepresentativeActions {
    #[must_use]
    pub const fn total_ticks(&self) -> usize {
        self.total_ticks
    }

    #[must_use]
    pub fn spans(&self) -> &[ActionSpan] {
        &self.spans
    }

    /// Expand the compact spans without allocating an intermediate vector.
    pub fn actions(&self) -> impl Iterator<Item = Action> + '_ {
        self.spans
            .iter()
            .flat_map(|span| std::iter::repeat_n(span.action, span.ticks))
    }
}

/// Canonical pickup demonstration retained alongside the door-route replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepresentativePickup {
    source_door_id: String,
    pickup_id: String,
    actions: RepresentativeActions,
}

impl RepresentativePickup {
    #[must_use]
    pub fn source_door_id(&self) -> &str {
        &self.source_door_id
    }

    #[must_use]
    pub fn pickup_id(&self) -> &str {
        &self.pickup_id
    }

    #[must_use]
    pub const fn actions(&self) -> &RepresentativeActions {
        &self.actions
    }
}

/// Stable format and policy identities retained from the manifest header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueIdentity {
    pub manifest_version: u32,
    pub compositional_generation_version: u32,
    pub experimental_backing_version: u32,
    pub selection_version: u32,
    pub solver_policy_version: u32,
    pub solver_config_id: String,
    pub difficulty_heuristic_version: u32,
    pub difficulty_config_id: String,
    pub manifest_fingerprint: String,
}

/// One curated room and its representative ordered-door challenge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueEntry {
    index: usize,
    id: String,
    band: CatalogueBand,
    key: CompositionalKey,
    source_door_id: String,
    target_door_id: String,
    visual_fingerprint: u64,
    successful_wall_jumps: usize,
    successful_dashes: usize,
    door_ids: Box<[String]>,
    pickup_ids: Box<[String]>,
    sockets: Box<[DoorSocket]>,
    representative_actions: Option<RepresentativeActions>,
    representative_pickup: Option<RepresentativePickup>,
}

impl CatalogueEntry {
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn band(&self) -> CatalogueBand {
        self.band
    }

    #[must_use]
    pub const fn key(&self) -> CompositionalKey {
        self.key
    }

    #[must_use]
    pub fn source_door_id(&self) -> &str {
        &self.source_door_id
    }

    #[must_use]
    pub fn target_door_id(&self) -> &str {
        &self.target_door_id
    }

    #[must_use]
    pub const fn visual_fingerprint(&self) -> u64 {
        self.visual_fingerprint
    }

    #[must_use]
    pub const fn successful_wall_jumps(&self) -> usize {
        self.successful_wall_jumps
    }

    #[must_use]
    pub const fn successful_dashes(&self) -> usize {
        self.successful_dashes
    }

    #[must_use]
    pub fn door_ids(&self) -> &[String] {
        &self.door_ids
    }

    #[must_use]
    pub fn sockets(&self) -> &[DoorSocket] {
        &self.sockets
    }

    #[must_use]
    pub const fn representative_actions(&self) -> Option<&RepresentativeActions> {
        self.representative_actions.as_ref()
    }

    #[must_use]
    pub const fn representative_pickup(&self) -> Option<&RepresentativePickup> {
        self.representative_pickup.as_ref()
    }
}

/// A fully checked, single-ability-tier runtime catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueManifest {
    identity: CatalogueIdentity,
    tier: AbilityTier,
    entries: Box<[CatalogueEntry]>,
}

impl CatalogueManifest {
    /// Convenience associated form of [`parse_manifest`].
    pub fn parse(input: &str) -> Result<Self, CatalogueError> {
        parse_manifest(input)
    }

    #[must_use]
    pub fn identity(&self) -> &CatalogueIdentity {
        &self.identity
    }

    #[must_use]
    pub const fn tier(&self) -> AbilityTier {
        self.tier
    }

    #[must_use]
    pub fn entries(&self) -> &[CatalogueEntry] {
        &self.entries
    }

    #[must_use]
    pub fn entry(&self, index: usize) -> Option<&CatalogueEntry> {
        self.entries.get(index)
    }
}

/// A strict catalogue parse or regeneration failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueError {
    line: Option<usize>,
    message: String,
}

impl CatalogueError {
    fn at(line: usize, message: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            message: message.into(),
        }
    }

    fn global(message: impl Into<String>) -> Self {
        Self {
            line: None,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        self.line
    }
}

impl fmt::Display for CatalogueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(formatter, "catalogue line {line}: {}", self.message),
            None => write!(formatter, "catalogue: {}", self.message),
        }
    }
}

impl Error for CatalogueError {}

/// Parse and fully verify one curation-manifest-v2 document.
pub fn parse_manifest(input: &str) -> Result<CatalogueManifest, CatalogueError> {
    let (body, fingerprint_line) = split_and_verify_fingerprint(input)?;
    let mut lines = Lines::new(body);

    lines.exact("downwards-curation-manifest-v3")?;
    expect_scalar_u32(&mut lines, "manifest-version", MANIFEST_VERSION)?;

    let generation = fields(lines.next_with_prefix("compositional-generation-version=")?)?;
    let compositional_generation_version =
        number::<u32>(&generation, "compositional-generation-version")?;
    if compositional_generation_version != COMPOSITIONAL_GENERATION_VERSION {
        return Err(lines.error(format!(
            "requires compositional generation version {COMPOSITIONAL_GENERATION_VERSION}, got {compositional_generation_version}"
        )));
    }
    let experimental_backing_version = number(&generation, "experimental-backing-version")?;
    if experimental_backing_version != EXPERIMENTAL_GENERATION_VERSION {
        return Err(lines.error(format!(
            "requires experimental backing version {EXPERIMENTAL_GENERATION_VERSION}, got {experimental_backing_version}"
        )));
    }
    require_only(
        &generation,
        &[
            "compositional-generation-version",
            "experimental-backing-version",
        ],
    )?;

    let descriptors = fields_after(
        lines.next_with_prefix("descriptor-versions ")?,
        "descriptor-versions ",
    )?;
    let static_visual_version = number::<u32>(&descriptors, "static-visual")?;
    let visual_fingerprint_version = number::<u32>(&descriptors, "visual-fingerprint")?;
    if static_visual_version != VISUAL_DESCRIPTOR_VERSION
        || visual_fingerprint_version != VISUAL_FINGERPRINT_VERSION
    {
        return Err(lines.error(format!(
            "unsupported visual descriptor/fingerprint versions {static_visual_version}/{visual_fingerprint_version}"
        )));
    }
    if number::<u32>(&descriptors, "collision-topology")? != COLLISION_TOPOLOGY_DESCRIPTOR_VERSION
        || number::<u32>(&descriptors, "witness")? != WITNESS_FINGERPRINT_VERSION
    {
        return Err(lines.error("unsupported collision topology or witness descriptor version"));
    }
    let requires_representative_actions =
        optional(&descriptors, "representative-actions").is_some();
    if let Some(version) = optional(&descriptors, "representative-actions") {
        let version = parse_number::<u32>(version, "representative-actions")?;
        if version != 1 {
            return Err(lines.error("unsupported representative action encoding version"));
        }
    }
    require_only(
        &descriptors,
        &[
            "static-visual",
            "collision-topology",
            "visual-fingerprint",
            "witness",
            "representative-actions",
        ],
    )?;

    let selection_version = scalar_u32(
        lines.next_with_prefix("selection-version=")?,
        "selection-version",
    )?;
    if selection_version != SELECTION_VERSION {
        return Err(lines.error(format!(
            "requires selection policy version {SELECTION_VERSION}, got {selection_version}"
        )));
    }
    let request = fields_after(lines.next_with_prefix("request ")?, "request ")?;
    let tier = parse_tier(required(&request, "tier")?)?;
    let request_abilities = AbilitySet::new(bit(&request, "wall-jump")?, bit(&request, "dash")?);
    if request_abilities != tier.abilities() {
        return Err(lines.error("request tier and exact ability flags disagree"));
    }

    let solver = fields_after(lines.next_with_prefix("solver-config ")?, "solver-config ")?;
    let solver_config_id = required(&solver, "id")?.to_owned();
    let solver_policy_version = number::<u32>(&solver, "policy-version")?;
    let expected_solver = SolverConfig::for_abilities(request_abilities);
    if !(OLDEST_SUPPORTED_SOLVER_POLICY_VERSION..=SOLVER_POLICY_VERSION)
        .contains(&solver_policy_version)
    {
        return Err(lines.error(format!(
            "supports solver policy versions {OLDEST_SUPPORTED_SOLVER_POLICY_VERSION}..={SOLVER_POLICY_VERSION}, got {solver_policy_version}"
        )));
    }
    let expected_solver_config_id = solver_config_identity(&expected_solver, solver_policy_version);
    if solver_config_id != expected_solver_config_id {
        return Err(lines.error(format!(
            "solver configuration identity changed: expected {expected_solver_config_id}, got {solver_config_id}"
        )));
    }
    if number::<usize>(&solver, "max-expanded-nodes")? != expected_solver.max_expanded_nodes
        || number::<usize>(&solver, "max-simulated-ticks")? != expected_solver.max_simulated_ticks
        || number::<usize>(&solver, "max-ticks-per-path")? != expected_solver.max_ticks_per_path
        || number::<usize>(&solver, "beam-width")? != expected_solver.beam_width
        || number::<i32>(&solver, "position-quantum")? != expected_solver.position_quantum
        || number::<i32>(&solver, "velocity-quantum")? != expected_solver.velocity_quantum
        || bit(&solver, "probe-direct-routes")? != expected_solver.probe_direct_routes
        || number::<usize>(&solver, "baseline-preview-max-expanded-nodes")?
            != expected_solver.baseline_preview_max_expanded_nodes
        || number::<usize>(&solver, "baseline-preview-max-simulated-ticks")?
            != expected_solver.baseline_preview_max_simulated_ticks
        || number::<usize>(&solver, "macros")? != expected_solver.macros.len()
    {
        return Err(lines.error("serialized solver configuration differs from the current policy"));
    }
    require_only(
        &solver,
        &[
            "id",
            "policy-version",
            "max-expanded-nodes",
            "max-simulated-ticks",
            "max-ticks-per-path",
            "beam-width",
            "position-quantum",
            "velocity-quantum",
            "probe-direct-routes",
            "baseline-preview-max-expanded-nodes",
            "baseline-preview-max-simulated-ticks",
            "macros",
        ],
    )?;

    let difficulty = fields_after(
        lines.next_with_prefix("difficulty-config ")?,
        "difficulty-config ",
    )?;
    let difficulty_config_id = required(&difficulty, "id")?.to_owned();
    let difficulty_heuristic_version = number::<u32>(&difficulty, "heuristic-version")?;
    let expected_difficulty = DifficultyConfig::default();
    if difficulty_heuristic_version != DIFFICULTY_HEURISTIC_VERSION {
        return Err(lines.error(format!(
            "requires difficulty heuristic version {DIFFICULTY_HEURISTIC_VERSION}, got {difficulty_heuristic_version}"
        )));
    }
    let expected_difficulty_config_id = difficulty_config_identity(&expected_difficulty);
    if difficulty_config_id != expected_difficulty_config_id {
        return Err(lines.error(format!(
            "difficulty configuration identity changed: expected {expected_difficulty_config_id}, got {difficulty_config_id}"
        )));
    }
    if number::<usize>(&difficulty, "perturbation-grace-ticks")?
        != expected_difficulty.perturbation_grace_ticks
        || required(&difficulty, "interpretation")? != "heuristic-not-human-difficulty"
    {
        return Err(
            lines.error("serialized difficulty configuration differs from the current heuristic")
        );
    }
    require_only(
        &difficulty,
        &[
            "id",
            "heuristic-version",
            "perturbation-grace-ticks",
            "interpretation",
        ],
    )?;

    lines.exact(ROUTE_BAND_POLICY)?;
    lines.exact(EASIEST_ROUTE_POLICY)?;
    lines.exact(FAIRNESS_POLICY)?;
    lines.exact(&selection_policy(tier))?;
    let has_strategy_coverage = lines
        .peek()
        .is_some_and(|line| line.starts_with("strategy-coverage "));
    if has_strategy_coverage {
        lines.next_with_prefix("strategy-coverage ")?;
        lines.next_with_prefix("intent-coverage ")?;
    }
    if lines
        .peek()
        .is_some_and(|line| line.starts_with("route-demand-coverage "))
    {
        lines.next_with_prefix("route-demand-coverage ")?;
    }
    lines.next_with_prefix("pool ")?;
    let coverage = fields_after(lines.next_with_prefix("coverage ")?, "coverage ")?;
    let declared_room_count = number::<usize>(&coverage, "selected-rooms")?;
    if required(&coverage, "all-sockets-have-mate")? != "true" {
        return Err(lines.error("coverage must declare socket-mate closure"));
    }

    let declared_ability_coverage = if lines
        .peek()
        .is_some_and(|line| line.starts_with("representative-ability-coverage "))
    {
        let ability_coverage = fields_after(
            lines.next().expect("peeked line exists"),
            "representative-ability-coverage ",
        )?;
        let required_wall = bit(&ability_coverage, "required-wall")?;
        let required_dash = bit(&ability_coverage, "required-dash")?;
        if required_wall != tier.abilities().wall_jump || required_dash != tier.abilities().dash {
            return Err(
                lines.error("representative ability requirements disagree with the catalogue tier")
            );
        }
        if required(&ability_coverage, "satisfied")? != "true" {
            return Err(lines.error("representative ability coverage is not satisfied"));
        }
        let wall_witnesses = number::<usize>(&ability_coverage, "wall-witnesses")?;
        let dash_witnesses = number::<usize>(&ability_coverage, "dash-witnesses")?;
        let both_witnesses = number::<usize>(&ability_coverage, "both-on-one-route")?;
        if (required_wall && wall_witnesses == 0) || (required_dash && dash_witnesses == 0) {
            return Err(lines.error("representative ability coverage declares no required witness"));
        }
        require_only(
            &ability_coverage,
            &[
                "wall-witnesses",
                "dash-witnesses",
                "both-on-one-route",
                "required-wall",
                "required-dash",
                "satisfied",
            ],
        )?;
        Some((wall_witnesses, dash_witnesses, both_witnesses))
    } else {
        None
    };
    let Some(declared_ability_coverage) = declared_ability_coverage else {
        return Err(lines.error("manifest v2 requires representative ability coverage"));
    };

    while lines
        .peek()
        .is_some_and(|line| line.starts_with("socket-inventory "))
    {
        let inventory = fields_after(
            lines.next().expect("peeked line exists"),
            "socket-inventory ",
        )?;
        let _ = parse_side(required(&inventory, "side")?)?;
        let _ = number::<i32>(&inventory, "offset")?;
        let _ = positive_i32(&inventory, "span")?;
        let _ = number::<usize>(&inventory, "count")?;
        let _ = number::<usize>(&inventory, "mate-count")?;
    }

    let mut entries = Vec::with_capacity(declared_room_count);
    while lines
        .peek()
        .is_some_and(|line| line.starts_with("room-begin "))
    {
        entries.push(parse_entry(
            &mut lines,
            tier,
            entries.len(),
            requires_representative_actions,
        )?);
    }
    if entries.len() != declared_room_count {
        return Err(lines.error(format!(
            "coverage declares {declared_room_count} rooms but {} were parsed",
            entries.len()
        )));
    }
    if lines.peek().is_some() {
        return Err(lines.error("unexpected content after final room"));
    }

    validate_collection(&entries)?;
    let actual_ability_coverage = (
        entries
            .iter()
            .filter(|entry| entry.successful_wall_jumps > 0)
            .count(),
        entries
            .iter()
            .filter(|entry| entry.successful_dashes > 0)
            .count(),
        entries
            .iter()
            .filter(|entry| entry.successful_wall_jumps > 0 && entry.successful_dashes > 0)
            .count(),
    );
    if actual_ability_coverage != declared_ability_coverage {
        return Err(lines.error(format!(
            "representative ability coverage disagrees with room records: declared={declared_ability_coverage:?} actual={actual_ability_coverage:?}"
        )));
    }
    let identity = CatalogueIdentity {
        manifest_version: MANIFEST_VERSION,
        compositional_generation_version,
        experimental_backing_version,
        selection_version,
        solver_policy_version,
        solver_config_id,
        difficulty_heuristic_version,
        difficulty_config_id,
        manifest_fingerprint: fingerprint_line.to_owned(),
    };
    Ok(CatalogueManifest {
        identity,
        tier,
        entries: entries.into_boxed_slice(),
    })
}

fn parse_entry(
    lines: &mut Lines<'_>,
    tier: AbilityTier,
    expected_index: usize,
    requires_representative_actions: bool,
) -> Result<CatalogueEntry, CatalogueError> {
    let begin = fields_after(lines.next_with_prefix("room-begin ")?, "room-begin ")?;
    let index = number::<usize>(&begin, "index")?;
    if index != expected_index {
        return Err(lines.error(format!(
            "room index {index} is not canonical index {expected_index}"
        )));
    }
    let id = required(&begin, "id")?.to_owned();
    if id.is_empty() {
        return Err(lines.error("room id must not be empty"));
    }
    let band = parse_band(band_value(&begin)?)?;
    let strategy = parse_strategy(required(&begin, "strategy")?)?;
    let intent = parse_intent(required(&begin, "intent")?)?;
    let seed = number(&begin, "seed")?;
    let room_tier = parse_tier(required(&begin, "tier")?)?;
    let abilities = AbilitySet::new(bit(&begin, "wall-jump")?, bit(&begin, "dash")?);
    if room_tier != tier || abilities != tier.abilities() {
        return Err(lines.error("room tier/abilities differ from the manifest request"));
    }
    let metadata_version = number::<u32>(&begin, "metadata-generation-version")?;
    if metadata_version != COMPOSITIONAL_GENERATION_VERSION {
        return Err(lines.error("room metadata is not generation version 6"));
    }
    let visual_fingerprint = parse_prefixed_hex(
        required(&begin, "visual-fingerprint")?,
        "downwards-static-v1-",
    )?;
    let key = CompositionalKey::new(seed, CompositionalProfile::new(abilities, strategy, intent));

    let route_plan = fields_after(lines.next_with_prefix("route-plan ")?, "route-plan ")?;
    let declared_ports = number::<usize>(&route_plan, "ports")?;
    if !(2..=4).contains(&declared_ports) {
        return Err(lines.error("room must declare 2 to 4 ports"));
    }
    lines.next_with_prefix("qd-stratum ")?;
    if lines
        .peek()
        .is_some_and(|line| line.starts_with("terrain-utility "))
    {
        lines.next_with_prefix("terrain-utility ")?;
    }

    let socket_line = lines.next_with_prefix("socket-signature ")?;
    let sockets = parse_socket_signature(
        socket_line
            .strip_prefix("socket-signature ")
            .expect("prefix checked"),
    )?;
    if sockets.len() != declared_ports {
        return Err(lines.error("socket count differs from route-plan port count"));
    }
    let mut door_ids = Vec::with_capacity(declared_ports);
    let mut declared_door_sockets = Vec::with_capacity(declared_ports);
    for _ in 0..declared_ports {
        let door = fields_after(lines.next_with_prefix("door ")?, "door ")?;
        let door_id = required(&door, "id")?.to_owned();
        if door_ids.last().is_some_and(|last| last >= &door_id) {
            return Err(lines.error("door declarations must be in strictly increasing id order"));
        }
        door_ids.push(door_id);
        declared_door_sockets.push(DoorSocket {
            side: parse_side(required(&door, "side")?)?,
            offset: number(&door, "offset")?,
            span: positive_i32(&door, "span")?,
        });
        let _ = number::<i32>(&door, "arrival-x")?;
        let _ = number::<i32>(&door, "arrival-y")?;
    }
    declared_door_sockets.sort_unstable();
    if declared_door_sockets != sockets {
        return Err(lines.error("door declarations and socket signature disagree"));
    }

    let representative = fields_after(
        lines.next_with_prefix("representative-route ")?,
        "representative-route ",
    )?;
    let representative_band = parse_band(band_value(&representative)?)?;
    if representative_band != band {
        return Err(lines.error("representative route band differs from room band"));
    }
    let source_door_id = required(&representative, "source")?.to_owned();
    let target_door_id = required(&representative, "target")?.to_owned();
    if source_door_id == target_door_id {
        return Err(lines.error("representative source and target must differ"));
    }
    if !door_ids.contains(&source_door_id) || !door_ids.contains(&target_door_id) {
        return Err(lines.error("representative route refers to an undeclared door"));
    }
    let successful_wall_jumps = number(&representative, "successful-wall-jumps")?;
    let successful_dashes = number(&representative, "successful-dashes")?;
    if (!abilities.wall_jump && successful_wall_jumps != 0)
        || (!abilities.dash && successful_dashes != 0)
    {
        return Err(lines.error("representative route uses an unavailable traversal ability"));
    }
    let representative_actions = if lines
        .peek()
        .is_some_and(|line| line.starts_with("representative-actions "))
    {
        Some(parse_actions(
            lines.next().expect("peeked line exists"),
            "representative-actions",
        )?)
    } else {
        None
    };
    if requires_representative_actions && representative_actions.is_none() {
        return Err(lines.error("manifest requires a representative route action replay"));
    }

    let routes = fields_after(lines.next_with_prefix("route-matrix ")?, "route-matrix ")?;
    let route_count = number::<usize>(&routes, "count")?;
    let expected_route_count = door_ids.len() * door_ids.len().saturating_sub(1);
    if route_count != expected_route_count {
        return Err(lines.error(format!(
            "route matrix has {route_count} records, expected {expected_route_count} ordered door pairs"
        )));
    }
    let mut representative_route_seen = false;
    let mut route_pairs = HashSet::with_capacity(route_count);
    let mut previous_route_pair = None::<(String, String)>;
    for route_index in 0..route_count {
        let route = fields_after(lines.next_with_prefix("route ")?, "route ")?;
        if number::<usize>(&route, "index")? != route_index {
            return Err(lines.error("route indices are not canonical"));
        }
        let route_source = required(&route, "source")?.to_owned();
        let route_target = required(&route, "target")?.to_owned();
        if route_source == route_target
            || !door_ids.contains(&route_source)
            || !door_ids.contains(&route_target)
        {
            return Err(lines.error("route matrix refers to an invalid ordered door pair"));
        }
        let route_pair = (route_source, route_target);
        if previous_route_pair
            .as_ref()
            .is_some_and(|previous| previous >= &route_pair)
        {
            return Err(lines.error("route matrix pairs are not in canonical order"));
        }
        previous_route_pair = Some(route_pair.clone());
        if !route_pairs.insert(route_pair.clone()) {
            return Err(lines.error("route matrix contains a duplicate ordered door pair"));
        }
        if route_pair.0 == source_door_id && route_pair.1 == target_door_id {
            representative_route_seen = true;
            if parse_band(required(&route, "band")?)? != band
                || number::<usize>(&route, "successful-wall-jumps")? != successful_wall_jumps
                || number::<usize>(&route, "successful-dashes")? != successful_dashes
            {
                return Err(
                    lines.error("representative route summary disagrees with its route record")
                );
            }
        }
    }
    if !representative_route_seen {
        return Err(lines.error("representative route is absent from route matrix"));
    }

    let pickups = fields_after(lines.next_with_prefix("pickup-matrix ")?, "pickup-matrix ")?;
    let pickup_count = number::<usize>(&pickups, "count")?;
    let mut pickup_pairs = HashSet::<(String, String)>::with_capacity(pickup_count);
    let mut pickup_ids = HashSet::<String>::new();
    let mut previous_pickup_pair = None::<(String, String)>;
    for _ in 0..pickup_count {
        let pickup = fields_after(lines.next_with_prefix("pickup-route ")?, "pickup-route ")?;
        let pickup_source = required(&pickup, "source")?.to_owned();
        let pickup_id = required(&pickup, "pickup")?.to_owned();
        if !door_ids.contains(&pickup_source) || pickup_id.is_empty() {
            return Err(lines.error("pickup matrix refers to an invalid source or pickup"));
        }
        let pickup_pair = (pickup_source, pickup_id.clone());
        if previous_pickup_pair
            .as_ref()
            .is_some_and(|previous| previous >= &pickup_pair)
        {
            return Err(lines.error("pickup matrix pairs are not in canonical order"));
        }
        previous_pickup_pair = Some(pickup_pair.clone());
        if !pickup_pairs.insert(pickup_pair) {
            return Err(lines.error("pickup matrix contains a duplicate source/pickup pair"));
        }
        pickup_ids.insert(pickup_id);
    }
    if pickup_count != door_ids.len() * pickup_ids.len()
        || door_ids.iter().any(|door_id| {
            pickup_ids
                .iter()
                .any(|pickup_id| !pickup_pairs.contains(&(door_id.clone(), pickup_id.clone())))
        })
    {
        return Err(lines.error("pickup matrix is not complete from every source door"));
    }
    let mut pickup_ids = pickup_ids.into_iter().collect::<Vec<_>>();
    pickup_ids.sort_unstable();
    let representative_pickup = if lines
        .peek()
        .is_some_and(|line| line.starts_with("representative-pickup "))
    {
        let pickup = fields_after(
            lines.next().expect("peeked line exists"),
            "representative-pickup ",
        )?;
        let pickup_source_door_id = required(&pickup, "source")?.to_owned();
        let pickup_id = required(&pickup, "pickup")?.to_owned();
        if pickup_source_door_id != source_door_id
            || !pickup_pairs.contains(&(pickup_source_door_id.clone(), pickup_id.clone()))
        {
            return Err(lines.error(
                "representative pickup must use the challenge source and appear in the pickup matrix",
            ));
        }
        if !lines
            .peek()
            .is_some_and(|line| line.starts_with("representative-pickup-actions "))
        {
            return Err(lines.error("representative pickup is missing its action replay"));
        }
        let actions = parse_actions(
            lines.next().expect("peeked line exists"),
            "representative-pickup-actions",
        )?;
        Some(RepresentativePickup {
            source_door_id: pickup_source_door_id,
            pickup_id,
            actions,
        })
    } else {
        None
    };
    if requires_representative_actions && pickup_count > 0 && representative_pickup.is_none() {
        return Err(lines.error("manifest requires a representative pickup action replay"));
    }

    let searches = fields_after(
        lines.next_with_prefix("source-search-matrix ")?,
        "source-search-matrix ",
    )?;
    let search_count = number::<usize>(&searches, "count")?;
    if search_count != door_ids.len() {
        return Err(lines.error("source-search matrix must contain one record per door"));
    }
    let mut search_sources = HashSet::with_capacity(search_count);
    for _ in 0..search_count {
        let search = fields_after(lines.next_with_prefix("source-search ")?, "source-search ")?;
        let source = required(&search, "source")?.to_owned();
        if !door_ids.contains(&source) || !search_sources.insert(source) {
            return Err(lines.error("source-search matrix has an invalid or duplicate door"));
        }
    }
    lines.exact("room-end")?;

    let entry = CatalogueEntry {
        index,
        id,
        band,
        key,
        source_door_id,
        target_door_id,
        visual_fingerprint,
        successful_wall_jumps,
        successful_dashes,
        door_ids: door_ids.into_boxed_slice(),
        pickup_ids: pickup_ids.into_boxed_slice(),
        sockets: sockets.into_boxed_slice(),
        representative_actions,
        representative_pickup,
    };
    verify_regeneration(&entry)?;
    Ok(entry)
}

fn validate_collection(entries: &[CatalogueEntry]) -> Result<(), CatalogueError> {
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    let mut visuals = HashSet::new();
    for entry in entries {
        if !ids.insert(entry.id.as_str()) {
            return Err(CatalogueError::global(format!(
                "duplicate room id {:?}",
                entry.id
            )));
        }
        if !keys.insert(entry.key) {
            return Err(CatalogueError::global(format!(
                "duplicate compositional key for room {:?}",
                entry.id
            )));
        }
        if !visuals.insert(entry.visual_fingerprint) {
            return Err(CatalogueError::global(format!(
                "duplicate visual fingerprint for room {:?}",
                entry.id
            )));
        }
    }
    for pair in entries.windows(2) {
        let left = canonical_key(&pair[0]);
        let right = canonical_key(&pair[1]);
        if left >= right {
            return Err(CatalogueError::global(
                "rooms are not in canonical band/strategy/intent/seed/visual order",
            ));
        }
    }
    let sockets = entries
        .iter()
        .flat_map(|entry| entry.sockets.iter().copied())
        .collect::<Vec<_>>();
    for socket in &sockets {
        if !sockets.iter().any(|other| socket.matches(*other)) {
            return Err(CatalogueError::global(format!(
                "socket {socket:?} has no mate in the catalogue"
            )));
        }
    }
    Ok(())
}

fn canonical_key(
    entry: &CatalogueEntry,
) -> (CatalogueBand, GenerationStrategy, ChallengeIntent, u64, u64) {
    (
        entry.band,
        entry.key.profile.strategy,
        entry.key.profile.intent,
        entry.key.seed,
        entry.visual_fingerprint,
    )
}

fn verify_regeneration(entry: &CatalogueEntry) -> Result<(), CatalogueError> {
    let candidate = generate_compositional(entry.key).map_err(|error| {
        CatalogueError::global(format!(
            "room {:?} no longer regenerates: {error}",
            entry.id
        ))
    })?;
    let room = &candidate.generated.room;
    let mut door_ids = room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    door_ids.sort_unstable();
    if door_ids != entry.door_ids.as_ref() {
        return Err(CatalogueError::global(format!(
            "room {:?} regenerated with different door ids",
            entry.id
        )));
    }
    let mut sockets = room
        .doors()
        .iter()
        .map(|door| door.socket())
        .collect::<Vec<_>>();
    sockets.sort_unstable();
    if sockets != entry.sockets.as_ref() {
        return Err(CatalogueError::global(format!(
            "room {:?} regenerated with different sockets",
            entry.id
        )));
    }
    let mut pickup_ids = room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect::<Vec<_>>();
    pickup_ids.sort_unstable();
    if pickup_ids != entry.pickup_ids.as_ref() {
        return Err(CatalogueError::global(format!(
            "room {:?} regenerated with different pickup ids: manifest={:?} regenerated={pickup_ids:?}",
            entry.id, entry.pickup_ids
        )));
    }
    let actual_visual = fingerprint_visual(room);
    if actual_visual != entry.visual_fingerprint {
        return Err(CatalogueError::global(format!(
            "room {:?} visual fingerprint changed: manifest={:016x} regenerated={actual_visual:016x}",
            entry.id, entry.visual_fingerprint
        )));
    }
    if let Some(actions) = &entry.representative_actions {
        verify_replay(entry, candidate.generated.room.clone(), actions)?;
    }
    if let Some(pickup) = &entry.representative_pickup {
        verify_pickup_replay(entry, candidate.generated.room, pickup)?;
    }
    Ok(())
}

fn verify_pickup_replay(
    entry: &CatalogueEntry,
    room: Room,
    pickup: &RepresentativePickup,
) -> Result<(), CatalogueError> {
    if !room
        .pickups()
        .iter()
        .any(|candidate| candidate.id() == pickup.pickup_id)
    {
        return Err(CatalogueError::global(format!(
            "room {:?} regenerated without representative pickup {:?}",
            entry.id, pickup.pickup_id
        )));
    }
    let mut simulation =
        Simulation::enter_via_door(room, entry.key.profile.abilities, &pickup.source_door_id)
            .map_err(|error| {
                CatalogueError::global(format!(
                    "room {:?} pickup replay entry failed: {error}",
                    entry.id
                ))
            })?;
    let mut collected = false;
    for action in pickup.actions.actions() {
        let report = simulation.step(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(_)))
        {
            return Err(CatalogueError::global(format!(
                "room {:?} representative pickup replay dies",
                entry.id
            )));
        }
        collected |= report.events.iter().any(|event| {
            matches!(event, SimulationEvent::PickupCollected { id } if id == &pickup.pickup_id)
        });
    }
    if !collected {
        return Err(CatalogueError::global(format!(
            "room {:?} representative pickup replay does not collect {:?}",
            entry.id, pickup.pickup_id
        )));
    }
    Ok(())
}

fn verify_replay(
    entry: &CatalogueEntry,
    room: Room,
    actions: &RepresentativeActions,
) -> Result<(), CatalogueError> {
    let mut simulation =
        Simulation::enter_via_door(room, entry.key.profile.abilities, &entry.source_door_id)
            .map_err(|error| {
                CatalogueError::global(format!("room {:?} replay entry failed: {error}", entry.id))
            })?;
    let mut successful_wall_jumps = 0;
    let mut successful_dashes = 0;
    for span in actions.spans() {
        for _ in 0..span.ticks {
            let report = simulation.step(span.action);
            if report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
            {
                return Err(CatalogueError::global(format!(
                    "room {:?} representative replay dies",
                    entry.id
                )));
            }
            successful_wall_jumps += report
                .events
                .iter()
                .filter(|event| matches!(event, SimulationEvent::Jumped(JumpKind::Wall { .. })))
                .count();
            successful_dashes += report
                .events
                .iter()
                .filter(|event| matches!(event, SimulationEvent::Dashed { .. }))
                .count();
        }
    }
    if simulation.reached_exit() != Some(entry.target_door_id.as_str()) {
        return Err(CatalogueError::global(format!(
            "room {:?} representative replay reaches {:?}, expected {:?}",
            entry.id,
            simulation.reached_exit(),
            entry.target_door_id
        )));
    }
    if successful_wall_jumps != entry.successful_wall_jumps
        || successful_dashes != entry.successful_dashes
    {
        return Err(CatalogueError::global(format!(
            "room {:?} representative replay ability events changed: manifest={}/{} replay={successful_wall_jumps}/{successful_dashes}",
            entry.id, entry.successful_wall_jumps, entry.successful_dashes
        )));
    }
    Ok(())
}

fn parse_actions(line: &str, prefix: &str) -> Result<RepresentativeActions, CatalogueError> {
    let values = fields(
        line.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix(' '))
            .ok_or_else(|| CatalogueError::global("invalid action line prefix"))?,
    )?;
    if required(&values, "encoding")? != ACTION_ENCODING
        || required(&values, "fields")? != ACTION_FIELDS
    {
        return Err(CatalogueError::global(
            "unsupported representative action encoding",
        ));
    }
    let total_ticks = number::<usize>(&values, "total-ticks")?;
    let declared_spans = number::<usize>(&values, "spans")?;
    let data = required(&values, "data")?;
    if total_ticks == 0 || declared_spans == 0 || data.is_empty() {
        return Err(CatalogueError::global(
            "representative action replay must be nonempty",
        ));
    }
    let mut spans = Vec::with_capacity(declared_spans);
    let mut sum = 0usize;
    for encoded in data.split(',') {
        let (action, ticks) = encoded
            .split_once('*')
            .ok_or_else(|| CatalogueError::global("action span lacks *ticks"))?;
        let parts = action.split(':').collect::<Vec<_>>();
        let [move_x, move_y, jump, dash, restart] = parts.as_slice() else {
            return Err(CatalogueError::global(
                "action span must have five action fields",
            ));
        };
        let action = Action {
            move_x: parse_number(move_x, "move-x")?,
            move_y: parse_number(move_y, "move-y")?,
            jump: parse_bit(jump, "jump")?,
            dash: parse_bit(dash, "dash")?,
            restart: parse_bit(restart, "restart")?,
        };
        if !(-1..=1).contains(&action.move_x) || !(-1..=1).contains(&action.move_y) {
            return Err(CatalogueError::global(
                "action movement axes must be -1, 0, or 1",
            ));
        }
        if action.restart {
            return Err(CatalogueError::global(
                "representative action replay may not restart",
            ));
        }
        let ticks = parse_number::<usize>(ticks, "span ticks")?;
        if ticks == 0 {
            return Err(CatalogueError::global("action span ticks must be positive"));
        }
        sum = sum
            .checked_add(ticks)
            .ok_or_else(|| CatalogueError::global("action tick count overflow"))?;
        spans.push(ActionSpan { action, ticks });
    }
    if spans.len() != declared_spans || sum != total_ticks {
        return Err(CatalogueError::global(
            "representative action span/tick totals disagree",
        ));
    }
    Ok(RepresentativeActions {
        total_ticks,
        spans: spans.into_boxed_slice(),
    })
}

fn parse_socket_signature(value: &str) -> Result<Vec<DoorSocket>, CatalogueError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut sockets = Vec::new();
    for encoded in value.split(',') {
        let parts = encoded.split(':').collect::<Vec<_>>();
        let [side, offset, span] = parts.as_slice() else {
            return Err(CatalogueError::global("invalid socket signature"));
        };
        sockets.push(DoorSocket {
            side: parse_side(side)?,
            offset: parse_number(offset, "socket offset")?,
            span: parse_number(span, "socket span")?,
        });
    }
    let original = sockets.clone();
    sockets.sort_unstable();
    if sockets != original {
        return Err(CatalogueError::global("socket signature is not canonical"));
    }
    if sockets.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(CatalogueError::global(
            "socket signature contains duplicates",
        ));
    }
    Ok(sockets)
}

fn fingerprint_visual(room: &Room) -> u64 {
    let mut hash = StableHash::domain(b"downwards-curation-static-visual");
    hash.u32(VISUAL_FINGERPRINT_VERSION);
    hash.u32(VISUAL_DESCRIPTOR_VERSION);
    hash.u16(room.width());
    hash.u16(room.height());
    hash.i32(room.tile_size());
    let spawn = room.spawn();
    hash.i32(spawn.x);
    hash.i32(spawn.y);
    hash.usize(room.tiles().len());
    for tile in room.tiles() {
        hash.u8(visual_tile(*tile));
    }
    let mut exits = room
        .exits()
        .iter()
        .map(|exit| exit.bounds)
        .collect::<Vec<_>>();
    exits.sort_unstable_by_key(rect_key);
    hash.usize(exits.len());
    for bounds in exits {
        hash.rect(bounds);
    }
    let mut doors = room
        .doors()
        .iter()
        .map(|door| (door.side, door.trigger_bounds))
        .collect::<Vec<_>>();
    doors.sort_unstable_by_key(|(side, bounds)| (*side as u8, rect_key(bounds)));
    hash.usize(doors.len());
    for (side, bounds) in doors {
        hash.u8(side as u8);
        hash.rect(bounds);
    }
    let mut pickups = room
        .pickups()
        .iter()
        .map(|pickup| pickup.bounds())
        .collect::<Vec<_>>();
    pickups.sort_unstable_by_key(rect_key);
    hash.usize(pickups.len());
    for bounds in pickups {
        hash.rect(bounds);
    }
    let mut hazards = room
        .timed_hazards()
        .iter()
        .map(|hazard| hazard.bounds())
        .collect::<Vec<_>>();
    hazards.sort_unstable_by_key(rect_key);
    hash.usize(hazards.len());
    for bounds in hazards {
        hash.rect(bounds);
    }
    hash.finish()
}

const fn visual_tile(tile: Tile) -> u8 {
    match tile {
        Tile::Empty => 0,
        Tile::Solid => 1,
        Tile::HazardUp => 2,
        Tile::OneWay => 3,
        Tile::HazardDown => 4,
        Tile::HazardLeft => 5,
        Tile::HazardRight => 6,
    }
}
const fn rect_key(rect: &Rect) -> (i32, i32, i32, i32) {
    (rect.x, rect.y, rect.width, rect.height)
}

fn split_and_verify_fingerprint(input: &str) -> Result<(&str, &str), CatalogueError> {
    let marker = "manifest-fingerprint=";
    let position = input
        .rfind(marker)
        .ok_or_else(|| CatalogueError::global("missing manifest fingerprint"))?;
    let body = &input[..position];
    let tail = &input[position..];
    if !body.ends_with('\n') || !tail.ends_with('\n') || tail[..tail.len() - 1].contains('\n') {
        return Err(CatalogueError::global(
            "manifest fingerprint must be the final newline-terminated line",
        ));
    }
    let fingerprint = tail.strip_suffix('\n').expect("checked suffix");
    let declared = parse_prefixed_hex(
        fingerprint,
        "manifest-fingerprint=downwards-curation-manifest-v3-",
    )?;
    let mut hash = StableHash::domain(b"downwards-curation-manifest");
    hash.u32(MANIFEST_VERSION);
    hash.bytes(body.as_bytes());
    if declared != hash.finish() {
        return Err(CatalogueError::global("manifest fingerprint mismatch"));
    }
    Ok((body, fingerprint))
}

struct Lines<'a> {
    lines: Vec<&'a str>,
    cursor: usize,
}
impl<'a> Lines<'a> {
    fn new(body: &'a str) -> Self {
        Self {
            lines: body.lines().collect(),
            cursor: 0,
        }
    }
    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.cursor).copied()
    }
    fn next(&mut self) -> Option<&'a str> {
        let line = self.peek()?;
        self.cursor += 1;
        Some(line)
    }
    fn exact(&mut self, expected: &str) -> Result<(), CatalogueError> {
        let actual = self
            .next()
            .ok_or_else(|| self.error(format!("expected {expected:?}, found end of input")))?;
        if actual != expected {
            return Err(self.error(format!("expected {expected:?}, got {actual:?}")));
        }
        Ok(())
    }
    fn next_with_prefix(&mut self, prefix: &str) -> Result<&'a str, CatalogueError> {
        let line = self
            .next()
            .ok_or_else(|| self.error(format!("expected {prefix:?}, found end of input")))?;
        if !line.starts_with(prefix) {
            return Err(self.error(format!("expected line beginning {prefix:?}, got {line:?}")));
        }
        Ok(line)
    }
    fn error(&self, message: impl Into<String>) -> CatalogueError {
        CatalogueError::at(self.cursor.max(1), message)
    }
}

#[derive(Debug)]
struct Fields(Vec<(String, String)>);
fn fields(line: &str) -> Result<Fields, CatalogueError> {
    let mut result = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        let equal = rest
            .find('=')
            .ok_or_else(|| CatalogueError::global(format!("field lacks '=' in {rest:?}")))?;
        let key = &rest[..equal];
        if key.is_empty() || key.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(CatalogueError::global("invalid field key"));
        }
        rest = &rest[equal + 1..];
        let (value, next) = if let Some(quoted) = rest.strip_prefix('"') {
            parse_quoted(quoted)?
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            (rest[..end].to_owned(), &rest[end..])
        };
        if result.iter().any(|(existing, _)| existing == key) {
            return Err(CatalogueError::global(format!("duplicate field {key:?}")));
        }
        result.push((key.to_owned(), value));
        rest = next.trim_start();
    }
    Ok(Fields(result))
}

fn fields_after(line: &str, prefix: &str) -> Result<Fields, CatalogueError> {
    fields(
        line.strip_prefix(prefix)
            .ok_or_else(|| CatalogueError::global(format!("expected prefix {prefix:?}")))?,
    )
}
fn parse_quoted(input: &str) -> Result<(String, &str), CatalogueError> {
    let mut output = String::new();
    let mut escaped = false;
    for (index, character) in input.char_indices() {
        if escaped {
            match character {
                '\\' | '"' => output.push(character),
                'n' => output.push('\n'),
                _ => return Err(CatalogueError::global("invalid quoted escape")),
            };
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => return Ok((output, &input[index + 1..])),
            _ => output.push(character),
        }
    }
    Err(CatalogueError::global("unterminated quoted field"))
}
fn required<'a>(fields: &'a Fields, key: &str) -> Result<&'a str, CatalogueError> {
    fields
        .0
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| CatalogueError::global(format!("missing field {key:?}")))
}
fn optional<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields
        .0
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}
fn band_value(fields: &Fields) -> Result<&str, CatalogueError> {
    match (
        optional(fields, "catalogue-band"),
        optional(fields, "requested-band"),
    ) {
        (Some(value), None) | (None, Some(value)) => Ok(value),
        (Some(_), Some(_)) => Err(CatalogueError::global(
            "both catalogue-band and legacy requested-band are present",
        )),
        (None, None) => Err(CatalogueError::global("missing catalogue-band")),
    }
}
fn require_only(fields: &Fields, allowed: &[&str]) -> Result<(), CatalogueError> {
    if let Some((name, _)) = fields
        .0
        .iter()
        .find(|(name, _)| !allowed.contains(&name.as_str()))
    {
        Err(CatalogueError::global(format!("unexpected field {name:?}")))
    } else {
        Ok(())
    }
}
fn number<T: std::str::FromStr>(fields: &Fields, key: &str) -> Result<T, CatalogueError> {
    parse_number(required(fields, key)?, key)
}
fn parse_number<T: std::str::FromStr>(value: &str, key: &str) -> Result<T, CatalogueError> {
    value
        .parse()
        .map_err(|_| CatalogueError::global(format!("invalid number for {key:?}: {value:?}")))
}
fn bit(fields: &Fields, key: &str) -> Result<bool, CatalogueError> {
    parse_bit(required(fields, key)?, key)
}
fn parse_bit(value: &str, key: &str) -> Result<bool, CatalogueError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(CatalogueError::global(format!("{key:?} must be 0 or 1"))),
    }
}
fn positive_i32(fields: &Fields, key: &str) -> Result<i32, CatalogueError> {
    let result = number(fields, key)?;
    if result <= 0 {
        Err(CatalogueError::global(format!("{key:?} must be positive")))
    } else {
        Ok(result)
    }
}
fn scalar_u32(line: &str, prefix: &str) -> Result<u32, CatalogueError> {
    parse_number(
        line.strip_prefix(&format!("{prefix}="))
            .ok_or_else(|| CatalogueError::global("invalid scalar line"))?,
        prefix,
    )
}
fn expect_scalar_u32(
    lines: &mut Lines<'_>,
    prefix: &str,
    expected: u32,
) -> Result<(), CatalogueError> {
    let actual = scalar_u32(lines.next_with_prefix(&format!("{prefix}="))?, prefix)?;
    if actual == expected {
        Ok(())
    } else {
        Err(lines.error(format!("expected {prefix}={expected}, got {actual}")))
    }
}
fn parse_prefixed_hex(value: &str, prefix: &str) -> Result<u64, CatalogueError> {
    let hex = value
        .strip_prefix(prefix)
        .ok_or_else(|| CatalogueError::global(format!("expected prefix {prefix:?}")))?;
    if hex.len() != 16
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CatalogueError::global(
            "fingerprint must contain 16 lowercase hexadecimal digits",
        ));
    }
    u64::from_str_radix(hex, 16)
        .map_err(|_| CatalogueError::global("invalid hexadecimal fingerprint"))
}
fn parse_band(value: &str) -> Result<CatalogueBand, CatalogueError> {
    match value {
        "gentle" => Ok(CatalogueBand::Gentle),
        "standard" => Ok(CatalogueBand::Standard),
        "technical" => Ok(CatalogueBand::Technical),
        _ => Err(CatalogueError::global(format!(
            "unknown catalogue band {value:?}"
        ))),
    }
}
fn parse_strategy(value: &str) -> Result<GenerationStrategy, CatalogueError> {
    match value {
        "cyclic-graph" => Ok(GenerationStrategy::CyclicGraph),
        "reachability-growth" => Ok(GenerationStrategy::ReachabilityGrowth),
        "rhythm-weave" => Ok(GenerationStrategy::RhythmWeave),
        _ => Err(CatalogueError::global(format!(
            "unknown generation strategy {value:?}"
        ))),
    }
}
fn parse_intent(value: &str) -> Result<ChallengeIntent, CatalogueError> {
    match value {
        "gentle" => Ok(ChallengeIntent::Gentle),
        "standard" => Ok(ChallengeIntent::Standard),
        "technical" => Ok(ChallengeIntent::Technical),
        _ => Err(CatalogueError::global(format!(
            "unknown challenge intent {value:?}"
        ))),
    }
}
fn parse_tier(value: &str) -> Result<AbilityTier, CatalogueError> {
    match value {
        "baseline" => Ok(AbilityTier::Baseline),
        "wall" => Ok(AbilityTier::WallJump),
        "dash" => Ok(AbilityTier::Dash),
        "both" => Ok(AbilityTier::WallJumpAndDash),
        _ => Err(CatalogueError::global(format!(
            "unknown ability tier {value:?}"
        ))),
    }
}
fn parse_side(value: &str) -> Result<BoundarySide, CatalogueError> {
    match value {
        "left" => Ok(BoundarySide::Left),
        "right" => Ok(BoundarySide::Right),
        "ceiling" => Ok(BoundarySide::Ceiling),
        "floor" => Ok(BoundarySide::Floor),
        _ => Err(CatalogueError::global(format!(
            "unknown boundary side {value:?}"
        ))),
    }
}

fn selection_policy(tier: AbilityTier) -> String {
    let ability_policy = match tier {
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
    };
    format!("{SELECTION_POLICY_PREFIX}{ability_policy}{SELECTION_POLICY_SUFFIX}")
}

fn solver_config_identity(config: &SolverConfig, solver_policy_version: u32) -> String {
    let mut hash = StableHash::domain(b"downwards-curation-solver-config");
    hash.u32(CONFIG_FINGERPRINT_VERSION);
    hash.u32(solver_policy_version);
    hash.usize(config.max_expanded_nodes);
    hash.usize(config.max_simulated_ticks);
    hash.usize(config.max_ticks_per_path);
    hash.usize(config.beam_width);
    hash.i32(config.position_quantum);
    hash.i32(config.velocity_quantum);
    hash.bool(config.probe_direct_routes);
    hash.usize(config.baseline_preview_max_expanded_nodes);
    hash.usize(config.baseline_preview_max_simulated_ticks);
    hash.usize(config.macros.len());
    for action_macro in &config.macros {
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

fn difficulty_config_identity(config: &DifficultyConfig) -> String {
    let mut hash = StableHash::domain(b"downwards-curation-difficulty-config");
    hash.u32(CONFIG_FINGERPRINT_VERSION);
    hash.u32(DIFFICULTY_HEURISTIC_VERSION);
    hash.u32(ROUTE_BAND_POLICY_VERSION);
    hash.usize(config.perturbation_grace_ticks);
    format!(
        "downwards-difficulty-config-v{CONFIG_FINGERPRINT_VERSION}-{:016x}",
        hash.finish()
    )
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
            self.u8(*byte);
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
    fn rect(&mut self, bounds: Rect) {
        self.i32(bounds.x);
        self.i32(bounds.y);
        self.i32(bounds.width);
        self.i32(bounds.height);
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

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = include_str!("../../../content/catalogues/v6/baseline.manifest");
    const WALL: &str = include_str!("../../../content/catalogues/v6/wall.manifest");
    const DASH: &str = include_str!("../../../content/catalogues/v6/dash.manifest");
    const BOTH: &str = include_str!("../../../content/catalogues/v6/both.manifest");

    fn refingerprint(mut manifest_body: String) -> String {
        let fingerprint = manifest_body
            .rfind("manifest-fingerprint=")
            .expect("fixture has a fingerprint");
        manifest_body.truncate(fingerprint);
        let mut hash = StableHash::domain(b"downwards-curation-manifest");
        hash.u32(MANIFEST_VERSION);
        hash.bytes(manifest_body.as_bytes());
        format!(
            "{manifest_body}manifest-fingerprint=downwards-curation-manifest-v{MANIFEST_VERSION}-{:016x}\n",
            hash.finish()
        )
    }

    fn mutate_and_refingerprint(source: &str, from: &str, to: &str) -> String {
        let changed = source.replacen(from, to, 1);
        assert_ne!(changed, source, "mutation source must occur in fixture");
        refingerprint(changed)
    }

    #[test]
    fn current_production_manifests_parse_regenerate_and_cover_policy_cells() {
        for (source, tier) in [
            (BASELINE, AbilityTier::Baseline),
            (WALL, AbilityTier::WallJump),
            (DASH, AbilityTier::Dash),
            (BOTH, AbilityTier::WallJumpAndDash),
        ] {
            let manifest = parse_manifest(source)
                .unwrap_or_else(|error| panic!("{tier:?} catalogue failed: {error}"));
            assert_eq!(manifest.identity().manifest_version, 3);
            assert_eq!(manifest.identity().selection_version, 5);
            assert_eq!(manifest.tier(), tier);
            assert_eq!(manifest.entries().len(), 9);
            for band in [
                CatalogueBand::Gentle,
                CatalogueBand::Standard,
                CatalogueBand::Technical,
            ] {
                assert_eq!(
                    manifest
                        .entries()
                        .iter()
                        .filter(|entry| entry.band() == band)
                        .count(),
                    3
                );
            }
            // Strategy diversity is preferred, not guaranteed: the selector
            // takes what certifies under the current physics.
            assert!(
                manifest
                    .entries()
                    .iter()
                    .map(|entry| entry.key().profile.strategy)
                    .collect::<HashSet<_>>()
                    .len()
                    >= 2
            );
            assert!(
                manifest
                    .entries()
                    .iter()
                    .all(|entry| (2..=4).contains(&entry.sockets().len()))
            );
        }
    }

    #[test]
    fn stable_hash_matches_curator_golden_fingerprint() {
        let body = "downwards-curation-manifest-v1\nroom-begin seed=7\nroom-end\n";
        let mut hash = StableHash::domain(b"downwards-curation-manifest");
        hash.u32(1);
        hash.bytes(body.as_bytes());
        assert_eq!(hash.finish(), 0x1e8a_f595_7dd1_ad16);
    }

    #[test]
    fn checksum_detects_body_mutation_before_parsing() {
        let changed = BASELINE.replacen("interior-tiles=", "interior-tiles=1", 1);
        assert_ne!(changed, BASELINE);
        let error = parse_manifest(&changed).unwrap_err();
        assert!(error.to_string().contains("fingerprint mismatch"));
    }

    #[test]
    fn supported_ai_policy_versions_and_exact_configurations_are_required() {
        for (changed, expected_error) in [
            (
                mutate_and_refingerprint(BASELINE, "policy-version=3", "policy-version=99"),
                "supports solver policy versions",
            ),
            (
                mutate_and_refingerprint(BASELINE, "heuristic-version=1", "heuristic-version=99"),
                "requires difficulty heuristic version",
            ),
            (
                mutate_and_refingerprint(
                    BASELINE,
                    "id=downwards-solver-config-v1-d3ebfd6f2f377945",
                    "id=downwards-solver-config-v1-0000000000000000",
                ),
                "solver configuration identity changed",
            ),
            (
                mutate_and_refingerprint(
                    BASELINE,
                    "id=downwards-difficulty-config-v1-4db5318b9cf9b2a9",
                    "id=downwards-difficulty-config-v1-0000000000000000",
                ),
                "difficulty configuration identity changed",
            ),
            (
                mutate_and_refingerprint(
                    BASELINE,
                    "max-expanded-nodes=60000",
                    "max-expanded-nodes=60001",
                ),
                "serialized solver configuration differs",
            ),
            (
                mutate_and_refingerprint(
                    BASELINE,
                    "perturbation-grace-ticks=12",
                    "perturbation-grace-ticks=13",
                ),
                "serialized difficulty configuration differs",
            ),
        ] {
            let error = parse_manifest(&changed).unwrap_err();
            assert!(
                error.to_string().contains(expected_error),
                "unexpected error for mutation: {error}"
            );
        }
    }

    #[test]
    fn historical_solver_identity_is_version_bound_and_replay_supported() {
        let manifest = parse_manifest(BASELINE).unwrap();
        assert_eq!(
            manifest.identity().solver_policy_version,
            SOLVER_POLICY_VERSION
        );

        let config = SolverConfig::for_abilities(AbilitySet::NONE);
        assert_eq!(
            solver_config_identity(&config, SOLVER_POLICY_VERSION),
            manifest.identity().solver_config_id
        );
        assert_ne!(
            solver_config_identity(&config, SOLVER_POLICY_VERSION - 1),
            solver_config_identity(&config, SOLVER_POLICY_VERSION)
        );
    }

    #[test]
    fn current_route_fairness_and_selection_policies_are_required() {
        for changed in [
            mutate_and_refingerprint(BASELINE, "ordinary-jumps-1-to-3", "ordinary-jumps-1-to-4"),
            mutate_and_refingerprint(
                BASELINE,
                "robustness-success-floor=1/4",
                "robustness-success-floor=0/4",
            ),
            mutate_and_refingerprint(
                BASELINE,
                "distinct-static-visuals=true",
                "distinct-static-visuals=false",
            ),
        ] {
            let error = parse_manifest(&changed).unwrap_err();
            assert!(
                error.to_string().contains("expected"),
                "unexpected policy mutation error: {error}"
            );
        }
    }

    #[test]
    fn pickup_matrix_must_name_every_regenerated_pickup_exactly() {
        let changed_body =
            BASELINE.replace("pickup=\"optional-cache\"", "pickup=\"counterfeit-cache\"");
        assert_ne!(changed_body, BASELINE);
        let changed = refingerprint(changed_body);

        let error = parse_manifest(&changed).unwrap_err();
        assert!(
            error.to_string().contains("different pickup ids"),
            "unexpected pickup identity error: {error}"
        );
    }

    #[test]
    fn action_rle_is_compact_and_strict() {
        let replay = parse_actions(
            "representative-actions encoding=semantic-rle-v1 fields=move-x:move-y:jump:dash:restart*ticks total-ticks=32 spans=2 data=\"1:0:0:0:0*24,1:0:1:0:0*8\"",
            "representative-actions",
        ).unwrap();
        assert_eq!(replay.total_ticks(), 32);
        assert_eq!(replay.spans().len(), 2);
        assert!(parse_actions(
            "representative-actions encoding=semantic-rle-v1 fields=move-x:move-y:jump:dash:restart*ticks total-ticks=1 spans=1 data=\"0:0:0:0:1*1\"",
            "representative-actions",
        ).is_err());
    }
}
