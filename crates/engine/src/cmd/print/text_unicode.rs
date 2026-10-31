use super::{Font, FontCatalog, Page, PdfFont, Point, Rgb};

pub(super) fn needs_unicode_text(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_control() && ch != '?' && gridcraft_pdf::win_ansi(ch) == b'?')
}

pub(super) enum TextRunFont {
    Embedded(PdfFont),
    Standard(Font),
}

pub(super) struct TextRun {
    pub(super) font: TextRunFont,
    pub(super) text: String,
    pub(super) width: f32,
}

pub(super) fn shaped_runs(
    catalog: &FontCatalog,
    family: &str,
    bold: bool,
    italic: bool,
    text: &str,
    size: f32,
) -> std::result::Result<Vec<TextRun>, String> {
    let standard = Font::styled(Font::family_for(family), bold, italic);
    let mut runs: Vec<TextRun> = Vec::new();
    let mut thai_fallback: Option<PdfFont> = None;
    for (_, cluster) in gridcraft_pdf::text_clusters(text) {
        let is_thai = cluster.chars().any(|c| ('\u{0e00}'..='\u{0e7f}').contains(&c));
        let latin = cluster.chars().all(|c| c.is_ascii() || c == '?' || gridcraft_pdf::win_ansi(c) != b'?');
        let embedded = if latin {
            catalog.family_face_for(family, bold, italic, cluster).ok()
        } else if is_thai {
            match thai_fallback.as_ref().filter(|face| face.covers(cluster)) {
                Some(face) => Some(face.clone()),
                None => catalog.face_for(family, bold, italic, cluster).ok().inspect(|face| {
                    thai_fallback = Some(face.clone());
                }),
            }
        } else {
            catalog.face_for(family, bold, italic, cluster).ok()
        };
        let runfont = match embedded {
            Some(face) => TextRunFont::Embedded(face),
            None => TextRunFont::Standard(standard),
        };
        let same = runs.last().is_some_and(|run| match (&run.font, &runfont) {
            (TextRunFont::Standard(a), TextRunFont::Standard(b)) => a == b,
            (TextRunFont::Embedded(a), TextRunFont::Embedded(b)) => a.same_face(b),
            _ => false,
        });
        if same {
            if let Some(last) = runs.last_mut() {
                last.text.push_str(cluster);
            }
        } else {
            runs.push(TextRun { font: runfont, text: cluster.to_string(), width: 0.0 });
        }
    }
    for run in &mut runs {
        run.width = match &run.font {
            TextRunFont::Embedded(font) => font.text_width(&run.text, size)?,
            TextRunFont::Standard(font) => gridcraft_pdf::text_width(*font, size, &run.text),
        };
    }
    Ok(runs)
}

pub(super) fn shaped_width(catalog: &FontCatalog, family: &str, bold: bool, italic: bool, text: &str, size: f32) -> std::result::Result<f32, String> {
    Ok(shaped_runs(catalog, family, bold, italic, text, size)?.iter().map(|r| r.width).sum())
}

pub(super) fn shaped_extents(
    catalog: &FontCatalog,
    family: &str,
    bold: bool,
    italic: bool,
    text: &str,
    size: f32,
) -> std::result::Result<(f32, f32), String> {
    let mut above: f32 = 0.0;
    let mut below: f32 = 0.0;
    for run in shaped_runs(catalog, family, bold, italic, text, size)? {
        let (a, d) = match run.font {
            TextRunFont::Embedded(font) => font.vertical_extents(size),
            TextRunFont::Standard(_) => (size * 0.8, size * 0.22),
        };
        above = above.max(a);
        below = below.max(d);
    }
    Ok((above, below))
}

/// Returns the next run's baseline origin after advancing along the rotated text baseline.
pub(super) fn advance_text_baseline(x: f32, y: f32, advance: f32, angle: f32) -> (f32, f32) {
    let (sn, cs) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
    (x + advance * cs, y + advance * sn)
}

