//! Cheap shared route-matrix evidence for final-path physical rooms.

use std::{collections::BTreeSet, error::Error, fmt};

use downwards_ai::{
    ActionMacro, DifficultyConfig, ReachedTarget, SOLVER_POLICY_VERSION, SearchStats, SearchTarget,
    SolverConfig,
};
use downwards_core::Action;
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, DoorTargetEvidenceError, ValidationConfig,
    WITNESS_FINGERPRINT_VERSION, evaluate_generated_door_targets_for_loadout,
};
use serde::{Deserialize, Serialize};

use super::{
    AbilityPromotionDecisionV2, CORPUS_ABILITY_PROMOTION_GATE_VERSION, CorpusBuildConfigV2,
    CorpusCandidate, CorpusCandidateKeyRecord, CorpusCandidateRegenerationError,
    CorpusRoomAnalysisConfig, CorpusRoomAnalysisConfigRecord, EvaluationLoadout,
    GeneratedCorpusBatchV2, GeneratedCorpusRoomV2, GenerationBatchSummaryV2, LoadoutRouteMatrix,
    RoomId, RouteMatrixSummary, VariantAbilityPromotionEvidenceV2, VariantAbilityPromotionGateV2,
    evaluate_variant_ability_promotion_gate_v2, rerun_validate_variant_ability_promotion_gate_v2,
    validate_variant_ability_promotion_gate_v2,
};

/// Version of all-target feasibility states in this slice.
pub const CORPUS_FEASIBILITY_GATE_VERSION: u32 = 1;

/// Version of the explicit post-feasibility canonical-regeneration policy.
pub const CORPUS_CANONICAL_REGENERATION_POLICY_VERSION: u32 = 2;

/// Version of the canonical identity for one materialized route-evaluation
/// configuration.
pub const CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEvaluationActionV2 {
    pub move_x: i8,
    pub move_y: i8,
    pub jump: bool,
    pub dash: bool,
    pub restart: bool,
}

impl From<Action> for RouteEvaluationActionV2 {
    fn from(action: Action) -> Self {
        Self {
            move_x: action.move_x,
            move_y: action.move_y,
            jump: action.jump,
            dash: action.dash,
            restart: action.restart,
        }
    }
}

