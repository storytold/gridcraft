use super::capture::{Capture, Letter, Segment};
use super::{ImportError, MAX_CELLS, PdfTable, TOLERANCE};
use std::collections::BTreeMap;

struct Sets {
    parents: Vec<usize>,
    sizes: Vec<usize>,
}
impl Sets {
    fn new(n: usize) -> Self {
        Self { parents: (0..n).collect(), sizes: vec![1; n] }
    }
    fn root(&mut self, mut n: usize) -> usize {
        while self.parents[n] != n {
            self.parents[n] = self.parents[self.parents[n]];
            n = self.parents[n];
        }
        n
    }
    fn join(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (self.root(a), self.root(b));
        if a == b {
            return;
        }
        if self.sizes[a] < self.sizes[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parents[b] = a;
        self.sizes[a] += self.sizes[b];
    }
}
fn normalized(mut segments: Vec<Segment>) -> Vec<Segment> {
    segments.sort_by(|a, b| a.horizontal.cmp(&b.horizontal).then(a.level.total_cmp(&b.level)).then(a.start.total_cmp(&b.start)));
    let mut grouped: Vec<Segment> = Vec::new();
    for mut s in segments {
        if let Some(last) = grouped.last_mut()
            && s.horizontal == last.horizontal
            && (s.level - last.level).abs() <= TOLERANCE
        {
            s.level = last.level;
        }
        grouped.push(s);
    }
    grouped.sort_by(|a, b| a.horizontal.cmp(&b.horizontal).then(a.level.total_cmp(&b.level)).then(a.start.total_cmp(&b.start)));
    let mut out: Vec<Segment> = Vec::new();
    for s in grouped {
        if let Some(last) = out.last_mut()
            && last.horizontal == s.horizontal
            && last.level == s.level
            && s.start <= last.end + TOLERANCE
        {
            last.end = last.end.max(s.end);
            continue;
        }
        out.push(s);
    }
    out
}
fn coordinates(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= TOLERANCE);
    values
}
/// An x-sorted set of verticals bounds each horizontal's intersection search.
/// The global work budget also bounds dense grids; no all-pairs component search.
pub(super) fn detect(capture: &mut Capture) -> Result<Vec<PdfTable>, ImportError> {
    let segments = normalized(std::mem::take(&mut capture.segments));
    let mut sets = Sets::new(segments.len());
    let vertical: Vec<_> = segments.iter().enumerate().filter(|(_, s)| !s.horizontal).collect();
    for (index, h) in segments.iter().enumerate().filter(|(_, s)| s.horizontal) {
        let first = vertical.partition_point(|(_, v)| v.level < h.start - TOLERANCE);
        for (other, v) in vertical.iter().skip(first).take_while(|(_, v)| v.level <= h.end + TOLERANCE) {
            capture.control.spend(1)?;
            if h.level >= v.start - TOLERANCE && h.level <= v.end + TOLERANCE {
                sets.join(index, *other);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<Segment>> = BTreeMap::new();
    for (i, s) in segments.into_iter().enumerate() {
        groups.entry(sets.root(i)).or_default().push(s);
    }
    let mut tables = Vec::new();
    let mut cells = 0usize;
    for lines in groups.into_values() {
        capture.control.spend(lines.len())?;
        if let Some(table) = table(&lines, capture)? {
            let count = table.rows.len().checked_mul(table.rows.first().map_or(0, Vec::len)).ok_or(ImportError::Limit("cells"))?;
            cells = cells.checked_add(count).ok_or(ImportError::Limit("cells"))?;
            if cells > MAX_CELLS {
                return Err(ImportError::Limit("cells"));
            }
            tables.push(table);
        }
    }
    tables.sort_by(|a, b| a.bounds[1].total_cmp(&b.bounds[1]).then(a.bounds[0].total_cmp(&b.bounds[0])));
    Ok(tables)
}
fn table(lines: &[Segment], capture: &mut Capture) -> Result<Option<PdfTable>, ImportError> {
    let xs = coordinates(lines.iter().filter(|s| !s.horizontal).map(|s| s.level).collect());
    let ys = coordinates(lines.iter().filter(|s| s.horizontal).map(|s| s.level).collect());
    if xs.len() < 2 || ys.len() < 2 || xs.len() == 2 && ys.len() == 2 {
        return Ok(None);
    }
    let bounds = [xs[0], ys[0], xs[xs.len() - 1], ys[ys.len() - 1]];
    let complete = lines.iter().all(|s| {
        let (lo, hi) = if s.horizontal { (bounds[0], bounds[2]) } else { (bounds[1], bounds[3]) };
        (s.start - lo).abs() <= TOLERANCE && (s.end - hi).abs() <= TOLERANCE
    });
    if !complete {
        return Err(ImportError::Unsupported("merged cells or incomplete table borders"));
    }
    let rect = kurbo::Rect::new(bounds[0], bounds[1], bounds[2], bounds[3]);
    if capture.images.iter().any(|image| image.intersect(rect).area() > 0.) {
        return Err(ImportError::Unsupported("an image overlaps table cells; OCR is not included"));
    }
    let count = (xs.len() - 1).checked_mul(ys.len() - 1).ok_or(ImportError::Limit("cells"))?;
    if count > MAX_CELLS {
        return Err(ImportError::Limit("cells"));
    }
    let mut members: Vec<Vec<&Letter>> = vec![Vec::new(); count];
    for letter in &capture.letters {
        capture.control.spend(1)?;
        if letter.center.x <= bounds[0] || letter.center.x >= bounds[2] || letter.center.y <= bounds[1] || letter.center.y >= bounds[3] {
            continue;
        }
        let col = xs.partition_point(|x| *x < letter.center.x).saturating_sub(1);
        let row = ys.partition_point(|y| *y < letter.center.y).saturating_sub(1);
        if col >= xs.len() - 1 || row >= ys.len() - 1 {
            return Err(ImportError::Unsupported("text on a cell boundary"));
        }
        if !letter.text.trim().is_empty() && (letter.baseline.x < xs[col] - TOLERANCE || letter.end > xs[col + 1] + TOLERANCE) {
            return Err(ImportError::Unsupported("text crossing table borders"));
        }
        members[row * (xs.len() - 1) + col].push(letter);
    }
    let mut rows = Vec::with_capacity(ys.len() - 1);
    for chunk in members.chunks_mut(xs.len() - 1) {
        let row: Vec<_> = chunk.iter_mut().map(|letters| cell_text(letters)).collect();
        if row.iter().any(|t| t.encode_utf16().take(super::MAX_CELL_TEXT + 1).count() > super::MAX_CELL_TEXT) {
            return Err(ImportError::Limit("cell text length"));
        }
        rows.push(row);
    }
    Ok(Some(PdfTable { bounds, rows }))
}
fn cell_text(letters: &mut [&Letter]) -> String {
    letters.sort_by(|a, b| a.baseline.y.total_cmp(&b.baseline.y).then(a.baseline.x.total_cmp(&b.baseline.x)));
    let mut text = String::new();
    let mut at = 0;
    while at < letters.len() {
        let base = letters[at].baseline.y;
        let end = at + letters[at..].partition_point(|g| g.baseline.y - base <= 1.0);
        let line = &mut letters[at..end];
        line.sort_by(|a, b| a.baseline.x.total_cmp(&b.baseline.x));
        if !text.is_empty() {
            text.push('\n');
        }
        let mut previous: Option<&Letter> = None;
        for letter in line.iter().copied() {
            if let Some(last) = previous {
                if last.text == letter.text && last.baseline.distance(letter.baseline) < 0.05 {
                    continue;
                }
                if letter.baseline.x - last.end > letter.height * 0.18
                    && !text.ends_with(char::is_whitespace)
                    && !letter.text.starts_with(char::is_whitespace)
                {
                    text.push(' ');
                }
            }
            text.push_str(&letter.text);
            previous = Some(letter);
        }
        at = end;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::super::Control;
    use super::*;
    use kurbo::{Point, Rect};
    use std::sync::{Arc, atomic::AtomicBool};
    fn grid() -> Capture {
        let mut c = Capture::new(Control::new(Arc::new(AtomicBool::new(false))), Rect::new(0., 0., 1000., 1000.));
        for level in [10., 30., 50.] {
            c.segments.push(Segment { horizontal: true, level, start: 10., end: 50. });
            c.segments.push(Segment { horizontal: false, level, start: 10., end: 50. });
        }
        c
    }
    #[test]
    fn split_and_overpainted_borders_form_one_table() {
        let mut c = grid();
        let line = c.segments.remove(0);
        c.segments.extend([Segment { end: 25., ..line }, Segment { start: 25.2, ..line }, line]);
        let tables = detect(&mut c).unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows, vec![vec!["", ""], vec!["", ""]]);
    }
    #[test]
    fn merged_or_incomplete_border_is_refused_not_silently_dropped() {
        let mut c = grid();
        c.segments.iter_mut().find(|s| !s.horizontal && s.level == 30.).unwrap().start = 30.;
        assert!(matches!(detect(&mut c), Err(ImportError::Unsupported(_))));
    }
    #[test]
    fn resource_budget_refuses_oversized_grid_before_cell_allocation() {
        let mut c = grid();
        c.segments.clear();
        for i in 0..=320 {
            c.segments.push(Segment { horizontal: true, level: i as f64, start: 0., end: 320. });
            c.segments.push(Segment { horizontal: false, level: i as f64, start: 0., end: 320. });
        }
        assert!(matches!(detect(&mut c), Err(ImportError::Limit("cells"))));
    }
    #[test]
    fn cell_border_text_is_refused_and_repeated_glyphs_are_not_duplicated() {
        let mut c = grid();
        for _ in 0..2 {
            c.letters.push(Letter { baseline: Point::new(12., 22.), center: Point::new(15., 20.), end: 18., height: 10., text: "A".into() });
        }
        assert_eq!(detect(&mut c).unwrap()[0].rows[0][0], "A");
        let mut c = grid();
        c.letters.push(Letter { baseline: Point::new(25., 22.), center: Point::new(28., 20.), end: 35., height: 10., text: "Wide".into() });
        assert!(matches!(detect(&mut c), Err(ImportError::Unsupported(_))));
    }
}
