//! In-memory deep analysis for one evaluated corpus room.
//!
//! This layer deliberately keeps two kinds of route evidence separate:
//!
//! - canonical matrix witnesses are measured exactly as discovered, under the
//!   matrix loadout that certified them; and
//! - direct-controller audits independently retain their easiest-known fronts
//!   across every subset of the complete kit.
//!
//! A canonical matrix witness is not thereby the easiest-known controller.
//! Bounded non-successes remain inconclusive: they are retained in the input
//! matrices and are never converted into unreachable claims here.

use std::{error::Error, fmt};

use downwards_ai::{DifficultyConfig, SOLVER_POLICY_VERSION, SearchStats, SolverConfig};
use downwards_core::AbilitySet;
use downwards_gen::{GeneratedLevel, experimental::RoutePlan};
use downwards_validation::BoundedTargetEvidence;
use serde::{Deserialize, Serialize};

use super::{
    CORPUS_CANONICAL_REGENERATION_POLICY_VERSION, CORPUS_FEASIBILITY_GATE_VERSION,
    CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY, CanonicalRegenerationPolicy,
    CorpusCandidateKeyRecord, CorpusCandidateRegenerationError, CorpusRouteMeasurement,
    CorpusRouteMeasurementError, EvaluatedCorpusRoom, EvaluatedCorpusRoomV2, EvaluationLoadout,
    FusedRouteCellAssessment, LoadoutRouteMatrix, RoomId, RouteControllerAssessmentError,
    RouteControllerAssessmentPolicy, RouteEvaluationDifficultyConfigV2,
    RouteEvaluationSolverConfigV2, RouteFusionError, SourceRouteControllerAssessmentBatch,
    TerrainAuditError, TerrainAuditReport, assess_easiest_known_routes_from_source,
    audit_generated_terrain, fuse_easiest_known_route_cell, measure_door_route,
    resolve_corpus_metric_candidate_v2, select_canonical_regeneration_v2,
};

/// Version of the in-memory room-analysis policy and result shape.
pub const CORPUS_ROOM_ANALYSIS_VERSION: u32 = 2;

/// Version of the content-addressed deep-analysis configuration identity.
pub const CORPUS_ROOM_ANALYSIS_CONFIG_ID_VERSION: u32 = 1;

/// Controls deterministic direct-controller auditing and exact measurement.
///
/// Shaky-hand analysis is intentionally absent from this initial slice.
/// Canonical matrix witnesses are always measured with `shaky_hand: None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusRoomAnalysisConfig {
    pub direct_controller_solver: SolverConfig,
    pub canonical_witness_difficulty: DifficultyConfig,
}

impl Default for CorpusRoomAnalysisConfig {
    fn default() -> Self {
        Self {
            direct_controller_solver: SolverConfig::for_abilities(AbilitySet::ALL),
            canonical_witness_difficulty: DifficultyConfig::default(),
        }
    }
}

impl CorpusRoomAnalysisConfig {
    /// Freeze every solver and difficulty input into a validated,
    /// content-addressed record suitable for attaching to derived evidence.
    pub fn identity_record(
        &self,
    ) -> Result<CorpusRoomAnalysisConfigRecord, CorpusRoomAnalysisConfigError> {
        CorpusRoomAnalysisConfigRecord::from_config(self)
    }
}

/// Exact configuration provenance for one deep room analysis.
///
/// The full inputs are retained beside the content ID so a consumer can
/// independently recompute the identity instead of trusting an opaque label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusRoomAnalysisConfigRecord {
    pub config_id_version: u32,
    pub analysis_policy_version: u32,
    pub solver_policy_version: u32,
    pub route_controller_policy: RouteControllerAssessmentPolicy,
    pub config_id: String,
    pub direct_controller_solver: RouteEvaluationSolverConfigV2,
    pub canonical_witness_difficulty: RouteEvaluationDifficultyConfigV2,
}

