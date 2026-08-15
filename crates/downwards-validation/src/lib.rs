//! Acceptance certificates for generated single-room scenarios.
//!
//! Generation establishes structural validity. This crate adds the stronger,
//! still bounded claim that a specific room, provenance record, traversal
//! loadout, exit objective, and optional provisional complexity band were
//! accepted by the authoritative simulation and game-playing solver. Pickup
//! routes have separate certificates so they cannot be confused with room
//! completion. Ordered door-pair certificates start from authored door
//! arrivals, and pickup-from-door certificates cover optional content from
//! every dungeon connection. Combined door-target validation shares one AI
//! exploration across every door and pickup objective for each entrance. A
//! solver non-success remains explicitly inconclusive; it is not proof that
//! an objective is unreachable.

#![forbid(unsafe_code)]

use std::{collections::HashSet, error::Error, fmt};

use downwards_ai::{
    BatchTargetResult, BatchTargetSolveError, ComplexityBand, DifficultyConfig, DifficultyError,
    DifficultyReport, InconclusiveReason, ReachedTarget, ReplayDivergence, SearchStats,
    SearchTarget, Solution, SolveOutcome, SolverConfig, SolverConfigError, TargetSolution,
    TargetSolveError, TargetSolveOutcome, analyze_solution, solve, solve_target, solve_targets,
};
use downwards_core::{AbilitySet, Action, DoorEntryError, Simulation};
use downwards_gen::{
    AbilityTier, GeneratedLevel, GeneratedMetadata, GenerationStats, LayoutFamily,
};

/// Version of the byte encoding used by [`WitnessFingerprint`].
pub const WITNESS_FINGERPRINT_VERSION: u32 = 2;

/// The exact acceptance target for one generated scenario.
///
/// `expected_metadata` deliberately duplicates the candidate's metadata. This
/// makes stale seed/version/family records fail validation instead of silently
/// certifying a different generated room. `loadout` is separate because it is
/// the actual run state supplied to the simulation and must agree with the
/// metadata's intended abilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioObjective {
    pub required_exit_id: String,
    pub expected_metadata: GeneratedMetadata,
    pub loadout: AbilitySet,
    pub requested_complexity_band: Option<ComplexityBand>,
}

impl ScenarioObjective {
    /// Define an objective with an explicit provenance record and loadout.
    #[must_use]
    pub fn new(
        required_exit_id: impl Into<String>,
        expected_metadata: GeneratedMetadata,
        loadout: AbilitySet,
    ) -> Self {
        Self {
            required_exit_id: required_exit_id.into(),
            expected_metadata,
            loadout,
            requested_complexity_band: None,
        }
    }

    /// Copy the candidate's exact provenance and intended loadout into an
    /// objective. The required exit remains explicit rather than inferred.
    #[must_use]
    pub fn for_generated(generated: &GeneratedLevel, required_exit_id: impl Into<String>) -> Self {
        Self::new(
            required_exit_id,
            generated.metadata.clone(),
            generated.metadata.intended_abilities,
        )
    }

    /// Require the provisional heuristic produced by difficulty analysis to
    /// land in `band` before a certificate is issued.
    #[must_use]
    pub const fn requesting_band(mut self, band: ComplexityBand) -> Self {
        self.requested_complexity_band = Some(band);
        self
    }
}

/// The exact provenance, loadout, and pickup ID for one optional-route claim.
///
/// This is intentionally separate from [`ScenarioObjective`]: collecting a
/// pickup is useful positive evidence about an optional route, but does not
/// certify that the required room exit was reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupReachabilityObjective {
    pub required_pickup_id: String,
    pub expected_metadata: GeneratedMetadata,
    pub loadout: AbilitySet,
}

impl PickupReachabilityObjective {
    #[must_use]
    pub fn new(
        required_pickup_id: impl Into<String>,
        expected_metadata: GeneratedMetadata,
        loadout: AbilitySet,
    ) -> Self {
        Self {
            required_pickup_id: required_pickup_id.into(),
            expected_metadata,
            loadout,
        }
    }

    #[must_use]
    pub fn for_generated(
        generated: &GeneratedLevel,
        required_pickup_id: impl Into<String>,
    ) -> Self {
        Self::new(
            required_pickup_id,
            generated.metadata.clone(),
            generated.metadata.intended_abilities,
        )
    }
}

/// One exact ordered door-to-door connectivity claim.
///
/// Source and target are deliberately ordered: a platforming route can be
/// reachable in one direction but not the reverse. Validation starts from the
/// source door's authored arrival point, never from the room's legacy spawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoorReachabilityObjective {
    pub source_door_id: String,
    pub target_door_id: String,
    pub expected_metadata: GeneratedMetadata,
    pub loadout: AbilitySet,
    pub requested_complexity_band: Option<ComplexityBand>,
}

impl DoorReachabilityObjective {
    #[must_use]
    pub fn new(
        source_door_id: impl Into<String>,
        target_door_id: impl Into<String>,
        expected_metadata: GeneratedMetadata,
        loadout: AbilitySet,
    ) -> Self {
        Self {
            source_door_id: source_door_id.into(),
            target_door_id: target_door_id.into(),
            expected_metadata,
            loadout,
            requested_complexity_band: None,
        }
    }

    #[must_use]
    pub fn for_generated(
        generated: &GeneratedLevel,
        source_door_id: impl Into<String>,
        target_door_id: impl Into<String>,
    ) -> Self {
        Self::new(
            source_door_id,
            target_door_id,
            generated.metadata.clone(),
            generated.metadata.intended_abilities,
        )
    }

    #[must_use]
    pub const fn requesting_band(mut self, band: ComplexityBand) -> Self {
        self.requested_complexity_band = Some(band);
        self
    }
}

/// A claim that one pickup is reachable after entering through one door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupFromDoorObjective {
    pub source_door_id: String,
    pub required_pickup_id: String,
    pub expected_metadata: GeneratedMetadata,
    pub loadout: AbilitySet,
}

impl PickupFromDoorObjective {
    #[must_use]
    pub fn new(
        source_door_id: impl Into<String>,
        required_pickup_id: impl Into<String>,
        expected_metadata: GeneratedMetadata,
        loadout: AbilitySet,
    ) -> Self {
        Self {
            source_door_id: source_door_id.into(),
            required_pickup_id: required_pickup_id.into(),
            expected_metadata,
            loadout,
        }
    }

    #[must_use]
    pub fn for_generated(
        generated: &GeneratedLevel,
        source_door_id: impl Into<String>,
        required_pickup_id: impl Into<String>,
    ) -> Self {
        Self::new(
            source_door_id,
            required_pickup_id,
            generated.metadata.clone(),
            generated.metadata.intended_abilities,
        )
    }
}

/// Solver and diagnostic-analysis controls used during acceptance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationConfig {
    pub solver: SolverConfig,
    pub difficulty: DifficultyConfig,
}

impl ValidationConfig {
    /// Use the solver vocabulary appropriate for the exact scenario loadout.
    #[must_use]
    pub fn for_loadout(loadout: AbilitySet) -> Self {
        Self {
            solver: SolverConfig::for_abilities(loadout),
            difficulty: DifficultyConfig::default(),
        }
    }
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self::for_loadout(AbilitySet::NONE)
    }
}

/// Stable, non-cryptographic identity of an exact accepted solver witness.
///
/// The fingerprint covers generator provenance, the required exit and
/// loadout, the replay's exact per-tick actions, state digests, event-stream
/// digests, and solver search statistics. The initial replay digest binds it
/// to the complete room contents. It is suitable for deterministic
/// regression/caching keys, not for adversarial integrity checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WitnessFingerprint(u64);

impl WitnessFingerprint {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WitnessFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-{:016x}",
            self.0
        )
    }
}

/// Owned proof artifact for one bounded, positively solved scenario.
///
/// Fields are exposed through read-only accessors so a certificate cannot be
/// assembled while bypassing the validation checks in this crate.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptanceCertificate {
    generated_level: GeneratedLevel,
    objective: ScenarioObjective,
    solution: Solution,
    difficulty: DifficultyReport,
    witness_fingerprint: WitnessFingerprint,
}

impl AcceptanceCertificate {
    #[must_use]
    pub const fn generated_level(&self) -> &GeneratedLevel {
        &self.generated_level
    }

    #[must_use]
    pub const fn objective(&self) -> &ScenarioObjective {
        &self.objective
    }

    #[must_use]
    pub const fn solution(&self) -> &Solution {
        &self.solution
    }

    #[must_use]
    pub const fn difficulty(&self) -> &DifficultyReport {
        &self.difficulty
    }

    #[must_use]
    pub const fn witness_fingerprint(&self) -> WitnessFingerprint {
        self.witness_fingerprint
    }
}

/// Owned positive proof that one generated pickup is reachable from a fresh
/// room attempt with the objective's exact loadout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupReachabilityCertificate {
    generated_level: GeneratedLevel,
    objective: PickupReachabilityObjective,
    solution: TargetSolution,
    witness_fingerprint: WitnessFingerprint,
}

impl PickupReachabilityCertificate {
    #[must_use]
    pub const fn generated_level(&self) -> &GeneratedLevel {
        &self.generated_level
    }

    #[must_use]
    pub const fn objective(&self) -> &PickupReachabilityObjective {
        &self.objective
    }

    #[must_use]
    pub const fn solution(&self) -> &TargetSolution {
        &self.solution
    }

    #[must_use]
    pub const fn witness_fingerprint(&self) -> WitnessFingerprint {
        self.witness_fingerprint
    }
}

/// Owned positive proof for one ordered source-door to target-door route.
#[derive(Clone, Debug, PartialEq)]
pub struct DoorReachabilityCertificate {
    generated_level: GeneratedLevel,
    objective: DoorReachabilityObjective,
    solution: TargetSolution,
    difficulty: DifficultyReport,
    witness_fingerprint: WitnessFingerprint,
}

impl DoorReachabilityCertificate {
    #[must_use]
    pub const fn generated_level(&self) -> &GeneratedLevel {
        &self.generated_level
    }

    #[must_use]
    pub const fn objective(&self) -> &DoorReachabilityObjective {
        &self.objective
    }

    #[must_use]
    pub const fn solution(&self) -> &TargetSolution {
        &self.solution
    }

    #[must_use]
    pub const fn difficulty(&self) -> &DifficultyReport {
        &self.difficulty
    }

    #[must_use]
    pub const fn witness_fingerprint(&self) -> WitnessFingerprint {
        self.witness_fingerprint
    }
}

/// Owned positive proof that a pickup is reachable from one door arrival.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupFromDoorCertificate {
    generated_level: GeneratedLevel,
    objective: PickupFromDoorObjective,
    solution: TargetSolution,
    witness_fingerprint: WitnessFingerprint,
}

impl PickupFromDoorCertificate {
    #[must_use]
    pub const fn generated_level(&self) -> &GeneratedLevel {
        &self.generated_level
    }

    #[must_use]
    pub const fn objective(&self) -> &PickupFromDoorObjective {
        &self.objective
    }

    #[must_use]
    pub const fn solution(&self) -> &TargetSolution {
        &self.solution
    }

    #[must_use]
    pub const fn witness_fingerprint(&self) -> WitnessFingerprint {
        self.witness_fingerprint
    }
}

/// Total shared solver work for one canonical source-door entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoorSourceSearchEffort {
    pub source_door_id: String,
    pub stats: SearchStats,
}

/// Exact positive evidence produced by replaying a targeted solver witness.
///
/// The witness fingerprint binds the replay to the generated room metadata,
/// explicit loadout, source door, and target identity. Construction is kept
/// private so [`BoundedTargetEvidence::Positive`] always means that the replay
/// has passed authoritative simulation verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayCertifiedTargetEvidence {
    solution: TargetSolution,
    witness_fingerprint: WitnessFingerprint,
}

impl ReplayCertifiedTargetEvidence {
    #[must_use]
    pub const fn solution(&self) -> &TargetSolution {
        &self.solution
    }

    #[must_use]
    pub const fn witness_fingerprint(&self) -> WitnessFingerprint {
        self.witness_fingerprint
    }
}

/// The bounded reason that an exact target has no positive witness yet.
///
/// This deliberately does not have an `Unreachable` variant. Exhausting a
/// configured search is evidence about that search only, never proof that the
/// route is impossible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedInconclusiveEvidence {
    pub reason: InconclusiveReason,
    pub search_effort: SearchStats,
}

/// Complete typed evidence for one requested target in a bounded search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundedTargetEvidence {
    /// An exact replay, re-run successfully by the authoritative simulation.
    Positive(ReplayCertifiedTargetEvidence),
    /// No witness was found within the recorded bound; reachability is open.
    Inconclusive(BoundedInconclusiveEvidence),
}

impl BoundedTargetEvidence {
    #[must_use]
    pub const fn positive(&self) -> Option<&ReplayCertifiedTargetEvidence> {
        match self {
            Self::Positive(evidence) => Some(evidence),
            Self::Inconclusive(_) => None,
        }
    }

    #[must_use]
    pub const fn inconclusive(&self) -> Option<BoundedInconclusiveEvidence> {
        match self {
            Self::Positive(_) => None,
            Self::Inconclusive(evidence) => Some(*evidence),
        }
    }
}

