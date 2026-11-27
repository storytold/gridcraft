//! Transfer sparse annotations directly into literal cell values, without a model reading
//! or retyping the sheet. Plan every write before changing the workbook copy in `edit`.
use super::{CellRef, DocState, EngineError, Json, Result, Session, bad, edit, json, str_param, target_sheet};
use gridcraft_core::{MAX_ROWS, MAX_TEXT_CHARS, Value, col_to_letters, letters_to_col};
use gridcraft_model::{Cell, Comment, HAlign, Sheet, ValidationKind};

const ID: &str = "review.commentsToColumn";

fn column(p: &Json, key: &str) -> Result<u32> {
    str_param(p, key).and_then(letters_to_col).ok_or_else(|| bad(ID, format!("{key} must be an Excel column from A through XFD")))
}

fn row(p: &Json, key: &str, default: u32) -> Result<u32> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=u64::from(MAX_ROWS)).contains(n))
            .map(|n| n as u32 - 1)
            .ok_or_else(|| bad(ID, format!("{key} must be a row from 1 through {MAX_ROWS}"))),
    }
}

fn check_length(text: &str, at: CellRef) -> Result<()> {
    if text.len() > MAX_TEXT_CHARS && text.encode_utf16().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS {
        return Err(bad(ID, format!("Text at {} exceeds the Excel cell limit of {MAX_TEXT_CHARS} UTF-16 units; no text was transferred.", at.a1())));
    }
    Ok(())
}

fn transcript(note: &Comment, at: CellRef) -> Result<String> {
    if note.replies.is_empty() {
        check_length(&note.text, at)?;
        return Ok(note.text.clone());
    }
    let mut out = String::new();
    let mut units = 0usize;
    let mut append = |part: &str| -> Result<()> {
        // Refuse even one overlong entry before allocating the transcript; counting the whole
        // accumulated string for each short reply would be quadratic.
        units += part.encode_utf16().take(MAX_TEXT_CHARS + 1).count();
        if units > MAX_TEXT_CHARS {
            return Err(bad(
                ID,
                format!("Thread at {} exceeds the Excel cell limit of {MAX_TEXT_CHARS} UTF-16 units; no text was transferred.", at.a1()),
            ));
        }
        out.push_str(part);
        Ok(())
    };
    for (index, (author, body)) in std::iter::once((&note.author, &note.text)).chain(note.replies.iter().map(|(a, b)| (a, b))).enumerate() {
        if index != 0 {
            append("\n")?;
        }
        if !author.is_empty() {
            append(author)?;
            append(": ")?;
        }
        append(body)?;
    }
    Ok(out)
}

fn check_lock(d: &DocState, sh: &Sheet, at: CellRef) -> Result<()> {
    if sh.is_protected()
        && d.wb.styles.get(sh.style_id(at)).protection.locked
        && let Some(reason) = super::super::locked_cell_error(sh, at, &d.unlocked_ranges)
    {
        return Err(EngineError::Other(format!("{}: {reason}", at.a1())));
    }
    Ok(())
}

fn check_destination(d: &DocState, sh: &Sheet, at: CellRef) -> Result<()> {
    check_lock(d, sh, at)?;
    let reason = if sh.merge_at(at).is_some() {
        "belongs to a merged range"
    } else if sh.spill.contains_key(&at) || sh.spill_anchor(at).is_some() {
        "is part of an array formula"
    } else if sh.pivots.iter().any(|pt| pt.last_range.is_some_and(|r| r.contains(at))) {
        "belongs to a PivotTable"
    } else if sh.table_at(at).is_some() {
        "belongs to a table"
    } else if sh.cell_pictures.contains_key(&at) || sh.sparklines.iter().any(|s| s.cell == at) {
        "contains an in-cell picture or sparkline"
    } else if sh.comments.contains_key(&at) || sh.hyperlinks.contains_key(&at) {
        "contains a note, comment or hyperlink"
    } else if sh.cell(at).is_some_and(Cell::has_content) {
        "already contains data or a formula"
    } else if sh.validations.iter().any(|v| v.kind != ValidationKind::Any && v.ranges.iter().any(|r| r.contains(at))) {
        "has a validation rule"
    } else {
        return Ok(());
    };
    Err(bad(ID, format!("{} {reason}. Choose an empty destination; nothing was transferred.", at.a1())))
}

