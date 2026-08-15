# Compositional WallJump physical-v3 experiment — rejected 2026-08-15

Status: **rejected and reverted; no corpus integration**.

This experiment tested the smallest proposed correction to the physical-v2 WallJump gate: use the
existing paired-wall shaft and increase every Wall-only rise from six rows to eight. The temporary
mapping used physical generation version 3 and gate-contract version 2. It did not change the
coordinate-free graph rewrite, solver budgets, promotion vetoes, baseline route-cut mapping, or
corpus candidate/config/evaluation/artifact code.

## Construction result

The first implementation exposed an unsound ability-rhythm memo key. Gate isolation depends on
the previously visited rows and completed reserved bands, but physical v2 memoized only
`(edge_index, current_row)`. A diagnostic-only coupled search removed that conflation, pruned band
violations incrementally, and backtracked from a valid rhythm into the support constraint search.

Over the fixed attempt-zero block of seeds 0 through 4 crossed with Gentle, Standard, and
Technical intents, the corrected experimental search constructed 12 of 15 WallJump keys with 12
distinct geometry fingerprints:

| Intent | Constructed | Typed refusals |
| --- | ---: | --- |
| Gentle | 4 / 5 | seed 2: boundary-door arrival overlap |
| Standard | 5 / 5 | none |
| Technical | 3 / 5 | seed 0: raster contract mismatch; seed 2: boundary-door arrival overlap |

Every constructed candidate passed the diagnostic graph-bridge, reverse-graph, whole-band support
isolation, cut-shelf, socket inventory, boundary-arrival, exact solid/empty reservation, and exact
regeneration checks. The three misses remained explicit typed failures.

## Authoritative veto

The authoritative run was stopped after the first eight constructed exact keys because four had a
replay-positive Dash-only advertised-pair matrix bypass: Gentle seeds 0, 1, and 3, and Standard
seed 2. Their finite direct-controller audits were still `CompleteNoPositive`, reproducing the
important distinction between a bounded direct vocabulary and the ordinary solver matrix. No
solver limit or promotion rule was weakened.

The first veto, Gentle seed 0, localized the physical defect precisely:

- gate rise: row 12 to row 4;
- clear shaft bounds: x 180–220, y 28–120 pixels;
- intended Wall-only matrix: 12 / 12 directed doors and 4 / 4 pickups positive, with five accepted
  wall jumps inside the gate;
- intended Both matrix: 12 / 12 directed doors and 4 / 4 pickups positive, with eight accepted
  wall jumps and two dashes;
- Baseline advertised forward: bounded with no positive; reverse was replay-positive;
- Dash-only advertised forward: replay-positive, with eight accepted dashes despite the direct
  audit being `CompleteNoPositive` after 334 expanded probes and 97,181 simulated ticks.

The Dash-only replay entered the Wall shaft and dashed upward at tick 537. Its vertical coverage
reached a player-bottom y coordinate of 49 pixels. The paired columns began at row 5, whose top is
y=50: that top face was a landable intermediate surface. Landing on the column cap refreshed the
dash, and a second upward dash at tick 557 cleared the upper lip. Making the shaft taller therefore
did not remove the bypass; it merely moved the dash-refresh cap.

## Disposition

Physical generation version 3, gate-contract version 2, the eight-row shared mapping, and the
temporary search changes were all reverted. The frozen physical-v2 public identity remains
generation version 2 and gate-contract version 1. This rejected experiment must not be used as an
ability promotion source.

A successor needs a genuinely cap-free construction, such as a ceiling-anchored chimney with a
side exit whose only lower wall top is flush with the declared upper endpoint. It must pass the
same exact intended matrices, reverse replay, missing-Wall matrix vetoes, and complete finite
direct audits before any corpus integration.
