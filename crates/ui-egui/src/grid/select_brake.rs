//! Dragging a selection down past the bottom of the window slows down near the last row of the
//! data, rests on it, then carries on if the button stays down, as in Excel (#4546). The pause
//! makes it easy to let go exactly on the last row.
//!
//! The data edge is where Ctrl+Down goes from the anchor's row, in whichever of the selected
//! columns goes furthest: Excel passed the shorter columns' ends, the anchor's own included, and
//! slowed down only at the longest. A column counts when its cell on the anchor's row and the one
//! below are filled (a drag inside a block of data). The pacing was measured in Excel for Mac with
//! the pointer held still below the grid. The same sequence of rows came up at every
//! cruise speed, so it is tied to the edge and not to the speed:
//! - the moving end stops 10 rows short of the edge ([`ZONE`]);
//! - it eases in 2 rows at a time ([`EASE_SECS`] apart), then 1 row at a time for the last 2
//!   ([`CREEP_SECS`]);
//! - it rests on the edge for [`REST_SECS`];
//! - it creeps 2 rows past ([`PAST_SECS`] apart), then scrolls at full speed again.
//!
//! Only the scroll is paced; letting go still selects the row under the pointer.

use super::{CellRef, Sheet};

/// The widest selection whose columns are searched for the data edge: each column's search walks
/// its run of data, so a drag across the whole sheet would walk every column. Wider drags scroll
/// freely.
const MAX_COLS_SEARCHED: u32 = 256;
/// How many rows short of the edge the moving end stops before easing in.
const ZONE: u32 = 10;
/// Seconds between 2-row steps while easing in.
const EASE_SECS: f64 = 0.14;
/// Seconds between 1-row steps over the last 2 rows.
const CREEP_SECS: f64 = 0.23;
/// Seconds the moving end rests on the last row of the data.
const REST_SECS: f64 = 0.56;
/// Seconds between 1-row steps past the edge.
const PAST_SECS: f64 = 0.22;
/// Rows past the edge that are still paced.
const PAST_ROWS: u32 = 2;

/// One drag's brake: the edge it slows down at, and how long the moving end has been on its row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectBrake {
    /// The drag's anchor and the selected columns the edge was found for.
    anchor: CellRef,
    cols: (u32, u32),
    /// The last row of the data the drag is in; `None` when it doesn't start in data.
    edge: Option<u32>,
    /// The moving end's row and when it got there (egui seconds).
    row: u32,
    since: f64,
    /// The row the last step lets the moving end reach. Auto-scroll may take more than one frame
    /// to get there; the wait for the next step starts once it has.
    granted: u32,
}

impl SelectBrake {
    /// The brake for a drag from `anchor` across the columns `cols` (first, last), with the moving
    /// end on `row` at `now`.
    pub(crate) fn new(sh: &Sheet, anchor: CellRef, cols: (u32, u32), row: u32, now: f64) -> SelectBrake {
        SelectBrake { anchor, cols, edge: data_edge(sh, anchor.row, cols), row, since: now, granted: row }
    }

    /// Whether this brake belongs to a drag from `anchor` across `cols`.
    pub(crate) fn is_for(&self, anchor: CellRef, cols: (u32, u32)) -> bool {
        self.anchor == anchor && self.cols == cols
    }

    /// The furthest row the moving end may reach by the next frame, with it on `row` at `now`;
    /// `None` when it may scroll freely.
    pub(crate) fn limit(&mut self, row: u32, now: f64) -> Option<u32> {
        let edge = self.edge?;
        if row < self.granted {
            return Some(self.granted);
        }
        if row != self.row {
            self.row = row;
            self.since = now;
        }
        let cap = limit(edge, row, now - self.since);
        self.granted = cap.unwrap_or(row);
        cap
    }
}

/// The furthest of the columns `cols` (first, last) goes down in data from `row`: where Ctrl+Down
/// goes from there. Only columns whose cell on `row` and the one below are filled count; `None`
/// when none does.
fn data_edge(sh: &Sheet, row: u32, cols: (u32, u32)) -> Option<u32> {
    if cols.1.saturating_sub(cols.0) >= MAX_COLS_SEARCHED {
        return None;
    }
    let filled = |c: CellRef| sh.value_ref(c).is_some_and(|v| !v.is_empty());
    (cols.0..=cols.1)
        .map(|col| CellRef::new(row, col))
        .filter(|&c| filled(c) && c.offset(1, 0).is_some_and(filled))
        .map(|c| gridcraft_engine::selection::jump(sh, c, 1, 0).row)
        .max()
}