/// Evidence for one ordered source-door to target-door route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoorRouteEvidence {
    pub source_door_id: String,
    pub target_door_id: String,
    pub evidence: BoundedTargetEvidence,
}

/// Evidence for one pickup objective starting at a particular door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupRouteEvidence {
    pub source_door_id: String,
    pub required_pickup_id: String,
    pub evidence: BoundedTargetEvidence,
}

/// Untrusted persisted evidence supplied to the replay-only rehydration seam.
/// Positive solutions are replayed and fingerprinted again; bounded rows are
/// retained as bounded observations and never upgraded to reachability facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedTargetEvidence {
    PositiveReplay {
        solution: TargetSolution,
        recorded_witness_id: String,
    },
    BoundedInconclusive(BoundedInconclusiveEvidence),
}

/// One checksum-bound observation from a shared, source-batched solver run.
///
/// This is deliberately an operational consistency record, not a claim that
/// the search has been independently reproduced. Positive replays are
/// authoritatively checked elsewhere; this shape binds their cumulative
/// discovery snapshots and bounded terminal observations to the recorded
/// source total and exact solver limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordedSearchObservation {
    Positive {
        search_effort: SearchStats,
        replay_ticks: usize,
    },
    BoundedInconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedSearchObservationValidationError(String);

impl fmt::Display for RecordedSearchObservationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RecordedSearchObservationValidationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedDoorRouteEvidence {
    pub source_door_id: String,
    pub target_door_id: String,
    pub evidence: RecordedTargetEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedPickupRouteEvidence {
    pub source_door_id: String,
    pub required_pickup_id: String,
    pub evidence: RecordedTargetEvidence,
}

/// A malformed persisted route matrix. This is deliberately distinct from a
/// bounded solver non-success, which remains ordinary row evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoorTargetEvidenceRehydrationError {
    InvalidContract(String),
    EntryRejected {
        source_door_id: String,
        error: DoorEntryError,
    },
    DoorReplayCertification {
        source_door_id: String,
        target_door_id: String,
        error: Box<DoorValidationError>,
    },
    PickupReplayCertification {
        source_door_id: String,
        required_pickup_id: String,
        error: Box<PickupFromDoorValidationError>,
    },
    WitnessFingerprintMismatch {
        source_door_id: String,
        target_id: String,
        recorded: String,
        recomputed: String,
    },
}

impl fmt::Display for DoorTargetEvidenceRehydrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidContract(detail) => {
                write!(formatter, "invalid persisted route matrix: {detail}")
            }
            Self::EntryRejected {
                source_door_id,
                error,
            } => write!(
                formatter,
                "persisted matrix entry through {source_door_id:?} failed: {error}"
            ),
            Self::DoorReplayCertification {
                source_door_id,
                target_door_id,
                error,
            } => write!(
                formatter,
                "persisted positive door replay {source_door_id:?} -> {target_door_id:?} failed: {error}"
            ),
            Self::PickupReplayCertification {
                source_door_id,
                required_pickup_id,
                error,
            } => write!(
                formatter,
                "persisted positive pickup replay {source_door_id:?} -> {required_pickup_id:?} failed: {error}"
            ),
            Self::WitnessFingerprintMismatch {
                source_door_id,
                target_id,
                recorded,
                recomputed,
            } => write!(
                formatter,
                "persisted witness fingerprint for {source_door_id:?} -> {target_id:?} differs: {recorded:?} != {recomputed:?}"
            ),
        }
    }
}

impl Error for DoorTargetEvidenceRehydrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EntryRejected { error, .. } => Some(error),
            Self::DoorReplayCertification { error, .. } => Some(error.as_ref()),
            Self::PickupReplayCertification { error, .. } => Some(error.as_ref()),
            Self::InvalidContract(_) | Self::WitnessFingerprintMismatch { .. } => None,
        }
    }
}

/// Full bounded route matrix for one explicit traversal loadout.
///
/// Sources and door targets use canonical lexical door-ID order. Pickups use
/// lexical pickup-ID order, independent of declaration order. Exactly one
/// shared solver frontier is run per source door. Aggregate effort sums work
/// across those frontiers and takes the maximum deepest-path observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoorTargetEvidenceBatch {
    loadout: AbilitySet,
    door_routes: Vec<DoorRouteEvidence>,
    pickup_routes: Vec<PickupRouteEvidence>,
    source_search_effort: Vec<DoorSourceSearchEffort>,
    aggregate_search_effort: SearchStats,
}

impl DoorTargetEvidenceBatch {
    #[must_use]
    pub const fn loadout(&self) -> AbilitySet {
        self.loadout
    }

    #[must_use]
    pub fn door_routes(&self) -> &[DoorRouteEvidence] {
        &self.door_routes
    }

    #[must_use]
    pub fn pickup_routes(&self) -> &[PickupRouteEvidence] {
        &self.pickup_routes
    }

    #[must_use]
    pub fn source_search_effort(&self) -> &[DoorSourceSearchEffort] {
        &self.source_search_effort
    }

    #[must_use]
    pub const fn aggregate_search_effort(&self) -> SearchStats {
        self.aggregate_search_effort
    }
}

/// Positive certificates for every door and pickup objective reached from
/// every source door.
///
/// Door pairs are ordered by canonical source ID and then canonical target
/// ID. Pickup certificates use canonical source ID and pickup declaration
/// order. `source_search_effort` has one entry per canonical source and makes
/// the actual cost of the shared searches observable to offline curation.
#[derive(Clone, Debug, PartialEq)]
pub struct DoorTargetCertificateBatch {
    door_pairs: Vec<DoorReachabilityCertificate>,
    pickups_from_doors: Vec<PickupFromDoorCertificate>,
    source_search_effort: Vec<DoorSourceSearchEffort>,
}

impl DoorTargetCertificateBatch {
    #[must_use]
    pub fn door_pairs(&self) -> &[DoorReachabilityCertificate] {
        &self.door_pairs
    }

    #[must_use]
    pub fn pickups_from_doors(&self) -> &[PickupFromDoorCertificate] {
        &self.pickups_from_doors
    }

    #[must_use]
    pub fn source_search_effort(&self) -> &[DoorSourceSearchEffort] {
        &self.source_search_effort
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        Vec<DoorReachabilityCertificate>,
        Vec<PickupFromDoorCertificate>,
        Vec<DoorSourceSearchEffort>,
    ) {
        (
            self.door_pairs,
            self.pickups_from_doors,
            self.source_search_effort,
        )
    }
}

/// Every certificate rejection is classified without upgrading bounded
/// search uncertainty into an impossibility claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    EmptyRequiredExitId,
    MetadataMismatch {
        expected: GeneratedMetadata,
        actual: GeneratedMetadata,
    },
    ObjectiveLoadoutMismatch {
        objective_loadout: AbilitySet,
        metadata_loadout: AbilitySet,
    },
    MetadataAbilityTierMismatch {
        ability_tier: AbilityTier,
        intended_abilities: AbilitySet,
    },
    RequiredExitNotDefined {
        required_exit_id: String,
    },
    /// Targeted multi-exit search is intentionally not represented by the
    /// current any-exit solver. Reject such input until that capability exists.
    UnsupportedAmbiguousObjective {
        required_exit_id: String,
        available_exit_ids: Vec<String>,
    },
    WrongExit {
        required_exit_id: String,
        actual_exit_id: String,
    },
    SolverConfiguration(SolverConfigError),
    Inconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
    ReplayDiverged(ReplayDivergence),
    ReplayDidNotReachRequiredExit {
        required_exit_id: String,
    },
    DifficultyAnalysis(DifficultyError),
    ComplexityBandMismatch {
        requested: ComplexityBand,
        observed: ComplexityBand,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequiredExitId => write!(formatter, "required exit ID must not be empty"),
            Self::MetadataMismatch { expected, actual } => write!(
                formatter,
                "generated metadata differs from the objective: expected {expected:?}, got {actual:?}"
            ),
            Self::ObjectiveLoadoutMismatch {
                objective_loadout,
                metadata_loadout,
            } => write!(
                formatter,
                "objective loadout {objective_loadout:?} differs from metadata loadout {metadata_loadout:?}"
            ),
            Self::MetadataAbilityTierMismatch {
                ability_tier,
                intended_abilities,
            } => write!(
                formatter,
                "metadata tier {ability_tier:?} does not represent intended abilities {intended_abilities:?}"
            ),
            Self::RequiredExitNotDefined { required_exit_id } => write!(
                formatter,
                "required exit {required_exit_id:?} is not defined by the room"
            ),
            Self::UnsupportedAmbiguousObjective {
                required_exit_id,
                available_exit_ids,
            } => write!(
                formatter,
                "room has multiple exits {available_exit_ids:?}; targeted search for required exit {required_exit_id:?} is not supported yet"
            ),
            Self::WrongExit {
                required_exit_id,
                actual_exit_id,
            } => write!(
                formatter,
                "required exit was {required_exit_id:?}, but the scenario selected {actual_exit_id:?}"
            ),
            Self::SolverConfiguration(error) => {
                write!(formatter, "invalid solver configuration: {error}")
            }
            Self::Inconclusive {
                reason,
                search_effort,
            } => write!(
                formatter,
                "bounded solver was inconclusive ({reason:?}) after {search_effort:?}"
            ),
            Self::ReplayDiverged(error) => write!(formatter, "solver replay diverged: {error}"),
            Self::ReplayDidNotReachRequiredExit { required_exit_id } => write!(
                formatter,
                "verified solver replay did not finish at required exit {required_exit_id:?}"
            ),
            Self::DifficultyAnalysis(error) => {
                write!(
                    formatter,
                    "difficulty analysis rejected the solution: {error}"
                )
            }
            Self::ComplexityBandMismatch {
                requested,
                observed,
            } => write!(
                formatter,
                "requested provisional complexity band {requested:?}, observed {observed:?}"
            ),
        }
    }
}

impl Error for ValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SolverConfiguration(error) => Some(error),
            Self::ReplayDiverged(error) => Some(error),
            Self::DifficultyAnalysis(error) => Some(error),
            _ => None,
        }
    }
}

/// Rejections for a pickup-only reachability certificate.
///
/// Bounded search non-success remains [`Self::Inconclusive`], never a claim
/// that the pickup is unreachable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickupValidationError {
    EmptyRequiredPickupId,
    MetadataMismatch {
        expected: GeneratedMetadata,
        actual: GeneratedMetadata,
    },
    ObjectiveLoadoutMismatch {
        objective_loadout: AbilitySet,
        metadata_loadout: AbilitySet,
    },
    MetadataAbilityTierMismatch {
        ability_tier: AbilityTier,
        intended_abilities: AbilitySet,
    },
    RequiredPickupNotDefined {
        required_pickup_id: String,
    },
    SolverConfiguration(SolverConfigError),
    SearchTargetRejected(TargetSolveError),
    Inconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
    ReplayDiverged(ReplayDivergence),
    ReplayDidNotCollectRequiredPickup {
        required_pickup_id: String,
        collected_pickup_ids: Vec<String>,
    },
    WrongReachedTarget {
        required_pickup_id: String,
        actual: ReachedTarget,
    },
}

impl fmt::Display for PickupValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequiredPickupId => {
                write!(formatter, "required pickup ID must not be empty")
            }
            Self::MetadataMismatch { expected, actual } => write!(
                formatter,
                "generated metadata differs from the pickup objective: expected {expected:?}, got {actual:?}"
            ),
            Self::ObjectiveLoadoutMismatch {
                objective_loadout,
                metadata_loadout,
            } => write!(
                formatter,
                "pickup objective loadout {objective_loadout:?} differs from metadata loadout {metadata_loadout:?}"
            ),
            Self::MetadataAbilityTierMismatch {
                ability_tier,
                intended_abilities,
            } => write!(
                formatter,
                "metadata tier {ability_tier:?} does not represent intended abilities {intended_abilities:?}"
            ),
            Self::RequiredPickupNotDefined { required_pickup_id } => write!(
                formatter,
                "required pickup {required_pickup_id:?} is not defined by the room"
            ),
            Self::SolverConfiguration(error) => {
                write!(formatter, "invalid pickup solver configuration: {error}")
            }
            Self::SearchTargetRejected(error) => {
                write!(formatter, "pickup search target was rejected: {error}")
            }
            Self::Inconclusive {
                reason,
                search_effort,
            } => write!(
                formatter,
                "bounded pickup search was inconclusive ({reason:?}) after {search_effort:?}"
            ),
            Self::ReplayDiverged(error) => write!(formatter, "pickup replay diverged: {error}"),
            Self::ReplayDidNotCollectRequiredPickup {
                required_pickup_id,
                collected_pickup_ids,
            } => write!(
                formatter,
                "verified replay did not collect required pickup {required_pickup_id:?}; collected {collected_pickup_ids:?}"
            ),
            Self::WrongReachedTarget {
                required_pickup_id,
                actual,
            } => write!(
                formatter,
                "pickup search for {required_pickup_id:?} reported the wrong target {actual:?}"
            ),
        }
    }
}

impl Error for PickupValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SolverConfiguration(error) => Some(error),
            Self::SearchTargetRejected(error) => Some(error),
            Self::ReplayDiverged(error) => Some(error),
            _ => None,
        }
    }
}

