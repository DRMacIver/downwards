//! Deterministic measurements for generator experiments.
//!
//! Exact visual identity, collision structure, spatial traversal, and input
//! behavior are deliberately separate. In particular, a large tile Hamming
//! distance is not evidence that two rooms demand different play.

#![forbid(unsafe_code)]

mod ablation;
mod difficulty_vector;
mod distance;
mod features;
mod geometry;
mod landing_precision;
mod route_diversity;
mod simulation_identity;
mod witness;

pub use ablation::{
    ROOM_ABLATION_VERSION, RoomAblation, RoomAblationError, RoomAblationKind,
    room_ablation_variants,
};
pub use difficulty_vector::{
    AcceptedMovement, ComparisonSide, DifficultyComparisonTolerances, DifficultyCoordinate,
    HazardClearanceEvidence, HazardPressure, InsufficientEvidenceIssue, InvalidNoisePointReason,
    MissingEvidenceReason, OperationalSolverCost,
    PERFECT_CONTROL_ROUTE_DIFFICULTY_COMPARISON_VERSION, ROUTE_DIFFICULTY_DISCLAIMER,
    ROUTE_DIFFICULTY_NOISE_POINTS, ROUTE_DIFFICULTY_VECTOR_VERSION, RouteDifficultyComparison,
    RouteDifficultyVector, RouteDifficultyVectorError, ShakyHandEvidence, TimingNoiseKey,
    TimingNoisePoint, TimingOutcomeProbabilities, TimingPointEvidence, TimingRobustnessVector,
    ToleranceField, TraversalDemand, compare_perfect_control_route_difficulty,
    compare_route_difficulty, route_difficulty_vector,
};
pub use distance::{
    CollisionTopologyDistance, SemanticActionDistance, StaticVisualDistance, TraversalDistance,
    collision_topology_distance, semantic_action_distance, static_visual_distance,
    traversal_distance,
};
pub use features::{
    OBSERVATION_FEATURE_COUNT, OBSERVATION_FEATURE_VERSION, OBSERVATION_FEATURES,
    ObservationFeature, ObservationFeatureVector, observation_feature_vector,
};
pub use geometry::{
    BoundaryMask, COLLISION_TOPOLOGY_DESCRIPTOR_VERSION, CollisionCell, CollisionFace,
    CollisionFaceKind, CollisionTopologyDescriptor, DescriptorDoor, DescriptorPoint,
    DescriptorRect, PassableRegion, STATIC_VISUAL_DESCRIPTOR_VERSION, StaticVisualDescriptor,
    VisualTile,
};
pub use landing_precision::{
    LANDING_PRECISION_DISCLAIMER, LANDING_PRECISION_VERSION, LandingPrecisionError,
    LandingPrecisionReport, LandingSample, LandingSupportKind, analyze_replay_landings,
};
pub use route_diversity::{
    PairwiseDistanceSummary, ROUTE_DIVERSITY_VERSION, RouteDiversityReport, route_diversity,
};
pub use simulation_identity::{
    SIMULATION_GEOMETRY_DESCRIPTOR_VERSION, SimulationDoorGeometry, SimulationGeometryDescriptor,
    SimulationTimedHazardGeometry,
};
pub use witness::{
    ActionSpan, SemanticAction, SemanticActionTrace, SemanticEvent, SemanticEventAt,
    SuccessfulWitnessObservation, TraversalCell, TraversalGrid, TraversalGridError, TraversalSpan,
    TraversalTrace, WitnessObservationError, observe_solution, observe_successful_replay,
};
