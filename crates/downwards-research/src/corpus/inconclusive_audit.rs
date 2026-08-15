//! Deterministic, staged follow-up for bounded-inconclusive corpus cells.
//!
//! The original route matrices remain the source of truth.  This audit only
//! selects cells which were inconclusive in those matrices, reruns the public
//! source-batched validation path with larger deterministic bounds, and keeps
//! the first replay-certified positive (if any).  A bounded non-success is
//! always reported as an observation about a configured search, never as an
//! impossibility claim.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

use downwards_ai::{
    ActionMacro, InconclusiveReason, SOLVER_POLICY_VERSION, SearchStats, SolverConfig,
};
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, DoorTargetEvidenceError, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};
use serde::Serialize;

use super::{
    CorpusBuildConfigV1, EvaluatedCorpusRoom, EvaluationLoadout, LoadoutRouteMatrix, RoomId,
    evaluate_route_matrices, generate_seed_block,
};

/// Version of target selection, staged retry, and report semantics.
pub const INCONCLUSIVE_AUDIT_VERSION: u32 = 1;

/// Version of the multi-seed CLI plan, checkpoints, and aggregate summary.
pub const INCONCLUSIVE_AUDIT_RUN_VERSION: u32 = 1;

/// Version of the stable identity assigned to a materialized solver config.
pub const INCONCLUSIVE_SOLVER_CONFIG_ID_VERSION: u32 = 1;

/// Interpretation boundary for persisted audit output.
pub const INCONCLUSIVE_AUDIT_DISCLAIMER: &str = "bounded non-success means that no replay-certified witness was found within the recorded deterministic search configuration; it is not proof that the route is impossible";

/// Which loadout matrices contribute initially inconclusive report targets.
///
/// Gate-critical mode deliberately puts the room's construction loadout
/// first, followed by the complete kit.  If those are the same loadout it is
/// evaluated only once.  `AllLoadouts` retains the canonical corpus order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InconclusiveAuditScope {
    ConstructionAndCompleteKit,
    AllLoadouts,
    Explicit { loadouts: Vec<EvaluationLoadout> },
}

/// Deterministic movement-macro policy for one retry stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryMacroPrecision {
    /// The solver's loadout-aware production macro vocabulary.
    Standard,
    /// Preserve the production vocabulary and append deterministic two-tick
    /// variants of its ordinary held run and jump inputs.
    StandardWithTwoTickInputs,
}

/// A loadout-aware [`SolverConfig`] template for one retry stage.
///
/// Numeric and probe fields map directly to `SolverConfig`.  Macros are
/// materialized separately for each exact loadout so a dash-capable audit can
/// never accidentally inherit a baseline-only vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditStage {
    pub id: String,
    pub max_expanded_nodes: usize,
    pub max_simulated_ticks: usize,
    pub max_ticks_per_path: usize,
    pub beam_width: usize,
    pub position_quantum: i32,
    pub velocity_quantum: i32,
    pub probe_direct_routes: bool,
    pub baseline_preview_max_expanded_nodes: usize,
    pub baseline_preview_max_simulated_ticks: usize,
    pub macro_precision: RetryMacroPrecision,
}

impl InconclusiveAuditStage {
    /// Copy every non-macro field from a solver template.
    ///
    /// The template's macro list is intentionally not copied.  The stage's
    /// macro policy is materialized for the audited loadout at execution time.
    #[must_use]
    pub fn from_solver_template(
        id: impl Into<String>,
        template: &SolverConfig,
        macro_precision: RetryMacroPrecision,
    ) -> Self {
        Self {
            id: id.into(),
            max_expanded_nodes: template.max_expanded_nodes,
            max_simulated_ticks: template.max_simulated_ticks,
            max_ticks_per_path: template.max_ticks_per_path,
            beam_width: template.beam_width,
            position_quantum: template.position_quantum,
            velocity_quantum: template.velocity_quantum,
            probe_direct_routes: template.probe_direct_routes,
            baseline_preview_max_expanded_nodes: template.baseline_preview_max_expanded_nodes,
            baseline_preview_max_simulated_ticks: template.baseline_preview_max_simulated_ticks,
            macro_precision,
        }
    }

    fn solver_for_loadout(&self, loadout: EvaluationLoadout) -> SolverConfig {
        let mut macros = SolverConfig::for_abilities(loadout.abilities()).macros;
        if self.macro_precision == RetryMacroPrecision::StandardWithTwoTickInputs {
            append_two_tick_precision_macros(&mut macros);
        }
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
            macros,
        }
    }
}

/// Complete, versioned retry policy supplied to the audit.
///
/// Policies must contain two or three stages.  Each later stage strictly
/// raises path horizon, beam width, expanded-node budget, and simulated-tick
/// budget.  Quantization may stay fixed or become finer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditPolicy {
    pub version: u32,
    pub scope: InconclusiveAuditScope,
    pub stages: Vec<InconclusiveAuditStage>,
}

impl InconclusiveAuditPolicy {
    #[must_use]
    pub fn new(scope: InconclusiveAuditScope, stages: Vec<InconclusiveAuditStage>) -> Self {
        Self {
            version: INCONCLUSIVE_AUDIT_VERSION,
            scope,
            stages,
        }
    }

    /// Current gate-focused policy for corpus failure triage.
    #[must_use]
    pub fn current() -> Self {
        let defaults = SolverConfig::default();

        let mut first = defaults.clone();
        first.max_expanded_nodes = 120_000;
        first.max_simulated_ticks = 4_000_000;
        first.max_ticks_per_path = 900;
        first.beam_width = 144;
        first.baseline_preview_max_expanded_nodes = 20_000;
        first.baseline_preview_max_simulated_ticks = 500_000;

        let mut second = defaults.clone();
        second.max_expanded_nodes = 300_000;
        second.max_simulated_ticks = 10_000_000;
        second.max_ticks_per_path = 1_200;
        second.beam_width = 256;
        second.baseline_preview_max_expanded_nodes = 50_000;
        second.baseline_preview_max_simulated_ticks = 1_250_000;

        let mut third = defaults;
        third.max_expanded_nodes = 650_000;
        third.max_simulated_ticks = 24_000_000;
        third.max_ticks_per_path = 1_800;
        third.beam_width = 384;
        third.position_quantum = (third.position_quantum / 2).max(1);
        third.velocity_quantum = (third.velocity_quantum / 2).max(1);
        third.baseline_preview_max_expanded_nodes = 100_000;
        third.baseline_preview_max_simulated_ticks = 3_000_000;

        Self::new(
            InconclusiveAuditScope::ConstructionAndCompleteKit,
            vec![
                InconclusiveAuditStage::from_solver_template(
                    "expanded-standard",
                    &first,
                    RetryMacroPrecision::Standard,
                ),
                InconclusiveAuditStage::from_solver_template(
                    "deep-standard",
                    &second,
                    RetryMacroPrecision::Standard,
                ),
                InconclusiveAuditStage::from_solver_template(
                    "fine-two-tick",
                    &third,
                    RetryMacroPrecision::StandardWithTwoTickInputs,
                ),
            ],
        )
    }