/// Rejections for an ordered door-connectivity certificate.
///
/// Inconclusive bounded search is kept distinct from malformed room topology
/// and from replay verification failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoorValidationError {
    FewerThanTwoDoors {
        actual: usize,
    },
    NonCanonicalDoorId {
        door_id: String,
    },
    DuplicateDoorId {
        door_id: String,
    },
    EmptySourceDoorId,
    EmptyTargetDoorId,
    SameSourceAndTarget {
        door_id: String,
    },
    SourceDoorNotDefined {
        source_door_id: String,
    },
    TargetDoorNotDefined {
        target_door_id: String,
    },
    MetadataMismatch {
        expected: GeneratedMetadata,
        actual: GeneratedMetadata,
    },
    ObjectiveLoadoutMismatch {
        objective_loadout: AbilitySet,
        metadata_loadout: AbilitySet,
    },
    MetadataAbilityTierMismatch {
        ability_tier: AbilityTier,
        intended_abilities: AbilitySet,
    },
    EntryRejected(DoorEntryError),
    SolverConfiguration(SolverConfigError),
    SearchTargetRejected(TargetSolveError),
    Inconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
    WrongReachedTarget {
        target_door_id: String,
        actual: ReachedTarget,
    },
    ReplayDiverged(ReplayDivergence),
    ReplayDidNotReachTargetDoor {
        target_door_id: String,
        actual_terminal_id: Option<String>,
    },
    DifficultyAnalysis(DifficultyError),
    ComplexityBandMismatch {
        requested: ComplexityBand,
        observed: ComplexityBand,
    },
}

impl fmt::Display for DoorValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FewerThanTwoDoors { actual } => write!(
                formatter,
                "door connectivity requires at least two doors, found {actual}"
            ),
            Self::NonCanonicalDoorId { door_id } => write!(
                formatter,
                "door ID {door_id:?} is not canonical; leading and trailing whitespace are forbidden"
            ),
            Self::DuplicateDoorId { door_id } => {
                write!(formatter, "duplicate door ID {door_id:?}")
            }
            Self::EmptySourceDoorId => write!(formatter, "source door ID must not be empty"),
            Self::EmptyTargetDoorId => write!(formatter, "target door ID must not be empty"),
            Self::SameSourceAndTarget { door_id } => write!(
                formatter,
                "source and target door must be distinct, both were {door_id:?}"
            ),
            Self::SourceDoorNotDefined { source_door_id } => write!(
                formatter,
                "source door {source_door_id:?} is not defined by the room"
            ),
            Self::TargetDoorNotDefined { target_door_id } => write!(
                formatter,
                "target door {target_door_id:?} is not defined by the room"
            ),
            Self::MetadataMismatch { expected, actual } => write!(
                formatter,
                "generated metadata differs from the door objective: expected {expected:?}, got {actual:?}"
            ),
            Self::ObjectiveLoadoutMismatch {
                objective_loadout,
                metadata_loadout,
            } => write!(
                formatter,
                "door objective loadout {objective_loadout:?} differs from metadata loadout {metadata_loadout:?}"
            ),
            Self::MetadataAbilityTierMismatch {
                ability_tier,
                intended_abilities,
            } => write!(
                formatter,
                "metadata tier {ability_tier:?} does not represent intended abilities {intended_abilities:?}"
            ),
            Self::EntryRejected(error) => write!(formatter, "source door entry failed: {error}"),
            Self::SolverConfiguration(error) => {
                write!(
                    formatter,
                    "invalid door-route solver configuration: {error}"
                )
            }
            Self::SearchTargetRejected(error) => {
                write!(formatter, "door search target was rejected: {error}")
            }
            Self::Inconclusive {
                reason,
                search_effort,
            } => write!(
                formatter,
                "bounded door-route search was inconclusive ({reason:?}) after {search_effort:?}"
            ),
            Self::WrongReachedTarget {
                target_door_id,
                actual,
            } => write!(
                formatter,
                "door search for {target_door_id:?} reported the wrong target {actual:?}"
            ),
            Self::ReplayDiverged(error) => write!(formatter, "door-route replay diverged: {error}"),
            Self::ReplayDidNotReachTargetDoor {
                target_door_id,
                actual_terminal_id,
            } => write!(
                formatter,
                "verified replay did not reach target door {target_door_id:?}; reached {actual_terminal_id:?}"
            ),
            Self::DifficultyAnalysis(error) => {
                write!(formatter, "door-route difficulty analysis failed: {error}")
            }
            Self::ComplexityBandMismatch {
                requested,
                observed,
            } => write!(
                formatter,
                "requested door-route complexity band {requested:?}, observed {observed:?}"
            ),
        }
    }
}

impl Error for DoorValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EntryRejected(error) => Some(error),
            Self::SolverConfiguration(error) => Some(error),
            Self::SearchTargetRejected(error) => Some(error),
            Self::ReplayDiverged(error) => Some(error),
            Self::DifficultyAnalysis(error) => Some(error),
            _ => None,
        }
    }
}

/// Rejections for a pickup witness whose initial state is a door arrival.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickupFromDoorValidationError {
    DoorTopology(DoorValidationError),
    PickupObjective(PickupValidationError),
    EntryRejected(DoorEntryError),
    SolverConfiguration(SolverConfigError),
    SearchTargetRejected(TargetSolveError),
    Inconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
    WrongReachedTarget {
        required_pickup_id: String,
        actual: ReachedTarget,
    },
    ReplayDiverged(ReplayDivergence),
    ReplayDidNotCollectRequiredPickup {
        required_pickup_id: String,
        collected_pickup_ids: Vec<String>,
    },
}

impl fmt::Display for PickupFromDoorValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DoorTopology(error) => write!(formatter, "invalid pickup source door: {error}"),
            Self::PickupObjective(error) => {
                write!(formatter, "invalid pickup-from-door objective: {error}")
            }
            Self::EntryRejected(error) => write!(formatter, "pickup source entry failed: {error}"),
            Self::SolverConfiguration(error) => write!(
                formatter,
                "invalid pickup-from-door solver configuration: {error}"
            ),
            Self::SearchTargetRejected(error) => {
                write!(formatter, "pickup-from-door target was rejected: {error}")
            }
            Self::Inconclusive {
                reason,
                search_effort,
            } => write!(
                formatter,
                "bounded pickup-from-door search was inconclusive ({reason:?}) after {search_effort:?}"
            ),
            Self::WrongReachedTarget {
                required_pickup_id,
                actual,
            } => write!(
                formatter,
                "pickup-from-door search for {required_pickup_id:?} reported the wrong target {actual:?}"
            ),
            Self::ReplayDiverged(error) => {
                write!(formatter, "pickup-from-door replay diverged: {error}")
            }
            Self::ReplayDidNotCollectRequiredPickup {
                required_pickup_id,
                collected_pickup_ids,
            } => write!(
                formatter,
                "verified door-entry replay did not collect required pickup {required_pickup_id:?}; collected {collected_pickup_ids:?}"
            ),
        }
    }
}

impl Error for PickupFromDoorValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DoorTopology(error) => Some(error),
            Self::PickupObjective(error) => Some(error),
            Self::EntryRejected(error) => Some(error),
            Self::SolverConfiguration(error) => Some(error),
            Self::SearchTargetRejected(error) => Some(error),
            Self::ReplayDiverged(error) => Some(error),
            _ => None,
        }
    }
}

/// First rejected objective while certifying every door and pickup target in
/// one shared-search pass per source door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoorTargetBatchValidationError {
    DoorTopology(Box<DoorValidationError>),
    Door {
        source_door_id: String,
        target_door_id: String,
        error: Box<DoorValidationError>,
    },
    Pickup {
        source_door_id: String,
        required_pickup_id: String,
        error: Box<PickupFromDoorValidationError>,
    },
}

impl fmt::Display for DoorTargetBatchValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DoorTopology(error) => write!(formatter, "door topology is invalid: {error}"),
            Self::Door {
                source_door_id,
                target_door_id,
                error,
            } => write!(
                formatter,
                "door target {source_door_id:?} -> {target_door_id:?} failed: {error}"
            ),
            Self::Pickup {
                source_door_id,
                required_pickup_id,
                error,
            } => write!(
                formatter,
                "pickup {required_pickup_id:?} from door {source_door_id:?} failed: {error}"
            ),
        }
    }
}

impl Error for DoorTargetBatchValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DoorTopology(error) => Some(error),
            Self::Door { error, .. } => Some(error),
            Self::Pickup { error, .. } => Some(error),
        }
    }
}

/// Invalid input or failed replay certification while gathering a complete
/// bounded route-evidence matrix.
///
/// Ordinary solver non-success is not an error here: it is retained per
/// target as [`BoundedTargetEvidence::Inconclusive`]. These variants instead
/// identify malformed topology/configuration or a violated solver/replay
/// contract, all of which make the whole evidence batch untrustworthy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoorTargetEvidenceError {
    DoorTopology(Box<DoorValidationError>),
    EntryRejected {
        source_door_id: String,
        error: DoorEntryError,
    },
    SolverConfiguration(SolverConfigError),
    SearchTargetRejected {
        source_door_id: String,
        target: SearchTarget,
        error: TargetSolveError,
    },
    SolverContractMismatch {
        source_door_id: String,
        requested: SearchTarget,
        reported: SearchTarget,
    },
    DoorReplayCertification {
        source_door_id: String,
        target_door_id: String,
        error: Box<DoorValidationError>,
    },
    PickupReplayCertification {
        source_door_id: String,
        required_pickup_id: String,
        error: Box<PickupFromDoorValidationError>,
    },
}

impl fmt::Display for DoorTargetEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DoorTopology(error) => write!(formatter, "door topology is invalid: {error}"),
            Self::EntryRejected {
                source_door_id,
                error,
            } => write!(
                formatter,
                "entry through source door {source_door_id:?} failed: {error}"
            ),
            Self::SolverConfiguration(error) => {
                write!(
                    formatter,
                    "invalid route-evidence solver configuration: {error}"
                )
            }
            Self::SearchTargetRejected {
                source_door_id,
                target,
                error,
            } => write!(
                formatter,
                "target {target:?} from source door {source_door_id:?} was rejected: {error}"
            ),
            Self::SolverContractMismatch {
                source_door_id,
                requested,
                reported,
            } => write!(
                formatter,
                "shared search from {source_door_id:?} returned target {reported:?} for request {requested:?}"
            ),
            Self::DoorReplayCertification {
                source_door_id,
                target_door_id,
                error,
            } => write!(
                formatter,
                "positive door result {source_door_id:?} -> {target_door_id:?} failed replay certification: {error}"
            ),
            Self::PickupReplayCertification {
                source_door_id,
                required_pickup_id,
                error,
            } => write!(
                formatter,
                "positive pickup result {required_pickup_id:?} from {source_door_id:?} failed replay certification: {error}"
            ),
        }
    }
}

impl Error for DoorTargetEvidenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DoorTopology(error) => Some(error.as_ref()),
            Self::EntryRejected { error, .. } => Some(error),
            Self::SolverConfiguration(error) => Some(error),
            Self::SearchTargetRejected { error, .. } => Some(error),
            Self::SolverContractMismatch { .. } => None,
            Self::DoorReplayCertification { error, .. } => Some(error.as_ref()),
            Self::PickupReplayCertification { error, .. } => Some(error.as_ref()),
        }
    }
}

/// Validate with loadout-aware default solver settings.
pub fn validate_generated_scenario(
    generated_level: GeneratedLevel,
    objective: ScenarioObjective,
) -> Result<AcceptanceCertificate, ValidationError> {
    let config = ValidationConfig::for_loadout(objective.loadout);
    validate_generated_scenario_with_config(generated_level, objective, &config)
}

/// Validate one exact scenario and return an owned positive certificate.
///
/// This function supports precisely one room exit because the current solver
/// searches for any exit. Rooms with multiple exits are rejected explicitly,
/// even when the requested ID is among them, rather than misrepresenting an
/// any-exit witness as a targeted search result.
pub fn validate_generated_scenario_with_config(
    generated_level: GeneratedLevel,
    objective: ScenarioObjective,
    config: &ValidationConfig,
) -> Result<AcceptanceCertificate, ValidationError> {
    validate_objective(&generated_level, &objective)?;

    let initial = Simulation::with_abilities(generated_level.room.clone(), objective.loadout);
    let solution =
        match solve(&initial, &config.solver).map_err(ValidationError::SolverConfiguration)? {
            SolveOutcome::Solved(solution) => solution,
            SolveOutcome::Inconclusive { reason, stats } => {
                return Err(ValidationError::Inconclusive {
                    reason,
                    search_effort: stats,
                });
            }
        };

    if solution.exit_id != objective.required_exit_id {
        return Err(ValidationError::WrongExit {
            required_exit_id: objective.required_exit_id.clone(),
            actual_exit_id: solution.exit_id,
        });
    }

    let verification = solution
        .replay
        .verify(&initial)
        .map_err(ValidationError::ReplayDiverged)?;
    match verification.reached_exit {
        Some(actual_exit_id) if actual_exit_id != objective.required_exit_id => {
            return Err(ValidationError::WrongExit {
                required_exit_id: objective.required_exit_id.clone(),
                actual_exit_id,
            });
        }
        Some(_) => {}
        None => {
            return Err(ValidationError::ReplayDidNotReachRequiredExit {
                required_exit_id: objective.required_exit_id.clone(),
            });
        }
    }

    let difficulty = analyze_solution(&initial, &solution, &config.difficulty)
        .map_err(ValidationError::DifficultyAnalysis)?;
    if difficulty.exit_id != objective.required_exit_id {
        return Err(ValidationError::WrongExit {
            required_exit_id: objective.required_exit_id.clone(),
            actual_exit_id: difficulty.exit_id,
        });
    }
    if let Some(requested) = objective.requested_complexity_band {
        let observed = difficulty.provisional_complexity.band;
        if requested != observed {
            return Err(ValidationError::ComplexityBandMismatch {
                requested,
                observed,
            });
        }
    }

    let witness_fingerprint = fingerprint_witness(&generated_level, &objective, &solution);
    Ok(AcceptanceCertificate {
        generated_level,
        objective,
        solution,
        difficulty,
        witness_fingerprint,
    })
}