impl CorpusRoomAnalysisConfigRecord {
    pub fn from_config(
        config: &CorpusRoomAnalysisConfig,
    ) -> Result<Self, CorpusRoomAnalysisConfigError> {
        let mut record = Self {
            config_id_version: CORPUS_ROOM_ANALYSIS_CONFIG_ID_VERSION,
            analysis_policy_version: CORPUS_ROOM_ANALYSIS_VERSION,
            solver_policy_version: SOLVER_POLICY_VERSION,
            route_controller_policy: CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY,
            config_id: String::new(),
            direct_controller_solver: RouteEvaluationSolverConfigV2::from(
                &config.direct_controller_solver,
            ),
            canonical_witness_difficulty: RouteEvaluationDifficultyConfigV2::from(
                config.canonical_witness_difficulty,
            ),
        };
        record.config_id = record.recomputed_config_id();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), CorpusRoomAnalysisConfigError> {
        if self.config_id_version != CORPUS_ROOM_ANALYSIS_CONFIG_ID_VERSION {
            return Err(CorpusRoomAnalysisConfigError::UnsupportedConfigIdVersion {
                expected: CORPUS_ROOM_ANALYSIS_CONFIG_ID_VERSION,
                actual: self.config_id_version,
            });
        }
        if self.analysis_policy_version != CORPUS_ROOM_ANALYSIS_VERSION {
            return Err(
                CorpusRoomAnalysisConfigError::UnsupportedAnalysisPolicyVersion {
                    expected: CORPUS_ROOM_ANALYSIS_VERSION,
                    actual: self.analysis_policy_version,
                },
            );
        }
        if self.solver_policy_version != SOLVER_POLICY_VERSION {
            return Err(
                CorpusRoomAnalysisConfigError::UnsupportedSolverPolicyVersion {
                    expected: SOLVER_POLICY_VERSION,
                    actual: self.solver_policy_version,
                },
            );
        }
        if self.route_controller_policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY {
            return Err(CorpusRoomAnalysisConfigError::UnsupportedRouteControllerPolicy);
        }
        let solver = &self.direct_controller_solver;
        if solver.max_ticks_per_path == 0
            || solver.beam_width == 0
            || solver.position_quantum <= 0
            || solver.velocity_quantum <= 0
            || solver.macros.is_empty()
        {
            return Err(CorpusRoomAnalysisConfigError::InvalidSolverConfig);
        }
        for (macro_index, action_macro) in solver.macros.iter().enumerate() {
            if action_macro.actions.is_empty() {
                return Err(CorpusRoomAnalysisConfigError::InvalidMacro { macro_index });
            }
            if action_macro.actions.iter().any(|action| action.restart) {
                return Err(CorpusRoomAnalysisConfigError::RestartAction { macro_index });
            }
        }
        let expected = self.recomputed_config_id();
        if self.config_id != expected {
            return Err(CorpusRoomAnalysisConfigError::ConfigIdMismatch {
                expected,
                actual: self.config_id.clone(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn recomputed_config_id(&self) -> String {
        let mut hash = AnalysisConfigIdentityHash::new();
        hash.bytes(b"downwards-corpus-room-analysis-config");
        hash.u32(self.config_id_version);
        hash.u32(self.analysis_policy_version);
        hash.u32(self.solver_policy_version);
        hash.u32(self.route_controller_policy.assessment_version);
        hash.u32(self.route_controller_policy.direct_probe_audit_version);
        hash.u32(self.route_controller_policy.semantic_trace_version);
        hash.u32(self.route_controller_policy.controller_demand_version);
        let solver = &self.direct_controller_solver;
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
        hash.usize(self.canonical_witness_difficulty.perturbation_grace_ticks);
        format!(
            "downwards-corpus-room-analysis-config-v{}-{:016x}",
            self.config_id_version,
            hash.finish()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusRoomAnalysisConfigError {
    UnsupportedConfigIdVersion { expected: u32, actual: u32 },
    UnsupportedAnalysisPolicyVersion { expected: u32, actual: u32 },
    UnsupportedSolverPolicyVersion { expected: u32, actual: u32 },
    UnsupportedRouteControllerPolicy,
    InvalidSolverConfig,
    InvalidMacro { macro_index: usize },
    RestartAction { macro_index: usize },
    ConfigIdMismatch { expected: String, actual: String },
}

impl fmt::Display for CorpusRoomAnalysisConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfigIdVersion { expected, actual } => write!(
                formatter,
                "room-analysis config ID version {actual} is unsupported; expected {expected}"
            ),
            Self::UnsupportedAnalysisPolicyVersion { expected, actual } => write!(
                formatter,
                "room-analysis policy version {actual} is unsupported; expected {expected}"
            ),
            Self::UnsupportedSolverPolicyVersion { expected, actual } => write!(
                formatter,
                "room-analysis solver policy version {actual} is unsupported; expected {expected}"
            ),
            Self::UnsupportedRouteControllerPolicy => formatter.write_str(
                "room-analysis route-controller policy does not equal the current exact policy",
            ),
            Self::InvalidSolverConfig => {
                formatter.write_str("room-analysis solver config is structurally invalid")
            }
            Self::InvalidMacro { macro_index } => write!(
                formatter,
                "room-analysis solver macro {macro_index} has an empty action sequence"
            ),
            Self::RestartAction { macro_index } => write!(
                formatter,
                "room-analysis solver macro {macro_index} contains restart"
            ),
            Self::ConfigIdMismatch { expected, actual } => write!(
                formatter,
                "room-analysis config ID mismatch: {actual:?} != {expected:?}"
            ),
        }
    }
}

impl Error for CorpusRoomAnalysisConfigError {}

struct AnalysisConfigIdentityHash(u64);

impl AnalysisConfigIdentityHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn bool(&mut self, value: bool) {
        self.bytes(&[u8::from(value)]);
    }

    fn i8(&mut self, value: i8) {
        self.bytes(&[value.cast_unsigned()]);
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

/// Complete in-memory deep analysis for one exact evaluated room.
#[derive(Clone, Debug, PartialEq)]
pub struct CorpusRoomAnalysis {
    pub version: u32,
    pub config: CorpusRoomAnalysisConfigRecord,
    pub room_id: RoomId,
    /// One batch per canonical source door. Each batch preserves canonical
    /// target order and contains every subset-loadout audit for authoritative
    /// [`EvaluationLoadout::Both`].
    pub source_route_assessments: Vec<SourceRouteControllerAssessmentBatch>,
    /// Operational work summed once per source/loadout audit. Per-route audit
    /// rows repeat these stats and must not be added to this total.
    pub direct_controller_operational_stats: SearchStats,
    /// Measurements of the canonical solver witnesses from every positive
    /// door row, in loadout/source/target order. These are not claimed to be
    /// easiest-known witnesses.
    pub canonical_route_measurements: Vec<CorpusRouteMeasurement>,
    /// Generator-neutral direct + canonical positive fusion for every exact
    /// directed-door/loadout cell. Canonical measurements remain separately
    /// retained above so reachability provenance is never overwritten by the
    /// deterministic representative of the easiest-known front.
    pub fused_route_cells: Vec<FusedRouteCellAssessment>,
    /// Positive-only terrain and ablation evidence from the complete-kit
    /// (`Both`) route matrix.
    pub terrain_audit: TerrainAuditReport,
}

/// Run every in-memory deep-analysis pass for one evaluated room.
///
/// Direct-controller probes are batched once per source door, with the
/// complete kit as the authoritative loadout so the batch covers all four
/// subset loadouts. Canonical positive door witnesses are measured under the
/// exact loadout of their source matrix, without shaky-hand perturbation. The
/// terrain audit consumes only positive controllers from the complete-kit
/// matrix.
pub fn analyze_corpus_room(
    evaluated: &EvaluatedCorpusRoom,
    config: &CorpusRoomAnalysisConfig,
) -> Result<CorpusRoomAnalysis, CorpusRoomAnalysisError> {
    let room_id = evaluated.generated.id.clone();
    let Some(canonical) = evaluated.generated.variants.first() else {
        return Err(CorpusRoomAnalysisError::MissingCanonicalVariant { room_id });
    };

    analyze_corpus_room_common(
        &evaluated.generated.id,
        &canonical.generated,
        &canonical.route_plan,
        &evaluated.matrices,
        config,
    )
}

/// Run deep analysis for a final-path room using only its explicit
/// post-feasibility canonical regeneration key.
///
/// Shared matrix witnesses remain bound to the alias that carried object
/// labels and simulation digests during evaluation. A canonical alias with a
/// different exact [`downwards_core::Room`] therefore produces an explicit
/// mismatch instead of silently replaying evidence against another identity.
pub fn analyze_corpus_room_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    config: &CorpusRoomAnalysisConfig,
) -> Result<CorpusRoomAnalysis, CorpusRoomAnalysisError> {
    let room_id = &evaluated.generated.id;
    resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        canonical_mismatch(
            room_id,
            format!("shared evaluated-room validation failed: {source}"),
        )
    })?;
    let selection = &evaluated.canonical_regeneration;
    if selection.policy_version != CORPUS_CANONICAL_REGENERATION_POLICY_VERSION {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "canonical policy version {} does not equal required version {}",
                selection.policy_version, CORPUS_CANONICAL_REGENERATION_POLICY_VERSION
            ),
        ));
    }
    if selection.policy
        != CanonicalRegenerationPolicy::LexicographicallySmallestExactKeyPassingAllGates
    {
        return Err(canonical_mismatch(
            room_id,
            format!("unsupported canonical policy {}", selection.policy.slug()),
        ));
    }
    let Some(selected_key) = selection.selected_key.as_ref() else {
        return Err(CorpusRoomAnalysisError::MissingPostFeasibilityCanonical {
            room_id: room_id.clone(),
        });
    };
    let expected_selection = select_canonical_regeneration_v2(
        &evaluated.variant_construction_gates,
        &evaluated.variant_ability_promotion_gates,
        evaluated.complete_kit_gate,
    );
    if expected_selection.selected_key.as_ref() != Some(selected_key) {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} does not equal the key derived by the recorded post-feasibility policy",
                selected_key.stable_slug()
            ),
        ));
    }

    let mut matching_gates = evaluated
        .variant_construction_gates
        .iter()
        .filter(|gate| gate.key == *selected_key);
    let Some(gate) = matching_gates.next() else {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} has no construction-loadout gate",
                selected_key.stable_slug()
            ),
        ));
    };
    if matching_gates.next().is_some() {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} has duplicate construction-loadout gates",
                selected_key.stable_slug()
            ),
        ));
    }
    if gate.gate_version != CORPUS_FEASIBILITY_GATE_VERSION
        || gate.construction_loadout != selected_key.construction_loadout()
        || !gate.state.passes()
    {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} does not have one current positive construction-loadout gate",
                selected_key.stable_slug()
            ),
        ));
    }

    let mut matching_variants = evaluated
        .generated
        .variants
        .iter()
        .filter(|candidate| candidate.exact_key() == *selected_key);
    let Some(retained_canonical) = matching_variants.next() else {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} has no retained native variant",
                selected_key.stable_slug()
            ),
        ));
    };
    if matching_variants.next().is_some() {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} names multiple retained native variants",
                selected_key.stable_slug()
            ),
        ));
    }
    let canonical = selected_key.regenerate().map_err(|source| {
        CorpusRoomAnalysisError::CanonicalRegeneration {
            room_id: room_id.clone(),
            key: Box::new(selected_key.clone()),
            source: Box::new(source),
        }
    })?;
    if canonical != *retained_canonical {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} does not regenerate its retained native variant exactly",
                selected_key.stable_slug()
            ),
        ));
    }
    if canonical.construction_abilities() != selected_key.construction_loadout().abilities() {
        return Err(canonical_mismatch(
            room_id,
            format!(
                "selected key {} disagrees with generated construction metadata",
                selected_key.stable_slug()
            ),
        ));
    }
    if canonical.physical_room_descriptor_v3() != evaluated.generated.physical_descriptor
        || evaluated.generated.physical_descriptor.room_id() != *room_id
    {
        return Err(canonical_mismatch(
            room_id,
            "selected native variant does not match the stored physical descriptor and room-v3 ID",
        ));
    }
    let mut evidence_sources = evaluated
        .generated
        .variants
        .iter()
        .filter(|candidate| candidate.exact_key() == evaluated.physical_evidence_source_key);
    let evidence_source = evidence_sources.next();
    if evidence_source.is_none()
        || evidence_sources.next().is_some()
        || evidence_source
            .is_some_and(|source| source.generated().room != canonical.generated().room)
    {
        return Err(CorpusRoomAnalysisError::CanonicalEvidenceSourceMismatch {
            room_id: room_id.clone(),
            selected_key: Box::new(selected_key.clone()),
            evidence_source_key: Box::new(evaluated.physical_evidence_source_key.clone()),
        });
    }

    analyze_corpus_room_common(
        room_id,
        canonical.generated(),
        canonical.route_plan(),
        &evaluated.matrices,
        config,
    )
}