    /// Validate the policy independently of a room.
    pub fn validate(&self) -> Result<(), InconclusiveAuditPolicyError> {
        if self.version != INCONCLUSIVE_AUDIT_VERSION {
            return Err(InconclusiveAuditPolicyError::UnsupportedVersion {
                expected: INCONCLUSIVE_AUDIT_VERSION,
                actual: self.version,
            });
        }
        if !(2..=3).contains(&self.stages.len()) {
            return Err(InconclusiveAuditPolicyError::InvalidStageCount {
                actual: self.stages.len(),
            });
        }
        if let InconclusiveAuditScope::Explicit { loadouts } = &self.scope {
            if loadouts.is_empty() {
                return Err(InconclusiveAuditPolicyError::EmptyExplicitScope);
            }
            let mut seen = BTreeSet::new();
            for &loadout in loadouts {
                if !seen.insert(loadout) {
                    return Err(InconclusiveAuditPolicyError::DuplicateExplicitLoadout { loadout });
                }
            }
        }

        let mut stage_ids = BTreeSet::new();
        for stage in &self.stages {
            if stage.id.trim().is_empty() {
                return Err(InconclusiveAuditPolicyError::EmptyStageId);
            }
            if !stage_ids.insert(stage.id.clone()) {
                return Err(InconclusiveAuditPolicyError::DuplicateStageId {
                    stage_id: stage.id.clone(),
                });
            }
            if stage.max_ticks_per_path == 0 {
                return Err(InconclusiveAuditPolicyError::InvalidStageField {
                    stage_id: stage.id.clone(),
                    field: "max_ticks_per_path",
                });
            }
            if stage.beam_width == 0 {
                return Err(InconclusiveAuditPolicyError::InvalidStageField {
                    stage_id: stage.id.clone(),
                    field: "beam_width",
                });
            }
            if stage.position_quantum <= 0 {
                return Err(InconclusiveAuditPolicyError::InvalidStageField {
                    stage_id: stage.id.clone(),
                    field: "position_quantum",
                });
            }
            if stage.velocity_quantum <= 0 {
                return Err(InconclusiveAuditPolicyError::InvalidStageField {
                    stage_id: stage.id.clone(),
                    field: "velocity_quantum",
                });
            }
        }

        for pair in self.stages.windows(2) {
            let earlier = &pair[0];
            let later = &pair[1];
            ensure_strictly_increases(
                earlier,
                later,
                "max_expanded_nodes",
                earlier.max_expanded_nodes,
                later.max_expanded_nodes,
            )?;
            ensure_strictly_increases(
                earlier,
                later,
                "max_simulated_ticks",
                earlier.max_simulated_ticks,
                later.max_simulated_ticks,
            )?;
            ensure_strictly_increases(
                earlier,
                later,
                "max_ticks_per_path",
                earlier.max_ticks_per_path,
                later.max_ticks_per_path,
            )?;
            ensure_strictly_increases(
                earlier,
                later,
                "beam_width",
                earlier.beam_width,
                later.beam_width,
            )?;
            if later.position_quantum > earlier.position_quantum {
                return Err(InconclusiveAuditPolicyError::CoarserLaterStage {
                    earlier_stage_id: earlier.id.clone(),
                    later_stage_id: later.id.clone(),
                    field: "position_quantum",
                });
            }
            if later.velocity_quantum > earlier.velocity_quantum {
                return Err(InconclusiveAuditPolicyError::CoarserLaterStage {
                    earlier_stage_id: earlier.id.clone(),
                    later_stage_id: later.id.clone(),
                    field: "velocity_quantum",
                });
            }
            if later.macro_precision < earlier.macro_precision {
                return Err(InconclusiveAuditPolicyError::CoarserLaterStage {
                    earlier_stage_id: earlier.id.clone(),
                    later_stage_id: later.id.clone(),
                    field: "macro_precision",
                });
            }
        }
        Ok(())
    }
}

impl Default for InconclusiveAuditPolicy {
    fn default() -> Self {
        Self::current()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InconclusiveAuditPolicyError {
    UnsupportedVersion {
        expected: u32,
        actual: u32,
    },
    InvalidStageCount {
        actual: usize,
    },
    EmptyExplicitScope,
    DuplicateExplicitLoadout {
        loadout: EvaluationLoadout,
    },
    EmptyStageId,
    DuplicateStageId {
        stage_id: String,
    },
    InvalidStageField {
        stage_id: String,
        field: &'static str,
    },
    NonEscalatingStage {
        earlier_stage_id: String,
        later_stage_id: String,
        field: &'static str,
    },
    CoarserLaterStage {
        earlier_stage_id: String,
        later_stage_id: String,
        field: &'static str,
    },
}

impl fmt::Display for InconclusiveAuditPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { expected, actual } => write!(
                formatter,
                "unsupported inconclusive-audit policy version {actual}; expected {expected}"
            ),
            Self::InvalidStageCount { actual } => write!(
                formatter,
                "inconclusive-audit policy needs two or three stages, got {actual}"
            ),
            Self::EmptyExplicitScope => {
                formatter.write_str("explicit inconclusive-audit scope must name a loadout")
            }
            Self::DuplicateExplicitLoadout { loadout } => write!(
                formatter,
                "explicit inconclusive-audit scope repeats loadout {}",
                loadout.slug()
            ),
            Self::EmptyStageId => {
                formatter.write_str("inconclusive-audit stage IDs must not be empty")
            }
            Self::DuplicateStageId { stage_id } => {
                write!(
                    formatter,
                    "duplicate inconclusive-audit stage ID {stage_id:?}"
                )
            }
            Self::InvalidStageField { stage_id, field } => write!(
                formatter,
                "inconclusive-audit stage {stage_id:?} has invalid {field}"
            ),
            Self::NonEscalatingStage {
                earlier_stage_id,
                later_stage_id,
                field,
            } => write!(
                formatter,
                "inconclusive-audit stage {later_stage_id:?} must strictly increase {field} beyond stage {earlier_stage_id:?}"
            ),
            Self::CoarserLaterStage {
                earlier_stage_id,
                later_stage_id,
                field,
            } => write!(
                formatter,
                "inconclusive-audit stage {later_stage_id:?} must not make {field} coarser than stage {earlier_stage_id:?}"
            ),
        }
    }
}

impl Error for InconclusiveAuditPolicyError {}

/// Stable, serializable copy of solver work counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditSearchStats {
    pub expanded_nodes: usize,
    pub generated_nodes: usize,
    pub simulated_ticks: usize,
    pub deepest_path_ticks: usize,
}

impl From<SearchStats> for AuditSearchStats {
    fn from(stats: SearchStats) -> Self {
        Self {
            expanded_nodes: stats.expanded_nodes,
            generated_nodes: stats.generated_nodes,
            simulated_ticks: stats.simulated_ticks,
            deepest_path_ticks: stats.deepest_path_ticks,
        }
    }
}

impl AuditSearchStats {
    fn accumulate(&mut self, value: Self) {
        self.expanded_nodes = self.expanded_nodes.saturating_add(value.expanded_nodes);
        self.generated_nodes = self.generated_nodes.saturating_add(value.generated_nodes);
        self.simulated_ticks = self.simulated_ticks.saturating_add(value.simulated_ticks);
        self.deepest_path_ticks = self.deepest_path_ticks.max(value.deepest_path_ticks);
    }
}

/// Serializable bounded-search termination classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditInconclusiveReason {
    NoExitsDefined,
    ExpandedNodeBudget,
    SimulatedTickBudget,
    PathHorizon,
    FrontierExhausted,
}

impl AuditInconclusiveReason {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::NoExitsDefined => "no_exits_defined",
            Self::ExpandedNodeBudget => "expanded_node_budget",
            Self::SimulatedTickBudget => "simulated_tick_budget",
            Self::PathHorizon => "path_horizon",
            Self::FrontierExhausted => "frontier_exhausted",
        }
    }
}

impl From<InconclusiveReason> for AuditInconclusiveReason {
    fn from(reason: InconclusiveReason) -> Self {
        match reason {
            InconclusiveReason::NoExitsDefined => Self::NoExitsDefined,
            InconclusiveReason::ExpandedNodeBudget => Self::ExpandedNodeBudget,
            InconclusiveReason::SimulatedTickBudget => Self::SimulatedTickBudget,
            InconclusiveReason::PathHorizon => Self::PathHorizon,
            InconclusiveReason::FrontierExhausted => Self::FrontierExhausted,
        }
    }
}

/// Typed target half of an exact audit key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InconclusiveAuditTarget {
    Door { door_id: String },
    Pickup { pickup_id: String },
}

/// Exact matrix-cell identity retained from the original evaluation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditTargetKey {
    pub loadout: EvaluationLoadout,
    pub source_door_id: String,
    pub target: InconclusiveAuditTarget,
}

/// Original bounded observation for one selected cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalBoundedEvidence {
    pub reason: AuditInconclusiveReason,
    pub search_effort: AuditSearchStats,
}

/// One retry observation in chronological stage order.
///
/// A target stops accumulating entries after its first certified positive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InconclusiveRetryEvidence {
    BoundedInconclusive {
        stage_number: usize,
        stage_id: String,
        solver_config_id: String,
        reason: AuditInconclusiveReason,
        search_effort: AuditSearchStats,
    },
    PositiveRescue {
        stage_number: usize,
        stage_id: String,
        solver_config_id: String,
        witness_id: String,
        discovery_search_effort: AuditSearchStats,
    },
}

impl InconclusiveRetryEvidence {
    #[must_use]
    pub const fn is_positive_rescue(&self) -> bool {
        matches!(self, Self::PositiveRescue { .. })
    }
}

/// Full evidence history for one cell that was initially inconclusive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveTargetHistory {
    pub key: InconclusiveAuditTargetKey,
    pub original: OriginalBoundedEvidence,
    pub retries: Vec<InconclusiveRetryEvidence>,
}

