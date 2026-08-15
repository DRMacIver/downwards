# Game pixel art

## Player

`player-sprite-sheet-preview-v1.png` is the generated source sheet for the first animated player
character. It was generated with OpenAI's built-in image generation tool on 2026-08-15.

The current production asset is `crates/downwards-client/assets/player-sprites-v2.png`: a
deterministic 96x72 reduction of the source into twelve 24x24 cells. The original 16x16-cell v1
sheet is retained for visual comparison. The client renders v2 with nearest-neighbour sampling,
mirrors right-facing poses for left-facing movement, and derives animation state from simulation
state without changing physics or replay data.

The source prompt requested one consistent cream-white hooded climber with a navy face, readable
eye, cyan scarf, boots, and gloves; a limited crisp pixel palette; and a regular 4x3 transparent
sheet containing idle, four run poses, rise, fall, wall cling, wall jump, and two skid poses. It
explicitly prohibited text, props, shadows, motion trails, antialiasing, and changes of character
scale or costume between cells.

<details>
<summary>Exact generation prompt</summary>

> Use case: stylized-concept
>
> Asset type: production sprite sheet for a tiny fast-paced 2D pixel platformer
>
> Primary request: Create a clean 4-column by 3-row pixel-art sprite sheet of one consistent tiny
> climber character, with exactly 12 isolated full-body animation poses.
>
> Subject: A compact cream-white hooded climber with a very dark navy face opening, one bright
> readable eye looking in the facing direction, cyan scarf/belt accent, cyan boots and mitten-like
> hands. Strong simple silhouette, charming but not cute-baby proportions. Every pose faces RIGHT.
> Row 1: idle A, idle B, run contact, run passing. Row 2: run contact opposite, run passing
> opposite, rising jump, falling. Row 3: wall cling with hands toward the right wall, explosive
> wall-jump departure toward the left, braking skid, deep braking skid.
>
> Style/medium: authentic limited-palette pixel art, crisp hard square pixels, approximately 16x24
> source-pixel detail per character, no antialiasing, no subpixel blur, no painterly texture,
> consistent anatomy and costume across every cell.
>
> Composition/framing: exact regular 4x3 grid with equal generous padding; one centered sprite per
> invisible cell; no pose overlaps; all feet share consistent baseline within their row; no grid
> lines and no labels.
>
> Color palette: cream, near-black navy, bright cyan, muted blue-grey; do not use magenta in the
> character.
>
> Scene/backdrop: perfectly flat solid #ff00ff chroma-key background for background removal. The
> background must be one uniform color with no shadows, gradients, texture, floor plane,
> reflections, lighting variation, or grid.
>
> Constraints: no text, no numbers, no watermark, no weapons, no environmental props, no cast
> shadows, no glow, no motion trails. Preserve identical character scale and outfit in all 12
> poses. Make the eye/facing direction readable after substantial downscaling.

</details>

The checked-in production sheet was reduced with:

```console
ffmpeg -i docs/art/player-sprite-sheet-preview-v1.png \
  -vf scale=96:72:flags=area \
  crates/downwards-client/assets/player-sprites-v2.png
```

## Environment

`environment-tiles-preview-v2.png` is the current built-in image-generation result.
`environment-tiles-source-v2.png` is the transparent chroma-keyed source, and
`crates/downwards-client/assets/environment-tiles-v2.png` is the deterministic 96x72 runtime atlas
of twelve 24x24 cells. The v1 files remain checked in as historical source material.

The runtime atlas covers two seamless stone fill textures, a top-aligned one-way bridge,
up/down/left/right spikes, an exit portal, a deliberately minimal unframed door, a pickup, and
active/inactive timed hazards. Runtime code draws only the exposed outer contour of a connected
solid region, so neighbouring blocks visually merge rather than appearing as individually framed
boxes. The bridge art is flush with the exact top collision surface. Every spike sprite fills its
24x24 collision cell so the visible dangerous envelope is not narrower than the physics.

Spike direction is authored room data, not inferred presentation. The ASCII authoring characters
are `^`, `v`, `<`, and `>`, mapping to `HazardUp`, `HazardDown`, `HazardLeft`, and `HazardRight`.
The pointed face is lethal; the back and perpendicular faces are solid, nonlethal obstacles.
Rendering, collision, room identity, descriptors, and artifact records all consume that same
explicit direction.

<details>
<summary>Exact v2 environment edit prompt</summary>

> Edit the supplied 4-column by 3-row pixel-art environment sheet while preserving the exact grid,
> scale, palette, lighting, crisp pixel treatment, and every cell except the requested changes.
> Replace the first two cells with dark blue-grey seamless stone fill textures: no baked frame,
> bevel, border, bright top rim, or edge treatment, because adjacent runtime tiles must merge.
> Retain two subtly different chipped/cracked variants. Replace row 3 column 1 with a very simple
> narrow cyan-blue doorway/arch with a dark interior, no surrounding stone box, no pedestal, and no
> oversized frame. Preserve the one-way platform, all directional spikes, exit, pickup, and both
> timed-hazard cells. Keep the background perfectly flat solid #ff00ff, with no grid, labels, text,
> character, cast shadows, or glow crossing cell boundaries.

</details>

The checked-in transparent source and production sheet were built with Pillow isolated through
`uv`. The atlas packer validates that solids and spikes fill their collision cells and that the
bridge begins at the top collision surface:

```console
uv run --with pillow python \
  "$CODEX_HOME/skills/.system/imagegen/scripts/remove_chroma_key.py" \
  --input docs/art/environment-tiles-preview-v2.png \
  --out docs/art/environment-tiles-source-v2.png \
  --auto-key border --soft-matte --transparent-threshold 12 \
  --opaque-threshold 220 --despill --force

uv run --with pillow python docs/art/build_environment_atlas.py \
  docs/art/environment-tiles-source-v2.png \
  crates/downwards-client/assets/environment-tiles-v2.png
```