/// Validate one ordered source-door to target-door route with loadout-aware
/// default solver settings.
pub fn validate_generated_door_reachability(
    generated_level: GeneratedLevel,
    objective: DoorReachabilityObjective,
) -> Result<DoorReachabilityCertificate, DoorValidationError> {
    let config = ValidationConfig::for_loadout(objective.loadout);
    validate_generated_door_reachability_with_config(generated_level, objective, &config)
}

/// Validate one exact ordered door route and return an owned positive proof.
pub fn validate_generated_door_reachability_with_config(
    generated_level: GeneratedLevel,
    objective: DoorReachabilityObjective,
    config: &ValidationConfig,
) -> Result<DoorReachabilityCertificate, DoorValidationError> {
    validate_door_objective(&generated_level, &objective)?;

    let initial = Simulation::enter_via_door(
        generated_level.room.clone(),
        objective.loadout,
        &objective.source_door_id,
    )
    .map_err(DoorValidationError::EntryRejected)?;
    let outcome = solve_target(
        &initial,
        SearchTarget::door(&objective.target_door_id),
        &config.solver,
    )
    .map_err(|error| match error {
        TargetSolveError::SolverConfiguration(error) => {
            DoorValidationError::SolverConfiguration(error)
        }
        other => DoorValidationError::SearchTargetRejected(other),
    })?;

    certify_door_outcome(generated_level, objective, &initial, outcome, config)
}

fn certify_door_outcome(
    generated_level: GeneratedLevel,
    objective: DoorReachabilityObjective,
    initial: &Simulation,
    outcome: TargetSolveOutcome,
    config: &ValidationConfig,
) -> Result<DoorReachabilityCertificate, DoorValidationError> {
    let solution = match outcome {
        TargetSolveOutcome::Solved(solution) => solution,
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            return Err(DoorValidationError::Inconclusive {
                reason,
                search_effort: stats,
            });
        }
    };

    let witness_fingerprint =
        verify_door_solution(&generated_level, &objective, initial, &solution)?;

    let completion_solution = Solution {
        exit_id: objective.target_door_id.clone(),
        replay: solution.replay.clone(),
        stats: solution.stats,
    };
    let difficulty = analyze_solution(initial, &completion_solution, &config.difficulty)
        .map_err(DoorValidationError::DifficultyAnalysis)?;
    if let Some(requested) = objective.requested_complexity_band {
        let observed = difficulty.provisional_complexity.band;
        if requested != observed {
            return Err(DoorValidationError::ComplexityBandMismatch {
                requested,
                observed,
            });
        }
    }

    Ok(DoorReachabilityCertificate {
        generated_level,
        objective,
        solution,
        difficulty,
        witness_fingerprint,
    })
}

fn verify_door_solution(
    generated_level: &GeneratedLevel,
    objective: &DoorReachabilityObjective,
    initial: &Simulation,
    solution: &TargetSolution,
) -> Result<WitnessFingerprint, DoorValidationError> {
    let expected_reached = ReachedTarget::Door(objective.target_door_id.clone());
    if solution.reached != expected_reached {
        return Err(DoorValidationError::WrongReachedTarget {
            target_door_id: objective.target_door_id.clone(),
            actual: solution.reached.clone(),
        });
    }

    let verification = solution
        .replay
        .verify(initial)
        .map_err(DoorValidationError::ReplayDiverged)?;
    if verification.reached_exit.as_deref() != Some(objective.target_door_id.as_str()) {
        return Err(DoorValidationError::ReplayDidNotReachTargetDoor {
            target_door_id: objective.target_door_id.clone(),
            actual_terminal_id: verification.reached_exit,
        });
    }

    Ok(fingerprint_door_witness(
        generated_level,
        objective,
        solution,
    ))
}

/// Certify every ordered pair of distinct doors in canonical ID order.
///
/// For `n` doors this returns exactly `n * (n - 1)` certificates. Every pair
/// starts from a fresh simulation entered through its source door.
pub fn validate_all_generated_door_pairs(
    generated_level: &GeneratedLevel,
) -> Result<Vec<DoorReachabilityCertificate>, DoorValidationError> {
    let config = ValidationConfig::for_loadout(generated_level.metadata.intended_abilities);
    validate_all_generated_door_pairs_with_config(generated_level, &config)
}

/// Certify every ordered door pair with explicit bounded-search controls.
pub fn validate_all_generated_door_pairs_with_config(
    generated_level: &GeneratedLevel,
    config: &ValidationConfig,
) -> Result<Vec<DoorReachabilityCertificate>, DoorValidationError> {
    let door_ids = canonical_door_ids(generated_level)?;
    let pickup_ids = declared_pickup_ids(generated_level);
    let pair_count = door_ids.len() * (door_ids.len() - 1);
    let mut certificates = Vec::with_capacity(pair_count);
    for source_door_id in &door_ids {
        let objectives: Vec<_> = door_ids
            .iter()
            .filter(|target_door_id| *target_door_id != source_door_id)
            .map(|target_door_id| {
                DoorReachabilityObjective::for_generated(
                    generated_level,
                    source_door_id,
                    target_door_id,
                )
            })
            .collect();
        for objective in &objectives {
            validate_door_objective(generated_level, objective)?;
        }

        let batch = search_all_targets_from_door(
            generated_level,
            source_door_id,
            &door_ids,
            &pickup_ids,
            generated_level.metadata.intended_abilities,
            config,
        )
        .map_err(map_source_search_to_door_error)?;
        debug_assert_eq!(batch.door_result_count, objectives.len());
        for (objective, result) in objectives
            .into_iter()
            .zip(batch.results.into_iter().take(batch.door_result_count))
        {
            debug_assert_eq!(result.target, SearchTarget::door(&objective.target_door_id));
            certificates.push(certify_door_outcome(
                generated_level.clone(),
                objective,
                &batch.initial,
                result.outcome,
                config,
            )?);
        }
    }
    Ok(certificates)
}

/// Validate one pickup route from one door arrival with loadout-aware
/// defaults.
pub fn validate_generated_pickup_from_door(
    generated_level: GeneratedLevel,
    objective: PickupFromDoorObjective,
) -> Result<PickupFromDoorCertificate, PickupFromDoorValidationError> {
    let config = ValidationConfig::for_loadout(objective.loadout);
    validate_generated_pickup_from_door_with_config(generated_level, objective, &config)
}

/// Validate one exact pickup-from-door objective.
pub fn validate_generated_pickup_from_door_with_config(
    generated_level: GeneratedLevel,
    objective: PickupFromDoorObjective,
    config: &ValidationConfig,
) -> Result<PickupFromDoorCertificate, PickupFromDoorValidationError> {
    validate_pickup_from_door_objective(&generated_level, &objective)?;

    let initial = Simulation::enter_via_door(
        generated_level.room.clone(),
        objective.loadout,
        &objective.source_door_id,
    )
    .map_err(PickupFromDoorValidationError::EntryRejected)?;
    let outcome = solve_target(
        &initial,
        SearchTarget::pickup(&objective.required_pickup_id),
        &config.solver,
    )
    .map_err(|error| match error {
        TargetSolveError::SolverConfiguration(error) => {
            PickupFromDoorValidationError::SolverConfiguration(error)
        }
        other => PickupFromDoorValidationError::SearchTargetRejected(other),
    })?;

    certify_pickup_from_door_outcome(generated_level, objective, &initial, outcome)
}

fn validate_pickup_from_door_objective(
    generated_level: &GeneratedLevel,
    objective: &PickupFromDoorObjective,
) -> Result<(), PickupFromDoorValidationError> {
    if objective.source_door_id.trim().is_empty() {
        return Err(PickupFromDoorValidationError::DoorTopology(
            DoorValidationError::EmptySourceDoorId,
        ));
    }
    let door_ids =
        canonical_door_ids(generated_level).map_err(PickupFromDoorValidationError::DoorTopology)?;
    if door_ids.binary_search(&objective.source_door_id).is_err() {
        return Err(PickupFromDoorValidationError::DoorTopology(
            DoorValidationError::SourceDoorNotDefined {
                source_door_id: objective.source_door_id.clone(),
            },
        ));
    }

    let pickup_objective = PickupReachabilityObjective::new(
        &objective.required_pickup_id,
        objective.expected_metadata.clone(),
        objective.loadout,
    );
    validate_pickup_objective(generated_level, &pickup_objective)
        .map_err(PickupFromDoorValidationError::PickupObjective)?;
    Ok(())
}

fn certify_pickup_from_door_outcome(
    generated_level: GeneratedLevel,
    objective: PickupFromDoorObjective,
    initial: &Simulation,
    outcome: TargetSolveOutcome,
) -> Result<PickupFromDoorCertificate, PickupFromDoorValidationError> {
    let solution = match outcome {
        TargetSolveOutcome::Solved(solution) => solution,
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            return Err(PickupFromDoorValidationError::Inconclusive {
                reason,
                search_effort: stats,
            });
        }
    };

    let witness_fingerprint =
        verify_pickup_from_door_solution(&generated_level, &objective, initial, &solution)?;
    Ok(PickupFromDoorCertificate {
        generated_level,
        objective,
        solution,
        witness_fingerprint,
    })
}

fn verify_pickup_from_door_solution(
    generated_level: &GeneratedLevel,
    objective: &PickupFromDoorObjective,
    initial: &Simulation,
    solution: &TargetSolution,
) -> Result<WitnessFingerprint, PickupFromDoorValidationError> {
    let expected_reached = ReachedTarget::Pickup(objective.required_pickup_id.clone());
    if solution.reached != expected_reached {
        return Err(PickupFromDoorValidationError::WrongReachedTarget {
            required_pickup_id: objective.required_pickup_id.clone(),
            actual: solution.reached.clone(),
        });
    }
    let verification = solution
        .replay
        .verify(initial)
        .map_err(PickupFromDoorValidationError::ReplayDiverged)?;
    if !verification
        .collected_pickup_ids
        .iter()
        .any(|id| id == &objective.required_pickup_id)
    {
        return Err(
            PickupFromDoorValidationError::ReplayDidNotCollectRequiredPickup {
                required_pickup_id: objective.required_pickup_id.clone(),
                collected_pickup_ids: verification.collected_pickup_ids,
            },
        );
    }

    Ok(fingerprint_pickup_from_door_witness(
        generated_level,
        objective,
        solution,
    ))
}

/// Certify every declared pickup independently from every door arrival.
///
/// Results are grouped by canonical source-door ID, then preserve pickup
/// declaration order. A room with no pickups returns an empty list after its
/// multi-door topology has still been checked.
pub fn validate_all_generated_pickups_from_every_door(
    generated_level: &GeneratedLevel,
) -> Result<Vec<PickupFromDoorCertificate>, PickupFromDoorValidationError> {
    let config = ValidationConfig::for_loadout(generated_level.metadata.intended_abilities);
    validate_all_generated_pickups_from_every_door_with_config(generated_level, &config)
}

/// Certify all pickup-from-door combinations with explicit search controls.
pub fn validate_all_generated_pickups_from_every_door_with_config(
    generated_level: &GeneratedLevel,
    config: &ValidationConfig,
) -> Result<Vec<PickupFromDoorCertificate>, PickupFromDoorValidationError> {
    let door_ids =
        canonical_door_ids(generated_level).map_err(PickupFromDoorValidationError::DoorTopology)?;
    let pickup_ids = declared_pickup_ids(generated_level);
    let mut certificates = Vec::with_capacity(door_ids.len() * pickup_ids.len());
    if pickup_ids.is_empty() {
        return Ok(certificates);
    }

    for source_door_id in &door_ids {
        let objectives: Vec<_> = pickup_ids
            .iter()
            .map(|pickup_id| {
                PickupFromDoorObjective::for_generated(generated_level, source_door_id, pickup_id)
            })
            .collect();
        for objective in &objectives {
            validate_pickup_from_door_objective(generated_level, objective)?;
        }

        let batch = search_all_targets_from_door(
            generated_level,
            source_door_id,
            &door_ids,
            &pickup_ids,
            generated_level.metadata.intended_abilities,
            config,
        )
        .map_err(map_source_search_to_pickup_error)?;
        let pickup_results = batch.results.into_iter().skip(batch.door_result_count);
        for (objective, result) in objectives.into_iter().zip(pickup_results) {
            debug_assert_eq!(
                result.target,
                SearchTarget::pickup(&objective.required_pickup_id)
            );
            certificates.push(certify_pickup_from_door_outcome(
                generated_level.clone(),
                objective,
                &batch.initial,
                result.outcome,
            )?);
        }
    }
    Ok(certificates)
}