impl InconclusiveTargetHistory {
    #[must_use]
    pub fn was_rescued(&self) -> bool {
        self.retries
            .last()
            .is_some_and(InconclusiveRetryEvidence::is_positive_rescue)
    }
}

/// Exact materialized config identity for one loadout/stage rerun.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditSolverConfigIdentity {
    pub id_version: u32,
    pub solver_policy_version: u32,
    pub config_id: String,
    pub max_expanded_nodes: usize,
    pub max_simulated_ticks: usize,
    pub max_ticks_per_path: usize,
    pub beam_width: usize,
    pub position_quantum: i32,
    pub velocity_quantum: i32,
    pub probe_direct_routes: bool,
    pub baseline_preview_max_expanded_nodes: usize,
    pub baseline_preview_max_simulated_ticks: usize,
    pub macro_precision: RetryMacroPrecision,
    pub macro_count: usize,
    pub macro_action_ticks: usize,
}

/// Operational accounting for one actual validation rerun.
///
/// The aggregate work is stored here once.  Target histories contain only
/// target-local bounded snapshots or positive discovery snapshots and must
/// not be summed to estimate audit cost.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditRerun {
    pub loadout: EvaluationLoadout,
    pub stage_number: usize,
    pub stage_id: String,
    pub solver: AuditSolverConfigIdentity,
    pub reported_pending_targets: usize,
    pub validation_batch_targets: usize,
    pub source_searches: usize,
    pub rescued_targets: usize,
    pub remaining_targets: usize,
    pub aggregate_operational_stats: AuditSearchStats,
}

/// Top-level interpretation of a completed staged audit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InconclusiveAuditDisposition {
    NoInitiallyInconclusiveTargets,
    AllRescued,
    BoundedInconclusiveRemain { remaining_targets: usize },
}

/// Auditable result for one evaluated corpus room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditReport {
    pub audit_version: u32,
    pub solver_policy_version: u32,
    pub disclaimer: &'static str,
    pub room_id: RoomId,
    pub policy: InconclusiveAuditPolicy,
    pub selected_loadouts: Vec<EvaluationLoadout>,
    pub excluded_initial_positive_cells: usize,
    pub initial_inconclusive_targets: usize,
    pub rescued_targets: usize,
    pub final_bounded_targets: usize,
    pub disposition: InconclusiveAuditDisposition,
    pub targets: Vec<InconclusiveTargetHistory>,
    pub reruns: Vec<InconclusiveAuditRerun>,
}

impl InconclusiveAuditReport {
    /// Sum actual rerun work without multiplying shared work by target count.
    #[must_use]
    pub fn total_operational_stats(&self) -> AuditSearchStats {
        self.reruns
            .iter()
            .fold(AuditSearchStats::default(), |mut total, rerun| {
                total.accumulate(rerun.aggregate_operational_stats);
                total
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InconclusiveAuditError {
    InvalidPolicy(InconclusiveAuditPolicyError),
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    MissingLoadoutMatrix {
        room_id: RoomId,
        loadout: EvaluationLoadout,
    },
    DuplicateLoadoutMatrix {
        room_id: RoomId,
        loadout: EvaluationLoadout,
    },
    MatrixLoadoutMismatch {
        room_id: RoomId,
        matrix_loadout: EvaluationLoadout,
    },
    DuplicateTarget {
        room_id: RoomId,
        key: InconclusiveAuditTargetKey,
    },
    MissingRerunTarget {
        room_id: RoomId,
        stage_id: String,
        key: InconclusiveAuditTargetKey,
    },
    Validation {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        stage_id: String,
        source: Box<DoorTargetEvidenceError>,
    },
}

impl fmt::Display for InconclusiveAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy(error) => write!(formatter, "invalid audit policy: {error}"),
            Self::MissingCanonicalVariant { room_id } => {
                write!(
                    formatter,
                    "corpus room {} has no canonical variant",
                    room_id.0
                )
            }
            Self::MissingLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "corpus room {} has no {} route matrix",
                room_id.0,
                loadout.slug()
            ),
            Self::DuplicateLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "corpus room {} has duplicate {} route matrices",
                room_id.0,
                loadout.slug()
            ),
            Self::MatrixLoadoutMismatch {
                room_id,
                matrix_loadout,
            } => write!(
                formatter,
                "corpus room {} matrix {} contains evidence for a different loadout",
                room_id.0,
                matrix_loadout.slug()
            ),
            Self::DuplicateTarget { room_id, key } => write!(
                formatter,
                "corpus room {} repeats original audit target {key:?}",
                room_id.0
            ),
            Self::MissingRerunTarget {
                room_id,
                stage_id,
                key,
            } => write!(
                formatter,
                "corpus room {} stage {stage_id:?} omitted audit target {key:?}",
                room_id.0
            ),
            Self::Validation {
                room_id,
                loadout,
                stage_id,
                source,
            } => write!(
                formatter,
                "corpus room {} {} stage {stage_id:?} validation failed: {source}",
                room_id.0,
                loadout.slug()
            ),
        }
    }
}

impl Error for InconclusiveAuditError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPolicy(error) => Some(error),
            Self::Validation { source, .. } => Some(source.as_ref()),
            Self::MissingCanonicalVariant { .. }
            | Self::MissingLoadoutMatrix { .. }
            | Self::DuplicateLoadoutMatrix { .. }
            | Self::MatrixLoadoutMismatch { .. }
            | Self::DuplicateTarget { .. }
            | Self::MissingRerunTarget { .. } => None,
        }
    }
}

impl From<InconclusiveAuditPolicyError> for InconclusiveAuditError {
    fn from(error: InconclusiveAuditPolicyError) -> Self {
        Self::InvalidPolicy(error)
    }
}