/// The furthest row the moving end may reach, with the data ending at `edge` and the moving end
/// on `row` for `held` seconds; `None` when it may scroll freely.
fn limit(edge: u32, row: u32, held: f64) -> Option<u32> {
    let step = |secs: f64, rows: u32| if held >= secs { row.saturating_add(rows) } else { row };
    if row > edge.saturating_add(PAST_ROWS) {
        None
    } else if row > edge {
        Some(step(PAST_SECS, 1))
    } else if row == edge {
        Some(step(REST_SECS, 1))
    } else if edge - row <= 2 {
        Some(step(CREEP_SECS, 1))
    } else if edge - row <= ZONE {
        Some(step(EASE_SECS, 2).min(edge - 2))
    } else {
        Some(edge - ZONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays the brake at 60 frames a second with the moving end always taking the furthest row
    /// allowed, and returns each row it reaches with the time it got there.
    fn run(edge: u32, from: u32, secs: f64) -> Vec<(u32, f64)> {
        let mut brake = SelectBrake { anchor: CellRef::new(0, 0), cols: (0, 0), edge: Some(edge), row: from, since: 0.0, granted: from };
        let (mut row, mut reached) = (from, vec![(from, 0.0)]);
        for frame in 0..(secs * 60.0) as u32 {
            let now = f64::from(frame) / 60.0;
            // Full speed: about a row a frame, as the drag auto-scrolls.
            let next = brake.limit(row, now).map_or(row + 1, |cap| cap.min(row + 1).max(row));
            if next != row {
                row = next;
                reached.push((row, now));
            }
        }
        reached
    }

    #[test]
    fn the_moving_end_eases_into_the_last_row_and_rests_there() {
        let rows: Vec<u32> = run(299, 250, 4.0).into_iter().map(|(r, _)| r).filter(|r| (285..=304).contains(r)).collect();
        // 0-based: row 299 is the sheet's row 300. Full speed up to 10 rows short, then the
        // full-speed frames stop at 289 and the brake steps by 2, then by 1.
        assert_eq!(rows, [285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297, 298, 299, 300, 301, 302, 303, 304]);
        let at = |row: u32| run(299, 250, 4.0).into_iter().find(|(r, _)| *r == row).map(|(_, t)| t).unwrap();
        // A row a frame reaches a 2-row step in 2 frames; the next step's wait starts from there.
        assert!(at(297) - at(289) < 4.0 * (EASE_SECS + 3.0 / 60.0), "easing in takes 4 steps of 2 rows: {}", at(297) - at(289));
        assert!(at(300) - at(299) >= REST_SECS, "the moving end rests on the last data row");
        assert!(at(299) - at(297) >= 2.0 * CREEP_SECS - 1.0 / 60.0, "the last 2 rows creep");
        assert!(at(304) - at(303) < 0.05, "3 rows past the edge it scrolls at full speed again");
    }

    #[test]
    fn far_from_the_edge_the_end_stops_10_rows_short() {
        assert_eq!(limit(299, 100, 0.0), Some(289));
        assert_eq!(limit(299, 289, 0.0), Some(289));
        assert_eq!(limit(299, 289, EASE_SECS), Some(291));
        assert_eq!(limit(299, 296, EASE_SECS), Some(297), "easing stops 2 rows short, where the creep begins");
        assert_eq!(limit(299, 299, REST_SECS - 0.01), Some(299));
        assert_eq!(limit(299, 299, REST_SECS), Some(300));
        assert_eq!(limit(299, 301, 0.0), Some(301));
        assert_eq!(limit(299, 302, 0.0), None);
    }

    #[test]
    fn the_edge_is_where_ctrl_down_goes_from_inside_the_data() {
        let mut sh = Sheet::new("Sheet1");
        for r in 0..300 {
            sh.set_value(CellRef::new(r, 0), gridcraft_engine::core::Value::Number(f64::from(r)));
        }
        sh.set_value(CellRef::new(0, 2), gridcraft_engine::core::Value::Number(1.0));
        assert_eq!(data_edge(&sh, 0, (0, 0)), Some(299));
        assert_eq!(data_edge(&sh, 120, (0, 0)), Some(299));
        assert_eq!(data_edge(&sh, 299, (0, 0)), None, "the last data row has nothing below it");
        assert_eq!(data_edge(&sh, 0, (1, 1)), None, "an empty column has no data edge");
        assert_eq!(data_edge(&sh, 0, (2, 2)), None, "a lone value is not a block of data");
        assert_eq!(data_edge(&sh, 0, (0, 2)), Some(299), "columns without data on the anchor's row don't count");
    }

    #[test]
    fn across_columns_of_different_lengths_the_longest_sets_the_edge() {
        // A ends at row 300, B at 250, C at 350 (1-based), as observed in Excel.
        let mut sh = Sheet::new("Sheet1");
        for (col, rows) in [(0, 300), (1, 250), (2, 350)] {
            for r in 0..rows {
                sh.set_value(CellRef::new(r, col), gridcraft_engine::core::Value::Number(f64::from(r)));
            }
        }
        assert_eq!(data_edge(&sh, 0, (0, 2)), Some(349));
        assert_eq!(data_edge(&sh, 0, (0, 1)), Some(299));
        assert_eq!(data_edge(&sh, 0, (1, 1)), Some(249));
        assert_eq!(data_edge(&sh, 0, (0, MAX_COLS_SEARCHED)), None, "a drag across too many columns isn't searched");
    }

    #[test]
    fn data_to_the_last_row_of_the_sheet_never_overflows() {
        let last = super::super::MAX_ROWS - 1;
        assert_eq!(limit(last, last, REST_SECS), Some(last.saturating_add(1)));
        assert_eq!(limit(last, last - 1, CREEP_SECS), Some(last));
    }
}
