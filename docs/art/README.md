# Player sprite art

`player-sprite-sheet-preview-v1.png` is the generated source sheet for the first animated player
character. It was generated with OpenAI's built-in image generation tool on 2026-08-15.

The production asset is `crates/downwards-client/assets/player-sprites-v1.png`: a deterministic
64x48 reduction of the source into twelve 16x16 cells. The client renders it with nearest-neighbour
sampling, mirrors right-facing poses for left-facing movement, and derives animation state from
simulation state without changing physics or replay data.

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
  -vf scale=64:48:flags=area \
  crates/downwards-client/assets/player-sprites-v1.png
```