/// Retry only bounded-inconclusive cells from one evaluated corpus room.
///
/// Every actual rerun delegates to
/// [`evaluate_generated_door_targets_for_loadout`], retaining its shared
/// source searches and authoritative replay verification.  The validation
/// batch necessarily contains every target for its loadout; initially
/// positive cells may therefore be encountered by that shared search, but
/// they never enter `report.targets` and are never treated as retry targets.
pub fn audit_bounded_inconclusives(
    evaluated: &EvaluatedCorpusRoom,
    policy: &InconclusiveAuditPolicy,
) -> Result<InconclusiveAuditReport, InconclusiveAuditError> {
    policy.validate()?;
    let Some(canonical) = evaluated.generated.variants.first() else {
        return Err(InconclusiveAuditError::MissingCanonicalVariant {
            room_id: evaluated.generated.id.clone(),
        });
    };
    let construction_loadout = EvaluationLoadout::ALL
        .into_iter()
        .find(|loadout| loadout.abilities() == canonical.key.source.profile.abilities)
        .expect("the four evaluation loadouts cover every ability set");
    let selected_loadouts = selected_loadouts(&policy.scope, construction_loadout);

    let mut targets = Vec::new();
    let mut target_keys = BTreeSet::new();
    let mut excluded_initial_positive_cells = 0_usize;
    for &loadout in &selected_loadouts {
        let matrix = unique_matrix(evaluated, loadout)?;
        if matrix.evidence.loadout() != loadout.abilities() {
            return Err(InconclusiveAuditError::MatrixLoadoutMismatch {
                room_id: evaluated.generated.id.clone(),
                matrix_loadout: loadout,
            });
        }
        collect_original_targets(
            evaluated,
            matrix,
            &mut targets,
            &mut target_keys,
            &mut excluded_initial_positive_cells,
        )?;
    }
    let initial_inconclusive_targets = targets.len();
    let mut reruns = Vec::new();
    for (stage_index, stage) in policy.stages.iter().enumerate() {
        let stage_number = stage_index + 1;
        for &loadout in &selected_loadouts {
            let pending_indices = targets
                .iter()
                .enumerate()
                .filter_map(|(index, history)| {
                    (history.key.loadout == loadout && !history.was_rescued()).then_some(index)
                })
                .collect::<Vec<_>>();
            if pending_indices.is_empty() {
                continue;
            }

            let solver = stage.solver_for_loadout(loadout);
            let solver_identity = solver_config_identity(stage.macro_precision, &solver);
            let mut validation = ValidationConfig::for_loadout(loadout.abilities());
            validation.solver = solver;
            let batch = evaluate_generated_door_targets_for_loadout(
                &canonical.generated,
                loadout.abilities(),
                &validation,
            )
            .map_err(|source| InconclusiveAuditError::Validation {
                room_id: evaluated.generated.id.clone(),
                loadout,
                stage_id: stage.id.clone(),
                source: Box::new(source),
            })?;
            let indexed = index_rerun_batch(evaluated, loadout, &batch)?;
            let mut rescued_targets = 0_usize;
            for index in pending_indices.iter().copied() {
                let key = targets[index].key.clone();
                let coordinate = TargetCoordinate::from_key(&key);
                let evidence = indexed.get(&coordinate).ok_or_else(|| {
                    InconclusiveAuditError::MissingRerunTarget {
                        room_id: evaluated.generated.id.clone(),
                        stage_id: stage.id.clone(),
                        key: key.clone(),
                    }
                })?;
                let observation = match evidence {
                    BoundedTargetEvidence::Positive(positive) => {
                        rescued_targets += 1;
                        InconclusiveRetryEvidence::PositiveRescue {
                            stage_number,
                            stage_id: stage.id.clone(),
                            solver_config_id: solver_identity.config_id.clone(),
                            witness_id: positive.witness_fingerprint().to_string(),
                            discovery_search_effort: positive.solution().stats.into(),
                        }
                    }
                    BoundedTargetEvidence::Inconclusive(inconclusive) => {
                        InconclusiveRetryEvidence::BoundedInconclusive {
                            stage_number,
                            stage_id: stage.id.clone(),
                            solver_config_id: solver_identity.config_id.clone(),
                            reason: inconclusive.reason.into(),
                            search_effort: inconclusive.search_effort.into(),
                        }
                    }
                };
                targets[index].retries.push(observation);
            }

            let remaining_targets = pending_indices.len() - rescued_targets;
            reruns.push(InconclusiveAuditRerun {
                loadout,
                stage_number,
                stage_id: stage.id.clone(),
                solver: solver_identity,
                reported_pending_targets: pending_indices.len(),
                validation_batch_targets: batch.door_routes().len() + batch.pickup_routes().len(),
                source_searches: batch.source_search_effort().len(),
                rescued_targets,
                remaining_targets,
                aggregate_operational_stats: batch.aggregate_search_effort().into(),
            });
        }
    }

    let rescued_targets = targets
        .iter()
        .filter(|history| history.was_rescued())
        .count();
    let final_bounded_targets = initial_inconclusive_targets - rescued_targets;
    let disposition = if initial_inconclusive_targets == 0 {
        InconclusiveAuditDisposition::NoInitiallyInconclusiveTargets
    } else if final_bounded_targets == 0 {
        InconclusiveAuditDisposition::AllRescued
    } else {
        InconclusiveAuditDisposition::BoundedInconclusiveRemain {
            remaining_targets: final_bounded_targets,
        }
    };

    Ok(InconclusiveAuditReport {
        audit_version: INCONCLUSIVE_AUDIT_VERSION,
        solver_policy_version: SOLVER_POLICY_VERSION,
        disclaimer: INCONCLUSIVE_AUDIT_DISCLAIMER,
        room_id: evaluated.generated.id.clone(),
        policy: policy.clone(),
        selected_loadouts,
        excluded_initial_positive_cells,
        initial_inconclusive_targets,
        rescued_targets,
        final_bounded_targets,
        disposition,
        targets,
        reruns,
    })
}

fn ensure_strictly_increases(
    earlier: &InconclusiveAuditStage,
    later: &InconclusiveAuditStage,
    field: &'static str,
    earlier_value: usize,
    later_value: usize,
) -> Result<(), InconclusiveAuditPolicyError> {
    if later_value > earlier_value {
        return Ok(());
    }
    Err(InconclusiveAuditPolicyError::NonEscalatingStage {
        earlier_stage_id: earlier.id.clone(),
        later_stage_id: later.id.clone(),
        field,
    })
}

fn selected_loadouts(
    scope: &InconclusiveAuditScope,
    construction: EvaluationLoadout,
) -> Vec<EvaluationLoadout> {
    match scope {
        InconclusiveAuditScope::ConstructionAndCompleteKit => {
            if construction == EvaluationLoadout::Both {
                vec![EvaluationLoadout::Both]
            } else {
                vec![construction, EvaluationLoadout::Both]
            }
        }
        InconclusiveAuditScope::AllLoadouts => EvaluationLoadout::ALL.to_vec(),
        InconclusiveAuditScope::Explicit { loadouts } => loadouts.clone(),
    }
}

fn unique_matrix(
    evaluated: &EvaluatedCorpusRoom,
    loadout: EvaluationLoadout,
) -> Result<&LoadoutRouteMatrix, InconclusiveAuditError> {
    let mut matching = evaluated
        .matrices
        .iter()
        .filter(|matrix| matrix.loadout == loadout);
    let Some(matrix) = matching.next() else {
        return Err(InconclusiveAuditError::MissingLoadoutMatrix {
            room_id: evaluated.generated.id.clone(),
            loadout,
        });
    };
    if matching.next().is_some() {
        return Err(InconclusiveAuditError::DuplicateLoadoutMatrix {
            room_id: evaluated.generated.id.clone(),
            loadout,
        });
    }
    Ok(matrix)
}

fn collect_original_targets(
    evaluated: &EvaluatedCorpusRoom,
    matrix: &LoadoutRouteMatrix,
    targets: &mut Vec<InconclusiveTargetHistory>,
    target_keys: &mut BTreeSet<InconclusiveAuditTargetKey>,
    excluded_positive_cells: &mut usize,
) -> Result<(), InconclusiveAuditError> {
    for row in matrix.evidence.door_routes() {
        let key = InconclusiveAuditTargetKey {
            loadout: matrix.loadout,
            source_door_id: row.source_door_id.clone(),
            target: InconclusiveAuditTarget::Door {
                door_id: row.target_door_id.clone(),
            },
        };
        collect_original_evidence(
            evaluated,
            key,
            &row.evidence,
            targets,
            target_keys,
            excluded_positive_cells,
        )?;
    }
    for row in matrix.evidence.pickup_routes() {
        let key = InconclusiveAuditTargetKey {
            loadout: matrix.loadout,
            source_door_id: row.source_door_id.clone(),
            target: InconclusiveAuditTarget::Pickup {
                pickup_id: row.required_pickup_id.clone(),
            },
        };
        collect_original_evidence(
            evaluated,
            key,
            &row.evidence,
            targets,
            target_keys,
            excluded_positive_cells,
        )?;
    }
    Ok(())
}

fn collect_original_evidence(
    evaluated: &EvaluatedCorpusRoom,
    key: InconclusiveAuditTargetKey,
    evidence: &BoundedTargetEvidence,
    targets: &mut Vec<InconclusiveTargetHistory>,
    target_keys: &mut BTreeSet<InconclusiveAuditTargetKey>,
    excluded_positive_cells: &mut usize,
) -> Result<(), InconclusiveAuditError> {
    let BoundedTargetEvidence::Inconclusive(inconclusive) = evidence else {
        *excluded_positive_cells += 1;
        return Ok(());
    };
    if !target_keys.insert(key.clone()) {
        return Err(InconclusiveAuditError::DuplicateTarget {
            room_id: evaluated.generated.id.clone(),
            key,
        });
    }
    targets.push(InconclusiveTargetHistory {
        key,
        original: OriginalBoundedEvidence {
            reason: inconclusive.reason.into(),
            search_effort: inconclusive.search_effort.into(),
        },
        retries: Vec::new(),
    });
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TargetCoordinate {
    source_door_id: String,
    target: InconclusiveAuditTarget,
}

impl TargetCoordinate {
    fn from_key(key: &InconclusiveAuditTargetKey) -> Self {
        Self {
            source_door_id: key.source_door_id.clone(),
            target: key.target.clone(),
        }
    }
}

fn index_rerun_batch<'a>(
    evaluated: &EvaluatedCorpusRoom,
    loadout: EvaluationLoadout,
    batch: &'a DoorTargetEvidenceBatch,
) -> Result<BTreeMap<TargetCoordinate, &'a BoundedTargetEvidence>, InconclusiveAuditError> {
    if batch.loadout() != loadout.abilities() {
        return Err(InconclusiveAuditError::MatrixLoadoutMismatch {
            room_id: evaluated.generated.id.clone(),
            matrix_loadout: loadout,
        });
    }
    let mut indexed = BTreeMap::new();
    for row in batch.door_routes() {
        let coordinate = TargetCoordinate {
            source_door_id: row.source_door_id.clone(),
            target: InconclusiveAuditTarget::Door {
                door_id: row.target_door_id.clone(),
            },
        };
        if indexed.insert(coordinate.clone(), &row.evidence).is_some() {
            return Err(InconclusiveAuditError::DuplicateTarget {
                room_id: evaluated.generated.id.clone(),
                key: InconclusiveAuditTargetKey {
                    loadout,
                    source_door_id: coordinate.source_door_id,
                    target: coordinate.target,
                },
            });
        }
    }
    for row in batch.pickup_routes() {
        let coordinate = TargetCoordinate {
            source_door_id: row.source_door_id.clone(),
            target: InconclusiveAuditTarget::Pickup {
                pickup_id: row.required_pickup_id.clone(),
            },
        };
        if indexed.insert(coordinate.clone(), &row.evidence).is_some() {
            return Err(InconclusiveAuditError::DuplicateTarget {
                room_id: evaluated.generated.id.clone(),
                key: InconclusiveAuditTargetKey {
                    loadout,
                    source_door_id: coordinate.source_door_id,
                    target: coordinate.target,
                },
            });
        }
    }
    Ok(indexed)
}

