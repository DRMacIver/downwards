#!/usr/bin/env python3
"""Pack the generated 4x3 environment source into the 24px runtime atlas."""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image


SHEET_COLUMNS = 4
SHEET_ROWS = 3
RUNTIME_CELL = 24


def alpha_bounds(image: Image.Image) -> tuple[int, int, int, int] | None:
    return image.getchannel("A").getbbox()


def alpha_crop(image: Image.Image) -> Image.Image:
    bounds = alpha_bounds(image)
    if bounds is None:
        raise ValueError("generated cell contains no visible pixels")
    return image.crop(bounds)


def fit_cell(
    image: Image.Image,
    *,
    maximum_width: int = RUNTIME_CELL,
    maximum_height: int = RUNTIME_CELL,
    vertical_alignment: str = "center",
) -> Image.Image:
    image = alpha_crop(image)
    scale = min(maximum_width / image.width, maximum_height / image.height)
    size = (
        max(1, round(image.width * scale)),
        max(1, round(image.height * scale)),
    )
    image = image.resize(size, Image.Resampling.BOX)
    cell = Image.new("RGBA", (RUNTIME_CELL, RUNTIME_CELL), (0, 0, 0, 0))
    x = (RUNTIME_CELL - image.width) // 2
    if vertical_alignment == "top":
        y = 0
    elif vertical_alignment == "bottom":
        y = RUNTIME_CELL - image.height
    else:
        y = (RUNTIME_CELL - image.height) // 2
    cell.alpha_composite(image, (x, y))
    return cell


def build(source_path: Path, output_path: Path) -> None:
    source = Image.open(source_path).convert("RGBA")
    if source.width % SHEET_COLUMNS or source.height % SHEET_ROWS:
        raise ValueError(
            f"source dimensions {source.size} are not an exact {SHEET_COLUMNS}x{SHEET_ROWS} grid"
        )
    source_cell_width = source.width // SHEET_COLUMNS
    source_cell_height = source.height // SHEET_ROWS
    source_cells = []
    for row in range(SHEET_ROWS):
        for column in range(SHEET_COLUMNS):
            source_cells.append(
                source.crop(
                    (
                        column * source_cell_width,
                        row * source_cell_height,
                        (column + 1) * source_cell_width,
                        (row + 1) * source_cell_height,
                    )
                )
            )

    packed = [
        # Seamless stone textures deliberately occupy the full cell. Exposed edges are
        # neighbor-aware runtime decoration rather than borders baked into every tile.
        alpha_crop(source_cells[0]).resize((24, 24), Image.Resampling.BOX),
        alpha_crop(source_cells[1]).resize((24, 24), Image.Resampling.BOX),
        fit_cell(source_cells[2], maximum_width=24, maximum_height=7, vertical_alignment="top"),
        # Spike sprites occupy the whole collision cell: the base spans the rear face and the
        # points reach the opposite face. This keeps their visible envelope honest.
        alpha_crop(source_cells[3]).resize((24, 24), Image.Resampling.BOX),
        alpha_crop(source_cells[4]).resize((24, 24), Image.Resampling.BOX),
        alpha_crop(source_cells[5]).resize((24, 24), Image.Resampling.BOX),
        alpha_crop(source_cells[6]).resize((24, 24), Image.Resampling.BOX),
        fit_cell(source_cells[7]),
        fit_cell(source_cells[8], maximum_width=17, maximum_height=24, vertical_alignment="bottom"),
        fit_cell(source_cells[9], maximum_width=16, maximum_height=21),
        fit_cell(source_cells[10]),
        fit_cell(source_cells[11]),
    ]

    atlas = Image.new(
        "RGBA",
        (SHEET_COLUMNS * RUNTIME_CELL, SHEET_ROWS * RUNTIME_CELL),
        (0, 0, 0, 0),
    )
    for index, cell in enumerate(packed):
        atlas.alpha_composite(
            cell,
            ((index % SHEET_COLUMNS) * RUNTIME_CELL, (index // SHEET_COLUMNS) * RUNTIME_CELL),
        )
    for index in (0, 1, 3, 4, 5, 6):
        if alpha_bounds(packed[index]) != (0, 0, 24, 24):
            raise ValueError(f"runtime cell {index} does not fill its collision bounds")
    one_way_bounds = alpha_bounds(packed[2])
    if one_way_bounds is None or one_way_bounds[1] != 0:
        raise ValueError("one-way art is not flush with its collision surface")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    atlas.save(output_path)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    arguments = parser.parse_args()
    build(arguments.source, arguments.output)


if __name__ == "__main__":
    main()