pub(super) fn draw_text_runs(
    pg: &mut Page,
    runs: Vec<TextRun>,
    x: f32,
    y: f32,
    size: f32,
    color: Rgb,
    angle: f32,
) -> std::result::Result<f32, String> {
    if runs.is_empty() {
        return Ok(0.0);
    }
    let total_width = runs.iter().map(|run| run.width).sum();
    let semantic_text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
    pg.begin_actual_text(&semantic_text);
    let mut origin = (x, y);
    let result: std::result::Result<(), String> = (|| {
        for run in runs {
            match run.font {
                TextRunFont::Embedded(font) => {
                    let id = pg.register_unicode_font(font);
                    pg.text_unicode_rotated_without_actual_text(origin.0, origin.1, size, id, color, &run.text, angle)?;
                }
                TextRunFont::Standard(font) => pg.text_rotated(Point::new(origin.0, origin.1), size, font, color, &run.text, angle),
            }
            origin = advance_text_baseline(origin.0, origin.1, run.width, angle);
        }
        Ok(())
    })();
    pg.end_actual_text();
    result?;
    Ok(total_width)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_shaped_text(
    pg: &mut Page,
    catalog: &FontCatalog,
    x: f32,
    y: f32,
    size: f32,
    family: &str,
    bold: bool,
    italic: bool,
    color: Rgb,
    text: &str,
    angle: f32,
) -> std::result::Result<f32, String> {
    let runs = shaped_runs(catalog, family, bold, italic, text, size)?;
    draw_text_runs(pg, runs, x, y, size, color, angle)
}

/// Bracket a line-sized prefix before binary search. Every probe measures a complete run,
/// but a short line never shapes the entire remaining paragraph or collects all its clusters.
pub(super) fn fitting_text_prefix(
    text: &str,
    max_clusters: usize,
    width: f32,
    mut measure: impl FnMut(&str) -> std::result::Result<f32, String>,
) -> std::result::Result<(usize, usize), String> {
    let mut clusters = gridcraft_pdf::text_clusters(text).take(max_clusters);
    let mut ends = Vec::new();
    let mut low = 0usize;
    let mut high = 1usize.min(max_clusters);
    loop {
        while ends.len() < high {
            let Some((start, cluster)) = clusters.next() else { break };
            ends.push(start.saturating_add(cluster.len()));
        }
        let count = ends.len();
        if count == low {
            return Ok((ends.last().copied().unwrap_or(0), count));
        }
        let end = ends.last().copied().unwrap_or(0);
        let prefix = text.get(..end).ok_or("grapheme boundary is outside wrapped PDF text")?;
        if measure(prefix.trim_end_matches(char::is_whitespace))? > width {
            high = count;
            break;
        }
        low = count;
        if count < high || count == max_clusters {
            return Ok((end, count));
        }
        high = count.saturating_mul(2).min(max_clusters);
    }
    while low.saturating_add(1) < high {
        let mid = low + (high - low) / 2;
        let end = ends.get(mid.saturating_sub(1)).copied().unwrap_or(0);
        let prefix = text.get(..end).ok_or("grapheme boundary is outside wrapped PDF text")?;
        if measure(prefix.trim_end_matches(char::is_whitespace))? <= width {
            low = mid;
        } else {
            high = mid;
        }
    }
    let count = low.max(1);
    Ok((ends.get(count.saturating_sub(1)).copied().unwrap_or(0), count))
}

/// Find the longest fitting prefix of word-boundary candidates without shaping every prefix.
/// Each probe still measures the complete accumulated text, preserving run shaping context.
pub(super) fn fitting_candidate_count(
    candidate_count: usize,
    width: f32,
    mut measure: impl FnMut(usize) -> std::result::Result<f32, String>,
) -> std::result::Result<usize, String> {
    if candidate_count == 0 {
        return Ok(0);
    }
    let mut low = 0usize;
    let mut high = 1usize;
    loop {
        if measure(high)? > width {
            break;
        }
        low = high;
        if high == candidate_count {
            return Ok(low);
        }
        high = high.saturating_mul(2).min(candidate_count);
    }
    while low.saturating_add(1) < high {
        let mid = low + (high - low) / 2;
        if measure(mid)? <= width {
            low = mid;
        } else {
            high = mid;
        }
    }
    Ok(low)
}

pub(super) fn wrap_shaped(
    text: &str,
    catalog: &FontCatalog,
    family: &str,
    bold: bool,
    italic: bool,
    size: f32,
    width: f32,
) -> std::result::Result<Vec<String>, String> {
    const MAX_GRAPHEMES_PER_LINE: usize = 8192;

    fn prefix_for_clusters(text: &str, limit: usize) -> (usize, usize) {
        let mut end = 0usize;
        let mut count = 0usize;
        for (start, cluster) in gridcraft_pdf::text_clusters(text).take(limit) {
            end = start.saturating_add(cluster.len());
            count = count.saturating_add(1);
        }
        (end, count)
    }

    // Keep whitespace within a line, but omit separators at a wrap boundary. Measuring a
    // trailing separator must not push a word that otherwise fits onto the next line.
    fn take_wrapped_line(line: &mut String) -> String {
        line.truncate(line.trim_end_matches(char::is_whitespace).len());
        std::mem::take(line)
    }

    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut line_clusters = 0usize;
        let mut words = paragraph.split_inclusive(char::is_whitespace).peekable();
        while let Some(word) = words.next() {
            let mut remaining = word;
            while !remaining.is_empty() {
                if line_clusters >= MAX_GRAPHEMES_PER_LINE {
                    out.push(take_wrapped_line(&mut line));
                    line_clusters = 0;
                    if out.len() >= 2000 {
                        return Ok(out);
                    }
                }
                let capacity = MAX_GRAPHEMES_PER_LINE.saturating_sub(line_clusters);
                if line.is_empty() {
                    let (fit_end, fit_clusters) =
                        fitting_text_prefix(remaining, capacity, width, |prefix| shaped_width(catalog, family, bold, italic, prefix, size))?;
                    if fit_end == 0 || fit_clusters == 0 {
                        break;
                    }
                    if let Some(prefix) = remaining.get(..fit_end) {
                        line.push_str(prefix);
                    }
                    line_clusters = fit_clusters;
                    remaining = remaining.get(fit_end..).unwrap_or_default();
                    if !remaining.is_empty() {
                        out.push(take_wrapped_line(&mut line));
                        line_clusters = 0;
                    }
                    if out.len() >= 2000 {
                        return Ok(out);
                    }
                    continue;
                }
                let (segment_end, segment_clusters) = prefix_for_clusters(remaining, capacity);
                if segment_end == 0 || segment_clusters == 0 {
                    break;
                }
                let Some(segment) = remaining.get(..segment_end) else {
                    return Err("grapheme boundary is outside wrapped PDF text".to_string());
                };
                let combined_clusters = line_clusters.saturating_add(segment_clusters);

                // Gather complete following words up to the line's grapheme cap, then search
                // their boundaries with complete-line shaping. This keeps the same greedy wrap
                // boundary while avoiding one full-line shape for every short segment.
                if segment_end == remaining.len() {
                    let mut additions = segment.to_string();
                    let mut endpoints = vec![(additions.len(), segment_clusters)];
                    let mut added_clusters = segment_clusters;
                    let mut lookahead = words.clone();
                    while added_clusters < capacity {
                        let Some(next_word) = lookahead.next() else { break };
                        let (next_end, next_clusters) = prefix_for_clusters(next_word, capacity.saturating_sub(added_clusters));
                        if next_end != next_word.len() || next_clusters == 0 {
                            break;
                        }
                        additions.push_str(next_word);
                        added_clusters = added_clusters.saturating_add(next_clusters);
                        endpoints.push((additions.len(), added_clusters));
                    }
                    if endpoints.len() > 1 {
                        let fitting = fitting_candidate_count(endpoints.len(), width, |count| {
                            let end = endpoints.get(count.saturating_sub(1)).map(|(end, _)| *end).unwrap_or(0);
                            let addition = additions.get(..end).ok_or("word boundary is outside wrapped PDF text")?;
                            let candidate = format!("{line}{addition}");
                            shaped_width(catalog, family, bold, italic, candidate.trim_end_matches(char::is_whitespace), size)
                        })?;
                        if fitting == 0 {
                            out.push(take_wrapped_line(&mut line));
                            line_clusters = 0;
                            if out.len() >= 2000 {
                                return Ok(out);
                            }
                            continue;
                        }
                        let (end, clusters) = endpoints.get(fitting - 1).copied().ok_or("word boundary is outside wrapped PDF text")?;
                        let addition = additions.get(..end).ok_or("word boundary is outside wrapped PDF text")?;
                        line.push_str(addition);
                        line_clusters = line_clusters.saturating_add(clusters);
                        for _ in 1..fitting {
                            let _ = words.next();
                        }
                        remaining = "";
                        continue;
                    }
                }
                let candidate = format!("{line}{segment}");
                let fits = shaped_width(catalog, family, bold, italic, candidate.trim_end_matches(char::is_whitespace), size)? <= width;
                if fits {
                    line.push_str(segment);
                    line_clusters = combined_clusters;
                    remaining = remaining.get(segment_end..).unwrap_or_default();
                    continue;
                }
                out.push(take_wrapped_line(&mut line));
                line_clusters = 0;
                if out.len() >= 2000 {
                    return Ok(out);
                }
            }
        }
        out.push(line);
        if out.len() >= 2000 {
            return Ok(out);
        }
    }
    Ok(out)
}