fn analyze_corpus_room_common(
    room_id: &RoomId,
    generated: &GeneratedLevel,
    route_plan: &RoutePlan,
    matrices: &[LoadoutRouteMatrix],
    config: &CorpusRoomAnalysisConfig,
) -> Result<CorpusRoomAnalysis, CorpusRoomAnalysisError> {
    let analysis_config = config.identity_record().map_err(|source| {
        CorpusRoomAnalysisError::InvalidAnalysisConfig {
            room_id: room_id.clone(),
            source,
        }
    })?;
    let mut door_ids = generated
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    door_ids.sort_unstable();
    if let Some(duplicate) = door_ids
        .windows(2)
        .find(|pair| pair[0] == pair[1])
        .map(|pair| pair[0].clone())
    {
        return Err(CorpusRoomAnalysisError::DuplicateDoorId {
            room_id: room_id.clone(),
            door_id: duplicate,
        });
    }

    let matrices = ordered_matrices(room_id, matrices, &door_ids)?;

    let mut source_route_assessments = Vec::with_capacity(door_ids.len());
    let mut direct_controller_operational_stats = SearchStats::default();
    for source_door_id in &door_ids {
        let target_door_ids = door_ids
            .iter()
            .filter(|target_door_id| *target_door_id != source_door_id)
            .cloned()
            .collect::<Vec<_>>();
        let batch = assess_easiest_known_routes_from_source(
            &generated.room,
            source_door_id,
            target_door_ids,
            EvaluationLoadout::Both,
            &config.direct_controller_solver,
        )
        .map_err(|source| CorpusRoomAnalysisError::SourceRouteAssessment {
            room_id: room_id.clone(),
            source_door_id: source_door_id.clone(),
            source: Box::new(source),
        })?;
        accumulate_search_stats(
            &mut direct_controller_operational_stats,
            batch.total_operational_stats(),
        );
        source_route_assessments.push(batch);
    }

    let mut fused_route_cells = Vec::with_capacity(
        door_ids
            .len()
            .saturating_mul(door_ids.len().saturating_sub(1))
            .saturating_mul(EvaluationLoadout::ALL.len()),
    );
    for matrix in &matrices {
        for row in matrix.evidence.door_routes() {
            let route = source_route_assessments
                .iter()
                .find(|batch| batch.source_door_id == row.source_door_id)
                .and_then(|batch| {
                    batch
                        .routes
                        .iter()
                        .find(|route| route.target_door_id == row.target_door_id)
                })
                .expect("analysis creates every canonical directed route before fusion");
            let fused = fuse_easiest_known_route_cell(
                generated,
                matrix,
                route,
                &row.source_door_id,
                &row.target_door_id,
                &config.canonical_witness_difficulty,
            )
            .map_err(|source| CorpusRoomAnalysisError::RouteFusion {
                room_id: room_id.clone(),
                loadout: matrix.loadout,
                source_door_id: row.source_door_id.clone(),
                target_door_id: row.target_door_id.clone(),
                source: Box::new(source),
            })?;
            fused_route_cells.push(fused);
        }
    }

    let positive_door_rows = matrices
        .iter()
        .map(|matrix| {
            matrix
                .evidence
                .door_routes()
                .iter()
                .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
                .count()
        })
        .sum();
    let mut canonical_route_measurements = Vec::with_capacity(positive_door_rows);
    for matrix in &matrices {
        for row in matrix.evidence.door_routes() {
            let BoundedTargetEvidence::Positive(evidence) = &row.evidence else {
                continue;
            };
            let measurement = measure_door_route(
                generated,
                &row.source_door_id,
                matrix.loadout,
                evidence,
                &config.canonical_witness_difficulty,
                None,
            )
            .map_err(
                |source| CorpusRoomAnalysisError::CanonicalRouteMeasurement {
                    room_id: room_id.clone(),
                    loadout: matrix.loadout,
                    source_door_id: row.source_door_id.clone(),
                    target_door_id: row.target_door_id.clone(),
                    source: Box::new(source),
                },
            )?;
            if measurement.target_door_id != row.target_door_id {
                return Err(CorpusRoomAnalysisError::CanonicalWitnessTargetMismatch {
                    room_id: room_id.clone(),
                    loadout: matrix.loadout,
                    source_door_id: row.source_door_id.clone(),
                    matrix_target_door_id: row.target_door_id.clone(),
                    witness_target_door_id: measurement.target_door_id,
                });
            }
            canonical_route_measurements.push(measurement);
        }
    }

    let complete_kit_matrix = matrices
        .iter()
        .find(|matrix| matrix.loadout == EvaluationLoadout::Both)
        .expect("ordered_matrices returns every evaluation loadout");
    let terrain_audit =
        audit_generated_terrain(generated, route_plan, &complete_kit_matrix.evidence).map_err(
            |source| CorpusRoomAnalysisError::TerrainAudit {
                room_id: room_id.clone(),
                source: Box::new(source),
            },
        )?;

    Ok(CorpusRoomAnalysis {
        version: CORPUS_ROOM_ANALYSIS_VERSION,
        config: analysis_config,
        room_id: room_id.clone(),
        source_route_assessments,
        direct_controller_operational_stats,
        canonical_route_measurements,
        fused_route_cells,
        terrain_audit,
    })
}