fn append_two_tick_precision_macros(macros: &mut Vec<ActionMacro>) {
    let fine = macros
        .iter()
        .filter(|action_macro| {
            matches!(
                action_macro.name.as_str(),
                "left" | "idle" | "right" | "jump-left" | "jump" | "jump-right"
            ) && action_macro.actions.len() > 2
                && action_macro
                    .actions
                    .iter()
                    .all(|action| *action == action_macro.actions[0])
        })
        .map(|action_macro| {
            ActionMacro::held(
                format!("{}-precision-2", action_macro.name),
                action_macro.actions[0],
                2,
            )
        })
        .collect::<Vec<_>>();
    macros.extend(fine);
}

fn solver_config_identity(
    macro_precision: RetryMacroPrecision,
    solver: &SolverConfig,
) -> AuditSolverConfigIdentity {
    let mut hash = StableHash::domain(b"downwards-inconclusive-audit-solver-config");
    hash.u32(INCONCLUSIVE_SOLVER_CONFIG_ID_VERSION);
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
    let config_id = format!(
        "downwards-inconclusive-solver-config-v{}-{:016x}",
        INCONCLUSIVE_SOLVER_CONFIG_ID_VERSION,
        hash.finish()
    );
    AuditSolverConfigIdentity {
        id_version: INCONCLUSIVE_SOLVER_CONFIG_ID_VERSION,
        solver_policy_version: SOLVER_POLICY_VERSION,
        config_id,
        max_expanded_nodes: solver.max_expanded_nodes,
        max_simulated_ticks: solver.max_simulated_ticks,
        max_ticks_per_path: solver.max_ticks_per_path,
        beam_width: solver.beam_width,
        position_quantum: solver.position_quantum,
        velocity_quantum: solver.velocity_quantum,
        probe_direct_routes: solver.probe_direct_routes,
        baseline_preview_max_expanded_nodes: solver.baseline_preview_max_expanded_nodes,
        baseline_preview_max_simulated_ticks: solver.baseline_preview_max_simulated_ticks,
        macro_precision,
        macro_count: solver.macros.len(),
        macro_action_ticks: solver
            .macros
            .iter()
            .map(|action_macro| action_macro.actions.len())
            .sum(),
    }
}

/// Concise counts for one loadout across a multi-room audit run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditLoadoutSummary {
    pub initial_targets: usize,
    pub rescued_targets: usize,
    pub final_bounded_targets: usize,
}

/// Concise cost and result counts for one exact loadout/stage combination.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditStageSummary {
    pub stage_number: usize,
    pub stage_id: String,
    pub loadout: Option<EvaluationLoadout>,
    pub reruns: usize,
    pub pending_targets: usize,
    pub rescued_targets: usize,
    pub remaining_targets: usize,
    pub operational_stats: AuditSearchStats,
}

/// Compact aggregation used by per-seed checkpoints and the final CLI result.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditAggregate {
    pub audited_rooms: usize,
    pub rooms_without_targets: usize,
    pub all_rescued_rooms: usize,
    pub rooms_with_final_bounded_targets: usize,
    pub initial_targets: usize,
    pub rescued_targets: usize,
    pub final_bounded_targets: usize,
    pub by_loadout: BTreeMap<String, InconclusiveAuditLoadoutSummary>,
    pub by_stage: BTreeMap<String, InconclusiveAuditStageSummary>,
    pub original_bounded_reasons: BTreeMap<String, usize>,
    pub final_bounded_reasons: BTreeMap<String, usize>,
    pub total_operational_stats: AuditSearchStats,
}

impl InconclusiveAuditAggregate {
    fn include(&mut self, report: &InconclusiveAuditReport) {
        self.audited_rooms += 1;
        match report.disposition {
            InconclusiveAuditDisposition::NoInitiallyInconclusiveTargets => {
                self.rooms_without_targets += 1;
            }
            InconclusiveAuditDisposition::AllRescued => self.all_rescued_rooms += 1,
            InconclusiveAuditDisposition::BoundedInconclusiveRemain { .. } => {
                self.rooms_with_final_bounded_targets += 1;
            }
        }
        self.initial_targets = self
            .initial_targets
            .saturating_add(report.initial_inconclusive_targets);
        self.rescued_targets = self.rescued_targets.saturating_add(report.rescued_targets);
        self.final_bounded_targets = self
            .final_bounded_targets
            .saturating_add(report.final_bounded_targets);

        for history in &report.targets {
            let loadout = history.key.loadout.slug().to_owned();
            let loadout_summary = self.by_loadout.entry(loadout.clone()).or_default();
            loadout_summary.initial_targets += 1;
            if history.was_rescued() {
                loadout_summary.rescued_targets += 1;
            } else {
                loadout_summary.final_bounded_targets += 1;
            }
            increment_count(
                &mut self.original_bounded_reasons,
                format!("{loadout}/{}", history.original.reason.slug()),
            );
            if !history.was_rescued() {
                let final_reason = history
                    .retries
                    .last()
                    .and_then(|retry| match retry {
                        InconclusiveRetryEvidence::BoundedInconclusive { reason, .. } => {
                            Some(*reason)
                        }
                        InconclusiveRetryEvidence::PositiveRescue { .. } => None,
                    })
                    .unwrap_or(history.original.reason);
                increment_count(
                    &mut self.final_bounded_reasons,
                    format!("{loadout}/{}", final_reason.slug()),
                );
            }
        }

        for rerun in &report.reruns {
            let key = format!(
                "{:02}-{}-{}",
                rerun.stage_number,
                rerun.stage_id,
                rerun.loadout.slug()
            );
            let stage = self
                .by_stage
                .entry(key)
                .or_insert_with(|| InconclusiveAuditStageSummary {
                    stage_number: rerun.stage_number,
                    stage_id: rerun.stage_id.clone(),
                    loadout: Some(rerun.loadout),
                    ..InconclusiveAuditStageSummary::default()
                });
            stage.reruns += 1;
            stage.pending_targets = stage
                .pending_targets
                .saturating_add(rerun.reported_pending_targets);
            stage.rescued_targets = stage.rescued_targets.saturating_add(rerun.rescued_targets);
            stage.remaining_targets = stage
                .remaining_targets
                .saturating_add(rerun.remaining_targets);
            stage
                .operational_stats
                .accumulate(rerun.aggregate_operational_stats);
            self.total_operational_stats
                .accumulate(rerun.aggregate_operational_stats);
        }
    }

