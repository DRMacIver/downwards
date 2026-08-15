# downwards-gen

`downwards-gen` creates deterministic candidate rooms from a seed and an
intended traversal-ability loadout. Every candidate is constructed through
`downwards_core::Room::new`, so the core room invariants remain authoritative.

Generation only establishes structural properties such as fixed dimensions,
safe spawn geometry, boundaries, exits, platforms, hazards, optional one-way
routes, pickups, and timed hazards. It deliberately does **not** claim that a
candidate is solvable or that its requested ability tier matches a particular
difficulty. The `downwards-ai` solver, running the authoritative Rust simulation
with `GeneratedMetadata::intended_abilities`, is the acceptance authority for
both solvability and measured difficulty.

Optional-cache geometry is held to a separate structural legibility budget.
Baseline branches use staged supports with at most 20 pixels of vertical rise,
comfortably below the roughly 31-pixel full-jump apex. Dash branches leave at
least 20 pixels beneath the conservative combined range of a jump into an
up-dash, while wall-jump branches retain continuous opposing walls. These are
regression-resistant geometry margins, not proofs that a pickup is reachable:
pickup objectives must still be exercised by the authoritative AI before they
can be certified.

The random stream is an in-crate SplitMix64 implementation. Its output and the
mapping from seeds to rooms are treated as stable content inputs; changing
either should be an intentional `GENERATION_VERSION` change. Generated metadata
records that version so saved seeds and validation reports remain attributable.

The v6 compositional facade also exposes cumulative evaluation feature sets via
`StagedCompositionalKey` and `generate_staged_compositional`. `TerrainOnly`
retains solids, one-way platforms, doors, and pickups; `StaticHazards` also
retains hazard tiles; and `TimedHazards` is the complete current v6 content.
These variants are deterministic post-construction views of one unchanged v6
source key, so their route graph and non-hazard geometry stay comparable. Their
typed keys and room IDs include the feature set, while the legacy
`generate_compositional` identity and output remain unchanged.