fn ordered_matrices<'a>(
    room_id: &RoomId,
    matrices: &'a [LoadoutRouteMatrix],
    door_ids: &[String],
) -> Result<Vec<&'a LoadoutRouteMatrix>, CorpusRoomAnalysisError> {
    let expected_routes = door_ids
        .iter()
        .flat_map(|source| {
            door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.clone(), target.clone()))
        })
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(EvaluationLoadout::ALL.len());
    for loadout in EvaluationLoadout::ALL {
        let mut matches = matrices.iter().filter(|matrix| matrix.loadout == loadout);
        let Some(matrix) = matches.next() else {
            return Err(CorpusRoomAnalysisError::MissingLoadoutMatrix {
                room_id: room_id.clone(),
                loadout,
            });
        };
        if matches.next().is_some() {
            return Err(CorpusRoomAnalysisError::DuplicateLoadoutMatrix {
                room_id: room_id.clone(),
                loadout,
            });
        }
        if matrix.evidence.loadout() != loadout.abilities() {
            return Err(CorpusRoomAnalysisError::MatrixLoadoutMismatch {
                room_id: room_id.clone(),
                declared_loadout: loadout,
                evidence_loadout: matrix.evidence.loadout(),
            });
        }
        if matrix.evidence.door_routes().len() != expected_routes.len() {
            return Err(CorpusRoomAnalysisError::DoorRouteCardinality {
                room_id: room_id.clone(),
                loadout,
                expected: expected_routes.len(),
                actual: matrix.evidence.door_routes().len(),
            });
        }
        for (row_index, (row, (expected_source, expected_target))) in matrix
            .evidence
            .door_routes()
            .iter()
            .zip(&expected_routes)
            .enumerate()
        {
            if row.source_door_id != *expected_source || row.target_door_id != *expected_target {
                return Err(CorpusRoomAnalysisError::DoorRouteOrder {
                    room_id: room_id.clone(),
                    loadout,
                    row_index,
                    expected: Box::new((expected_source.clone(), expected_target.clone())),
                    actual: Box::new((row.source_door_id.clone(), row.target_door_id.clone())),
                });
            }
        }
        ordered.push(matrix);
    }
    Ok(ordered)
}