impl From<RouteEvaluationActionV2> for Action {
    fn from(action: RouteEvaluationActionV2) -> Self {
        Self {
            move_x: action.move_x,
            move_y: action.move_y,
            jump: action.jump,
            dash: action.dash,
            restart: action.restart,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEvaluationMacroV2 {
    pub name: String,
    pub actions: Vec<RouteEvaluationActionV2>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEvaluationSolverConfigV2 {
    pub max_expanded_nodes: usize,
    pub max_simulated_ticks: usize,
    pub max_ticks_per_path: usize,
    pub beam_width: usize,
    pub position_quantum: i32,
    pub velocity_quantum: i32,
    pub probe_direct_routes: bool,
    pub baseline_preview_max_expanded_nodes: usize,
    pub baseline_preview_max_simulated_ticks: usize,
    pub macros: Vec<RouteEvaluationMacroV2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEvaluationDifficultyConfigV2 {
    pub perturbation_grace_ticks: usize,
}

/// Exact solver/difficulty inputs materialized once for one loadout.
///
/// Keeping this in the evaluated batch prevents a stateful configuration
/// callback from silently changing search bounds between physical rooms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEvaluationConfigV2 {
    pub config_id_version: u32,
    pub solver_policy_version: u32,
    pub witness_fingerprint_version: u32,
    pub config_id: String,
    pub loadout: EvaluationLoadout,
    pub solver: RouteEvaluationSolverConfigV2,
    pub difficulty: RouteEvaluationDifficultyConfigV2,
}

impl RouteEvaluationConfigV2 {
    pub fn from_validation_config(
        loadout: EvaluationLoadout,
        config: &ValidationConfig,
    ) -> Result<Self, RouteEvaluationConfigV2Error> {
        let mut record = Self {
            config_id_version: CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION,
            solver_policy_version: SOLVER_POLICY_VERSION,
            witness_fingerprint_version: WITNESS_FINGERPRINT_VERSION,
            config_id: String::new(),
            loadout,
            solver: RouteEvaluationSolverConfigV2::from(&config.solver),
            difficulty: RouteEvaluationDifficultyConfigV2::from(config.difficulty),
        };
        record.config_id = record.recomputed_config_id();
        record.validate()?;
        Ok(record)
    }

    #[must_use]
    pub fn to_validation_config(&self) -> ValidationConfig {
        ValidationConfig {
            solver: self.solver.to_solver_config(),
            difficulty: DifficultyConfig {
                perturbation_grace_ticks: self.difficulty.perturbation_grace_ticks,
            },
        }
    }

    pub fn validate(&self) -> Result<(), RouteEvaluationConfigV2Error> {
        if self.config_id_version != CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION {
            return Err(RouteEvaluationConfigV2Error::UnsupportedConfigIdVersion {
                expected: CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION,
                actual: self.config_id_version,
            });
        }
        if self.solver_policy_version != SOLVER_POLICY_VERSION {
            return Err(
                RouteEvaluationConfigV2Error::UnsupportedSolverPolicyVersion {
                    expected: SOLVER_POLICY_VERSION,
                    actual: self.solver_policy_version,
                },
            );
        }
        if self.witness_fingerprint_version != WITNESS_FINGERPRINT_VERSION {
            return Err(
                RouteEvaluationConfigV2Error::UnsupportedWitnessFingerprintVersion {
                    expected: WITNESS_FINGERPRINT_VERSION,
                    actual: self.witness_fingerprint_version,
                },
            );
        }
        if self.solver.max_ticks_per_path == 0
            || self.solver.beam_width == 0
            || self.solver.position_quantum <= 0
            || self.solver.velocity_quantum <= 0
            || self.solver.macros.is_empty()
        {
            return Err(RouteEvaluationConfigV2Error::InvalidSolverConfig);
        }
        for (macro_index, action_macro) in self.solver.macros.iter().enumerate() {
            if action_macro.name.is_empty() || action_macro.actions.is_empty() {
                return Err(RouteEvaluationConfigV2Error::InvalidMacro { macro_index });
            }
            for action in &action_macro.actions {
                if action.restart {
                    return Err(RouteEvaluationConfigV2Error::RestartAction { macro_index });
                }
                if !self.loadout.abilities().dash && action.dash {
                    return Err(RouteEvaluationConfigV2Error::LoadoutIncompatibleDashMacro {
                        loadout: self.loadout,
                        macro_index,
                    });
                }
            }
        }
        let expected = self.recomputed_config_id();
        if self.config_id != expected {
            return Err(RouteEvaluationConfigV2Error::ConfigIdMismatch {
                expected,
                actual: self.config_id.clone(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn recomputed_config_id(&self) -> String {
        let mut hash = ConfigIdentityHash::new();
        hash.bytes(b"downwards-corpus-route-evaluation-config");
        hash.u32(self.config_id_version);
        hash.u32(self.solver_policy_version);
        hash.u32(self.witness_fingerprint_version);
        hash.byte(match self.loadout {
            EvaluationLoadout::Baseline => 0,
            EvaluationLoadout::WallJump => 1,
            EvaluationLoadout::Dash => 2,
            EvaluationLoadout::Both => 3,
        });
        hash.usize(self.solver.max_expanded_nodes);
        hash.usize(self.solver.max_simulated_ticks);
        hash.usize(self.solver.max_ticks_per_path);
        hash.usize(self.solver.beam_width);
        hash.i32(self.solver.position_quantum);
        hash.i32(self.solver.velocity_quantum);
        hash.bool(self.solver.probe_direct_routes);
        hash.usize(self.solver.baseline_preview_max_expanded_nodes);
        hash.usize(self.solver.baseline_preview_max_simulated_ticks);
        hash.usize(self.solver.macros.len());
        for action_macro in &self.solver.macros {
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
        hash.usize(self.difficulty.perturbation_grace_ticks);
        format!(
            "downwards-corpus-route-evaluation-config-v{}-{:016x}",
            self.config_id_version,
            hash.finish()
        )
    }
}

impl From<&SolverConfig> for RouteEvaluationSolverConfigV2 {
    fn from(config: &SolverConfig) -> Self {
        Self {
            max_expanded_nodes: config.max_expanded_nodes,
            max_simulated_ticks: config.max_simulated_ticks,
            max_ticks_per_path: config.max_ticks_per_path,
            beam_width: config.beam_width,
            position_quantum: config.position_quantum,
            velocity_quantum: config.velocity_quantum,
            probe_direct_routes: config.probe_direct_routes,
            baseline_preview_max_expanded_nodes: config.baseline_preview_max_expanded_nodes,
            baseline_preview_max_simulated_ticks: config.baseline_preview_max_simulated_ticks,
            macros: config
                .macros
                .iter()
                .map(|action_macro| RouteEvaluationMacroV2 {
                    name: action_macro.name.clone(),
                    actions: action_macro
                        .actions
                        .iter()
                        .copied()
                        .map(RouteEvaluationActionV2::from)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl RouteEvaluationSolverConfigV2 {
    #[must_use]
    pub fn to_solver_config(&self) -> SolverConfig {
        SolverConfig {
            max_expanded_nodes: self.max_expanded_nodes,
            max_simulated_ticks: self.max_simulated_ticks,
            max_ticks_per_path: self.max_ticks_per_path,
            beam_width: self.beam_width,
            position_quantum: self.position_quantum,
            velocity_quantum: self.velocity_quantum,
            probe_direct_routes: self.probe_direct_routes,
            baseline_preview_max_expanded_nodes: self.baseline_preview_max_expanded_nodes,
            baseline_preview_max_simulated_ticks: self.baseline_preview_max_simulated_ticks,
            macros: self
                .macros
                .iter()
                .map(|action_macro| ActionMacro {
                    name: action_macro.name.clone(),
                    actions: action_macro
                        .actions
                        .iter()
                        .copied()
                        .map(Action::from)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<DifficultyConfig> for RouteEvaluationDifficultyConfigV2 {
    fn from(config: DifficultyConfig) -> Self {
        Self {
            perturbation_grace_ticks: config.perturbation_grace_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteEvaluationConfigV2Error {
    UnsupportedConfigIdVersion {
        expected: u32,
        actual: u32,
    },
    UnsupportedSolverPolicyVersion {
        expected: u32,
        actual: u32,
    },
    UnsupportedWitnessFingerprintVersion {
        expected: u32,
        actual: u32,
    },
    InvalidSolverConfig,
    InvalidMacro {
        macro_index: usize,
    },
    RestartAction {
        macro_index: usize,
    },
    LoadoutIncompatibleDashMacro {
        loadout: EvaluationLoadout,
        macro_index: usize,
    },
    ConfigIdMismatch {
        expected: String,
        actual: String,
    },
    MissingOrDuplicateLoadout {
        loadout: EvaluationLoadout,
        count: usize,
    },
    NonCanonicalLoadoutOrder,
}

impl fmt::Display for RouteEvaluationConfigV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfigIdVersion { expected, actual } => write!(
                formatter,
                "route-evaluation config ID version {actual} is unsupported; expected {expected}"
            ),
            Self::UnsupportedSolverPolicyVersion { expected, actual } => write!(
                formatter,
                "route-evaluation solver policy version {actual} is unsupported; expected {expected}"
            ),
            Self::UnsupportedWitnessFingerprintVersion { expected, actual } => write!(
                formatter,
                "route-evaluation witness fingerprint version {actual} is unsupported; expected {expected}"
            ),
            Self::InvalidSolverConfig => {
                formatter.write_str("route-evaluation solver config is structurally invalid")
            }
            Self::InvalidMacro { macro_index } => write!(
                formatter,
                "route-evaluation solver macro {macro_index} has an empty name or action sequence"
            ),
            Self::RestartAction { macro_index } => write!(
                formatter,
                "route-evaluation solver macro {macro_index} contains restart"
            ),
            Self::LoadoutIncompatibleDashMacro {
                loadout,
                macro_index,
            } => write!(
                formatter,
                "route-evaluation solver macro {macro_index} requests dash under {}",
                loadout.slug()
            ),
            Self::ConfigIdMismatch { expected, actual } => write!(
                formatter,
                "route-evaluation config ID mismatch: {actual:?} != {expected:?}"
            ),
            Self::MissingOrDuplicateLoadout { loadout, count } => write!(
                formatter,
                "route-evaluation config table has {count} entries for {} instead of one",
                loadout.slug()
            ),
            Self::NonCanonicalLoadoutOrder => {
                formatter.write_str("route-evaluation configs are not in canonical loadout order")
            }
        }
    }
}

impl Error for RouteEvaluationConfigV2Error {}

pub fn validate_route_evaluation_configs_v2(
    configs: &[RouteEvaluationConfigV2],
) -> Result<(), RouteEvaluationConfigV2Error> {
    for config in configs {
        config.validate()?;
    }
    for loadout in EvaluationLoadout::ALL {
        let count = configs
            .iter()
            .filter(|config| config.loadout == loadout)
            .count();
        if count != 1 {
            return Err(RouteEvaluationConfigV2Error::MissingOrDuplicateLoadout { loadout, count });
        }
    }
    if configs
        .iter()
        .map(|config| config.loadout)
        .ne(EvaluationLoadout::ALL)
    {
        return Err(RouteEvaluationConfigV2Error::NonCanonicalLoadoutOrder);
    }
    Ok(())
}

struct ConfigIdentityHash(u64);

impl ConfigIdentityHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn byte(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn i8(&mut self, value: i8) {
        self.byte(value.cast_unsigned());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.bytes(&(value as u64).to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.usize(value.len());
        self.bytes(value.as_bytes());
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

/// A bounded all-target gate. Non-success is deliberately inconclusive; this
/// type has no unreachable or impossible state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum CorpusFeasibilityGateState {
    ReplayCertifiedAllTargets,
    BoundedInconclusive {
        door_rows: usize,
        positive_door_rows: usize,
        pickup_rows: usize,
        positive_pickup_rows: usize,
    },
}

impl CorpusFeasibilityGateState {
    #[must_use]
    pub const fn passes(self) -> bool {
        matches!(self, Self::ReplayCertifiedAllTargets)
    }

    fn from_summary(summary: RouteMatrixSummary) -> Self {
        if summary.all_positive() {
            Self::ReplayCertifiedAllTargets
        } else {
            Self::BoundedInconclusive {
                door_rows: summary.door_rows,
                positive_door_rows: summary.positive_door_rows,
                pickup_rows: summary.pickup_rows,
                positive_pickup_rows: summary.positive_pickup_rows,
            }
        }
    }
}

/// Construction-loadout feasibility for one native alias.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantConstructionGateV2 {
    pub gate_version: u32,
    pub key: CorpusCandidateKeyRecord,
    pub construction_loadout: EvaluationLoadout,
    pub state: CorpusFeasibilityGateState,
}

/// The only canonical choice in this slice, applied after complete-kit,
/// construction-loadout, and variant-specific promotion gates pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CanonicalRegenerationPolicy {
    LexicographicallySmallestExactKeyPassingAllGates,
}

impl CanonicalRegenerationPolicy {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::LexicographicallySmallestExactKeyPassingAllGates => {
                "lexicographically-smallest-exact-key-passing-all-gates"
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalRegenerationSelectionV2 {
    pub policy_version: u32,
    pub policy: CanonicalRegenerationPolicy,
    pub selected_key: Option<CorpusCandidateKeyRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluatedCorpusRoomV2 {
    pub generated: GeneratedCorpusRoomV2,
    /// Exact alias used only to carry room-local object labels into the one
    /// shared physical evidence run. It is not the canonical alias.
    pub physical_evidence_source_key: CorpusCandidateKeyRecord,
    pub matrices: Vec<LoadoutRouteMatrix>,
    pub variant_construction_gates: Vec<VariantConstructionGateV2>,
    pub ability_promotion_audit_config: CorpusRoomAnalysisConfigRecord,
    pub variant_ability_promotion_gates: Vec<VariantAbilityPromotionGateV2>,
    pub complete_kit_gate: CorpusFeasibilityGateState,
    pub canonical_regeneration: CanonicalRegenerationSelectionV2,
}

/// Validate every identity, ordering, cardinality, summary, and feasibility
/// claim retained for one final-path evaluated room.
///
/// This is the shared trust boundary for freshly evaluated rooms and
/// replay-rehydrated artifacts. In particular, gate rows and the canonical
/// selection are derived data: callers must never accept them merely because
/// they are self-consistent with each other.
pub fn validate_evaluated_corpus_room_v2(
    evaluated: &EvaluatedCorpusRoomV2,
) -> Result<(), CorpusMetricInputV2Error> {
    let room_id = &evaluated.generated.id;
    let invalid = |detail: String| CorpusMetricInputV2Error::InvalidIdentity {
        room_id: room_id.clone(),
        detail,
    };

    if evaluated.generated.variants.is_empty() {
        return Err(invalid("the retained native alias set is empty".to_owned()));
    }
    if evaluated.generated.physical_descriptor.room_id() != *room_id {
        return Err(invalid(
            "the room-v3 ID was not derived from the stored exact physical descriptor pair"
                .to_owned(),
        ));
    }
    if evaluated
        .generated
        .variants
        .windows(2)
        .any(|pair| pair[0].exact_key().stable_slug() >= pair[1].exact_key().stable_slug())
    {
        return Err(invalid(
            "retained native aliases are not in strict distinct exact-key order".to_owned(),
        ));
    }
    for candidate in &evaluated.generated.variants {
        let key = candidate.exact_key();
        if candidate.physical_room_descriptor_v3() != evaluated.generated.physical_descriptor {
            return Err(invalid(format!(
                "retained candidate {} does not equal the stored physical descriptor pair",
                key.stable_slug()
            )));
        }
        if candidate.construction_abilities() != key.construction_loadout().abilities() {
            return Err(invalid(format!(
                "retained candidate {} disagrees with its construction loadout",
                key.stable_slug()
            )));
        }
        let regenerated =
            key.regenerate()
                .map_err(|source| CorpusMetricInputV2Error::CanonicalRegeneration {
                    room_id: room_id.clone(),
                    key: Box::new(key.clone()),
                    source: Box::new(source),
                })?;
        if regenerated != *candidate {
            return Err(invalid(format!(
                "retained candidate {} does not regenerate from its exact key",
                key.stable_slug()
            )));
        }
    }

    let evidence_source = evaluated
        .generated
        .variants
        .first()
        .expect("nonempty alias set was checked");
    if evaluated.physical_evidence_source_key != evidence_source.exact_key() {
        return Err(invalid(format!(
            "physical evidence source {} is not the canonical least exact key {}",
            evaluated.physical_evidence_source_key.stable_slug(),
            evidence_source.exact_key().stable_slug()
        )));
    }

    let generated_level = evidence_source.generated();
    let mut door_ids = generated_level
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    if door_ids.len() < 2 {
        return Err(invalid(format!(
            "physical evidence source has {} doors instead of at least two",
            door_ids.len()
        )));
    }
    if door_ids
        .iter()
        .any(|door_id| door_id.is_empty() || door_id.trim() != door_id)
    {
        return Err(invalid(
            "physical evidence source has an empty or noncanonical door ID".to_owned(),
        ));
    }
    if door_ids.iter().collect::<BTreeSet<_>>().len() != door_ids.len() {
        return Err(invalid(
            "physical evidence source has duplicate door IDs".to_owned(),
        ));
    }
    door_ids.sort_unstable();
    let mut pickup_ids = generated_level
        .room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect::<Vec<_>>();
    if pickup_ids.iter().collect::<BTreeSet<_>>().len() != pickup_ids.len() {
        return Err(invalid(
            "physical evidence source has duplicate pickup IDs".to_owned(),
        ));
    }
    pickup_ids.sort_unstable();

    if evaluated
        .matrices
        .iter()
        .map(|matrix| matrix.loadout)
        .ne(EvaluationLoadout::ALL)
    {
        return Err(invalid(
            "route matrices are not exactly the four canonical loadouts in canonical order"
                .to_owned(),
        ));
    }
    for matrix in &evaluated.matrices {
        if matrix.evidence.loadout() != matrix.loadout.abilities() {
            return Err(invalid(format!(
                "{} matrix evidence uses a different traversal loadout",
                matrix.loadout.slug()
            )));
        }
        let expected_door_coordinates = door_ids.iter().flat_map(|source_door_id| {
            door_ids
                .iter()
                .filter(move |target_door_id| *target_door_id != source_door_id)
                .map(move |target_door_id| (source_door_id.as_str(), target_door_id.as_str()))
        });
        if matrix
            .evidence
            .door_routes()
            .iter()
            .map(|row| (row.source_door_id.as_str(), row.target_door_id.as_str()))
            .ne(expected_door_coordinates)
        {
            return Err(invalid(format!(
                "{} door rows do not have exact canonical source/target coordinates",
                matrix.loadout.slug()
            )));
        }
        let expected_pickup_coordinates = door_ids.iter().flat_map(|source_door_id| {
            pickup_ids
                .iter()
                .map(move |pickup_id| (source_door_id.as_str(), pickup_id.as_str()))
        });
        if matrix
            .evidence
            .pickup_routes()
            .iter()
            .map(|row| (row.source_door_id.as_str(), row.required_pickup_id.as_str()))
            .ne(expected_pickup_coordinates)
        {
            return Err(invalid(format!(
                "{} pickup rows do not have exact canonical source/target coordinates",
                matrix.loadout.slug()
            )));
        }
        if matrix
            .evidence
            .source_search_effort()
            .iter()
            .map(|source| source.source_door_id.as_str())
            .ne(door_ids.iter().map(String::as_str))
        {
            return Err(invalid(format!(
                "{} per-source effort rows are not in exact canonical door order",
                matrix.loadout.slug()
            )));
        }
        let mut aggregate = SearchStats::default();
        for source in matrix.evidence.source_search_effort() {
            accumulate_search_stats_v2(&mut aggregate, source.stats);
        }
        if aggregate != matrix.evidence.aggregate_search_effort() {
            return Err(invalid(format!(
                "{} aggregate search effort differs from its per-source sum/max",
                matrix.loadout.slug()
            )));
        }
        for row in matrix.evidence.door_routes() {
            if let BoundedTargetEvidence::Positive(positive) = &row.evidence {
                let solution = positive.solution();
                if solution.target != SearchTarget::Door(row.target_door_id.clone())
                    || solution.reached != ReachedTarget::Door(row.target_door_id.clone())
                {
                    return Err(invalid(format!(
                        "{} positive door row {:?} -> {:?} carries a different target identity",
                        matrix.loadout.slug(),
                        row.source_door_id,
                        row.target_door_id
                    )));
                }
            }
        }
        for row in matrix.evidence.pickup_routes() {
            if let BoundedTargetEvidence::Positive(positive) = &row.evidence {
                let solution = positive.solution();
                if solution.target != SearchTarget::Pickup(row.required_pickup_id.clone())
                    || solution.reached != ReachedTarget::Pickup(row.required_pickup_id.clone())
                {
                    return Err(invalid(format!(
                        "{} positive pickup row {:?} -> {:?} carries a different target identity",
                        matrix.loadout.slug(),
                        row.source_door_id,
                        row.required_pickup_id
                    )));
                }
            }
        }
        let expected_summary = summarize_evidence(&matrix.evidence);
        if matrix.summary != expected_summary {
            return Err(invalid(format!(
                "{} stored matrix summary differs from its exact evidence rows",
                matrix.loadout.slug()
            )));
        }
    }

    let expected_gates = evaluated
        .generated
        .variants
        .iter()
        .map(|candidate| {
            let key = candidate.exact_key();
            let construction_loadout = key.construction_loadout();
            let matrix = &evaluated.matrices[EvaluationLoadout::ALL
                .iter()
                .position(|loadout| *loadout == construction_loadout)
                .expect("all four canonical matrices were checked")];
            VariantConstructionGateV2 {
                gate_version: CORPUS_FEASIBILITY_GATE_VERSION,
                key,
                construction_loadout,
                state: CorpusFeasibilityGateState::from_summary(matrix.summary),
            }
        })
        .collect::<Vec<_>>();
    if evaluated.variant_construction_gates != expected_gates {
        return Err(invalid(
            "variant construction gates differ from the canonical alias/matrix derivation"
                .to_owned(),
        ));
    }
    let both_matrix = evaluated
        .matrices
        .iter()
        .find(|matrix| matrix.loadout == EvaluationLoadout::Both)
        .expect("all four canonical matrices were checked");
    let expected_complete_kit_gate = CorpusFeasibilityGateState::from_summary(both_matrix.summary);
    if evaluated.complete_kit_gate != expected_complete_kit_gate {
        return Err(invalid(
            "complete-kit gate differs from the exact Both matrix summary".to_owned(),
        ));
    }
    evaluated
        .ability_promotion_audit_config
        .validate()
        .map_err(|error| {
            invalid(format!(
                "ability-promotion audit config is invalid: {error}"
            ))
        })?;
    if evaluated.variant_ability_promotion_gates.len() != evaluated.generated.variants.len() {
        return Err(invalid(format!(
            "ability-promotion gate count {} differs from native alias count {}",
            evaluated.variant_ability_promotion_gates.len(),
            evaluated.generated.variants.len()
        )));
    }
    for (candidate, gate) in evaluated
        .generated
        .variants
        .iter()
        .zip(&evaluated.variant_ability_promotion_gates)
    {
        if gate.key != candidate.exact_key() {
            return Err(invalid(format!(
                "ability-promotion gate {} is not aligned with native alias {}",
                gate.key.stable_slug(),
                candidate.exact_key().stable_slug()
            )));
        }
        validate_variant_ability_promotion_gate_v2(
            candidate,
            evidence_source,
            &evaluated.matrices,
            gate,
        )
        .map_err(|error| {
            invalid(format!(
                "ability-promotion gate for {} is invalid: {error}",
                candidate.exact_key().stable_slug()
            ))
        })?;
    }
    let expected_selection = select_canonical_regeneration_v2(
        &expected_gates,
        &evaluated.variant_ability_promotion_gates,
        expected_complete_kit_gate,
    );
    if evaluated.canonical_regeneration != expected_selection {
        return Err(invalid(
            "canonical regeneration selection differs from the exact gate-derived selection"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Production trust seam for a complete evaluated room. This first applies
/// the retained-evidence validator, then reruns every native ability alias's
/// advertised-pair finite audit under the stored content-addressed config.
/// Ordinary aliases are rederived as NotApplicable without running an audit.
pub fn rerun_validate_evaluated_ability_promotions_v2(
    evaluated: &EvaluatedCorpusRoomV2,
) -> Result<(), CorpusMetricInputV2Error> {
    validate_evaluated_corpus_room_v2(evaluated)?;
    let room_id = &evaluated.generated.id;
    let evidence_source = evaluated
        .generated
        .variants
        .first()
        .expect("the full evaluated-room validator required a nonempty alias set");
    for (candidate, gate) in evaluated
        .generated
        .variants
        .iter()
        .zip(&evaluated.variant_ability_promotion_gates)
    {
        rerun_validate_variant_ability_promotion_gate_v2(
            candidate,
            evidence_source,
            &evaluated.matrices,
            &evaluated.ability_promotion_audit_config,
            gate,
        )
        .map_err(|error| CorpusMetricInputV2Error::InvalidIdentity {
            room_id: room_id.clone(),
            detail: format!(
                "ability-promotion audit rerun for {} failed: {error}",
                candidate.exact_key().stable_slug()
            ),
        })?;
    }
    Ok(())
}

fn accumulate_search_stats_v2(aggregate: &mut SearchStats, source: SearchStats) {
    aggregate.expanded_nodes = aggregate
        .expanded_nodes
        .saturating_add(source.expanded_nodes);
    aggregate.generated_nodes = aggregate
        .generated_nodes
        .saturating_add(source.generated_nodes);
    aggregate.simulated_ticks = aggregate
        .simulated_ticks
        .saturating_add(source.simulated_ticks);
    aggregate.deepest_path_ticks = aggregate.deepest_path_ticks.max(source.deepest_path_ticks);
}

/// Resolve the one native candidate on which generator-neutral deep metrics
/// may operate.
///
/// This is deliberately stricter than matching a compact room ID or the two
/// physical descriptors. It validates every retained alias against the exact
/// descriptor pair, rederives the current post-feasibility canonical
/// selection from its gates, regenerates that exact key, and requires exact
/// [`downwards_core::Room`] equality between the selected candidate and the
/// matrix evidence source. Nonselected aliases may legitimately differ in
/// presentation-only room labels/IDs omitted from physical grouping. This
/// helper does not inspect route-matrix rows; consumers must still validate
/// exact loadout, source, and target identities before interpreting evidence.
pub fn resolve_corpus_metric_candidate_v2(
    evaluated: &EvaluatedCorpusRoomV2,
) -> Result<&CorpusCandidate, CorpusMetricInputV2Error> {
    validate_evaluated_corpus_room_v2(evaluated)?;
    let room_id = &evaluated.generated.id;
    let invalid = |detail: String| CorpusMetricInputV2Error::InvalidIdentity {
        room_id: room_id.clone(),
        detail,
    };

    let selected_key = evaluated
        .canonical_regeneration
        .selected_key
        .as_ref()
        .ok_or_else(|| {
            invalid(
                "the physical room has no post-feasibility canonical regeneration key".to_owned(),
            )
        })?;

    let matching_gates = evaluated
        .variant_construction_gates
        .iter()
        .filter(|gate| gate.key == *selected_key)
        .collect::<Vec<_>>();
    let [selected_gate] = matching_gates.as_slice() else {
        return Err(invalid(format!(
            "selected key {} has {} construction-gate rows instead of one",
            selected_key.stable_slug(),
            matching_gates.len()
        )));
    };
    if selected_gate.gate_version != CORPUS_FEASIBILITY_GATE_VERSION
        || selected_gate.construction_loadout != selected_key.construction_loadout()
        || !selected_gate.state.passes()
        || !evaluated.complete_kit_gate.passes()
    {
        return Err(invalid(format!(
            "selected key {} lacks current positive construction and complete-kit gates",
            selected_key.stable_slug()
        )));
    }

    let matching_candidates = evaluated
        .generated
        .variants
        .iter()
        .filter(|candidate| candidate.exact_key() == *selected_key)
        .collect::<Vec<_>>();
    let [selected] = matching_candidates.as_slice() else {
        return Err(invalid(format!(
            "selected key {} names {} retained candidates instead of one",
            selected_key.stable_slug(),
            matching_candidates.len()
        )));
    };
    if selected.construction_abilities() != selected_key.construction_loadout().abilities() {
        return Err(invalid(format!(
            "selected key {} disagrees with generated construction abilities",
            selected_key.stable_slug()
        )));
    }
    let regenerated = selected_key.regenerate().map_err(|source| {
        CorpusMetricInputV2Error::CanonicalRegeneration {
            room_id: room_id.clone(),
            key: Box::new(selected_key.clone()),
            source: Box::new(source),
        }
    })?;
    if regenerated != **selected {
        return Err(invalid(format!(
            "selected key {} does not regenerate the retained native candidate exactly",
            selected_key.stable_slug()
        )));
    }

    let evidence_sources = evaluated
        .generated
        .variants
        .iter()
        .filter(|candidate| candidate.exact_key() == evaluated.physical_evidence_source_key)
        .collect::<Vec<_>>();
    let [evidence_source] = evidence_sources.as_slice() else {
        return Err(invalid(format!(
            "physical evidence source {} names {} retained candidates instead of one",
            evaluated.physical_evidence_source_key.stable_slug(),
            evidence_sources.len()
        )));
    };
    if evidence_source.generated().room != selected.generated().room {
        return Err(invalid(format!(
            "physical evidence source {} does not equal the selected candidate's exact Room",
            evaluated.physical_evidence_source_key.stable_slug()
        )));
    }

    Ok(selected)
}

/// Failure to bind a deep metric to one exact final-path physical room and
/// post-feasibility canonical native candidate.
#[derive(Debug)]
pub enum CorpusMetricInputV2Error {
    InvalidIdentity {
        room_id: RoomId,
        detail: String,
    },
    CanonicalRegeneration {
        room_id: RoomId,
        key: Box<CorpusCandidateKeyRecord>,
        source: Box<CorpusCandidateRegenerationError>,
    },
}

impl fmt::Display for CorpusMetricInputV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity { room_id, detail } => {
                write!(
                    formatter,
                    "invalid corpus-v2 metric input {}: {detail}",
                    room_id.0
                )
            }
            Self::CanonicalRegeneration {
                room_id,
                key,
                source,
            } => write!(
                formatter,
                "cannot regenerate corpus-v2 metric candidate {} for {}: {source}",
                key.stable_slug(),
                room_id.0
            ),
        }
    }
}

impl Error for CorpusMetricInputV2Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CanonicalRegeneration { source, .. } => Some(source.as_ref()),
            Self::InvalidIdentity { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluatedCorpusBatchV2 {
    pub config: CorpusBuildConfigV2,
    pub evaluation_configs: Vec<RouteEvaluationConfigV2>,
    pub construction_records: Vec<super::CorpusConstructionAttemptV2>,
    pub generation_summary: GenerationBatchSummaryV2,
    pub rooms: Vec<EvaluatedCorpusRoomV2>,
}

pub fn evaluate_route_matrices_v2(
    generated: GeneratedCorpusBatchV2,
) -> Result<EvaluatedCorpusBatchV2, CorpusEvaluationV2Error> {
    evaluate_route_matrices_v2_with(generated, |loadout| {
        ValidationConfig::for_loadout(loadout.abilities())
    })
}

/// Evaluate all four loadouts once per physical room, then project the shared
/// evidence onto every native variant's construction loadout.
pub fn evaluate_route_matrices_v2_with(
    generated: GeneratedCorpusBatchV2,
    mut config_for_loadout: impl FnMut(EvaluationLoadout) -> ValidationConfig,
) -> Result<EvaluatedCorpusBatchV2, CorpusEvaluationV2Error> {
    let GeneratedCorpusBatchV2 {
        config,
        construction_records,
        rooms: source_rooms,
        summary: generation_summary,
    } = generated;
    let materialized_configs = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| {
            let config = config_for_loadout(loadout);
            let record = RouteEvaluationConfigV2::from_validation_config(loadout, &config)
                .map_err(|source| CorpusEvaluationV2Error::EvaluationConfig { loadout, source })?;
            Ok((record, config))
        })
        .collect::<Result<Vec<_>, CorpusEvaluationV2Error>>()?;
    let evaluation_configs = materialized_configs
        .iter()
        .map(|(record, _)| record.clone())
        .collect::<Vec<_>>();
    validate_route_evaluation_configs_v2(&evaluation_configs)
        .map_err(|source| CorpusEvaluationV2Error::EvaluationConfigTable { source })?;
    let ability_promotion_audit_config = CorpusRoomAnalysisConfig::default()
        .identity_record()
        .map_err(
            |source| CorpusEvaluationV2Error::AbilityPromotionAuditConfig {
                detail: source.to_string(),
            },
        )?;
    let mut rooms = Vec::with_capacity(source_rooms.len());
    for generated_room in source_rooms {
        validate_physical_group(&generated_room)?;
        let evidence_source = generated_room
            .variants
            .iter()
            .min_by_key(|candidate| candidate.exact_key().stable_slug())
            .expect("nonempty physical group was validated");
        let physical_evidence_source_key = evidence_source.exact_key();
        let generated_level = evidence_source.generated();
        let door_count = generated_level.room.doors().len();
        let pickup_count = generated_level.room.pickups().len();
        let expected_door_rows = door_count.saturating_mul(door_count.saturating_sub(1));
        let expected_pickup_rows = door_count.saturating_mul(pickup_count);

        let mut matrices = Vec::with_capacity(EvaluationLoadout::ALL.len());
        for (config_record, validation_config) in &materialized_configs {
            let loadout = config_record.loadout;
            let evidence = evaluate_generated_door_targets_for_loadout(
                generated_level,
                loadout.abilities(),
                validation_config,
            )
            .map_err(|source| CorpusEvaluationV2Error::Evidence {
                room_id: generated_room.id.clone(),
                evidence_source_key: physical_evidence_source_key.clone(),
                loadout,
                source: Box::new(source),
            })?;
            if evidence.door_routes().len() != expected_door_rows
                || evidence.pickup_routes().len() != expected_pickup_rows
            {
                return Err(CorpusEvaluationV2Error::MatrixCardinality {
                    room_id: generated_room.id.clone(),
                    loadout,
                    expected_door_rows,
                    actual_door_rows: evidence.door_routes().len(),
                    expected_pickup_rows,
                    actual_pickup_rows: evidence.pickup_routes().len(),
                });
            }
            let summary = summarize_evidence(&evidence);
            matrices.push(LoadoutRouteMatrix {
                loadout,
                evidence,
                summary,
            });
        }

        let variant_construction_gates = generated_room
            .variants
            .iter()
            .map(|candidate| {
                let construction_loadout = EvaluationLoadout::ALL
                    .into_iter()
                    .find(|loadout| loadout.abilities() == candidate.construction_abilities())
                    .expect("all AbilitySet combinations have an EvaluationLoadout");
                let matrix = matrices
                    .iter()
                    .find(|matrix| matrix.loadout == construction_loadout)
                    .expect("all four loadout matrices were constructed");
                VariantConstructionGateV2 {
                    gate_version: CORPUS_FEASIBILITY_GATE_VERSION,
                    key: candidate.exact_key(),
                    construction_loadout,
                    state: CorpusFeasibilityGateState::from_summary(matrix.summary),
                }
            })
            .collect::<Vec<_>>();
        let variant_ability_promotion_gates = generated_room
            .variants
            .iter()
            .map(|candidate| {
                evaluate_variant_ability_promotion_gate_v2(
                    candidate,
                    evidence_source,
                    &matrices,
                    &ability_promotion_audit_config,
                )
                .map_err(|source| CorpusEvaluationV2Error::AbilityPromotionGate {
                    room_id: generated_room.id.clone(),
                    key: candidate.exact_key(),
                    detail: source.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let complete_kit_gate = matrices
            .iter()
            .find(|matrix| matrix.loadout == EvaluationLoadout::Both)
            .map(|matrix| CorpusFeasibilityGateState::from_summary(matrix.summary))
            .expect("the complete-kit matrix was constructed");
        let canonical_regeneration = select_canonical_regeneration_v2(
            &variant_construction_gates,
            &variant_ability_promotion_gates,
            complete_kit_gate,
        );
        let evaluated_room = EvaluatedCorpusRoomV2 {
            generated: generated_room,
            physical_evidence_source_key,
            matrices,
            variant_construction_gates,
            ability_promotion_audit_config: ability_promotion_audit_config.clone(),
            variant_ability_promotion_gates,
            complete_kit_gate,
            canonical_regeneration,
        };
        validate_evaluated_corpus_room_v2(&evaluated_room).map_err(|source| {
            CorpusEvaluationV2Error::InvalidEvaluatedRoom {
                room_id: evaluated_room.generated.id.clone(),
                detail: source.to_string(),
            }
        })?;
        rooms.push(evaluated_room);
    }
    Ok(EvaluatedCorpusBatchV2 {
        config,
        evaluation_configs,
        construction_records,
        generation_summary,
        rooms,
    })
}

/// Apply the frozen canonical policy only to aliases that have passed their
/// own construction gate and belong to a physical room that passed complete
/// kit. The result is presentation/regeneration metadata, never structural
/// evidence or physical identity.
#[must_use]
pub fn select_canonical_regeneration_v2(
    variant_gates: &[VariantConstructionGateV2],
    promotion_gates: &[VariantAbilityPromotionGateV2],
    complete_kit_gate: CorpusFeasibilityGateState,
) -> CanonicalRegenerationSelectionV2 {
    let selected_key = complete_kit_gate.passes().then(|| {
        variant_gates
            .iter()
            .filter(|gate| {
                if gate.gate_version != CORPUS_FEASIBILITY_GATE_VERSION
                    || gate.construction_loadout != gate.key.construction_loadout()
                    || !gate.state.passes()
                    || variant_gates
                        .iter()
                        .filter(|other| other.key == gate.key)
                        .count()
                        != 1
                {
                    return false;
                }
                let mut matching = promotion_gates
                    .iter()
                    .filter(|promotion| promotion.key == gate.key);
                let Some(promotion) = matching.next() else {
                    return false;
                };
                if matching.next().is_some()
                    || promotion.gate_version != CORPUS_ABILITY_PROMOTION_GATE_VERSION
                {
                    return false;
                }
                matches!(
                    (&gate.key, &promotion.evidence, promotion.decision),
                    (
                        CorpusCandidateKeyRecord::CompositionalAbility(_),
                        VariantAbilityPromotionEvidenceV2::Ability { .. },
                        AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass,
                    ) | (
                        CorpusCandidateKeyRecord::PartitionRoute(_)
                            | CorpusCandidateKeyRecord::CompositionalRouteCut(_),
                        VariantAbilityPromotionEvidenceV2::NotApplicable,
                        AbilityPromotionDecisionV2::NotApplicable,
                    )
                )
            })
            .min_by_key(|gate| gate.key.stable_slug())
            .map(|gate| gate.key.clone())
    });
    CanonicalRegenerationSelectionV2 {
        policy_version: CORPUS_CANONICAL_REGENERATION_POLICY_VERSION,
        policy: CanonicalRegenerationPolicy::LexicographicallySmallestExactKeyPassingAllGates,
        selected_key: selected_key.flatten(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusEvaluationV2Error {
    EvaluationConfig {
        loadout: EvaluationLoadout,
        source: RouteEvaluationConfigV2Error,
    },
    EvaluationConfigTable {
        source: RouteEvaluationConfigV2Error,
    },
    AbilityPromotionAuditConfig {
        detail: String,
    },
    AbilityPromotionGate {
        room_id: RoomId,
        key: CorpusCandidateKeyRecord,
        detail: String,
    },
    InvalidPhysicalGroup {
        room_id: RoomId,
        detail: String,
    },
    InvalidEvaluatedRoom {
        room_id: RoomId,
        detail: String,
    },
    Evidence {
        room_id: RoomId,
        evidence_source_key: CorpusCandidateKeyRecord,
        loadout: EvaluationLoadout,
        source: Box<DoorTargetEvidenceError>,
    },
    MatrixCardinality {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        expected_door_rows: usize,
        actual_door_rows: usize,
        expected_pickup_rows: usize,
        actual_pickup_rows: usize,
    },
}

impl fmt::Display for CorpusEvaluationV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvaluationConfig { loadout, source } => write!(
                formatter,
                "invalid corpus-v2 route-evaluation config for {}: {source}",
                loadout.slug()
            ),
            Self::EvaluationConfigTable { source } => {
                write!(
                    formatter,
                    "invalid corpus-v2 route-evaluation config table: {source}"
                )
            }
            Self::AbilityPromotionAuditConfig { detail } => write!(
                formatter,
                "invalid corpus-v2 ability-promotion audit config: {detail}"
            ),
            Self::AbilityPromotionGate {
                room_id,
                key,
                detail,
            } => write!(
                formatter,
                "corpus-v2 ability-promotion gate failed for room {} alias {}: {detail}",
                room_id.0,
                key.stable_slug()
            ),
            Self::InvalidPhysicalGroup { room_id, detail } => {
                write!(
                    formatter,
                    "invalid corpus-v2 physical group {}: {detail}",
                    room_id.0
                )
            }
            Self::InvalidEvaluatedRoom { room_id, detail } => write!(
                formatter,
                "freshly evaluated corpus-v2 room {} failed full validation: {detail}",
                room_id.0
            ),
            Self::Evidence {
                room_id,
                evidence_source_key,
                loadout,
                source,
            } => write!(
                formatter,
                "shared route evidence failed for {} using {} under {}: {source}",
                room_id.0,
                evidence_source_key.stable_slug(),
                loadout.slug()
            ),
            Self::MatrixCardinality {
                room_id,
                loadout,
                expected_door_rows,
                actual_door_rows,
                expected_pickup_rows,
                actual_pickup_rows,
            } => write!(
                formatter,
                "corpus-v2 route matrix for {} under {} has {actual_door_rows}/{expected_door_rows} door rows and {actual_pickup_rows}/{expected_pickup_rows} pickup rows",
                room_id.0,
                loadout.slug()
            ),
        }
    }
}

impl Error for CorpusEvaluationV2Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EvaluationConfig { source, .. } | Self::EvaluationConfigTable { source } => {
                Some(source)
            }
            Self::Evidence { source, .. } => Some(source.as_ref()),
            Self::AbilityPromotionAuditConfig { .. }
            | Self::AbilityPromotionGate { .. }
            | Self::InvalidPhysicalGroup { .. }
            | Self::MatrixCardinality { .. }
            | Self::InvalidEvaluatedRoom { .. } => None,
        }
    }
}

fn validate_physical_group(room: &GeneratedCorpusRoomV2) -> Result<(), CorpusEvaluationV2Error> {
    if room.variants.is_empty() {
        return Err(invalid_group(room, "the alias set is empty"));
    }
    if room.id != room.physical_descriptor.room_id() {
        return Err(invalid_group(
            room,
            "the compact ID was not derived from the stored exact descriptor pair",
        ));
    }
    if room
        .variants
        .iter()
        .any(|candidate| candidate.physical_room_descriptor_v3() != room.physical_descriptor)
    {
        return Err(invalid_group(
            room,
            "a native variant does not equal the group's complete physical descriptor",
        ));
    }
    if room
        .variants
        .windows(2)
        .any(|pair| pair[0].exact_key().stable_slug() >= pair[1].exact_key().stable_slug())
    {
        return Err(invalid_group(
            room,
            "native variants are not strictly sorted by distinct exact keys",
        ));
    }
    Ok(())
}

fn invalid_group(room: &GeneratedCorpusRoomV2, detail: &str) -> CorpusEvaluationV2Error {
    CorpusEvaluationV2Error::InvalidPhysicalGroup {
        room_id: room.id.clone(),
        detail: detail.to_owned(),
    }
}

fn summarize_evidence(evidence: &DoorTargetEvidenceBatch) -> RouteMatrixSummary {
    let positive_door_rows = evidence
        .door_routes()
        .iter()
        .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
        .count();
    let positive_pickup_rows = evidence
        .pickup_routes()
        .iter()
        .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
        .count();
    RouteMatrixSummary {
        door_rows: evidence.door_routes().len(),
        positive_door_rows,
        inconclusive_door_rows: evidence.door_routes().len() - positive_door_rows,
        pickup_rows: evidence.pickup_routes().len(),
        positive_pickup_rows,
        inconclusive_pickup_rows: evidence.pickup_routes().len() - positive_pickup_rows,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use downwards_core::AbilitySet;
    use downwards_gen::experimental::{ChallengeIntent, PartitionRouteKey, PartitionRouteProfile};

    use super::*;
    use crate::corpus::{CorpusCandidate, GeneratedCorpusBatchV2};

    #[test]
    fn route_evaluation_config_identity_is_exact_strict_and_loadout_compatible() {
        let baseline = RouteEvaluationConfigV2::from_validation_config(
            EvaluationLoadout::Baseline,
            &ValidationConfig::for_loadout(AbilitySet::NONE),
        )
        .unwrap();
        baseline.validate().unwrap();
        let encoded = serde_json::to_string(&baseline).unwrap();
        assert_eq!(
            serde_json::from_str::<RouteEvaluationConfigV2>(&encoded).unwrap(),
            baseline
        );

        let mut changed = baseline.clone();
        changed.solver.max_expanded_nodes += 1;
        assert!(matches!(
            changed.validate(),
            Err(RouteEvaluationConfigV2Error::ConfigIdMismatch { .. })
        ));

        let mut incompatible = baseline;
        incompatible.solver.macros[0].actions[0].dash = true;
        incompatible.config_id = incompatible.recomputed_config_id();
        assert!(matches!(
            incompatible.validate(),
            Err(RouteEvaluationConfigV2Error::LoadoutIncompatibleDashMacro { .. })
        ));

        let complete = EvaluationLoadout::ALL
            .into_iter()
            .map(|loadout| {
                RouteEvaluationConfigV2::from_validation_config(
                    loadout,
                    &ValidationConfig::for_loadout(loadout.abilities()),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        validate_route_evaluation_configs_v2(&complete).unwrap();
        assert!(matches!(
            validate_route_evaluation_configs_v2(&complete[..3]),
            Err(RouteEvaluationConfigV2Error::MissingOrDuplicateLoadout {
                loadout: EvaluationLoadout::Both,
                count: 0
            })
        ));
    }

    #[test]
    fn config_callback_is_materialized_once_even_for_an_empty_room_batch() {
        let batch = GeneratedCorpusBatchV2 {
            config: CorpusBuildConfigV2::attempt_zero(0, 1),
            construction_records: Vec::new(),
            rooms: Vec::new(),
            summary: GenerationBatchSummaryV2::default(),
        };
        let calls = Cell::new(0);
        let evaluated = evaluate_route_matrices_v2_with(batch, |loadout| {
            calls.set(calls.get() + 1);
            ValidationConfig::for_loadout(loadout.abilities())
        })
        .unwrap();
        assert_eq!(calls.get(), EvaluationLoadout::ALL.len());
        validate_route_evaluation_configs_v2(&evaluated.evaluation_configs).unwrap();
    }

    #[test]
    fn one_room_runs_only_four_shared_search_matrices_with_exact_cardinality() {
        let batch = single_candidate_batch();
        let calls = Cell::new(0);
        let evaluated = evaluate_route_matrices_v2_with(batch.clone(), |loadout| {
            calls.set(calls.get() + 1);
            tiny_config(loadout)
        })
        .unwrap();
        assert_eq!(calls.get(), 4);
        let repeated = evaluate_route_matrices_v2_with(batch, tiny_config).unwrap();
        assert_eq!(evaluated, repeated);

        let room = &evaluated.rooms[0];
        assert_eq!(room.matrices.len(), 4);
        assert_eq!(room.variant_construction_gates.len(), 1);
        let candidate = &room.generated.variants[0];
        let door_count = candidate.generated().room.doors().len();
        let pickup_count = candidate.generated().room.pickups().len();
        for matrix in &room.matrices {
            assert_eq!(matrix.summary.door_rows, door_count * (door_count - 1));
            assert_eq!(matrix.summary.pickup_rows, door_count * pickup_count);
            assert_eq!(
                matrix.summary.positive_door_rows + matrix.summary.inconclusive_door_rows,
                matrix.summary.door_rows
            );
            assert_eq!(
                matrix.summary.positive_pickup_rows + matrix.summary.inconclusive_pickup_rows,
                matrix.summary.pickup_rows
            );
        }
    }

    #[test]
    fn full_room_validator_rejects_empty_matrices_and_counterfeit_gates() {
        let evaluated = evaluate_route_matrices_v2_with(single_candidate_batch(), tiny_config)
            .unwrap()
            .rooms
            .pop()
            .unwrap();
        validate_evaluated_corpus_room_v2(&evaluated).unwrap();

        let mut empty = evaluated.clone();
        empty.matrices.clear();
        assert!(matches!(
            validate_evaluated_corpus_room_v2(&empty),
            Err(CorpusMetricInputV2Error::InvalidIdentity { .. })
        ));

        let mut missing_promotion = evaluated.clone();
        missing_promotion.variant_ability_promotion_gates.clear();
        missing_promotion.canonical_regeneration = select_canonical_regeneration_v2(
            &missing_promotion.variant_construction_gates,
            &missing_promotion.variant_ability_promotion_gates,
            missing_promotion.complete_kit_gate,
        );
        assert!(matches!(
            validate_evaluated_corpus_room_v2(&missing_promotion),
            Err(CorpusMetricInputV2Error::InvalidIdentity { .. })
        ));

        let mut counterfeit_promotion = evaluated.clone();
        counterfeit_promotion.variant_ability_promotion_gates[0].decision =
            AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass;
        counterfeit_promotion.canonical_regeneration = select_canonical_regeneration_v2(
            &counterfeit_promotion.variant_construction_gates,
            &counterfeit_promotion.variant_ability_promotion_gates,
            counterfeit_promotion.complete_kit_gate,
        );
        assert!(matches!(
            validate_evaluated_corpus_room_v2(&counterfeit_promotion),
            Err(CorpusMetricInputV2Error::InvalidIdentity { .. })
        ));

        let mut counterfeit = evaluated;
        let recorded = counterfeit.variant_construction_gates[0].state;
        counterfeit.variant_construction_gates[0].state = if recorded.passes() {
            CorpusFeasibilityGateState::BoundedInconclusive {
                door_rows: 1,
                positive_door_rows: 0,
                pickup_rows: 0,
                positive_pickup_rows: 0,
            }
        } else {
            CorpusFeasibilityGateState::ReplayCertifiedAllTargets
        };
        counterfeit.canonical_regeneration = select_canonical_regeneration_v2(
            &counterfeit.variant_construction_gates,
            &counterfeit.variant_ability_promotion_gates,
            counterfeit.complete_kit_gate,
        );
        assert!(matches!(
            validate_evaluated_corpus_room_v2(&counterfeit),
            Err(CorpusMetricInputV2Error::InvalidIdentity { .. })
        ));
    }

    #[test]
    fn canonical_key_is_chosen_only_after_all_gate_layers_pass() {
        let keys = CorpusBuildConfigV2::attempt_zero(0, 1)
            .exact_candidate_keys()
            .unwrap()
            .into_iter()
            .filter(|key| !matches!(key, CorpusCandidateKeyRecord::CompositionalAbility(_)))
            .take(2)
            .collect::<Vec<_>>();
        let smaller = keys[0].clone();
        let larger = keys[1].clone();
        let incomplete = CorpusFeasibilityGateState::BoundedInconclusive {
            door_rows: 2,
            positive_door_rows: 1,
            pickup_rows: 2,
            positive_pickup_rows: 0,
        };
        let mut gates = vec![
            gate(smaller.clone(), incomplete),
            gate(
                larger.clone(),
                CorpusFeasibilityGateState::ReplayCertifiedAllTargets,
            ),
        ];
        let mut promotion_gates = vec![
            promotion_gate(smaller.clone(), AbilityPromotionDecisionV2::NotApplicable),
            promotion_gate(larger.clone(), AbilityPromotionDecisionV2::NotApplicable),
        ];

        let selection = select_canonical_regeneration_v2(
            &gates,
            &promotion_gates,
            CorpusFeasibilityGateState::BoundedInconclusive {
                door_rows: 2,
                positive_door_rows: 1,
                pickup_rows: 2,
                positive_pickup_rows: 1,
            },
        );
        assert_eq!(selection.selected_key, None);

        let selection = select_canonical_regeneration_v2(
            &gates,
            &promotion_gates,
            CorpusFeasibilityGateState::ReplayCertifiedAllTargets,
        );
        assert_eq!(selection.selected_key, Some(larger.clone()));

        gates[0].state = CorpusFeasibilityGateState::ReplayCertifiedAllTargets;
        gates.reverse();
        promotion_gates[0].decision = AbilityPromotionDecisionV2::BoundedIntendedRoute;
        let selection = select_canonical_regeneration_v2(
            &gates,
            &promotion_gates,
            CorpusFeasibilityGateState::ReplayCertifiedAllTargets,
        );
        assert_eq!(selection.selected_key, Some(larger.clone()));

        promotion_gates[0].decision = AbilityPromotionDecisionV2::NotApplicable;
        let selection = select_canonical_regeneration_v2(
            &gates,
            &promotion_gates,
            CorpusFeasibilityGateState::ReplayCertifiedAllTargets,
        );
        assert_eq!(selection.selected_key, Some(smaller));
        assert_eq!(
            selection.policy_version,
            CORPUS_CANONICAL_REGENERATION_POLICY_VERSION
        );
    }

    #[test]
    fn canonical_selector_rejects_duplicate_stale_and_key_incompatible_gate_rows() {
        let keys = CorpusBuildConfigV2::attempt_zero(0, 1)
            .exact_candidate_keys()
            .unwrap();
        let ordinary = keys
            .iter()
            .find(|key| matches!(key, CorpusCandidateKeyRecord::PartitionRoute(_)))
            .unwrap()
            .clone();
        let ability = keys
            .iter()
            .find(|key| matches!(key, CorpusCandidateKeyRecord::CompositionalAbility(_)))
            .unwrap()
            .clone();
        let complete = CorpusFeasibilityGateState::ReplayCertifiedAllTargets;

        let ordinary_construction = vec![gate(ordinary.clone(), complete)];
        let ordinary_promotion =
            promotion_gate(ordinary.clone(), AbilityPromotionDecisionV2::NotApplicable);
        assert_eq!(
            select_canonical_regeneration_v2(
                &ordinary_construction,
                &[ordinary_promotion.clone(), ordinary_promotion.clone()],
                complete,
            )
            .selected_key,
            None
        );

        let mut stale = ordinary_promotion;
        stale.gate_version = CORPUS_ABILITY_PROMOTION_GATE_VERSION - 1;
        assert_eq!(
            select_canonical_regeneration_v2(&ordinary_construction, &[stale], complete)
                .selected_key,
            None
        );

        let duplicate_construction =
            vec![gate(ordinary.clone(), complete), gate(ordinary, complete)];
        assert_eq!(
            select_canonical_regeneration_v2(
                &duplicate_construction,
                &[promotion_gate(
                    duplicate_construction[0].key.clone(),
                    AbilityPromotionDecisionV2::NotApplicable,
                )],
                complete,
            )
            .selected_key,
            None
        );

        // An ability key cannot evade promotion with an apparently benign
        // NotApplicable row.
        assert_eq!(
            select_canonical_regeneration_v2(
                &[gate(ability.clone(), complete)],
                &[promotion_gate(
                    ability,
                    AbilityPromotionDecisionV2::NotApplicable,
                )],
                complete,
            )
            .selected_key,
            None
        );
    }

    fn promotion_gate(
        key: CorpusCandidateKeyRecord,
        decision: AbilityPromotionDecisionV2,
    ) -> VariantAbilityPromotionGateV2 {
        VariantAbilityPromotionGateV2 {
            gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
            key,
            evidence: VariantAbilityPromotionEvidenceV2::NotApplicable,
            decision,
        }
    }

    fn gate(
        key: CorpusCandidateKeyRecord,
        state: CorpusFeasibilityGateState,
    ) -> VariantConstructionGateV2 {
        VariantConstructionGateV2 {
            gate_version: CORPUS_FEASIBILITY_GATE_VERSION,
            construction_loadout: key.construction_loadout(),
            key,
            state,
        }
    }

    fn tiny_config(loadout: EvaluationLoadout) -> ValidationConfig {
        let mut config = ValidationConfig::for_loadout(loadout.abilities());
        config.solver.max_expanded_nodes = 1;
        config.solver.max_simulated_ticks = 2_000;
        config
    }

    fn single_candidate_batch() -> GeneratedCorpusBatchV2 {
        let key = PartitionRouteKey::new(
            3,
            AbilitySet::NONE,
            ChallengeIntent::Standard,
            PartitionRouteProfile::MixedBsp,
        );
        let candidate = CorpusCandidate::from(key.regenerate().unwrap());
        let physical_descriptor = candidate.physical_room_descriptor_v3();
        GeneratedCorpusBatchV2 {
            config: CorpusBuildConfigV2::attempt_zero(3, 1),
            construction_records: Vec::new(),
            rooms: vec![GeneratedCorpusRoomV2 {
                id: physical_descriptor.room_id(),
                physical_descriptor,
                variants: vec![candidate],
            }],
            summary: GenerationBatchSummaryV2::default(),
        }
    }
}
