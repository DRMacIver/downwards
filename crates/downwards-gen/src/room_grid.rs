//! ASCII room-grid rendering and parsing shared by authored room content.

use downwards_core::Tile;

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;

/// One printable character per tile in the ASCII room-grid format.
const ROOM_GRID_LEGEND: [(char, Tile); 7] = [
    ('.', Tile::Empty),
    ('#', Tile::Solid),
    ('-', Tile::OneWay),
    ('^', Tile::HazardUp),
    ('v', Tile::HazardDown),
    ('<', Tile::HazardLeft),
    ('>', Tile::HazardRight),
];

/// Render a full room tile grid as newline-separated ASCII rows.
#[must_use]
pub fn render_room_grid(tiles: &[Tile]) -> String {
    assert_eq!(tiles.len(), usize::from(WIDTH) * usize::from(HEIGHT));
    let mut rendered = String::with_capacity(tiles.len() + usize::from(HEIGHT));
    for row in 0..usize::from(HEIGHT) {
        for column in 0..usize::from(WIDTH) {
            let tile = tiles[row * usize::from(WIDTH) + column];
            let (glyph, _) = ROOM_GRID_LEGEND
                .iter()
                .find(|(_, candidate)| *candidate == tile)
                .expect("every tile has a grid glyph");
            rendered.push(*glyph);
        }
        rendered.push('\n');
    }
    rendered
}

/// Parse an ASCII room grid rendered by [`render_room_grid`].
///
/// # Panics
///
/// Panics on wrong dimensions or an unknown glyph; grids are compiled-in
/// authored content, so a malformed grid is a build defect rather than a
/// runtime input error.
#[must_use]
pub fn parse_room_grid(source: &str) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity(usize::from(WIDTH) * usize::from(HEIGHT));
    let mut rows = 0;
    for (row_index, line) in source.lines().enumerate() {
        rows += 1;
        assert_eq!(
            line.chars().count(),
            usize::from(WIDTH),
            "room grid row {row_index} must have {WIDTH} columns"
        );
        for glyph in line.chars() {
            let (_, tile) = ROOM_GRID_LEGEND
                .iter()
                .find(|(candidate, _)| *candidate == glyph)
                .unwrap_or_else(|| panic!("unknown room grid glyph {glyph:?}"));
            tiles.push(*tile);
        }
    }
    assert_eq!(
        rows,
        usize::from(HEIGHT),
        "room grid must have {HEIGHT} rows"
    );
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_grid_round_trips_through_render_and_parse() {
        let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
        tiles[0] = Tile::Solid;
        tiles[33] = Tile::OneWay;
        tiles[64] = Tile::HazardUp;
        tiles[65] = Tile::HazardDown;
        tiles[66] = Tile::HazardLeft;
        tiles[67] = Tile::HazardRight;
        let rendered = render_room_grid(&tiles);
        assert_eq!(parse_room_grid(&rendered), tiles);
    }
}