fn accumulate_search_stats(total: &mut SearchStats, additional: SearchStats) {
    total.expanded_nodes = total
        .expanded_nodes
        .saturating_add(additional.expanded_nodes);
    total.generated_nodes = total
        .generated_nodes
        .saturating_add(additional.generated_nodes);
    total.simulated_ticks = total
        .simulated_ticks
        .saturating_add(additional.simulated_ticks);
    total.deepest_path_ticks = total.deepest_path_ticks.max(additional.deepest_path_ticks);
}

fn canonical_mismatch(room_id: &RoomId, detail: impl Into<String>) -> CorpusRoomAnalysisError {
    CorpusRoomAnalysisError::PostFeasibilityCanonicalMismatch {
        room_id: room_id.clone(),
        detail: detail.into(),
    }
}

#[derive(Debug)]
pub enum CorpusRoomAnalysisError {
    InvalidAnalysisConfig {
        room_id: RoomId,
        source: CorpusRoomAnalysisConfigError,
    },
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    MissingPostFeasibilityCanonical {
        room_id: RoomId,
    },
    PostFeasibilityCanonicalMismatch {
        room_id: RoomId,
        detail: String,
    },
    CanonicalRegeneration {
        room_id: RoomId,
        key: Box<CorpusCandidateKeyRecord>,
        source: Box<CorpusCandidateRegenerationError>,
    },
    CanonicalEvidenceSourceMismatch {
        room_id: RoomId,
        selected_key: Box<CorpusCandidateKeyRecord>,
        evidence_source_key: Box<CorpusCandidateKeyRecord>,
    },
    DuplicateDoorId {
        room_id: RoomId,
        door_id: String,
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
        declared_loadout: EvaluationLoadout,
        evidence_loadout: AbilitySet,
    },
    DoorRouteCardinality {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        expected: usize,
        actual: usize,
    },
    DoorRouteOrder {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        row_index: usize,
        expected: Box<(String, String)>,
        actual: Box<(String, String)>,
    },
    SourceRouteAssessment {
        room_id: RoomId,
        source_door_id: String,
        source: Box<RouteControllerAssessmentError>,
    },
    CanonicalRouteMeasurement {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source_door_id: String,
        target_door_id: String,
        source: Box<CorpusRouteMeasurementError>,
    },
    CanonicalWitnessTargetMismatch {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source_door_id: String,
        matrix_target_door_id: String,
        witness_target_door_id: String,
    },
    RouteFusion {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source_door_id: String,
        target_door_id: String,
        source: Box<RouteFusionError>,
    },
    TerrainAudit {
        room_id: RoomId,
        source: Box<TerrainAuditError>,
    },
}

impl fmt::Display for CorpusRoomAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAnalysisConfig { room_id, source } => write!(
                formatter,
                "invalid deep-analysis configuration for room {}: {source}",
                room_id.0
            ),
            Self::MissingCanonicalVariant { room_id } => write!(
                formatter,
                "evaluated corpus room {} has no canonical generated variant",
                room_id.0
            ),
            Self::MissingPostFeasibilityCanonical { room_id } => write!(
                formatter,
                "evaluated corpus-v2 room {} has no post-feasibility canonical regeneration key",
                room_id.0
            ),
            Self::PostFeasibilityCanonicalMismatch { room_id, detail } => write!(
                formatter,
                "evaluated corpus-v2 room {} has an invalid post-feasibility canonical selection: {detail}",
                room_id.0
            ),
            Self::CanonicalRegeneration {
                room_id,
                key,
                source,
            } => write!(
                formatter,
                "evaluated corpus-v2 room {} could not regenerate selected canonical key {}: {source}",
                room_id.0,
                key.stable_slug()
            ),
            Self::CanonicalEvidenceSourceMismatch {
                room_id,
                selected_key,
                evidence_source_key,
            } => write!(
                formatter,
                "evaluated corpus-v2 room {} selected canonical key {}, but its exact room is not replay-compatible with shared evidence-source key {}",
                room_id.0,
                selected_key.stable_slug(),
                evidence_source_key.stable_slug()
            ),
            Self::DuplicateDoorId { room_id, door_id } => write!(
                formatter,
                "evaluated corpus room {} repeats door ID {door_id:?}",
                room_id.0
            ),
            Self::MissingLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "evaluated corpus room {} has no {} route matrix",
                room_id.0,
                loadout.slug()
            ),
            Self::DuplicateLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "evaluated corpus room {} has multiple {} route matrices",
                room_id.0,
                loadout.slug()
            ),
            Self::MatrixLoadoutMismatch {
                room_id,
                declared_loadout,
                evidence_loadout,
            } => write!(
                formatter,
                "evaluated corpus room {} declares a {} matrix whose evidence uses {evidence_loadout:?}",
                room_id.0,
                declared_loadout.slug()
            ),
            Self::DoorRouteCardinality {
                room_id,
                loadout,
                expected,
                actual,
            } => write!(
                formatter,
                "evaluated corpus room {} has {actual}/{expected} door rows in its {} matrix",
                room_id.0,
                loadout.slug()
            ),
            Self::DoorRouteOrder {
                room_id,
                loadout,
                row_index,
                expected,
                actual,
            } => write!(
                formatter,
                "evaluated corpus room {} has non-canonical {} door row {row_index}: expected {:?}->{:?}, received {:?}->{:?}",
                room_id.0,
                loadout.slug(),
                expected.0,
                expected.1,
                actual.0,
                actual.1
            ),
            Self::SourceRouteAssessment {
                room_id,
                source_door_id,
                source,
            } => write!(
                formatter,
                "direct-controller source audit failed for room {} from {source_door_id:?}: {source}",
                room_id.0
            ),
            Self::CanonicalRouteMeasurement {
                room_id,
                loadout,
                source_door_id,
                target_door_id,
                source,
            } => write!(
                formatter,
                "canonical route measurement failed for room {} under {} on {source_door_id:?}->{target_door_id:?}: {source}",
                room_id.0,
                loadout.slug()
            ),
            Self::CanonicalWitnessTargetMismatch {
                room_id,
                loadout,
                source_door_id,
                matrix_target_door_id,
                witness_target_door_id,
            } => write!(
                formatter,
                "canonical matrix witness for room {} under {} from {source_door_id:?} reached {witness_target_door_id:?}, but the row targets {matrix_target_door_id:?}",
                room_id.0,
                loadout.slug()
            ),
            Self::RouteFusion {
                room_id,
                loadout,
                source_door_id,
                target_door_id,
                source,
            } => write!(
                formatter,
                "easiest-known route fusion failed for room {} under {} on {source_door_id:?}->{target_door_id:?}: {source}",
                room_id.0,
                loadout.slug()
            ),
            Self::TerrainAudit { room_id, source } => write!(
                formatter,
                "complete-kit terrain audit failed for room {}: {source}",
                room_id.0
            ),
        }
    }
}