/// Validate the internal accounting of one recorded shared solver search.
///
/// The solver is not rerun. Instead, this rejects observations that could not
/// be cumulative snapshots from the supplied source total under `config`:
/// every snapshot is within the configured bounds and source total, positive
/// snapshots form a component-wise cumulative chain, bounded rows carry the
/// exact terminal snapshot and a compatible common reason, and at least one
/// row records the final source total. These checks make operational metrics
/// checksum-bound recorded observations; only replay validation establishes
/// positive reachability.
pub fn validate_recorded_source_search_observations(
    config: &SolverConfig,
    source_total: SearchStats,
    observations: &[RecordedSearchObservation],
) -> Result<(), RecordedSearchObservationValidationError> {
    validate_recorded_solver_config(config)?;
    validate_recorded_search_stats(config, source_total, "source total")?;

    if observations.is_empty() {
        if source_total != SearchStats::default() {
            return Err(recorded_search_error(
                "an empty target batch must have zero source search effort",
            ));
        }
        return Ok(());
    }

    let mut snapshots = Vec::with_capacity(observations.len());
    let mut bounded_reason = None;
    let mut has_terminal_snapshot = false;
    for (index, observation) in observations.iter().copied().enumerate() {
        let (stats, replay_ticks) = match observation {
            RecordedSearchObservation::Positive {
                search_effort,
                replay_ticks,
            } => (search_effort, Some(replay_ticks)),
            RecordedSearchObservation::BoundedInconclusive {
                reason,
                search_effort,
            } => {
                if search_effort != source_total {
                    return Err(recorded_search_error(format!(
                        "bounded observation {index} does not carry the exact terminal source snapshot"
                    )));
                }
                if let Some(previous) = bounded_reason {
                    if previous != reason {
                        return Err(recorded_search_error(format!(
                            "bounded observation {index} has reason {reason:?}, not the shared terminal reason {previous:?}"
                        )));
                    }
                } else {
                    validate_recorded_inconclusive_reason(config, source_total, reason)?;
                    bounded_reason = Some(reason);
                }
                (search_effort, None)
            }
        };
        validate_recorded_search_stats(config, stats, &format!("observation {index}"))?;
        if !search_stats_componentwise_le(stats, source_total) {
            return Err(recorded_search_error(format!(
                "observation {index} exceeds its source terminal snapshot"
            )));
        }
        if let Some(replay_ticks) = replay_ticks {
            if replay_ticks > config.max_ticks_per_path
                || replay_ticks > stats.deepest_path_ticks
                || replay_ticks > stats.simulated_ticks
            {
                return Err(recorded_search_error(format!(
                    "positive observation {index} replay length {replay_ticks} is incompatible with its path/tick effort"
                )));
            }
            if replay_ticks > 0 && (stats.expanded_nodes == 0 || stats.generated_nodes == 0) {
                return Err(recorded_search_error(format!(
                    "positive observation {index} has a nonempty replay but zero expanded/generated work"
                )));
            }
        }
        has_terminal_snapshot |= stats == source_total;
        snapshots.push(stats);
    }

    for left in 0..snapshots.len() {
        for right in left + 1..snapshots.len() {
            if !search_stats_componentwise_le(snapshots[left], snapshots[right])
                && !search_stats_componentwise_le(snapshots[right], snapshots[left])
            {
                return Err(recorded_search_error(format!(
                    "observations {left} and {right} are incomparable cumulative snapshots"
                )));
            }
        }
    }
    if !has_terminal_snapshot {
        return Err(recorded_search_error(
            "no target observation records the final source search snapshot",
        ));
    }
    Ok(())
}

fn validate_recorded_solver_config(
    config: &SolverConfig,
) -> Result<(), RecordedSearchObservationValidationError> {
    if config.beam_width == 0
        || config.max_ticks_per_path == 0
        || config.position_quantum <= 0
        || config.velocity_quantum <= 0
        || config.macros.is_empty()
        || config.macros.iter().any(|action_macro| {
            action_macro.actions.is_empty()
                || action_macro.actions.iter().any(|action| action.restart)
        })
    {
        return Err(recorded_search_error(
            "recorded observations reference an invalid solver configuration",
        ));
    }
    Ok(())
}

fn validate_recorded_search_stats(
    config: &SolverConfig,
    stats: SearchStats,
    label: &str,
) -> Result<(), RecordedSearchObservationValidationError> {
    if stats.expanded_nodes > config.max_expanded_nodes
        || stats.simulated_ticks > config.max_simulated_ticks
        || stats.deepest_path_ticks > config.max_ticks_per_path
        || stats.deepest_path_ticks > stats.simulated_ticks
    {
        return Err(recorded_search_error(format!(
            "{label} exceeds configured node/tick/path bounds"
        )));
    }
    if stats.expanded_nodes == 0
        && (stats.generated_nodes != 0
            || stats.simulated_ticks != 0
            || stats.deepest_path_ticks != 0)
    {
        return Err(recorded_search_error(format!(
            "{label} records work without an expanded node"
        )));
    }
    let generated_ceiling = stats.expanded_nodes.saturating_mul(config.macros.len());
    if stats.generated_nodes > generated_ceiling {
        return Err(recorded_search_error(format!(
            "{label} generated-node count exceeds the configured macro fanout ceiling"
        )));
    }
    Ok(())
}

fn validate_recorded_inconclusive_reason(
    config: &SolverConfig,
    terminal: SearchStats,
    reason: InconclusiveReason,
) -> Result<(), RecordedSearchObservationValidationError> {
    let compatible = match reason {
        // Door/pickup matrices never request AnyExit, so this reason cannot be
        // produced for one of their exact targets.
        InconclusiveReason::NoExitsDefined => false,
        InconclusiveReason::ExpandedNodeBudget => {
            terminal.expanded_nodes == config.max_expanded_nodes
        }
        InconclusiveReason::SimulatedTickBudget => {
            terminal.simulated_ticks == config.max_simulated_ticks
                && config.max_expanded_nodes > 0
                && (config.max_simulated_ticks == 0 || terminal.expanded_nodes > 0)
        }
        InconclusiveReason::PathHorizon | InconclusiveReason::FrontierExhausted => {
            config.max_expanded_nodes > 0
                && config.max_simulated_ticks > 0
                && terminal.expanded_nodes > 0
        }
    };
    if !compatible {
        return Err(recorded_search_error(format!(
            "terminal reason {reason:?} is incompatible with the configured bounds and source total"
        )));
    }
    Ok(())
}

const fn search_stats_componentwise_le(left: SearchStats, right: SearchStats) -> bool {
    left.expanded_nodes <= right.expanded_nodes
        && left.generated_nodes <= right.generated_nodes
        && left.simulated_ticks <= right.simulated_ticks
        && left.deepest_path_ticks <= right.deepest_path_ticks
}

fn recorded_search_error(message: impl Into<String>) -> RecordedSearchObservationValidationError {
    RecordedSearchObservationValidationError(message.into())
}

#[cfg(test)]
mod recorded_search_observation_tests {
    use super::*;

    fn config() -> SolverConfig {
        let mut config = SolverConfig::default();
        config.max_expanded_nodes = 5;
        config.max_simulated_ticks = 20;
        config.max_ticks_per_path = 10;
        config
    }

    #[test]
    fn recorded_search_observations_accept_one_cumulative_shared_search() {
        let source_total = SearchStats {
            expanded_nodes: 2,
            generated_nodes: 3,
            simulated_ticks: 8,
            deepest_path_ticks: 4,
        };
        let observations = [
            RecordedSearchObservation::Positive {
                search_effort: SearchStats {
                    expanded_nodes: 1,
                    generated_nodes: 1,
                    simulated_ticks: 4,
                    deepest_path_ticks: 4,
                },
                replay_ticks: 4,
            },
            RecordedSearchObservation::BoundedInconclusive {
                reason: InconclusiveReason::FrontierExhausted,
                search_effort: source_total,
            },
        ];
        validate_recorded_source_search_observations(&config(), source_total, &observations)
            .unwrap();
    }

    #[test]
    fn recorded_search_observations_reject_forged_terminal_reason_and_stats() {
        let source_total = SearchStats {
            expanded_nodes: 2,
            generated_nodes: 2,
            simulated_ticks: 8,
            deepest_path_ticks: 4,
        };
        let wrong_snapshot = [RecordedSearchObservation::BoundedInconclusive {
            reason: InconclusiveReason::FrontierExhausted,
            search_effort: SearchStats {
                simulated_ticks: 7,
                ..source_total
            },
        }];
        assert!(
            validate_recorded_source_search_observations(&config(), source_total, &wrong_snapshot)
                .unwrap_err()
                .to_string()
                .contains("exact terminal source snapshot")
        );

        let wrong_reason = [RecordedSearchObservation::BoundedInconclusive {
            reason: InconclusiveReason::ExpandedNodeBudget,
            search_effort: source_total,
        }];
        assert!(
            validate_recorded_source_search_observations(&config(), source_total, &wrong_reason)
                .unwrap_err()
                .to_string()
                .contains("terminal reason")
        );
    }

    #[test]
    fn recorded_search_observations_reject_incomparable_discovery_snapshots() {
        let source_total = SearchStats {
            expanded_nodes: 2,
            generated_nodes: 2,
            simulated_ticks: 5,
            deepest_path_ticks: 4,
        };
        let observations = [
            RecordedSearchObservation::Positive {
                search_effort: SearchStats {
                    expanded_nodes: 1,
                    generated_nodes: 2,
                    simulated_ticks: 4,
                    deepest_path_ticks: 4,
                },
                replay_ticks: 0,
            },
            RecordedSearchObservation::Positive {
                search_effort: SearchStats {
                    expanded_nodes: 2,
                    generated_nodes: 1,
                    simulated_ticks: 5,
                    deepest_path_ticks: 3,
                },
                replay_ticks: 0,
            },
            RecordedSearchObservation::Positive {
                search_effort: source_total,
                replay_ticks: 0,
            },
        ];
        assert!(
            validate_recorded_source_search_observations(&config(), source_total, &observations)
                .unwrap_err()
                .to_string()
                .contains("incomparable cumulative snapshots")
        );
    }
}

/// Rebuild a complete matrix from persisted rows without running solver
/// search. Every positive is authoritatively replayed and refingerprinted;
/// bounded rows remain bounded. Canonical row/source order, cardinality,
/// config-bound cumulative observations, and aggregate shared-search
/// accounting are required exactly.
pub fn rehydrate_generated_door_target_evidence(
    generated_level: &GeneratedLevel,
    loadout: AbilitySet,
    solver_config: &SolverConfig,
    recorded_door_routes: Vec<RecordedDoorRouteEvidence>,
    recorded_pickup_routes: Vec<RecordedPickupRouteEvidence>,
    source_search_effort: Vec<DoorSourceSearchEffort>,
    aggregate_search_effort: SearchStats,
) -> Result<DoorTargetEvidenceBatch, DoorTargetEvidenceRehydrationError> {
    if AbilityTier::from_abilities(generated_level.metadata.intended_abilities)
        != generated_level.metadata.ability_tier
    {
        return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
            "generated metadata ability tier is inconsistent".to_owned(),
        ));
    }
    let door_ids = canonical_door_ids(generated_level).map_err(|error| {
        DoorTargetEvidenceRehydrationError::InvalidContract(format!(
            "generated door topology is invalid: {error}"
        ))
    })?;
    let mut pickup_ids = declared_pickup_ids(generated_level);
    pickup_ids.sort_unstable();
    let expected_door_rows = door_ids
        .len()
        .saturating_mul(door_ids.len().saturating_sub(1));
    let expected_pickup_rows = door_ids.len().saturating_mul(pickup_ids.len());
    if recorded_door_routes.len() != expected_door_rows
        || recorded_pickup_routes.len() != expected_pickup_rows
    {
        return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
            format!(
                "row cardinality is {}/{} doors and {}/{} pickups",
                recorded_door_routes.len(),
                expected_door_rows,
                recorded_pickup_routes.len(),
                expected_pickup_rows
            ),
        ));
    }
    if source_search_effort.len() != door_ids.len()
        || source_search_effort
            .iter()
            .map(|source| &source.source_door_id)
            .ne(door_ids.iter())
    {
        return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
            "per-source search effort is not in exact canonical door order".to_owned(),
        ));
    }
    let mut recomputed_aggregate = SearchStats::default();
    for source in &source_search_effort {
        accumulate_search_stats(&mut recomputed_aggregate, source.stats);
    }
    if recomputed_aggregate != aggregate_search_effort {
        return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
            "aggregate search effort differs from the exact per-source sum/max".to_owned(),
        ));
    }

    for (source_index, source_door_id) in door_ids.iter().enumerate() {
        let observations = recorded_door_routes
            .iter()
            .filter(|row| row.source_door_id == *source_door_id)
            .map(|row| recorded_search_observation(&row.evidence))
            .chain(
                recorded_pickup_routes
                    .iter()
                    .filter(|row| row.source_door_id == *source_door_id)
                    .map(|row| recorded_search_observation(&row.evidence)),
            )
            .collect::<Vec<_>>();
        validate_recorded_source_search_observations(
            solver_config,
            source_search_effort[source_index].stats,
            &observations,
        )
        .map_err(|error| {
            DoorTargetEvidenceRehydrationError::InvalidContract(format!(
                "source door {source_door_id:?} has inconsistent recorded search observations: {error}"
            ))
        })?;
    }

    let mut door_routes = Vec::with_capacity(expected_door_rows);
    let mut pickup_routes = Vec::with_capacity(expected_pickup_rows);
    let mut recorded_doors = recorded_door_routes.into_iter();
    let mut recorded_pickups = recorded_pickup_routes.into_iter();
    for source_door_id in &door_ids {
        let initial =
            Simulation::enter_via_door(generated_level.room.clone(), loadout, source_door_id)
                .map_err(|error| DoorTargetEvidenceRehydrationError::EntryRejected {
                    source_door_id: source_door_id.clone(),
                    error,
                })?;
        for target_door_id in door_ids
            .iter()
            .filter(|target_door_id| *target_door_id != source_door_id)
        {
            let recorded = recorded_doors
                .next()
                .expect("exact door cardinality was checked");
            if recorded.source_door_id != *source_door_id
                || recorded.target_door_id != *target_door_id
            {
                return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
                    format!(
                        "door row order differs at {:?} -> {:?}",
                        source_door_id, target_door_id
                    ),
                ));
            }
            let evidence = rehydrate_recorded_door_evidence(
                generated_level,
                loadout,
                source_door_id,
                target_door_id,
                &initial,
                recorded.evidence,
            )?;
            door_routes.push(DoorRouteEvidence {
                source_door_id: source_door_id.clone(),
                target_door_id: target_door_id.clone(),
                evidence,
            });
        }
        for required_pickup_id in &pickup_ids {
            let recorded = recorded_pickups
                .next()
                .expect("exact pickup cardinality was checked");
            if recorded.source_door_id != *source_door_id
                || recorded.required_pickup_id != *required_pickup_id
            {
                return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
                    format!(
                        "pickup row order differs at {:?} -> {:?}",
                        source_door_id, required_pickup_id
                    ),
                ));
            }
            let evidence = rehydrate_recorded_pickup_evidence(
                generated_level,
                loadout,
                source_door_id,
                required_pickup_id,
                &initial,
                recorded.evidence,
            )?;
            pickup_routes.push(PickupRouteEvidence {
                source_door_id: source_door_id.clone(),
                required_pickup_id: required_pickup_id.clone(),
                evidence,
            });
        }
    }
    debug_assert!(recorded_doors.next().is_none());
    debug_assert!(recorded_pickups.next().is_none());
    Ok(DoorTargetEvidenceBatch {
        loadout,
        door_routes,
        pickup_routes,
        source_search_effort,
        aggregate_search_effort,
    })
}