    fn merge(&mut self, other: &Self) {
        self.audited_rooms = self.audited_rooms.saturating_add(other.audited_rooms);
        self.rooms_without_targets = self
            .rooms_without_targets
            .saturating_add(other.rooms_without_targets);
        self.all_rescued_rooms = self
            .all_rescued_rooms
            .saturating_add(other.all_rescued_rooms);
        self.rooms_with_final_bounded_targets = self
            .rooms_with_final_bounded_targets
            .saturating_add(other.rooms_with_final_bounded_targets);
        self.initial_targets = self.initial_targets.saturating_add(other.initial_targets);
        self.rescued_targets = self.rescued_targets.saturating_add(other.rescued_targets);
        self.final_bounded_targets = self
            .final_bounded_targets
            .saturating_add(other.final_bounded_targets);
        for (loadout, value) in &other.by_loadout {
            let total = self.by_loadout.entry(loadout.clone()).or_default();
            total.initial_targets = total.initial_targets.saturating_add(value.initial_targets);
            total.rescued_targets = total.rescued_targets.saturating_add(value.rescued_targets);
            total.final_bounded_targets = total
                .final_bounded_targets
                .saturating_add(value.final_bounded_targets);
        }
        for (key, value) in &other.by_stage {
            let total =
                self.by_stage
                    .entry(key.clone())
                    .or_insert_with(|| InconclusiveAuditStageSummary {
                        stage_number: value.stage_number,
                        stage_id: value.stage_id.clone(),
                        loadout: value.loadout,
                        ..InconclusiveAuditStageSummary::default()
                    });
            total.reruns = total.reruns.saturating_add(value.reruns);
            total.pending_targets = total.pending_targets.saturating_add(value.pending_targets);
            total.rescued_targets = total.rescued_targets.saturating_add(value.rescued_targets);
            total.remaining_targets = total
                .remaining_targets
                .saturating_add(value.remaining_targets);
            total.operational_stats.accumulate(value.operational_stats);
        }
        merge_counts(
            &mut self.original_bounded_reasons,
            &other.original_bounded_reasons,
        );
        merge_counts(
            &mut self.final_bounded_reasons,
            &other.final_bounded_reasons,
        );
        self.total_operational_stats
            .accumulate(other.total_operational_stats);
    }
}

/// Final summary written by `corpus inconclusive-audit` and printed to stdout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InconclusiveAuditRunSummary {
    pub run_version: u32,
    pub status: String,
    pub start_seed: u64,
    pub seed_count: usize,
    pub evaluated_seed_blocks: usize,
    pub discovered_gate_failure_rooms: usize,
    pub discovered_complete_kit_failure_rooms: usize,
    pub discovered_complete_kit_inconclusive_cells: usize,
    pub discovered_construction_inconclusive_cells: usize,
    pub room_report_files: Vec<String>,
    pub aggregate: InconclusiveAuditAggregate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum AuditRoomPriority {
    CompleteKit,
    ConstructionOnly,
}

impl AuditRoomPriority {
    const ALL: [Self; 2] = [Self::CompleteKit, Self::ConstructionOnly];

