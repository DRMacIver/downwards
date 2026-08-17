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

/// Ticks of amber wind-up the client renders before a hazard's active window.
pub const HAZARD_AMBER_WIND_UP_TICKS: u32 = 24;

/// Minimum ticks of true off-time (neither active nor amber) every timed
/// hazard must show per cycle: 12 ticks = 0.2s at 60Hz, the floor below which
/// a hazard reads as permanently armed and cannot be timed by a human.
pub const HAZARD_MIN_OFF_TICKS: u32 = 12;

/// Check a timed hazard's cycle for a visible off-phase.
///
/// Returns `Some(deficit)` — how many ticks short of the required off-time the
/// cycle is — when `period_ticks - active_ticks` leaves less than
/// [`HAZARD_AMBER_WIND_UP_TICKS`] + [`HAZARD_MIN_OFF_TICKS`] of idle time, so
/// the hazard is never (or barely) visibly off.
#[must_use]
pub fn hazard_off_time_deficit(period_ticks: u32, active_ticks: u32) -> Option<u32> {
    let required_idle = HAZARD_AMBER_WIND_UP_TICKS + HAZARD_MIN_OFF_TICKS;
    let idle = period_ticks.saturating_sub(active_ticks);
    (idle < required_idle).then(|| required_idle - idle)
}

/// Find "useless" spikes: hazard tiles whose pointed face can never be hit
/// because the cell the point faces is solid, another spike (spike flanks and
/// backs are inert obstacles), or out of bounds. Empty and one-way cells leave
/// the point reachable (one-way platforms never block motion into the cell
/// from below, the sides, or a drop-through from above).
///
/// Returns `(row, column)` pairs in row-major order.
#[must_use]
pub fn useless_spikes(tiles: &[Tile]) -> Vec<(u16, u16)> {
    assert_eq!(tiles.len(), usize::from(WIDTH) * usize::from(HEIGHT));
    let tile_at = |column: i32, row: i32| -> Option<Tile> {
        (column >= 0 && column < i32::from(WIDTH) && row >= 0 && row < i32::from(HEIGHT)).then(
            || tiles[usize::try_from(row).unwrap() * usize::from(WIDTH) + usize::try_from(column).unwrap()],
        )
    };
    let mut useless = Vec::new();
    for row in 0..i32::from(HEIGHT) {
        for column in 0..i32::from(WIDTH) {
            let tile = tile_at(column, row).expect("in bounds");
            let (dx, dy) = match tile {
                Tile::HazardUp => (0, -1),
                Tile::HazardDown => (0, 1),
                Tile::HazardLeft => (-1, 0),
                Tile::HazardRight => (1, 0),
                _ => continue,
            };
            let blocked = match tile_at(column + dx, row + dy) {
                None | Some(Tile::Solid) => true,
                Some(
                    Tile::HazardUp | Tile::HazardDown | Tile::HazardLeft | Tile::HazardRight,
                ) => true,
                Some(Tile::Empty | Tile::OneWay) => false,
            };
            if blocked {
                useless.push((
                    u16::try_from(row).unwrap(),
                    u16::try_from(column).unwrap(),
                ));
            }
        }
    }
    useless
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

    fn grid_from_rows(rows: &[(u16, &str)]) -> Vec<Tile> {
        let mut lines = vec![".".repeat(usize::from(WIDTH)); usize::from(HEIGHT)];
        for &(row, content) in rows {
            assert_eq!(content.len(), usize::from(WIDTH));
            lines[usize::from(row)] = content.to_owned();
        }
        parse_room_grid(&(lines.join("\n") + "\n"))
    }

    #[test]
    fn spike_stack_terminating_in_wall_is_useless() {
        // Regression case modeled on the one-way-loop rooms: a vertical stack
        // of up-spikes buried inside a solid column, each point facing the
        // spike (or wall) above it.
        let tiles = grid_from_rows(&[
            (5, "################################"),
            (6, "#....^##########################"),
            (7, "#....^##########################"),
            (8, "#....^##########################"),
        ]);
        assert_eq!(useless_spikes(&tiles), vec![(6, 5), (7, 5), (8, 5)]);
    }

    #[test]
    fn spike_pointing_into_open_space_or_one_way_is_useful() {
        let tiles = grid_from_rows(&[
            (7, "#^........v....................#"),
            (8, "#.........-....................#"),
        ]);
        assert!(useless_spikes(&tiles).is_empty());
    }

    #[test]
    fn spike_pointing_out_of_bounds_is_useless() {
        let tiles = grid_from_rows(&[(0, "..............^................>")]);
        assert_eq!(useless_spikes(&tiles), vec![(0, 14), (0, 31)]);
    }

    #[test]
    fn horizontal_spike_into_solid_is_useless() {
        let tiles = grid_from_rows(&[(4, "#>#..........................#<#")]);
        assert_eq!(useless_spikes(&tiles), vec![(4, 1), (4, 30)]);
    }

    #[test]
    fn hazard_with_no_visible_off_phase_is_rejected() {
        // 51 active + 24 amber = 75 > 72 period: never off.
        assert_eq!(hazard_off_time_deficit(72, 51), Some(15));
        // Exactly at the floor: 96 - 60 = 36 = 24 amber + 12 off.
        assert_eq!(hazard_off_time_deficit(96, 60), None);
        // One tick short of the floor.
        assert_eq!(hazard_off_time_deficit(95, 60), Some(1));
    }
}
