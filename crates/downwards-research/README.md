# Downwards research harness

This standalone crate compares experimental non-template room generators under
the same deterministic core simulation, solver, certificate layer, difficulty
diagnostics, and structural/behavioral diversity metrics.

It is deliberately not part of the production workspace. Run a focused sweep:

```sh
cargo run --release --manifest-path crates/downwards-research/Cargo.toml -- \
  sweep 0 100 rhythm baseline standard
```

Use `all` for any of strategy, tier, or intent. Large all-combination sweeps
are offline jobs: wall-jump and pickup searches are substantially more
expensive than baseline exit checks.

For every constructed candidate the sweep explicitly certifies every ordered
pair of distinct boundary doors, then independently certifies every pickup
from every door arrival. A candidate is accepted only when all `n * (n - 1)`
door routes and all `n * pickups` optional routes have witnesses. Partial route
successes and stable rejection classes are retained, rather than discarding a
room at its first failed direction.

The report includes port-count and all-pairs rates, difficulty summaries by
boundary direction, static geometry, collision topology, traversal paths,
semantic action behavior, provisional difficulty, and ability use. Behavioral
distance uses the first 256 successful directed witnesses in deterministic
seed/door order. No scalar “interestingness” score is reported.

## Offline catalogue curation

`curate` overgenerates exact production-v6 `CompositionalKey`s across every
strategy/intent stratum for exactly one ability tier and writes a versioned
manifest to standard output:

```sh
cargo run --release --manifest-path crates/downwards-research/Cargo.toml -- \
  curate 0 16 3 baseline > baseline-prototype-v2.manifest
```

The numeric arguments are the first seed, seeds per strategy/intent stratum,
and required number of distinct rooms in each provisional route band. The
three band quotas are hard: a deficit exits unsuccessfully with diagnostics on
standard error, and never silently reduces a quota or changes a fairness
floor.

Static previews are deduplicated only after one provenance record has passed
the combined all-door/all-pickup certificate batch, so an uncertified timer
schedule cannot suppress a certifiable duplicate. Selection treats difficulty
as directed route data. Each catalogue entry is assigned to one catalogue band
through a named source/target representative, while its complete ordered route
matrix remains in the manifest. The representative route and a pickup witness
from the same source door carry compact deterministic action RLE streams. Wall
and dash are catalogue-level coverage requirements: at least one selected
representative must demonstrate each ability required by the tier, and the
combined tier prefers (but does not require) both on a single route.

The selector treats socket closure and catalogue ability coverage as a
constraint problem. It branches on the unmatched socket or ability with the
fewest compatible remaining room/band assignments, then orders that domain by
socket feasibility, strategy and intent coverage, vertical ports, QD strata,
route robustness and clearance, solver headroom, and deterministic
farthest-first static, collision, traversal, and semantic-action distance.
Every selected boundary socket must have an aligned opposite-side mate in the
selected catalogue. Solver effort is recorded per route and shared source
search and used only as operational quality/headroom; it is not part of the
curator's difficulty-band score.

The checked-in `catalogues/*-prototype-v2.manifest` files are the quota-three,
seeds `0..16` calibration catalogues for all four ability tiers. Their exact
production copies live in `content/catalogues/v6/*.manifest`. Regenerating any
file with the request encoded in its header produces the same bytes and
terminal manifest fingerprint; policy and generator version fields make stale
artefacts visible.
