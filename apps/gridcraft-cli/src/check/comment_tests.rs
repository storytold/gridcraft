//! Notes and comment threads carry content and state beyond their map entry count.

#![allow(clippy::unwrap_used)]

use gridcraft_engine::model::Comment;

use super::*;

fn note() -> Comment {
    Comment {
        author: "Author & Reviewer".into(),
        text: "Check <total> & \"units\"".into(),
        replies: vec![],
        threaded: false,
        resolved: false,
        visible: true,
        dates: vec![],
    }
}

fn thread() -> Comment {
    Comment {
        replies: vec![("Reviewer".into(), "First reply".into()), ("Owner".into(), "Second reply".into())],
        threaded: true,
        resolved: true,
        visible: false,
        dates: vec!["2026-10-12T09:00:00Z".into(), "2026-10-12T10:00:00Z".into(), "2026-10-12T11:00:00Z".into()],
        ..note()
    }
}

fn book(comments: &[(&str, Comment)]) -> Workbook {
    let mut wb = Workbook::new();
    for (cell, comment) in comments {
        wb.sheet_mut(0).unwrap().comments.insert(CellRef::parse(cell).unwrap(), comment.clone());
    }
    wb
}

fn comment_diff(before: &Workbook, after: &Workbook, max_report: usize) -> Category {
    diff_workbooks(before, after, &Options { max_report, ..Default::default() }).into_iter().find(|c| c.name == "comments").unwrap()
}

#[test]
fn same_count_comment_content_and_state_changes_are_detected() {
    type Change = fn(&mut Comment);
    let changes: &[(&str, Change)] = &[
        ("author", |c| c.author = "Different author".into()),
        ("text", |c| c.text = "Changed body".into()),
        ("reply author", |c| c.replies[0].0 = "Different reviewer".into()),
        ("reply text", |c| c.replies[0].1 = "Different reply".into()),
        ("reply order", |c| c.replies.reverse()),
        ("thread kind", |c| c.threaded = false),
        ("resolved", |c| c.resolved = false),
        ("visibility", |c| c.visible = true),
        ("thread date", |c| c.dates[0] = "2026-10-13T09:00:00Z".into()),
        ("reply date", |c| c.dates[1] = "2026-10-13T10:00:00Z".into()),
    ];
    let original = thread();
    let before = book(&[("C7", original.clone())]);
    for (name, change) in changes {
        let mut changed = original.clone();
        change(&mut changed);
        let category = comment_diff(&before, &book(&[("C7", changed)]), 20);
        assert_eq!(category.diffs, 1, "{name}: {category:?}");
        assert!(category.examples[0].contains("C7"), "missing cell context: {category:?}");
    }
}

#[test]
fn moving_a_comment_changes_both_source_and_destination_cells() {
    let before = book(&[("A1", note())]);
    let after = book(&[("B2", note())]);
    let category = comment_diff(&before, &after, 20);
    assert_eq!(category.diffs, 2);
    assert!(category.examples.iter().any(|e| e.contains("A1")));
    assert!(category.examples.iter().any(|e| e.contains("B2")));
}

#[test]
fn insertion_order_and_trailing_unknown_dates_are_equivalent() {
    let mut unknown = thread();
    unknown.dates.clear();
    let mut padded = unknown.clone();
    padded.dates = vec![String::new(); 3];
    let before = book(&[("A1", note()), ("B2", unknown)]);
    let after = book(&[("B2", padded), ("A1", note())]);
    assert_eq!(comment_diff(&before, &after, 20).diffs, 0);
}

#[test]
fn unknown_interior_date_slots_keep_their_reply_position() {
    let mut original = thread();
    original.dates = vec![String::new(), String::new(), "2026-10-12T11:00:00Z".into()];
    let mut shifted = original.clone();
    shifted.dates.remove(1);
    assert_eq!(comment_diff(&book(&[("A1", original)]), &book(&[("A1", shifted)]), 20).diffs, 1);
}

#[test]
fn notes_and_threads_with_replies_and_dates_survive_two_xlsx_saves() {
    let mut sparse = thread();
    sparse.dates = vec![String::new(), String::new(), "2026-10-12T11:00:00Z".into()];
    let original = book(&[("A1", note()), ("B2", thread()), ("C3", sparse)]);
    let mut current = original.clone();
    for _ in 0..2 {
        let bytes = gridcraft_engine::io::save_bytes(&current, "comments.xlsx").unwrap();
        current = gridcraft_engine::io::open_bytes("comments.xlsx", &bytes).unwrap().0;
        let category = comment_diff(&original, &current, 20);
        assert_eq!(category.diffs, 0, "{category:?}");
        let report = check_bytes("comments.xlsx", &bytes, &Options::default());
        assert!(report.is_clean(), "{report:?}");
    }
    assert_eq!(current.sheet(0).unwrap().comments[&CellRef::parse("A1").unwrap()].text, note().text);
    assert_eq!(current.sheet(0).unwrap().comments[&CellRef::parse("B2").unwrap()].replies, thread().replies);
}

#[test]
fn discarded_nonempty_dates_and_unsupported_note_state_are_reported() {
    let mut invalid_date = thread();
    invalid_date.dates[1] = "not an ISO timestamp".into();
    let mut extra_date = thread();
    extra_date.dates.push("2026-10-13T12:00:00Z".into());
    let mut unsupported_note = note();
    unsupported_note.resolved = true;
    unsupported_note.replies.push(("Reviewer".into(), "Reply attached to a classic note".into()));
    unsupported_note.dates.push("2026-10-12T09:00:00Z".into());
    for original in [invalid_date, extra_date, unsupported_note] {
        let before = book(&[("D4", original)]);
        let bytes = gridcraft_engine::io::save_bytes(&before, "loss.xlsx").unwrap();
        let after = gridcraft_engine::io::open_bytes("loss.xlsx", &bytes).unwrap().0;
        assert_eq!(comment_diff(&before, &after, 20).diffs, 1, "discarded model data must not be canonicalized away");
    }
}

#[test]
fn empty_comment_addition_and_removal_are_not_the_same_as_missing_entries() {
    let mut empty = note();
    empty.author.clear();
    empty.text.clear();
    let with = book(&[("A1", empty)]);
    let without = book(&[]);
    assert_eq!(comment_diff(&with, &without, 20).diffs, 1);
    assert_eq!(comment_diff(&without, &with, 20).diffs, 1);
}

#[test]
fn report_caps_preserve_total_changed_comment_count() {
    let before = book(&[("A1", note()), ("B2", note()), ("C3", note())]);
    let mut changed = note();
    changed.text = "Changed".into();
    let after = book(&[("A1", changed.clone()), ("B2", changed.clone()), ("C3", changed)]);
    for limit in [0, 1, 20] {
        let category = comment_diff(&before, &after, limit);
        assert_eq!(category.diffs, 3);
        assert_eq!(category.examples.len(), limit.min(3));
    }
}
