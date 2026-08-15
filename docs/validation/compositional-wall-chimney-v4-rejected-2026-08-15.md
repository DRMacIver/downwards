# Compositional WallJump ceiling-chimney-v4 experiment — rejected 2026-08-15

Status: **rejected, isolated, and unpromoted; no corpus integration or gameplay claim**.

This experiment replaced the rejected capped paired-wall shaft with a genuinely separate physical
mapping. Its ten-row chimney has one wall connected continuously to the room ceiling. The other
wall has a two-tile-high side aperture at the upper endpoint and a lower segment whose only
upward-facing cap is exactly flush with that endpoint. The mapping has its own versioned key,
contract, mapping, attempt, candidate, and typed-failure surface. It never falls back to physical
v2 and is not referenced by corpus candidate, configuration, evaluation, artifact, selection, or
promotion code.

The experiment retained the coordinate-free graph rewrite and its bridge/unavoidability
certificates. Its coupled finite search carries gate-band history explicitly, prunes whole-band
support isolation incrementally, and validates both wall segments, the full shaft and aperture,
cut shelves, incident route corridors, boundary sockets and arrivals, pickups, endpoint headroom,
and final raster bytes. No gameplay-solver budget, construction budget, graph claim, or promotion
veto was relaxed in response to the result.

## Frozen construction block

The construction gate was the exact attempt-zero cross product of source seeds 0 through 4 and
Gentle, Standard, and Technical intents, all with the WallJump graph profile. The required gate was
at the following zero-based spine edge in each derived mission:

| Intent | Seed | Spine nodes | Gate edge | Outcome |
| --- | ---: | ---: | ---: | --- |
| Gentle | 0 | 11 | 8 | constructed |
| Gentle | 1 | 11 | 3 | fork search exhausted after 100,000 candidates |
| Gentle | 2 | 11 | 8 | `port-0` arrival `(300,158)` blocked during final door validation |
| Gentle | 3 | 13 | 8 | no rhythm candidate after 4,944 finite row states |
| Gentle | 4 | 11 | 4 | constructed |
| Standard | 0 | 13 | 2 | gate 0 boundary-arrival conflict |
| Standard | 1 | 12 | 8 | `port-0` arrival `(300,158)` blocked during final door validation |
| Standard | 2 | 13 | 7 | constructed |
| Standard | 3 | 14 | 7 | no rhythm candidate after 764 finite row states |
| Standard | 4 | 13 | 9 | constructed |
| Technical | 0 | 15 | 5 | no rhythm candidate after 136 finite row states |
| Technical | 1 | 13 | 5 | no rhythm candidate after 112 finite row states |
| Technical | 2 | 14 | 5 | `port-0` arrival `(300,158)` blocked during final door validation |
| Technical | 3 | 13 | 8 | constructed |
| Technical | 4 | 14 | 10 | spine search exhausted after 1 candidate |

The result was **5/15 constructions**, split Gentle 2/5, Standard 2/5, and Technical 1/5. All five
successful candidates had distinct geometry fingerprints. Exact regeneration reproduced both the
five candidates and the ten typed refusals. The successful candidates also passed focused checks
for the ceiling-connected continuous wall, exact two-row aperture, endpoint-flush lower cap, empty
shaft raster, ten-row rise, and whole-band absence of non-endpoint route supports.

Four refusals had no complete row sequence in the finite v4 rhythm domain at all. Together with
the fork, arrival, and spine deficits, this makes the construction yield materially below the
predeclared 12/15 gate. Increasing search budgets cannot repair those zero-rhythm identities, and
changing the rise, isolation band, graph edge, arrivals, or baseline transition envelope would
change the experiment rather than validate it.

## Gameplay disposition

The authoritative gameplay phase was not run. Consequently there are zero audited Wall-only or
Both-loadout door/pickup matrices, zero accepted wall-jump event counts, zero reverse Baseline
replays, and zero missing-Wall Baseline/Dash-only veto matrices for v4. Construction success is not
gameplay evidence, and v4 makes no WallJump requirement or promotion claim.

The earlier physical-v3 experiment remains independently rejected because its capped eight-row
shaft admitted replay-positive Dash-only advertised-pair bypasses. The first localized bypass
landed on the wall-column top, refreshed dash, and dashed over the lip. V4 removed that specific
cap mechanism, but its construction deficit prevented the required gameplay audit; it does not
supersede or rehabilitate v3.

## Disposition

The public physical-v2 identity remains generation version 2 with gate-contract version 1, and
its key/output mapping is unchanged. V4 remains a named unpromoted research surface with focused
deterministic construction/refusal and raster-invariant tests only. It must not enter a broad
corpus run or any candidate-selection path. The final source policy is scaled back to Baseline and
Dash rather than treating this non-robust Wall experiment as usable generation coverage.