fn recorded_search_observation(recorded: &RecordedTargetEvidence) -> RecordedSearchObservation {
    match recorded {
        RecordedTargetEvidence::PositiveReplay { solution, .. } => {
            RecordedSearchObservation::Positive {
                search_effort: solution.stats,
                replay_ticks: solution.replay.frames.len(),
            }
        }
        RecordedTargetEvidence::BoundedInconclusive(evidence) => {
            RecordedSearchObservation::BoundedInconclusive {
                reason: evidence.reason,
                search_effort: evidence.search_effort,
            }
        }
    }
}

fn rehydrate_recorded_door_evidence(
    generated_level: &GeneratedLevel,
    loadout: AbilitySet,
    source_door_id: &str,
    target_door_id: &str,
    initial: &Simulation,
    recorded: RecordedTargetEvidence,
) -> Result<BoundedTargetEvidence, DoorTargetEvidenceRehydrationError> {
    match recorded {
        RecordedTargetEvidence::BoundedInconclusive(evidence) => {
            Ok(BoundedTargetEvidence::Inconclusive(evidence))
        }
        RecordedTargetEvidence::PositiveReplay {
            solution,
            recorded_witness_id,
        } => {
            let requested = SearchTarget::door(target_door_id);
            if solution.target != requested {
                return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
                    format!(
                        "positive door solution target {:?} differs from requested {requested:?}",
                        solution.target
                    ),
                ));
            }
            let objective = DoorReachabilityObjective::new(
                source_door_id,
                target_door_id,
                generated_level.metadata.clone(),
                loadout,
            );
            let witness_fingerprint =
                verify_door_solution(generated_level, &objective, initial, &solution).map_err(
                    |error| DoorTargetEvidenceRehydrationError::DoorReplayCertification {
                        source_door_id: source_door_id.to_owned(),
                        target_door_id: target_door_id.to_owned(),
                        error: Box::new(error),
                    },
                )?;
            let recomputed = witness_fingerprint.to_string();
            if recorded_witness_id != recomputed {
                return Err(
                    DoorTargetEvidenceRehydrationError::WitnessFingerprintMismatch {
                        source_door_id: source_door_id.to_owned(),
                        target_id: target_door_id.to_owned(),
                        recorded: recorded_witness_id,
                        recomputed,
                    },
                );
            }
            Ok(BoundedTargetEvidence::Positive(
                ReplayCertifiedTargetEvidence {
                    solution,
                    witness_fingerprint,
                },
            ))
        }
    }
}

fn rehydrate_recorded_pickup_evidence(
    generated_level: &GeneratedLevel,
    loadout: AbilitySet,
    source_door_id: &str,
    required_pickup_id: &str,
    initial: &Simulation,
    recorded: RecordedTargetEvidence,
) -> Result<BoundedTargetEvidence, DoorTargetEvidenceRehydrationError> {
    match recorded {
        RecordedTargetEvidence::BoundedInconclusive(evidence) => {
            Ok(BoundedTargetEvidence::Inconclusive(evidence))
        }
        RecordedTargetEvidence::PositiveReplay {
            solution,
            recorded_witness_id,
        } => {
            let requested = SearchTarget::pickup(required_pickup_id);
            if solution.target != requested {
                return Err(DoorTargetEvidenceRehydrationError::InvalidContract(
                    format!(
                        "positive pickup solution target {:?} differs from requested {requested:?}",
                        solution.target
                    ),
                ));
            }
            let objective = PickupFromDoorObjective::new(
                source_door_id,
                required_pickup_id,
                generated_level.metadata.clone(),
                loadout,
            );
            let witness_fingerprint =
                verify_pickup_from_door_solution(generated_level, &objective, initial, &solution)
                    .map_err(
                    |error| DoorTargetEvidenceRehydrationError::PickupReplayCertification {
                        source_door_id: source_door_id.to_owned(),
                        required_pickup_id: required_pickup_id.to_owned(),
                        error: Box::new(error),
                    },
                )?;
            let recomputed = witness_fingerprint.to_string();
            if recorded_witness_id != recomputed {
                return Err(
                    DoorTargetEvidenceRehydrationError::WitnessFingerprintMismatch {
                        source_door_id: source_door_id.to_owned(),
                        target_id: required_pickup_id.to_owned(),
                        recorded: recorded_witness_id,
                        recomputed,
                    },
                );
            }
            Ok(BoundedTargetEvidence::Positive(
                ReplayCertifiedTargetEvidence {
                    solution,
                    witness_fingerprint,
                },
            ))
        }
    }
}

/// Evaluate every ordered door route and pickup objective for one explicit
/// traversal loadout without requiring it to match generation metadata.
///
/// One shared [`solve_targets`] frontier is used per source door. Every valid
/// requested target gets an entry: replay-verified successes become
/// [`BoundedTargetEvidence::Positive`], while bounded non-successes remain
/// explicitly [`BoundedTargetEvidence::Inconclusive`]. The function never
/// infers unreachability and does not run heuristic difficulty analysis.
///
/// `config` controls the bounded search vocabulary and budgets. Callers
/// normally construct it with [`ValidationConfig::for_loadout`] for the same
/// `loadout`, but an explicit custom configuration is permitted for research.
pub fn evaluate_generated_door_targets_for_loadout(
    generated_level: &GeneratedLevel,
    loadout: AbilitySet,
    config: &ValidationConfig,
) -> Result<DoorTargetEvidenceBatch, DoorTargetEvidenceError> {
    if AbilityTier::from_abilities(generated_level.metadata.intended_abilities)
        != generated_level.metadata.ability_tier
    {
        return Err(DoorTargetEvidenceError::DoorTopology(Box::new(
            DoorValidationError::MetadataAbilityTierMismatch {
                ability_tier: generated_level.metadata.ability_tier,
                intended_abilities: generated_level.metadata.intended_abilities,
            },
        )));
    }

    let door_ids = canonical_door_ids(generated_level)
        .map_err(|error| DoorTargetEvidenceError::DoorTopology(Box::new(error)))?;
    let mut pickup_ids = declared_pickup_ids(generated_level);
    pickup_ids.sort_unstable();

    let pair_count = door_ids.len() * (door_ids.len() - 1);
    let mut door_routes = Vec::with_capacity(pair_count);
    let mut pickup_routes = Vec::with_capacity(door_ids.len() * pickup_ids.len());
    let mut source_search_effort = Vec::with_capacity(door_ids.len());
    let mut aggregate_search_effort = SearchStats::default();

    for source_door_id in &door_ids {
        let mut batch = search_all_targets_from_door(
            generated_level,
            source_door_id,
            &door_ids,
            &pickup_ids,
            loadout,
            config,
        )
        .map_err(|error| map_source_search_to_evidence_error(source_door_id, error))?;
        accumulate_search_stats(&mut aggregate_search_effort, batch.stats);
        source_search_effort.push(DoorSourceSearchEffort {
            source_door_id: source_door_id.clone(),
            stats: batch.stats,
        });

        let pickup_results = batch.results.split_off(batch.door_result_count);
        let target_door_ids = door_ids
            .iter()
            .filter(|target_door_id| *target_door_id != source_door_id);
        for (target_door_id, result) in target_door_ids.zip(batch.results) {
            let requested = SearchTarget::door(target_door_id);
            ensure_solver_target(source_door_id, &requested, &result.target)?;
            let objective = DoorReachabilityObjective::new(
                source_door_id,
                target_door_id,
                generated_level.metadata.clone(),
                loadout,
            );
            let evidence = match result.outcome {
                TargetSolveOutcome::Solved(solution) => {
                    ensure_solver_target(source_door_id, &requested, &solution.target)?;
                    let witness_fingerprint = verify_door_solution(
                        generated_level,
                        &objective,
                        &batch.initial,
                        &solution,
                    )
                    .map_err(|error| {
                        DoorTargetEvidenceError::DoorReplayCertification {
                            source_door_id: source_door_id.clone(),
                            target_door_id: target_door_id.clone(),
                            error: Box::new(error),
                        }
                    })?;
                    BoundedTargetEvidence::Positive(ReplayCertifiedTargetEvidence {
                        solution,
                        witness_fingerprint,
                    })
                }
                TargetSolveOutcome::Inconclusive { reason, stats } => {
                    BoundedTargetEvidence::Inconclusive(BoundedInconclusiveEvidence {
                        reason,
                        search_effort: stats,
                    })
                }
            };
            door_routes.push(DoorRouteEvidence {
                source_door_id: source_door_id.clone(),
                target_door_id: target_door_id.clone(),
                evidence,
            });
        }

        for (required_pickup_id, result) in pickup_ids.iter().zip(pickup_results) {
            let requested = SearchTarget::pickup(required_pickup_id);
            ensure_solver_target(source_door_id, &requested, &result.target)?;
            let objective = PickupFromDoorObjective::new(
                source_door_id,
                required_pickup_id,
                generated_level.metadata.clone(),
                loadout,
            );
            let evidence = match result.outcome {
                TargetSolveOutcome::Solved(solution) => {
                    ensure_solver_target(source_door_id, &requested, &solution.target)?;
                    let witness_fingerprint = verify_pickup_from_door_solution(
                        generated_level,
                        &objective,
                        &batch.initial,
                        &solution,
                    )
                    .map_err(|error| {
                        DoorTargetEvidenceError::PickupReplayCertification {
                            source_door_id: source_door_id.clone(),
                            required_pickup_id: required_pickup_id.clone(),
                            error: Box::new(error),
                        }
                    })?;
                    BoundedTargetEvidence::Positive(ReplayCertifiedTargetEvidence {
                        solution,
                        witness_fingerprint,
                    })
                }
                TargetSolveOutcome::Inconclusive { reason, stats } => {
                    BoundedTargetEvidence::Inconclusive(BoundedInconclusiveEvidence {
                        reason,
                        search_effort: stats,
                    })
                }
            };
            pickup_routes.push(PickupRouteEvidence {
                source_door_id: source_door_id.clone(),
                required_pickup_id: required_pickup_id.clone(),
                evidence,
            });
        }
    }

    Ok(DoorTargetEvidenceBatch {
        loadout,
        door_routes,
        pickup_routes,
        source_search_effort,
        aggregate_search_effort,
    })
}

fn ensure_solver_target(
    source_door_id: &str,
    requested: &SearchTarget,
    reported: &SearchTarget,
) -> Result<(), DoorTargetEvidenceError> {
    if reported == requested {
        return Ok(());
    }
    Err(DoorTargetEvidenceError::SolverContractMismatch {
        source_door_id: source_door_id.to_owned(),
        requested: requested.clone(),
        reported: reported.clone(),
    })
}