pub(super) fn transfer(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("sheet").is_some_and(|v| !v.is_string() && !v.is_u64()) {
        return Err(bad(ID, "sheet must be its name or index"));
    }
    let sheet = target_sheet(s, p)?;
    let src = column(p, "sourceColumn")?;
    let dst = column(p, "destinationColumn")?;
    if src == dst {
        return Err(bad(ID, "sourceColumn and destinationColumn must be different"));
    }
    let start = row(p, "startRow", 0)?;
    let end = row(p, "endRow", MAX_ROWS - 1)?;
    let destination_start = row(p, "destinationStartRow", start)?;
    if start > end {
        return Err(bad(ID, "startRow cannot exceed endRow"));
    }
    let mode = match p.get("mode") {
        None => "copy",
        Some(v) => v.as_str().filter(|v| matches!(*v, "copy" | "move")).ok_or_else(|| bad(ID, "mode must be copy or move"))?,
    };
    let header_at = CellRef::new(row(p, "headerRow", 0)?, dst);
    let header =
        p.get("headerText").map(|v| v.as_str().filter(|t| !t.is_empty()).ok_or_else(|| bad(ID, "headerText must be nonempty text"))).transpose()?;
    if header.is_none() && p.get("headerRow").is_some() {
        return Err(bad(ID, "headerRow requires headerText"));
    }
    let doc = s.doc()?;
    let sh = doc.wb.sheet(sheet).ok_or_else(|| bad(ID, "no such sheet"))?;
    let can_format = sh.protection.as_ref().is_none_or(|pr| pr.format_cells);
    let mut writes = Vec::new();
    for (&at, note) in sh.comments.range(CellRef::new(start, src)..=CellRef::new(end, src)).filter(|(at, _)| at.col == src) {
        let row = destination_start + (at.row - start);
        if row >= MAX_ROWS {
            return Err(bad(ID, format!("The destination for {} would be past the last worksheet row; nothing was transferred.", at.a1())));
        }
        let dest = CellRef::new(row, dst);
        if header.is_some() && dest == header_at {
            return Err(bad(
                ID,
                format!(
                    "The header at {} would cover a transferred comment. Set destinationStartRow below the header, choose a different headerRow, or omit headerText.",
                    dest.a1()
                ),
            ));
        }
        check_destination(doc, sh, dest)?;
        if mode == "move" {
            check_lock(doc, sh, at)?;
        }
        writes.push((dest, Some(at), transcript(note, at)?));
    }
    let count = writes.len();
    // A typo in the source column mustn't leave an orphan header (or an undo entry).
    let header = header.filter(|_| count != 0);
    if let Some(text) = header {
        check_length(text, header_at)?;
        check_destination(doc, sh, header_at)?;
        if !can_format {
            return Err(bad(ID, "Formatting the header is disabled on this protected sheet."));
        }
        writes.push((header_at, None, text.to_owned()));
    }
    if !writes.is_empty() {
        edit(s, |cx| {
            for (at, source, text) in writes {
                let is_header = source.is_none();
                let base = cx.sheet_mut(sheet)?.style_id(at);
                let mut style = base;
                let wrap = can_format && text.contains('\n');
                if is_header || wrap {
                    style = cx.wb.styles.derive(base, |st| {
                        if is_header {
                            st.align.h = HAlign::Center;
                            st.align.indent = 0;
                        }
                        if wrap {
                            st.align.wrap = true;
                        }
                    });
                }
                let sh = cx.sheet_mut(sheet)?;
                sh.set_cell(at, Cell::styled(Value::text(text), style));
                super::super::data::grow_autofilter(sh, at);
                if mode == "move"
                    && let Some(source) = source
                {
                    sh.comments.remove(&source);
                    cx.touch(sheet, source);
                }
                cx.touch(sheet, at);
            }
            Ok(())
        })?;
    }
    Ok(
        json!({"count":count,"mode":mode,"sourceColumn":col_to_letters(src),"destinationColumn":col_to_letters(dst),"headerCell":header.map(|_| header_at.a1())}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn at(s: &str) -> CellRef {
        CellRef::parse(s).unwrap()
    }
    fn sheet(s: &Session) -> &Sheet {
        s.doc().unwrap().wb.active().unwrap()
    }
    fn sample() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("review.newNote", json!({"cell":"AB1", "text":"=1+2\n  literal text  "})).unwrap();
        s.execute("review.newComment", json!({"cell":"AB8", "author":"Ada", "text":" first\nline "})).unwrap();
        s.execute("review.replyComment", json!({"cell":"AB8","author":"Mo","text":"=A1"})).unwrap();
        s
    }

    #[test]
    fn offset_move_keeps_sparse_rows_and_truly_literal_content_in_one_history_step() {
        let mut s = sample();
        let original = sheet(&s).comments.clone();
        let p = json!({"sourceColumn":"ab","destinationColumn":"AC","destinationStartRow":2,"headerText":"Notes","mode":"move"});
        let r = s.execute(ID, p).unwrap();
        assert_eq!(r["count"], original.len());
        let sh = sheet(&s);
        assert_eq!(sh.value(at("AC2")), Value::text("=1+2\n  literal text  "));
        assert!(sh.cell(at("AC2")).unwrap().formula.is_none());
        assert!(sh.cell(at("AC8")).is_none());
        assert_eq!(sh.value(at("AC9")), Value::text("Ada:  first\nline \nMo: =A1"));
        assert!(sh.comments.is_empty());
        assert_eq!(s.doc().unwrap().wb.styles.get(sh.style_id(at("AC1"))).align.h, HAlign::Center);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sheet(&s).comments, original);
        assert!(sheet(&s).cell(at("AC1")).is_none());
        assert!(sheet(&s).cell(at("AC9")).is_none());
        s.execute("edit.redo", json!({})).unwrap();
        assert!(sheet(&s).comments.is_empty());
    }

    #[test]
    fn later_collisions_and_oversized_text_preserve_the_entire_original() {
        let mut s = sample();
        let args = json!({"sourceColumn":"AB","destinationColumn":"AC","mode":"move"});
        let refuses = |s: &mut Session, args: Json| {
            let wb = s.doc().unwrap().wb.clone();
            let undo = s.doc().unwrap().undo.len();
            assert!(s.execute(ID, args).is_err());
            assert!(Arc::ptr_eq(&wb, &s.doc().unwrap().wb));
            assert_eq!(undo, s.doc().unwrap().undo.len());
        };
        let mut with_header = args.clone();
        with_header["headerText"] = "Notes".into();
        refuses(&mut s, with_header);
        let mut out_of_rows = args.clone();
        out_of_rows["destinationStartRow"] = MAX_ROWS.into();
        refuses(&mut s, out_of_rows);
        for (command, params) in [("home.mergeCells", json!({"range":"AC8:AD8"})), ("review.protectSheet", json!({}))] {
            s.execute(command, params).unwrap();
            refuses(&mut s, args.clone());
            s.execute("edit.undo", json!({})).unwrap();
        }
        s.execute("cell.set", json!({"cell":"AC8","input":"=\"\""})).unwrap();
        refuses(&mut s, args.clone());
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("review.replyComment", json!({"cell":"AB8","text":"😀".repeat(MAX_TEXT_CHARS / 2 + 1)})).unwrap();
        refuses(&mut s, args);
        let r = s.execute(ID, json!({"sourceColumn":"AB","destinationColumn":"AC","endRow":1})).unwrap();
        assert_eq!(r["count"], 1);
        assert!(sheet(&s).comments.contains_key(&at("AB1")));
        assert_eq!(sheet(&s).value(at("AC1")), Value::text("=1+2\n  literal text  "));
    }

    #[test]
    fn no_matches_leave_no_header_and_no_undo_step() {
        let mut s = sample();
        let before = s.doc().unwrap().wb.clone();
        let undo = s.doc().unwrap().undo.len();
        let r = s.execute(ID, json!({"sourceColumn":"AB","destinationColumn":"AC","startRow":2,"endRow":7,"headerText":"Notes"})).unwrap();
        assert_eq!(r["count"], 0);
        assert!(r["headerCell"].is_null());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
        assert_eq!(s.doc().unwrap().undo.len(), undo);
    }
}