    const fn slug(self) -> &'static str {
        match self {
            Self::CompleteKit => "complete-kit",
            Self::ConstructionOnly => "construction-only",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct PlannedAuditRoom {
    seed: u64,
    room_id: RoomId,
    priority: AuditRoomPriority,
    complete_kit_inconclusive_cells: usize,
    construction_inconclusive_cells: usize,
}

#[derive(Clone, Debug)]
struct SeedAuditPlan {
    seed: u64,
    rooms: Vec<PlannedAuditRoom>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct InconclusiveAuditRunPlan<'a> {
    run_version: u32,
    status: &'static str,
    start_seed: u64,
    seed_count: usize,
    policy: &'a InconclusiveAuditPolicy,
    rooms: Vec<&'a PlannedAuditRoom>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct InconclusiveAuditDiscoveryCheckpoint {
    run_version: u32,
    status: &'static str,
    seed: u64,
    rooms: Vec<PlannedAuditRoom>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct InconclusiveAuditPhaseCheckpoint {
    run_version: u32,
    status: &'static str,
    seed: u64,
    priority: AuditRoomPriority,
    room_report_files: Vec<String>,
    aggregate: InconclusiveAuditAggregate,
}

/// CLI adapter for freshly evaluated deterministic seed blocks.
///
/// Usage is `inconclusive-audit <start-seed> <seed-count> <new-output-directory>`.
/// The output directory and every contained file
/// are created with no-replace semantics, so production evidence artifacts
/// cannot be overwritten.  The discovery pass evaluates and checkpoints one
/// seed at a time.  The audit pass handles every discovered complete-kit
/// failure before construction-only failures, and each full room report is a
/// durable progress checkpoint.  Compact per-phase and final summaries
/// aggregate cost once per actual rerun.
pub fn run_inconclusive_audit_cli(
    start_seed: &str,
    seed_count: &str,
    output_directory: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let output_directory = Path::new(output_directory);
    if fs::symlink_metadata(output_directory).is_ok() {
        return Err(format!(
            "inconclusive-audit output already exists: {}",
            output_directory.display()
        )
        .into());
    }

    let policy = InconclusiveAuditPolicy::current();
    policy.validate()?;
    fs::create_dir(output_directory).map_err(|source| {
        io_error_with_path("create audit output directory", output_directory, source)
    })?;
    let room_directory = output_directory.join("rooms");
    let checkpoint_directory = output_directory.join("checkpoints");
    fs::create_dir(&room_directory).map_err(|source| {
        io_error_with_path("create room report directory", &room_directory, source)
    })?;
    fs::create_dir(&checkpoint_directory).map_err(|source| {
        io_error_with_path(
            "create audit checkpoint directory",
            &checkpoint_directory,
            source,
        )
    })?;
    let plan = InconclusiveAuditRunPlan {
        run_version: INCONCLUSIVE_AUDIT_RUN_VERSION,
        status: "discovering",
        start_seed,
        seed_count,
        policy: &policy,
        rooms: Vec::new(),
    };
    write_new_json(&output_directory.join("plan.json"), &plan)?;

    let mut seed_plans = Vec::with_capacity(seed_count);
    for offset in 0..seed_count {
        let seed = start_seed.wrapping_add(offset as u64);
        eprintln!(
            "inconclusive audit: evaluating discovery seed {}/{} ({seed})",
            offset + 1,
            seed_count
        );
        let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(seed, 1))?;
        let evaluated = evaluate_route_matrices(generated)?;
        let rooms = discover_gate_failure_rooms(seed, &evaluated.rooms)?;
        eprintln!(
            "inconclusive audit: seed {seed} has {} gate-failure rooms ({} complete-kit priority)",
            rooms.len(),
            rooms
                .iter()
                .filter(|room| room.priority == AuditRoomPriority::CompleteKit)
                .count()
        );
        let checkpoint = InconclusiveAuditDiscoveryCheckpoint {
            run_version: INCONCLUSIVE_AUDIT_RUN_VERSION,
            status: "complete",
            seed,
            rooms: rooms.clone(),
        };
        write_new_json(
            &checkpoint_directory.join(format!("seed-{seed:04}-discovery.json")),
            &checkpoint,
        )?;
        seed_plans.push(SeedAuditPlan { seed, rooms });
    }

    let planned_rooms = seed_plans
        .iter()
        .flat_map(|seed| seed.rooms.iter())
        .collect::<Vec<_>>();
    let audit_plan = InconclusiveAuditRunPlan {
        run_version: INCONCLUSIVE_AUDIT_RUN_VERSION,
        status: "auditing",
        start_seed,
        seed_count,
        policy: &policy,
        rooms: planned_rooms.clone(),
    };
    write_new_json(&output_directory.join("audit-plan.json"), &audit_plan)?;

    let total_rooms = planned_rooms.len();
    let mut completed_rooms = 0_usize;
    let mut room_report_files = Vec::with_capacity(total_rooms);
    let mut aggregate = InconclusiveAuditAggregate::default();
    for priority in AuditRoomPriority::ALL {
        for seed_plan in &seed_plans {
            let phase_rooms = seed_plan
                .rooms
                .iter()
                .filter(|room| room.priority == priority)
                .collect::<Vec<_>>();
            if phase_rooms.is_empty() {
                continue;
            }
            eprintln!(
                "inconclusive audit: evaluating seed {} {} rooms ({})",
                seed_plan.seed,
                priority.slug(),
                phase_rooms.len()
            );
            let mut generated =
                generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(seed_plan.seed, 1))?;
            let requested_ids = phase_rooms
                .iter()
                .map(|room| room.room_id.clone())
                .collect::<BTreeSet<_>>();
            generated
                .rooms
                .retain(|room| requested_ids.contains(&room.id));
            if generated.rooms.len() != requested_ids.len() {
                let found = generated
                    .rooms
                    .iter()
                    .map(|room| room.id.clone())
                    .collect::<BTreeSet<_>>();
                let missing = requested_ids.difference(&found).collect::<Vec<_>>();
                return Err(format!(
                    "seed {} regeneration omitted planned rooms: {missing:?}",
                    seed_plan.seed
                )
                .into());
            }
            let evaluated = evaluate_route_matrices(generated)?;
            let mut evaluated_by_id = evaluated
                .rooms
                .into_iter()
                .map(|room| (room.generated.id.clone(), room))
                .collect::<BTreeMap<_, _>>();
            let mut phase_aggregate = InconclusiveAuditAggregate::default();
            let mut phase_report_files = Vec::with_capacity(phase_rooms.len());
            for planned in phase_rooms {
                completed_rooms += 1;
                eprintln!(
                    "inconclusive audit: room {completed_rooms}/{total_rooms} seed={} priority={} id={}",
                    planned.seed,
                    planned.priority.slug(),
                    planned.room_id.0
                );
                let evaluated = evaluated_by_id.remove(&planned.room_id).ok_or_else(|| {
                    format!(
                        "seed {} evaluated batch omitted room {}",
                        planned.seed, planned.room_id.0
                    )
                })?;
                let report = audit_bounded_inconclusives(&evaluated, &policy)?;
                let relative_path =
                    format!("rooms/seed-{:04}--{}.json", planned.seed, planned.room_id.0);
                write_new_json(&output_directory.join(&relative_path), &report)?;
                phase_aggregate.include(&report);
                phase_report_files.push(relative_path.clone());
                room_report_files.push(relative_path);
                eprintln!(
                    "inconclusive audit: completed room {completed_rooms}/{total_rooms}: rescued={}/{} final-bounded={}",
                    report.rescued_targets,
                    report.initial_inconclusive_targets,
                    report.final_bounded_targets
                );
            }
            aggregate.merge(&phase_aggregate);
            let checkpoint = InconclusiveAuditPhaseCheckpoint {
                run_version: INCONCLUSIVE_AUDIT_RUN_VERSION,
                status: "complete",
                seed: seed_plan.seed,
                priority,
                room_report_files: phase_report_files,
                aggregate: phase_aggregate,
            };
            write_new_json(
                &checkpoint_directory.join(format!(
                    "seed-{:04}-{}.json",
                    seed_plan.seed,
                    priority.slug()
                )),
                &checkpoint,
            )?;
        }
    }

    let discovered_complete_kit_failure_rooms = planned_rooms
        .iter()
        .filter(|room| room.priority == AuditRoomPriority::CompleteKit)
        .count();
    let discovered_complete_kit_inconclusive_cells = planned_rooms
        .iter()
        .map(|room| room.complete_kit_inconclusive_cells)
        .sum();
    let discovered_construction_inconclusive_cells = planned_rooms
        .iter()
        .map(|room| room.construction_inconclusive_cells)
        .sum();
    let summary = InconclusiveAuditRunSummary {
        run_version: INCONCLUSIVE_AUDIT_RUN_VERSION,
        status: "complete".to_owned(),
        start_seed,
        seed_count,
        evaluated_seed_blocks: seed_plans.len(),
        discovered_gate_failure_rooms: total_rooms,
        discovered_complete_kit_failure_rooms,
        discovered_complete_kit_inconclusive_cells,
        discovered_construction_inconclusive_cells,
        room_report_files,
        aggregate,
    };
    write_new_json(&output_directory.join("summary.json"), &summary)?;
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

fn discover_gate_failure_rooms(
    seed: u64,
    rooms: &[EvaluatedCorpusRoom],
) -> Result<Vec<PlannedAuditRoom>, InconclusiveAuditError> {
    let mut planned = Vec::new();
    for room in rooms {
        let Some(canonical) = room.generated.variants.first() else {
            return Err(InconclusiveAuditError::MissingCanonicalVariant {
                room_id: room.generated.id.clone(),
            });
        };
        let construction_loadout = EvaluationLoadout::ALL
            .into_iter()
            .find(|loadout| loadout.abilities() == canonical.key.source.profile.abilities)
            .expect("the four evaluation loadouts cover every ability set");
        let complete_kit_inconclusive_cells =
            matrix_inconclusive_cells(unique_matrix(room, EvaluationLoadout::Both)?);
        let construction_inconclusive_cells = if construction_loadout == EvaluationLoadout::Both {
            complete_kit_inconclusive_cells
        } else {
            matrix_inconclusive_cells(unique_matrix(room, construction_loadout)?)
        };
        let priority = if complete_kit_inconclusive_cells > 0 {
            AuditRoomPriority::CompleteKit
        } else if construction_inconclusive_cells > 0 {
            AuditRoomPriority::ConstructionOnly
        } else {
            continue;
        };
        planned.push(PlannedAuditRoom {
            seed,
            room_id: room.generated.id.clone(),
            priority,
            complete_kit_inconclusive_cells,
            construction_inconclusive_cells,
        });
    }
    planned.sort_unstable_by(|left, right| left.room_id.cmp(&right.room_id));
    Ok(planned)
}

fn matrix_inconclusive_cells(matrix: &LoadoutRouteMatrix) -> usize {
    matrix.summary.inconclusive_door_rows + matrix.summary.inconclusive_pickup_rows
}

fn write_new_json(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn io_error_with_path(operation: &str, path: &Path, source: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        source.kind(),
        format!("{operation} {}: {source}", path.display()),
    )
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: String) {
    let count = counts.entry(key).or_default();
    *count = count.saturating_add(1);
}

fn merge_counts(target: &mut BTreeMap<String, usize>, source: &BTreeMap<String, usize>) {
    for (key, value) in source {
        let count = target.entry(key.clone()).or_default();
        *count = count.saturating_add(*value);
    }
}

/// Small deterministic FNV-1a stream used only for stable configuration IDs.
struct StableHash(u64);

impl StableHash {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn domain(domain: &[u8]) -> Self {
        let mut hash = Self(Self::OFFSET);
        hash.bytes(domain);
        hash
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        for &value in values {
            self.byte(value);
        }
    }

    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn i8(&mut self, value: i8) {
        self.byte(value as u8);
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
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

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use downwards_core::{AbilitySet, BoundarySide, Door, Pickup, Point, Rect, Room, Tile};
    use downwards_lab::{SimulationGeometryDescriptor, StaticVisualDescriptor};

    use super::*;
    use crate::corpus::{
        CorpusBuildConfigV1, GeneratedCorpusBatch, GeneratedCorpusRoom,
        evaluate_route_matrices_with, generate_seed_block,
    };

    const WIDTH: u16 = 32;
    const HEIGHT: u16 = 18;
    const TILE_SIZE: i32 = 10;

    fn seed_batch() -> &'static GeneratedCorpusBatch {
        static BATCH: OnceLock<GeneratedCorpusBatch> = OnceLock::new();
        BATCH.get_or_init(|| {
            generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap()
        })
    }

    fn evaluated_fixture(sealed: bool, generous_initial_bounds: bool) -> EvaluatedCorpusRoom {
        let mut generated = seed_batch().clone();
        let mut candidate = generated
            .rooms
            .iter()
            .flat_map(|room| room.variants.iter())
            .find(|candidate| candidate.key.source.profile.abilities == AbilitySet::NONE)
            .expect("the seed block contains a baseline construction")
            .clone();
        let room = flat_door_room(sealed);
        candidate.generated.room = room.clone();
        let generated_room = GeneratedCorpusRoom {
            id: RoomId(if sealed {
                "inconclusive-audit-sealed-fixture".to_owned()
            } else {
                "inconclusive-audit-rescue-fixture".to_owned()
            }),
            static_visual: StaticVisualDescriptor::from_room(&room),
            simulation_geometry: SimulationGeometryDescriptor::from_room(&room),
            variants: vec![candidate],
        };
        generated.rooms = vec![generated_room];
        evaluate_route_matrices_with(generated, |loadout| {
            let mut config = ValidationConfig::for_loadout(loadout.abilities());
            if generous_initial_bounds {
                config.solver.max_ticks_per_path = 600;
            } else {
                // One expanded state is enough to collect the pickup beside
                // the west arrival, while the one-tick budget keeps every
                // longer route bounded.  This gives the audit a deliberate
                // mixture of initial positives and inconclusives.
                config.solver.max_expanded_nodes = 1;
                config.solver.max_simulated_ticks = 1;
                config.solver.max_ticks_per_path = 40;
                config.solver.beam_width = 1;
                config.solver.probe_direct_routes = false;
            }
            config
        })
        .unwrap()
        .rooms
        .pop()
        .unwrap()
    }

    fn flat_door_room(sealed: bool) -> Room {
        let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
        for x in 0..WIDTH {
            set_tile(&mut tiles, x, 0, Tile::Solid);
            set_tile(&mut tiles, x, HEIGHT - 1, Tile::Solid);
        }
        for y in 1..HEIGHT - 1 {
            set_tile(&mut tiles, 0, y, Tile::Solid);
            set_tile(&mut tiles, WIDTH - 1, y, Tile::Solid);
            if sealed {
                set_tile(&mut tiles, WIDTH / 2, y, Tile::Solid);
            }
        }
        for y in 14..=16 {
            set_tile(&mut tiles, 0, y, Tile::Empty);
            set_tile(&mut tiles, WIDTH - 1, y, Tile::Empty);
        }
        let pickups = if sealed {
            // With no pickups, every target crosses the full-height seal.
            Vec::new()
        } else {
            vec![Pickup::new("near-west", Rect::new(20, 158, 8, 12)).unwrap()]
        };
        Room::new(
            "inconclusive-audit-flat-room",
            "Inconclusive audit fixture",
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
                id: "west".to_owned(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 140, 4, 30),
                arrival: Point::new(20, 158),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east".to_owned(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 140, 4, 30),
                arrival: Point::new(292, 158),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap()
    }

    fn set_tile(tiles: &mut [Tile], x: u16, y: u16, tile: Tile) {
        tiles[usize::from(y) * usize::from(WIDTH) + usize::from(x)] = tile;
    }

    fn policy(probe_direct_routes: bool, first_ticks: usize) -> InconclusiveAuditPolicy {
        let first = SolverConfig {
            max_expanded_nodes: 64,
            max_simulated_ticks: first_ticks,
            max_ticks_per_path: 600,
            beam_width: 16,
            probe_direct_routes,
            baseline_preview_max_expanded_nodes: 8,
            baseline_preview_max_simulated_ticks: 1_000,
            ..SolverConfig::default()
        };

        let mut second = first.clone();
        second.max_expanded_nodes = 128;
        second.max_simulated_ticks = first_ticks.saturating_mul(2);
        second.max_ticks_per_path = 900;
        second.beam_width = 32;
        second.baseline_preview_max_expanded_nodes = 16;
        second.baseline_preview_max_simulated_ticks = 2_000;

        InconclusiveAuditPolicy::new(
            InconclusiveAuditScope::Explicit {
                loadouts: vec![EvaluationLoadout::Baseline],
            },
            vec![
                InconclusiveAuditStage::from_solver_template(
                    "first",
                    &first,
                    RetryMacroPrecision::Standard,
                ),
                InconclusiveAuditStage::from_solver_template(
                    "second",
                    &second,
                    RetryMacroPrecision::Standard,
                ),
            ],
        )
    }

    #[test]
    fn known_bounded_fixture_is_rescued_by_an_escalated_batch() {
        let evaluated = evaluated_fixture(false, false);
        let report = audit_bounded_inconclusives(&evaluated, &policy(true, 300_000)).unwrap();

        assert!(report.initial_inconclusive_targets > 0);
        assert_eq!(report.rescued_targets, report.initial_inconclusive_targets);
        assert_eq!(report.final_bounded_targets, 0);
        assert_eq!(report.disposition, InconclusiveAuditDisposition::AllRescued);
        assert!(
            report
                .targets
                .iter()
                .all(InconclusiveTargetHistory::was_rescued)
        );
        assert!(report.targets.iter().all(|history| {
            matches!(
                history.retries.last(),
                Some(InconclusiveRetryEvidence::PositiveRescue {
                    stage_number: 1 | 2,
                    solver_config_id,
                    ..
                }) if solver_config_id.starts_with("downwards-inconclusive-solver-config-v1-")
            )
        }));
        assert!(report.reruns.iter().all(|rerun| {
            rerun.reported_pending_targets <= rerun.validation_batch_targets
                && rerun.source_searches == 2
        }));
    }

    #[test]
    fn sealed_fixture_remains_explicitly_bounded_after_every_stage() {
        let evaluated = evaluated_fixture(true, false);
        let report = audit_bounded_inconclusives(&evaluated, &policy(false, 10_000)).unwrap();

        assert_eq!(report.rescued_targets, 0);
        assert_eq!(
            report.final_bounded_targets,
            report.initial_inconclusive_targets
        );
        assert!(matches!(
            report.disposition,
            InconclusiveAuditDisposition::BoundedInconclusiveRemain {
                remaining_targets
            } if remaining_targets == report.initial_inconclusive_targets
        ));
        assert!(report.targets.iter().all(|history| {
            history.retries.len() == 2
                && history.retries.iter().all(|retry| {
                    matches!(retry, InconclusiveRetryEvidence::BoundedInconclusive { .. })
                })
        }));
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("unreachable"));
    }

    #[test]
    fn audit_is_byte_repeatable_for_the_same_room_and_policy() {
        let evaluated = evaluated_fixture(false, false);
        let policy = policy(true, 300_000);
        let first = audit_bounded_inconclusives(&evaluated, &policy).unwrap();
        let second = audit_bounded_inconclusives(&evaluated, &policy).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
    }

    #[test]
    fn initially_positive_cells_are_excluded_from_reported_retry_targets() {
        let evaluated = evaluated_fixture(false, false);
        let matrix = unique_matrix(&evaluated, EvaluationLoadout::Baseline).unwrap();
        let positive_keys = matrix
            .evidence
            .door_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .map(|row| InconclusiveAuditTargetKey {
                loadout: EvaluationLoadout::Baseline,
                source_door_id: row.source_door_id.clone(),
                target: InconclusiveAuditTarget::Door {
                    door_id: row.target_door_id.clone(),
                },
            })
            .chain(
                matrix
                    .evidence
                    .pickup_routes()
                    .iter()
                    .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
                    .map(|row| InconclusiveAuditTargetKey {
                        loadout: EvaluationLoadout::Baseline,
                        source_door_id: row.source_door_id.clone(),
                        target: InconclusiveAuditTarget::Pickup {
                            pickup_id: row.required_pickup_id.clone(),
                        },
                    }),
            )
            .collect::<BTreeSet<_>>();
        assert!(!positive_keys.is_empty());

        let report = audit_bounded_inconclusives(&evaluated, &policy(true, 300_000)).unwrap();
        assert_eq!(report.excluded_initial_positive_cells, positive_keys.len());
        assert!(
            report
                .targets
                .iter()
                .all(|history| !positive_keys.contains(&history.key))
        );
        assert!(
            report
                .reruns
                .iter()
                .any(|rerun| { rerun.validation_batch_targets > rerun.reported_pending_targets })
        );
    }

    #[test]
    fn no_targets_is_distinct_from_all_rescued_and_runs_no_searches() {
        let evaluated = evaluated_fixture(false, true);
        let report = audit_bounded_inconclusives(&evaluated, &policy(true, 300_000)).unwrap();

        assert_eq!(
            report.disposition,
            InconclusiveAuditDisposition::NoInitiallyInconclusiveTargets
        );
        assert_eq!(report.initial_inconclusive_targets, 0);
        assert_eq!(report.rescued_targets, 0);
        assert!(report.targets.is_empty());
        assert!(report.reruns.is_empty());
        assert_eq!(
            report.total_operational_stats(),
            AuditSearchStats::default()
        );
    }

    #[test]
    fn fresh_discovery_prioritizes_complete_kit_failures() {
        let evaluated = evaluated_fixture(false, false);
        let planned = discover_gate_failure_rooms(7, std::slice::from_ref(&evaluated)).unwrap();

        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].seed, 7);
        assert_eq!(planned[0].room_id, evaluated.generated.id);
        assert_eq!(planned[0].priority, AuditRoomPriority::CompleteKit);
        assert!(planned[0].complete_kit_inconclusive_cells > 0);
        assert!(planned[0].construction_inconclusive_cells > 0);

        let positive = evaluated_fixture(false, true);
        assert!(
            discover_gate_failure_rooms(8, std::slice::from_ref(&positive))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn concise_aggregate_counts_each_rerun_cost_once() {
        let evaluated = evaluated_fixture(false, false);
        let report = audit_bounded_inconclusives(&evaluated, &policy(true, 300_000)).unwrap();
        let mut aggregate = InconclusiveAuditAggregate::default();
        aggregate.include(&report);

        assert_eq!(aggregate.audited_rooms, 1);
        assert_eq!(
            aggregate.initial_targets,
            report.initial_inconclusive_targets
        );
        assert_eq!(aggregate.rescued_targets, report.rescued_targets);
        assert_eq!(
            aggregate.final_bounded_targets,
            report.final_bounded_targets
        );
        assert_eq!(
            aggregate.total_operational_stats,
            report.total_operational_stats()
        );
        assert_eq!(
            aggregate
                .by_stage
                .values()
                .map(|stage| stage.reruns)
                .sum::<usize>(),
            report.reruns.len()
        );
        assert!(
            aggregate
                .original_bounded_reasons
                .keys()
                .all(|key| key.starts_with("baseline/"))
        );
    }
}