fn accumulate_search_stats(aggregate: &mut SearchStats, source: SearchStats) {
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

/// Certify every ordered door pair and every pickup-from-door objective with
/// exactly one shared solver exploration per canonical source door.
pub fn validate_all_generated_door_targets(
    generated_level: &GeneratedLevel,
) -> Result<DoorTargetCertificateBatch, DoorTargetBatchValidationError> {
    let config = ValidationConfig::for_loadout(generated_level.metadata.intended_abilities);
    validate_all_generated_door_targets_with_config(generated_level, &config)
}

/// Certify every target reachable from every door using explicit bounded
/// search controls. Within a source, door targets are requested first in
/// canonical order and pickups follow in declaration order. The first failed
/// result in that ordering is returned with its source and target identity.
pub fn validate_all_generated_door_targets_with_config(
    generated_level: &GeneratedLevel,
    config: &ValidationConfig,
) -> Result<DoorTargetCertificateBatch, DoorTargetBatchValidationError> {
    let door_ids = canonical_door_ids(generated_level)
        .map_err(|error| DoorTargetBatchValidationError::DoorTopology(Box::new(error)))?;
    let pickup_ids = declared_pickup_ids(generated_level);
    let pair_count = door_ids.len() * (door_ids.len() - 1);
    let mut door_pairs = Vec::with_capacity(pair_count);
    let mut pickups_from_doors = Vec::with_capacity(door_ids.len() * pickup_ids.len());
    let mut source_search_effort = Vec::with_capacity(door_ids.len());

    for source_door_id in &door_ids {
        let door_objectives: Vec<_> = door_ids
            .iter()
            .filter(|target_door_id| *target_door_id != source_door_id)
            .map(|target_door_id| {
                DoorReachabilityObjective::for_generated(
                    generated_level,
                    source_door_id,
                    target_door_id,
                )
            })
            .collect();
        let pickup_objectives: Vec<_> = pickup_ids
            .iter()
            .map(|pickup_id| {
                PickupFromDoorObjective::for_generated(generated_level, source_door_id, pickup_id)
            })
            .collect();

        for objective in &door_objectives {
            validate_door_objective(generated_level, objective).map_err(|error| {
                DoorTargetBatchValidationError::Door {
                    source_door_id: objective.source_door_id.clone(),
                    target_door_id: objective.target_door_id.clone(),
                    error: Box::new(error),
                }
            })?;
        }
        for objective in &pickup_objectives {
            validate_pickup_from_door_objective(generated_level, objective).map_err(|error| {
                DoorTargetBatchValidationError::Pickup {
                    source_door_id: objective.source_door_id.clone(),
                    required_pickup_id: objective.required_pickup_id.clone(),
                    error: Box::new(error),
                }
            })?;
        }

        let mut batch = search_all_targets_from_door(
            generated_level,
            source_door_id,
            &door_ids,
            &pickup_ids,
            generated_level.metadata.intended_abilities,
            config,
        )
        .map_err(|error| DoorTargetBatchValidationError::Door {
            source_door_id: source_door_id.clone(),
            target_door_id: door_objectives[0].target_door_id.clone(),
            error: Box::new(map_source_search_to_door_error(error)),
        })?;
        debug_assert_eq!(batch.door_result_count, door_objectives.len());
        source_search_effort.push(DoorSourceSearchEffort {
            source_door_id: source_door_id.clone(),
            stats: batch.stats,
        });
        let pickup_results = batch.results.split_off(batch.door_result_count);

        for (objective, result) in door_objectives.into_iter().zip(batch.results.into_iter()) {
            debug_assert_eq!(result.target, SearchTarget::door(&objective.target_door_id));
            let source = objective.source_door_id.clone();
            let target = objective.target_door_id.clone();
            door_pairs.push(
                certify_door_outcome(
                    generated_level.clone(),
                    objective,
                    &batch.initial,
                    result.outcome,
                    config,
                )
                .map_err(|error| DoorTargetBatchValidationError::Door {
                    source_door_id: source,
                    target_door_id: target,
                    error: Box::new(error),
                })?,
            );
        }
        for (objective, result) in pickup_objectives.into_iter().zip(pickup_results) {
            debug_assert_eq!(
                result.target,
                SearchTarget::pickup(&objective.required_pickup_id)
            );
            let source = objective.source_door_id.clone();
            let pickup = objective.required_pickup_id.clone();
            pickups_from_doors.push(
                certify_pickup_from_door_outcome(
                    generated_level.clone(),
                    objective,
                    &batch.initial,
                    result.outcome,
                )
                .map_err(|error| DoorTargetBatchValidationError::Pickup {
                    source_door_id: source,
                    required_pickup_id: pickup,
                    error: Box::new(error),
                })?,
            );
        }
    }

    Ok(DoorTargetCertificateBatch {
        door_pairs,
        pickups_from_doors,
        source_search_effort,
    })
}

/// Validate one generated pickup with loadout-aware default solver settings.
pub fn validate_generated_pickup_reachability(
    generated_level: GeneratedLevel,
    objective: PickupReachabilityObjective,
) -> Result<PickupReachabilityCertificate, PickupValidationError> {
    let config = ValidationConfig::for_loadout(objective.loadout);
    validate_generated_pickup_reachability_with_config(generated_level, objective, &config)
}

/// Validate one exact pickup objective and return its positive replay proof.
pub fn validate_generated_pickup_reachability_with_config(
    generated_level: GeneratedLevel,
    objective: PickupReachabilityObjective,
    config: &ValidationConfig,
) -> Result<PickupReachabilityCertificate, PickupValidationError> {
    validate_pickup_objective(&generated_level, &objective)?;

    let initial = Simulation::with_abilities(generated_level.room.clone(), objective.loadout);
    let outcome = solve_target(
        &initial,
        SearchTarget::pickup(&objective.required_pickup_id),
        &config.solver,
    )
    .map_err(|error| match error {
        TargetSolveError::SolverConfiguration(error) => {
            PickupValidationError::SolverConfiguration(error)
        }
        other => PickupValidationError::SearchTargetRejected(other),
    })?;
    let solution = match outcome {
        TargetSolveOutcome::Solved(solution) => solution,
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            return Err(PickupValidationError::Inconclusive {
                reason,
                search_effort: stats,
            });
        }
    };

    let expected_reached = ReachedTarget::Pickup(objective.required_pickup_id.clone());
    if solution.reached != expected_reached {
        return Err(PickupValidationError::WrongReachedTarget {
            required_pickup_id: objective.required_pickup_id.clone(),
            actual: solution.reached,
        });
    }

    let verification = solution
        .replay
        .verify(&initial)
        .map_err(PickupValidationError::ReplayDiverged)?;
    if !verification
        .collected_pickup_ids
        .iter()
        .any(|id| id == &objective.required_pickup_id)
    {
        return Err(PickupValidationError::ReplayDidNotCollectRequiredPickup {
            required_pickup_id: objective.required_pickup_id.clone(),
            collected_pickup_ids: verification.collected_pickup_ids,
        });
    }

    let witness_fingerprint = fingerprint_pickup_witness(&generated_level, &objective, &solution);
    Ok(PickupReachabilityCertificate {
        generated_level,
        objective,
        solution,
        witness_fingerprint,
    })
}

/// Independently validate every pickup in a generated room from its fresh
/// spawn state. An empty room returns an empty certificate list.
pub fn validate_all_generated_pickups(
    generated_level: &GeneratedLevel,
) -> Result<Vec<PickupReachabilityCertificate>, PickupValidationError> {
    let config = ValidationConfig::for_loadout(generated_level.metadata.intended_abilities);
    validate_all_generated_pickups_with_config(generated_level, &config)
}

/// Independently validate every pickup using an explicit bounded-search
/// configuration. Certificates preserve room declaration order.
pub fn validate_all_generated_pickups_with_config(
    generated_level: &GeneratedLevel,
    config: &ValidationConfig,
) -> Result<Vec<PickupReachabilityCertificate>, PickupValidationError> {
    let pickup_ids: Vec<_> = generated_level
        .room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect();
    pickup_ids
        .into_iter()
        .map(|pickup_id| {
            let objective = PickupReachabilityObjective::for_generated(generated_level, pickup_id);
            validate_generated_pickup_reachability_with_config(
                generated_level.clone(),
                objective,
                config,
            )
        })
        .collect()
}

/// Shared exploration output for every exact objective originating at one
/// door. Door results occupy the canonical prefix; pickup results follow in
/// declaration order. Keeping this representation internal lets the public
/// batch APIs retain their established certificate and error types.
struct SourceDoorTargetSearch {
    initial: Simulation,
    results: Vec<BatchTargetResult>,
    door_result_count: usize,
    stats: SearchStats,
}

enum SourceDoorSearchError {
    Entry(DoorEntryError),
    Search(BatchTargetSolveError),
}

fn search_all_targets_from_door(
    generated_level: &GeneratedLevel,
    source_door_id: &str,
    canonical_door_ids: &[String],
    pickup_ids: &[String],
    loadout: AbilitySet,
    config: &ValidationConfig,
) -> Result<SourceDoorTargetSearch, SourceDoorSearchError> {
    let door_targets = canonical_door_ids
        .iter()
        .filter(|target_door_id| target_door_id.as_str() != source_door_id)
        .map(SearchTarget::door);
    let door_result_count = door_targets.clone().count();
    let targets: Vec<_> = door_targets
        .chain(pickup_ids.iter().map(SearchTarget::pickup))
        .collect();
    let initial = Simulation::enter_via_door(generated_level.room.clone(), loadout, source_door_id)
        .map_err(SourceDoorSearchError::Entry)?;
    let batch =
        solve_targets(&initial, &targets, &config.solver).map_err(SourceDoorSearchError::Search)?;
    debug_assert_eq!(batch.results.len(), targets.len());
    Ok(SourceDoorTargetSearch {
        initial,
        results: batch.results,
        door_result_count,
        stats: batch.stats,
    })
}

fn map_source_search_to_door_error(error: SourceDoorSearchError) -> DoorValidationError {
    match error {
        SourceDoorSearchError::Entry(error) => DoorValidationError::EntryRejected(error),
        SourceDoorSearchError::Search(BatchTargetSolveError::SolverConfiguration(error)) => {
            DoorValidationError::SolverConfiguration(error)
        }
        SourceDoorSearchError::Search(BatchTargetSolveError::InvalidTarget { error, .. }) => {
            DoorValidationError::SearchTargetRejected(error)
        }
    }
}

fn map_source_search_to_pickup_error(
    error: SourceDoorSearchError,
) -> PickupFromDoorValidationError {
    match error {
        SourceDoorSearchError::Entry(error) => PickupFromDoorValidationError::EntryRejected(error),
        SourceDoorSearchError::Search(BatchTargetSolveError::SolverConfiguration(error)) => {
            PickupFromDoorValidationError::SolverConfiguration(error)
        }
        SourceDoorSearchError::Search(BatchTargetSolveError::InvalidTarget { error, .. }) => {
            PickupFromDoorValidationError::SearchTargetRejected(error)
        }
    }
}

fn map_source_search_to_evidence_error(
    source_door_id: &str,
    error: SourceDoorSearchError,
) -> DoorTargetEvidenceError {
    match error {
        SourceDoorSearchError::Entry(error) => DoorTargetEvidenceError::EntryRejected {
            source_door_id: source_door_id.to_owned(),
            error,
        },
        SourceDoorSearchError::Search(BatchTargetSolveError::SolverConfiguration(error)) => {
            DoorTargetEvidenceError::SolverConfiguration(error)
        }
        SourceDoorSearchError::Search(BatchTargetSolveError::InvalidTarget {
            target,
            error,
            ..
        }) => DoorTargetEvidenceError::SearchTargetRejected {
            source_door_id: source_door_id.to_owned(),
            target,
            error,
        },
    }
}

fn declared_pickup_ids(generated_level: &GeneratedLevel) -> Vec<String> {
    generated_level
        .room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect()
}

fn canonical_door_ids(
    generated_level: &GeneratedLevel,
) -> Result<Vec<String>, DoorValidationError> {
    let doors = generated_level.room.doors();
    if doors.len() < 2 {
        return Err(DoorValidationError::FewerThanTwoDoors {
            actual: doors.len(),
        });
    }

    let mut seen = HashSet::with_capacity(doors.len());
    let mut ids = Vec::with_capacity(doors.len());
    for door in doors {
        if door.id.is_empty() || door.id.trim() != door.id {
            return Err(DoorValidationError::NonCanonicalDoorId {
                door_id: door.id.clone(),
            });
        }
        if !seen.insert(door.id.as_str()) {
            return Err(DoorValidationError::DuplicateDoorId {
                door_id: door.id.clone(),
            });
        }
        ids.push(door.id.clone());
    }
    ids.sort_unstable();
    Ok(ids)
}

