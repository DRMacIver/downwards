# Corpus playtest integration — 2026-08-15

The game-facing corpus path is a trust-preserving adapter, not a new room format.

`corpus_v3_offline_selection export-playtest` independently runs the full final-selection
verifier: it rebinds the exact cache and shard identities, reruns selection, and fully recomputes
every selected room before accepting exact equality. It then replay-rehydrates the exact shard
pool through the source-bound cache seam. The exporter chooses the first 64 room IDs in the
selector's recorded quality-diversity order and deterministically adds the earliest selected mate
from a different room required to close every exposed boundary socket.

For each retained physical room, the manifest records:

- its room-v3 ID and complete, versioned native regeneration key;
- its unchanged boundary socket inventory;
- its construction loadout;
- the authored source-to-sink route when that exact matrix cell is positive, otherwise the first
  deterministic positive cell with an explicit fallback label; and
- that positive's source, target, provenance witness fingerprint, initial digest, lossless semantic
  run-length-encoded actions, and a metadata-independent runtime checksum over the route, loadout,
  actions, and recorded state/event digests.

No tiles, doors, route plans, or native derivation evidence are flattened or copied into the
manifest. At runtime `downwards-catalogue` validates the manifest fingerprint and versions,
dispatches the exact native generator with no retry or fallback, compares regenerated sockets and
the construction-loadout entry digest, and replays the representative actions through the
authoritative simulation to the named target. It also recomputes the runtime replay checksum, so a
physics or trace change cannot be accepted merely because the same actions still happen to reach
the door. The selected rooms remain a socket-compatible
inventory; this browser does not claim to assemble or validate a dungeon graph.

The historical v6 catalogue remains the default `cargo run` experience. Corpus playtesting is
explicit:

```sh
cargo run -- --corpus content/corpora/v1/playtest.manifest
```

The browser partitions rows by their exact construction loadout instead of cloning a heterogeneous
room into four old-style tier catalogues. Empty tiers show only the development room. Every
exported physical room still passed the complete-kit all-target gate, but the stored `V` witness
is exposed only under the loadout it actually certifies rather than being relabelled as evidence
for another tier.

The only provisional path is deliberately double-gated: export with
`export-playtest-dev-provisional`, then launch with `--allow-provisional-corpus`. Normal export and
normal client loading both fail closed on provisional state.

The production export completed with 68 entries: the requested 64 plus four socket mates from
different rooms. It contains 32 PartitionRoute, 15 CompositionalRouteCut, and 21 certified
Dash-ability candidates, split into 47 Baseline and 21 Dash construction-loadout entries. All 68
use an authored source-to-sink witness and none use the fallback route policy. The canonical file
is 67,909 bytes with SHA-256
`a32163af520fc77d512f875bc6d5463ddbe340b84d9cb8cd42f7af44038bace5`.
