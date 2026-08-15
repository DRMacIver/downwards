# Downwards lab

`downwards-lab` contains deterministic measurements used while experimenting
with room generators and quality-diversity selection. It is deliberately not a
member of the production workspace and has no generator-specific policy.

The measurements keep several questions separate:

- `StaticVisualDescriptor` answers whether two static previews are exactly the
  same, including boundary-door placement. Timed-hazard schedules and content
  IDs are intentionally excluded.
- `CollisionTopologyDescriptor` captures collision classes, exposed collision
  faces, and traversable-region structure. Hazards do not masquerade as new
  collision geometry.
- `SimulationGeometryDescriptor` adds door arrivals and complete timed-hazard
  schedules to a canonical physical identity. Its stable digest is an index;
  descriptor equality remains the collision-free deduplication truth.
- `SuccessfulWitnessObservation` records where a verified replay travelled and
  what semantic inputs and authoritative traversal events it used.
- normalized distance reports expose their components instead of hiding every
  notion of novelty in one tile-Hamming score.
- `RouteDiversityReport` aggregates a set of successful routes, counts
  timing-insensitive spatial/controller/play-style classes, and reports
  pairwise plus nearest-neighbour behavior distances. Retiming the same
  controller does not manufacture a new play style.
- `RouteDifficultyVector` keeps route duration and geometry, controller
  demand, accepted movement events, hazard clearance, and family-specific
  shaky-hand outcome curves as transparent coordinates. Its conservative
  Pareto comparison reports clear dominance only when complete comparable
  evidence is no easier everywhere and strictly harder somewhere. Route
  trade-offs remain incomparable and missing studies remain insufficient.
  Solver search cost is kept separately for pipeline operations and never
  participates in player-difficulty comparisons. There are no scalar scores
  or named difficulty bands in this API.
- `room_ablation_variants` constructs deterministic one-component or
  one-hazard removal experiments. A variant is only an experiment input: a
  surviving replay is positive redundancy evidence for that controller, while
  the absence of a traversal is never treated as proof that terrain is useless.
- `ObservationFeatureVector` is a versioned, bounded behavior characterization
  for later MAP-Elites or other quality-diversity experiments. Its coordinates
  are observations, not claims about human difficulty.

Run its focused tests independently:

```sh
cargo test --manifest-path crates/downwards-lab/Cargo.toml
```