fn validate_door_objective(
    generated_level: &GeneratedLevel,
    objective: &DoorReachabilityObjective,
) -> Result<(), DoorValidationError> {
    if objective.source_door_id.trim().is_empty() {
        return Err(DoorValidationError::EmptySourceDoorId);
    }
    if objective.target_door_id.trim().is_empty() {
        return Err(DoorValidationError::EmptyTargetDoorId);
    }
    if objective.source_door_id == objective.target_door_id {
        return Err(DoorValidationError::SameSourceAndTarget {
            door_id: objective.source_door_id.clone(),
        });
    }
    if objective.expected_metadata != generated_level.metadata {
        return Err(DoorValidationError::MetadataMismatch {
            expected: objective.expected_metadata.clone(),
            actual: generated_level.metadata.clone(),
        });
    }
    if objective.loadout != objective.expected_metadata.intended_abilities {
        return Err(DoorValidationError::ObjectiveLoadoutMismatch {
            objective_loadout: objective.loadout,
            metadata_loadout: objective.expected_metadata.intended_abilities,
        });
    }
    if AbilityTier::from_abilities(generated_level.metadata.intended_abilities)
        != generated_level.metadata.ability_tier
    {
        return Err(DoorValidationError::MetadataAbilityTierMismatch {
            ability_tier: generated_level.metadata.ability_tier,
            intended_abilities: generated_level.metadata.intended_abilities,
        });
    }

    let door_ids = canonical_door_ids(generated_level)?;
    if door_ids.binary_search(&objective.source_door_id).is_err() {
        return Err(DoorValidationError::SourceDoorNotDefined {
            source_door_id: objective.source_door_id.clone(),
        });
    }
    if door_ids.binary_search(&objective.target_door_id).is_err() {
        return Err(DoorValidationError::TargetDoorNotDefined {
            target_door_id: objective.target_door_id.clone(),
        });
    }
    Ok(())
}

fn validate_pickup_objective(
    generated_level: &GeneratedLevel,
    objective: &PickupReachabilityObjective,
) -> Result<(), PickupValidationError> {
    if objective.required_pickup_id.trim().is_empty() {
        return Err(PickupValidationError::EmptyRequiredPickupId);
    }
    if objective.expected_metadata != generated_level.metadata {
        return Err(PickupValidationError::MetadataMismatch {
            expected: objective.expected_metadata.clone(),
            actual: generated_level.metadata.clone(),
        });
    }
    if objective.loadout != objective.expected_metadata.intended_abilities {
        return Err(PickupValidationError::ObjectiveLoadoutMismatch {
            objective_loadout: objective.loadout,
            metadata_loadout: objective.expected_metadata.intended_abilities,
        });
    }
    if AbilityTier::from_abilities(generated_level.metadata.intended_abilities)
        != generated_level.metadata.ability_tier
    {
        return Err(PickupValidationError::MetadataAbilityTierMismatch {
            ability_tier: generated_level.metadata.ability_tier,
            intended_abilities: generated_level.metadata.intended_abilities,
        });
    }
    if !generated_level
        .room
        .pickups()
        .iter()
        .any(|pickup| pickup.id() == objective.required_pickup_id)
    {
        return Err(PickupValidationError::RequiredPickupNotDefined {
            required_pickup_id: objective.required_pickup_id.clone(),
        });
    }
    Ok(())
}

fn validate_objective(
    generated_level: &GeneratedLevel,
    objective: &ScenarioObjective,
) -> Result<(), ValidationError> {
    if objective.required_exit_id.trim().is_empty() {
        return Err(ValidationError::EmptyRequiredExitId);
    }
    if objective.expected_metadata != generated_level.metadata {
        return Err(ValidationError::MetadataMismatch {
            expected: objective.expected_metadata.clone(),
            actual: generated_level.metadata.clone(),
        });
    }
    if objective.loadout != objective.expected_metadata.intended_abilities {
        return Err(ValidationError::ObjectiveLoadoutMismatch {
            objective_loadout: objective.loadout,
            metadata_loadout: objective.expected_metadata.intended_abilities,
        });
    }
    if AbilityTier::from_abilities(generated_level.metadata.intended_abilities)
        != generated_level.metadata.ability_tier
    {
        return Err(ValidationError::MetadataAbilityTierMismatch {
            ability_tier: generated_level.metadata.ability_tier,
            intended_abilities: generated_level.metadata.intended_abilities,
        });
    }

    let exits = generated_level.room.exits();
    match exits {
        [] => Err(ValidationError::RequiredExitNotDefined {
            required_exit_id: objective.required_exit_id.clone(),
        }),
        [only] if only.id == objective.required_exit_id => Ok(()),
        [only] => Err(ValidationError::WrongExit {
            required_exit_id: objective.required_exit_id.clone(),
            actual_exit_id: only.id.clone(),
        }),
        multiple => Err(ValidationError::UnsupportedAmbiguousObjective {
            required_exit_id: objective.required_exit_id.clone(),
            available_exit_ids: multiple.iter().map(|exit| exit.id.clone()).collect(),
        }),
    }
}

/// Recompute the stable fingerprint for an exact witness.
///
/// Validation calls this only after replay and exit verification, but it is
/// public so stored certificate data can be audited independently.
#[must_use]
pub fn fingerprint_witness(
    generated_level: &GeneratedLevel,
    objective: &ScenarioObjective,
    solution: &Solution,
) -> WitnessFingerprint {
    let mut hash = StableFingerprint::new();
    hash.bytes(b"downwards-validation-witness");
    hash.u32(WITNESS_FINGERPRINT_VERSION);
    hash.metadata(&generated_level.metadata);
    hash.string(&objective.required_exit_id);
    hash.abilities(objective.loadout);
    hash.string(&solution.exit_id);
    hash.u64(solution.replay.initial_digest.0);
    hash.search_stats(solution.stats);
    hash.u64(solution.replay.frames.len() as u64);
    for frame in &solution.replay.frames {
        hash.action(frame.action);
        hash.u64(frame.expected_digest.0);
        hash.u64(frame.expected_event_digest.0);
    }
    WitnessFingerprint(hash.finish())
}

/// Recompute the event-aware fingerprint for an exact pickup witness.
///
/// Its domain is separate from exit acceptance fingerprints, so even an
/// identical input stream cannot conflate the two kinds of claim.
#[must_use]
pub fn fingerprint_pickup_witness(
    generated_level: &GeneratedLevel,
    objective: &PickupReachabilityObjective,
    solution: &TargetSolution,
) -> WitnessFingerprint {
    let mut hash = StableFingerprint::new();
    hash.bytes(b"downwards-validation-pickup-witness");
    hash.u32(WITNESS_FINGERPRINT_VERSION);
    hash.metadata(&generated_level.metadata);
    hash.string(&objective.required_pickup_id);
    hash.abilities(objective.loadout);
    match &solution.target {
        SearchTarget::AnyExit => hash.byte(0),
        SearchTarget::Exit(id) => {
            hash.byte(1);
            hash.string(id);
        }
        SearchTarget::Pickup(id) => {
            hash.byte(2);
            hash.string(id);
        }
        SearchTarget::Door(id) => {
            hash.byte(3);
            hash.string(id);
        }
    }
    match &solution.reached {
        ReachedTarget::Exit(id) => {
            hash.byte(0);
            hash.string(id);
        }
        ReachedTarget::Pickup(id) => {
            hash.byte(1);
            hash.string(id);
        }
        ReachedTarget::Door(id) => {
            hash.byte(2);
            hash.string(id);
        }
    }
    hash.u64(solution.replay.initial_digest.0);
    hash.search_stats(solution.stats);
    hash.u64(solution.replay.frames.len() as u64);
    for frame in &solution.replay.frames {
        hash.action(frame.action);
        hash.u64(frame.expected_digest.0);
        hash.u64(frame.expected_event_digest.0);
    }
    WitnessFingerprint(hash.finish())
}

/// Recompute the domain-separated fingerprint for an ordered door witness.
///
/// The source ID is explicit in addition to the replay's entry-aware initial
/// digest, making the claimed direction auditable without decoding core state.
#[must_use]
pub fn fingerprint_door_witness(
    generated_level: &GeneratedLevel,
    objective: &DoorReachabilityObjective,
    solution: &TargetSolution,
) -> WitnessFingerprint {
    let mut hash = StableFingerprint::new();
    hash.bytes(b"downwards-validation-door-pair-witness");
    hash.u32(WITNESS_FINGERPRINT_VERSION);
    hash.metadata(&generated_level.metadata);
    hash.string(&objective.source_door_id);
    hash.string(&objective.target_door_id);
    hash.abilities(objective.loadout);
    match &solution.target {
        SearchTarget::Door(id) => {
            hash.byte(0);
            hash.string(id);
        }
        SearchTarget::AnyExit => hash.byte(1),
        SearchTarget::Exit(id) => {
            hash.byte(2);
            hash.string(id);
        }
        SearchTarget::Pickup(id) => {
            hash.byte(3);
            hash.string(id);
        }
    }
    match &solution.reached {
        ReachedTarget::Door(id) => {
            hash.byte(0);
            hash.string(id);
        }
        ReachedTarget::Exit(id) => {
            hash.byte(1);
            hash.string(id);
        }
        ReachedTarget::Pickup(id) => {
            hash.byte(2);
            hash.string(id);
        }
    }
    hash.u64(solution.replay.initial_digest.0);
    hash.search_stats(solution.stats);
    hash.u64(solution.replay.frames.len() as u64);
    for frame in &solution.replay.frames {
        hash.action(frame.action);
        hash.u64(frame.expected_digest.0);
        hash.u64(frame.expected_event_digest.0);
    }
    WitnessFingerprint(hash.finish())
}

/// Recompute the domain-separated fingerprint for a pickup reached from a
/// specific door arrival.
#[must_use]
pub fn fingerprint_pickup_from_door_witness(
    generated_level: &GeneratedLevel,
    objective: &PickupFromDoorObjective,
    solution: &TargetSolution,
) -> WitnessFingerprint {
    let mut hash = StableFingerprint::new();
    hash.bytes(b"downwards-validation-pickup-from-door-witness");
    hash.u32(WITNESS_FINGERPRINT_VERSION);
    hash.metadata(&generated_level.metadata);
    hash.string(&objective.source_door_id);
    hash.string(&objective.required_pickup_id);
    hash.abilities(objective.loadout);
    match &solution.target {
        SearchTarget::Pickup(id) => {
            hash.byte(0);
            hash.string(id);
        }
        SearchTarget::Door(id) => {
            hash.byte(1);
            hash.string(id);
        }
        SearchTarget::Exit(id) => {
            hash.byte(2);
            hash.string(id);
        }
        SearchTarget::AnyExit => hash.byte(3),
    }
    match &solution.reached {
        ReachedTarget::Pickup(id) => {
            hash.byte(0);
            hash.string(id);
        }
        ReachedTarget::Door(id) => {
            hash.byte(1);
            hash.string(id);
        }
        ReachedTarget::Exit(id) => {
            hash.byte(2);
            hash.string(id);
        }
    }
    hash.u64(solution.replay.initial_digest.0);
    hash.search_stats(solution.stats);
    hash.u64(solution.replay.frames.len() as u64);
    for frame in &solution.replay.frames {
        hash.action(frame.action);
        hash.u64(frame.expected_digest.0);
        hash.u64(frame.expected_event_digest.0);
    }
    WitnessFingerprint(hash.finish())
}

/// FNV-1a with an explicitly versioned, fixed-width little-endian encoding.
/// This avoids relying on `std` hash implementation details or randomized
/// hash-builder keys for a persisted regression identifier.
struct StableFingerprint(u64);

impl StableFingerprint {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        self.u64(values.len() as u64);
        for &value in values {
            self.byte(value);
        }
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn abilities(&mut self, abilities: AbilitySet) {
        self.bool(abilities.wall_jump);
        self.bool(abilities.dash);
    }

    fn action(&mut self, action: Action) {
        self.byte(action.move_x as u8);
        self.byte(action.move_y as u8);
        self.bool(action.jump);
        self.bool(action.dash);
        self.bool(action.restart);
    }

    fn metadata(&mut self, metadata: &GeneratedMetadata) {
        self.u32(metadata.generation_version);
        self.u64(metadata.seed);
        self.byte(layout_family_tag(metadata.layout_family));
        self.byte(ability_tier_tag(metadata.ability_tier));
        self.abilities(metadata.intended_abilities);
        self.generation_stats(metadata.stats);
    }

    fn generation_stats(&mut self, stats: GenerationStats) {
        self.u16(stats.solid_tiles);
        self.u16(stats.boundary_solid_tiles);
        self.u16(stats.interior_solid_tiles);
        self.u16(stats.one_way_tiles);
        self.u16(stats.hazard_tiles);
        self.u16(stats.hazard_clusters);
        self.u16(stats.timed_hazards);
        self.u16(stats.pickups);
        self.u16(stats.route_waypoints);
    }

    fn search_stats(&mut self, stats: SearchStats) {
        self.u64(stats.expanded_nodes as u64);
        self.u64(stats.generated_nodes as u64);
        self.u64(stats.simulated_ticks as u64);
        self.u64(stats.deepest_path_ticks as u64);
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

const fn layout_family_tag(family: LayoutFamily) -> u8 {
    match family {
        LayoutFamily::HazardRun => 0,
        LayoutFamily::TerracedAscent => 1,
        LayoutFamily::Chimney => 2,
        LayoutFamily::DashGallery => 3,
    }
}

const fn ability_tier_tag(tier: AbilityTier) -> u8 {
    match tier {
        AbilityTier::Baseline => 0,
        AbilityTier::WallJump => 1,
        AbilityTier::Dash => 2,
        AbilityTier::WallJumpAndDash => 3,
    }
}
