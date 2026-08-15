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

`environment-tiles-preview-v1.png` is the built-in image-generation result, using the player sheet
as a style-only reference. `environment-tiles-source-v1.png` is its transparent source produced by
the image-generation skill's chroma-key helper, run through `uv` with an isolated Pillow
dependency. `crates/downwards-client/assets/environment-tiles-v1.png` is the 96x72 runtime atlas of
twelve 24x24 cells.

The runtime atlas covers two stone variants, one-way platforms, upward/downward/horizontal spikes,
an exit portal, a door, a pickup, and active/inactive timed hazards. Horizontal spikes are mirrored
for exact left/right symmetry. Spike direction is inferred from the room's hazard strip, solid
anchors, and nearest route corridor; it remains presentation-only and does not alter collision.

<details>
<summary>Exact environment generation prompt</summary>

> Use case: stylized-concept
>
> Asset type: production environment tileset for the same tiny fast-paced 2D pixel platformer
>
> Input images: Image 1 is a STYLE REFERENCE ONLY for palette, pixel treatment, outlining, and
> finish. Do not include the character or copy any character pose.
>
> Primary request: Create a clean 4-column by 3-row pixel-art environment sheet with exactly 12
> isolated landscape/object cells, visually matching Image 1.
>
> Cell contents in exact reading order: Row 1: dark blue-grey stone block A with a bright chipped
> top edge; dark blue-grey stone block B with a different subtle crack pattern; a slim one-way
> platform with metal/ice-blue upper lip and dark underside; a pair of sharp triangular spikes
> pointing UP. Row 2: the same spikes pointing DOWN; spikes pointing LEFT; spikes pointing RIGHT;
> a small glowing cyan-green exit portal inset in dark stone. Row 3: a small blue doorway with cyan
> rim and dark interior; a faceted gold coin/crystal pickup; a compact red timed-hazard block in
> ACTIVE state; the same timed-hazard block in INACTIVE dark state.
>
> Style/medium: authentic limited-palette pixel art, crisp hard square pixels, approximately 16x16
> source-pixel detail per cell, no antialiasing, no subpixel blur, no painterly texture. Use the
> reference's near-black navy, muted blue-grey, cream highlight, and bright cyan accents; reserve
> warm red for hazards and gold for the pickup.
>
> Composition/framing: exact regular 4x3 grid with equal padding; one centered asset per invisible
> square cell; no overlaps, no grid lines, no labels. Each stone block and hazard block fills most
> of its cell edge-to-edge so it can tile. Directional spike silhouettes must be unmistakable.
>
> Scene/backdrop: perfectly flat solid #ff00ff chroma-key background. One uniform color, no
> shadows, gradients, texture, floor plane, reflections, lighting variation, or grid. Do not use
> magenta in any asset.
>
> Constraints: no character, no text, no numbers, no watermark, no environmental scene, no cast
> shadows, no glow extending across cell boundaries. Keep scale, palette, lighting direction,
> outline weight, and material language consistent across all 12 cells.

</details>

The checked-in transparent source and production sheet can be rebuilt with:

```console
uv run --with pillow python \
  "$CODEX_HOME/skills/.system/imagegen/scripts/remove_chroma_key.py" \
  --input docs/art/environment-tiles-preview-v1.png \
  --out docs/art/environment-tiles-source-v1.png \
  --auto-key border --soft-matte --transparent-threshold 12 \
  --opaque-threshold 220 --despill --force

ffmpeg -i docs/art/environment-tiles-source-v1.png \
  -vf scale=96:72:flags=area \
  crates/downwards-client/assets/environment-tiles-v1.png
```