impl Error for CorpusRoomAnalysisError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAnalysisConfig { source, .. } => Some(source),
            Self::SourceRouteAssessment { source, .. } => Some(source.as_ref()),
            Self::CanonicalRouteMeasurement { source, .. } => Some(source.as_ref()),
            Self::RouteFusion { source, .. } => Some(source.as_ref()),
            Self::TerrainAudit { source, .. } => Some(source.as_ref()),
            Self::CanonicalRegeneration { source, .. } => Some(source.as_ref()),
            Self::MissingCanonicalVariant { .. }
            | Self::MissingPostFeasibilityCanonical { .. }
            | Self::PostFeasibilityCanonicalMismatch { .. }
            | Self::CanonicalEvidenceSourceMismatch { .. }
            | Self::DuplicateDoorId { .. }
            | Self::MissingLoadoutMatrix { .. }
            | Self::DuplicateLoadoutMatrix { .. }
            | Self::MatrixLoadoutMismatch { .. }
            | Self::DoorRouteCardinality { .. }
            | Self::DoorRouteOrder { .. }
            | Self::CanonicalWitnessTargetMismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use downwards_gen::experimental::{ChallengeIntent, PartitionRouteKey, PartitionRouteProfile};
    use downwards_validation::{ValidationConfig, WitnessFingerprint};

    use super::*;
    use crate::corpus::{
        AbilityPromotionDecisionV2, CORPUS_ABILITY_PROMOTION_GATE_VERSION,
        CanonicalRegenerationSelectionV2, CorpusBuildConfigV1, CorpusBuildConfigV2,
        CorpusCandidate, CorpusFeasibilityGateState, EvaluatedCorpusRoomV2, GeneratedCorpusBatchV2,
        GeneratedCorpusRoomV2, GenerationBatchSummaryV2, PartitionRouteKeyRecord,
        VariantAbilityPromotionEvidenceV2, VariantAbilityPromotionGateV2,
        VariantConstructionGateV2, evaluate_route_matrices, evaluate_route_matrices_v2_with,
        generate_seed_block,
    };

    fn real_room_with_at_least_three_doors() -> EvaluatedCorpusRoom {
        let mut generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1))
            .expect("seed-zero terrain generation is a stable real-room fixture");
        let room = generated
            .rooms
            .iter()
            .find(|room| room.variants[0].generated.room.doors().len() >= 3)
            .cloned()
            .expect("the seed-zero block contains a room with at least three doors");
        generated.rooms = vec![room];
        evaluate_route_matrices(generated)
            .expect("the selected real room evaluates")
            .rooms
            .pop()
            .unwrap()
    }

    fn analysis_config() -> CorpusRoomAnalysisConfig {
        let direct_controller_solver = SolverConfig {
            max_expanded_nodes: 10_000,
            max_simulated_ticks: 2_000_000,
            max_ticks_per_path: 240,
            ..SolverConfig::default()
        };
        CorpusRoomAnalysisConfig {
            direct_controller_solver,
            canonical_witness_difficulty: DifficultyConfig::default(),
        }
    }

    fn real_final_path_v2_room() -> EvaluatedCorpusRoomV2 {
        (0_u64..64)
            .find_map(|seed| {
                let key = PartitionRouteKey::new(
                    seed,
                    AbilitySet::NONE,
                    ChallengeIntent::Standard,
                    PartitionRouteProfile::MixedBsp,
                );
                let candidate = key.regenerate().ok().map(CorpusCandidate::from)?;
                let physical_descriptor = candidate.physical_room_descriptor_v3();
                let generated = GeneratedCorpusBatchV2 {
                    config: CorpusBuildConfigV2::attempt_zero(seed, 1),
                    construction_records: Vec::new(),
                    rooms: vec![GeneratedCorpusRoomV2 {
                        id: physical_descriptor.room_id(),
                        physical_descriptor,
                        variants: vec![candidate],
                    }],
                    summary: GenerationBatchSummaryV2::default(),
                };
                let evaluated = evaluate_route_matrices_v2_with(generated, |loadout| {
                    ValidationConfig::for_loadout(loadout.abilities())
                })
                .ok()?
                .rooms
                .pop()?;
                evaluated
                    .canonical_regeneration
                    .selected_key
                    .is_some()
                    .then_some(evaluated)
            })
            .expect("the bounded fixture range contains a positive final-path v2 room")
    }

    /// Manual release-mode benchmark for one bounded final-path v2 room.
    #[test]
    #[ignore = "run explicitly in release mode"]
    fn release_benchmark_route_fusion_one_final_path_room() {
        let evaluated = real_final_path_v2_room();
        let config = CorpusRoomAnalysisConfig::default();

        let full_wall_start = Instant::now();
        let analysis = analyze_corpus_room_v2(&evaluated, &config).unwrap();
        let full_wall_seconds = full_wall_start.elapsed().as_secs_f64();

        let canonical = resolve_corpus_metric_candidate_v2(&evaluated).unwrap();
        let fusion_wall_start = Instant::now();
        let mut fused = Vec::new();
        for matrix in &evaluated.matrices {
            for row in matrix.evidence.door_routes() {
                let route = analysis
                    .source_route_assessments
                    .iter()
                    .find(|batch| batch.source_door_id == row.source_door_id)
                    .and_then(|batch| {
                        batch
                            .routes
                            .iter()
                            .find(|route| route.target_door_id == row.target_door_id)
                    })
                    .unwrap();
                fused.push(
                    fuse_easiest_known_route_cell(
                        canonical.generated(),
                        matrix,
                        route,
                        &row.source_door_id,
                        &row.target_door_id,
                        &config.canonical_witness_difficulty,
                    )
                    .unwrap(),
                );
            }
        }
        let fusion_wall_seconds = fusion_wall_start.elapsed().as_secs_f64();
        let raw_direct_positives = fused
            .iter()
            .map(|cell| cell.raw_direct_positive_witnesses)
            .sum::<usize>();
        let retained_direct_positives = fused
            .iter()
            .map(|cell| cell.retained_direct_positive_witnesses)
            .sum::<usize>();
        let canonical_positive_inputs = fused
            .iter()
            .filter(|cell| {
                matches!(
                    cell.canonical_matrix_status,
                    crate::corpus::CanonicalMatrixCellStatus::Positive { .. }
                )
            })
            .count();
        let deduplicated_fused_candidates = fused
            .iter()
            .map(|cell| cell.candidates.len())
            .sum::<usize>();
        let ambiguous_fronts = fused
            .iter()
            .filter(|cell| {
                matches!(
                    cell.selected.map(|selection| selection.status),
                    Some(crate::corpus::EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront {
                        ..
                    })
                )
            })
            .count();
        eprintln!(
            "route-fusion release benchmark: room={} directed_cells={} raw_direct_positives={} retained_direct_positives={} canonical_positive_inputs={} retained_fused_candidates={} ambiguous_fronts={} fusion_wall_seconds={fusion_wall_seconds:.6} full_analysis_wall_seconds={full_wall_seconds:.6} projected_500_fusion_wall_hours={:.3} projected_500_full_wall_hours={:.3}",
            analysis.room_id.0,
            fused.len(),
            raw_direct_positives,
            retained_direct_positives,
            canonical_positive_inputs,
            deduplicated_fused_candidates,
            ambiguous_fronts,
            fusion_wall_seconds * 500.0 / 3600.0,
            full_wall_seconds * 500.0 / 3600.0,
        );
    }

    fn stats_sum(values: impl IntoIterator<Item = SearchStats>) -> SearchStats {
        values
            .into_iter()
            .fold(SearchStats::default(), |mut total, value| {
                accumulate_search_stats(&mut total, value);
                total
            })
    }

    #[test]
    fn legacy_wrapper_equals_common_analysis_with_identical_order_and_accounting() {
        let evaluated = real_room_with_at_least_three_doors();
        let config = analysis_config();
        let first = analyze_corpus_room(&evaluated, &config).unwrap();
        let canonical = &evaluated.generated.variants[0];
        let second = analyze_corpus_room_common(
            &evaluated.generated.id,
            &canonical.generated,
            &canonical.route_plan,
            &evaluated.matrices,
            &config,
        )
        .unwrap();
        assert_eq!(first, second);

        let mut door_ids = evaluated.generated.variants[0]
            .generated
            .room
            .doors()
            .iter()
            .map(|door| door.id.clone())
            .collect::<Vec<_>>();
        door_ids.sort_unstable();
        assert!(door_ids.len() >= 3);
        assert_eq!(first.source_route_assessments.len(), door_ids.len());
        for (batch, source_door_id) in first.source_route_assessments.iter().zip(&door_ids) {
            let expected_targets = door_ids
                .iter()
                .filter(|target_door_id| *target_door_id != source_door_id)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(&batch.source_door_id, source_door_id);
            assert_eq!(batch.target_door_ids, expected_targets);
            assert_eq!(
                batch
                    .routes
                    .iter()
                    .map(|route| route.target_door_id.clone())
                    .collect::<Vec<_>>(),
                expected_targets
            );
            assert_eq!(batch.authoritative_loadout, EvaluationLoadout::Both);
            assert_eq!(batch.expected_subset_loadouts, EvaluationLoadout::ALL);
            assert_eq!(batch.shared_audits.len(), EvaluationLoadout::ALL.len());
        }

        let expected_positive_rows = evaluated
            .matrices
            .iter()
            .flat_map(|matrix| matrix.evidence.door_routes())
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        assert_eq!(
            first.canonical_route_measurements.len(),
            expected_positive_rows
        );
        assert!(
            first
                .canonical_route_measurements
                .iter()
                .all(|measurement| measurement.shaky_hand.is_none())
        );
        assert_eq!(
            first.fused_route_cells.len(),
            door_ids.len() * door_ids.len().saturating_sub(1) * EvaluationLoadout::ALL.len()
        );
        assert!(first.fused_route_cells.iter().all(|cell| {
            cell.selected.is_none_or(|selection| {
                cell.candidates
                    .get(selection.candidate_index)
                    .is_some_and(|candidate| candidate.replay_identity == selection.replay_identity)
            })
        }));

        let canonical_witnesses = evaluated
            .matrices
            .iter()
            .flat_map(|matrix| {
                matrix.evidence.door_routes().iter().filter_map(|row| {
                    let BoundedTargetEvidence::Positive(evidence) = &row.evidence else {
                        return None;
                    };
                    Some((
                        (
                            matrix.loadout,
                            row.source_door_id.clone(),
                            row.target_door_id.clone(),
                        ),
                        evidence.witness_fingerprint(),
                    ))
                })
            })
            .collect::<BTreeMap<_, WitnessFingerprint>>();
        for measurement in &first.canonical_route_measurements {
            assert_eq!(
                canonical_witnesses.get(&(
                    measurement.loadout,
                    measurement.source_door_id.clone(),
                    measurement.target_door_id.clone(),
                )),
                Some(&measurement.witness_fingerprint)
            );
        }

        let shared_total = stats_sum(
            first
                .source_route_assessments
                .iter()
                .map(SourceRouteControllerAssessmentBatch::total_operational_stats),
        );
        assert_eq!(first.direct_controller_operational_stats, shared_total);
        let duplicated_route_total = stats_sum(
            first
                .source_route_assessments
                .iter()
                .flat_map(|batch| &batch.routes)
                .flat_map(|route| &route.audits)
                .map(|audit| audit.operational_stats),
        );
        assert!(
            duplicated_route_total.expanded_nodes
                > first.direct_controller_operational_stats.expanded_nodes
        );

        let complete_kit = evaluated
            .matrices
            .iter()
            .find(|matrix| matrix.loadout == EvaluationLoadout::Both)
            .unwrap();
        let positive_doors = complete_kit.summary.positive_door_rows;
        let positive_pickups = complete_kit.summary.positive_pickup_rows;
        assert_eq!(first.terrain_audit.loadout, AbilitySet::ALL);
        assert_eq!(
            first.terrain_audit.positive_door_controller_count,
            positive_doors
        );
        assert_eq!(
            first.terrain_audit.positive_pickup_controller_count,
            positive_pickups
        );
        assert_eq!(
            first.terrain_audit.positive_witnesses.len(),
            positive_doors + positive_pickups
        );

        // Matrix witness measurements and direct-controller fronts are
        // deliberately separate collections with different evidence identity.
        assert!(first.source_route_assessments.iter().all(|batch| {
            batch
                .routes
                .iter()
                .all(|route| route.authoritative_loadout == EvaluationLoadout::Both)
        }));
    }

    #[test]
    fn v2_analysis_rejects_malformed_evaluated_room_before_deep_analysis() {
        let native_key = PartitionRouteKey::new(
            3,
            AbilitySet::NONE,
            ChallengeIntent::Standard,
            PartitionRouteProfile::MixedBsp,
        );
        let candidate = CorpusCandidate::from(native_key.regenerate().unwrap());
        let selected_key = candidate.exact_key();
        let physical_descriptor = candidate.physical_room_descriptor_v3();
        let mut evaluated = EvaluatedCorpusRoomV2 {
            generated: GeneratedCorpusRoomV2 {
                id: physical_descriptor.room_id(),
                physical_descriptor,
                variants: vec![candidate],
            },
            physical_evidence_source_key: selected_key.clone(),
            matrices: Vec::new(),
            variant_construction_gates: vec![VariantConstructionGateV2 {
                gate_version: CORPUS_FEASIBILITY_GATE_VERSION,
                key: selected_key.clone(),
                construction_loadout: selected_key.construction_loadout(),
                state: CorpusFeasibilityGateState::ReplayCertifiedAllTargets,
            }],
            ability_promotion_audit_config: CorpusRoomAnalysisConfig::default()
                .identity_record()
                .unwrap(),
            variant_ability_promotion_gates: vec![VariantAbilityPromotionGateV2 {
                gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
                key: selected_key.clone(),
                evidence: VariantAbilityPromotionEvidenceV2::NotApplicable,
                decision: AbilityPromotionDecisionV2::NotApplicable,
            }],
            complete_kit_gate: CorpusFeasibilityGateState::ReplayCertifiedAllTargets,
            canonical_regeneration: CanonicalRegenerationSelectionV2 {
                policy_version: CORPUS_CANONICAL_REGENERATION_POLICY_VERSION,
                policy:
                    CanonicalRegenerationPolicy::LexicographicallySmallestExactKeyPassingAllGates,
                selected_key: None,
            },
        };
        assert!(matches!(
            analyze_corpus_room_v2(&evaluated, &CorpusRoomAnalysisConfig::default()),
            Err(CorpusRoomAnalysisError::PostFeasibilityCanonicalMismatch { .. })
        ));

        evaluated.canonical_regeneration.selected_key = Some(selected_key.clone());
        assert!(matches!(
            analyze_corpus_room_v2(&evaluated, &CorpusRoomAnalysisConfig::default()),
            Err(CorpusRoomAnalysisError::PostFeasibilityCanonicalMismatch { .. })
        ));

        evaluated.physical_evidence_source_key = CorpusCandidateKeyRecord::PartitionRoute(
            PartitionRouteKeyRecord::from(PartitionRouteKey::new(
                4,
                AbilitySet::NONE,
                ChallengeIntent::Standard,
                PartitionRouteProfile::MixedBsp,
            )),
        );
        assert!(matches!(
            analyze_corpus_room_v2(&evaluated, &CorpusRoomAnalysisConfig::default()),
            Err(CorpusRoomAnalysisError::PostFeasibilityCanonicalMismatch { .. })
        ));
    }
}
