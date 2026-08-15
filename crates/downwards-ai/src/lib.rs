//! Headless solving, replay validation, and difficulty estimation.

#![forbid(unsafe_code)]

mod difficulty;
mod replay;
mod shaky_hand;
mod solver;
mod waypoint;

pub use difficulty::{
    BatchDifficultyResult, ComplexityBand, ComplexityComponents, DIFFICULTY_HEURISTIC_VERSION,
    DifficultyAnalysis, DifficultyCase, DifficultyConfig, DifficultyError,
    DifficultyInterpretation, DifficultyReport, HEURISTIC_DIFFICULTY_DISCLAIMER, HazardReference,
    MinimumHazardClearance, PerturbationDivergence, PerturbationFailureDiagnostic,
    PerturbationOutcome, PerturbationTrial, ProvisionalComplexity, TemporalRobustness,
    analyze_batch, analyze_solution, analyze_solve_outcome, minimum_hazard_clearance,
};
pub use replay::{
    EVENT_DIGEST_VERSION, EventDigest, Replay, ReplayDivergence, ReplayFrame, ReplayVerification,
    digest_events, record_replay, verify_replay,
};
pub use shaky_hand::{
    DeathDiagnostic, ExactConvergence, MalformedScheduleReason, NoiseFamily, NoisyReplayCurve,
    NoisyReplayDivergence, NoisyReplayFailureDiagnostic, NoisyReplayOutcome, NoisyReplayTrial,
    PerturbationEdit, PerturbationSchedule, RecordedNoiseCurve, ReplanningSupport,
    SHAKY_HAND_CONFIG_VERSION, SHAKY_HAND_EVIDENCE_DISCLAIMER, SHAKY_HAND_POLICY_VERSION,
    SHAKY_HAND_TIMING_STRENGTHS, SemanticControl, ShakyHandConfig, ShakyHandConfigError,
    ShakyHandError, ShakyHandInterpretation, ShakyHandReport, ShakyHandStudy,
    ShakyHandStudyIdentity, evaluate_recorded_shaky_hand_study, evaluate_shaky_hand,
    record_shaky_hand_study,
};
pub use solver::{
    ActionMacro, BatchTargetResult, BatchTargetSolveError, BatchTargetSolveOutcome,
    DIRECT_PROBE_AUDIT_VERSION, DirectProbeAudit, DirectProbeAuditStatus, DirectProbeBudgetLimit,
    DirectProbePolicy, DirectProbeProvenance, DirectProbeWitness, InconclusiveReason,
    ReachedTarget, SOLVER_POLICY_VERSION, SearchStats, SearchTarget, Solution, SolveOutcome,
    SolverConfig, SolverConfigError, TargetSolution, TargetSolveError, TargetSolveOutcome,
    audit_direct_controller_probes, solve, solve_target, solve_targets,
};
pub use waypoint::{
    GroundedStandingRegion, GroundedSupportSolution, GroundedSupportSolveError,
    GroundedSupportSolveOutcome, GroundedSupportTarget, GroundedSupportTargetError,
    WAYPOINT_DIAGNOSTIC_POLICY_VERSION, solve_grounded_support,
};
