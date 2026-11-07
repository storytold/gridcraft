//! Application-wide Custom Lists for AutoFill and Sort.

use gridcraft_core::{CellRef, Value};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, always, bad, cmd, has_doc, ok, target_ranges, target_sheet};

const MAX_ITEMS: usize = 10_000;
const MAX_LISTS: usize = 255;
const MAX_ITEM_BYTES: usize = 4_096;
const MAX_TOTAL_BYTES: usize = 1 << 20;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "options.customLists", "Custom Lists", ["File", "Options"], None, "{} → application Custom Lists", always, list),
        cmd!(noundo "options.addCustomList", "Add Custom List", [], None, "{items: [text, ...]}", always, add),
        cmd!(noundo "options.replaceCustomList", "Replace Custom List", [], None, "{index: 1-based, items: [text, ...]}", always, replace),
        cmd!(noundo "options.deleteCustomList", "Delete Custom List", [], None, "{index: 1-based}", always, delete),
        cmd!(noundo "options.importCustomList", "Import Custom List", [], None, "{range?: one row or column of text cells}", has_doc, import),
    ]
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!(s.prefs.custom_lists))
}

fn parse_items(p: &Json, cmd: &str) -> Result<Vec<String>> {
    let items = p.get("items").and_then(Json::as_array).ok_or_else(|| bad(cmd, "missing `items` array"))?;
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(bad(cmd, format!("a list must contain 1 to {MAX_ITEMS} items")));
    }
    let mut total = 0usize;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let raw_text = item.as_str().ok_or_else(|| bad(cmd, "list items must be text"))?;
        if raw_text.contains('\r') {
            return Err(bad(cmd, "list items may not contain carriage returns"));
        }
        let text = raw_text.trim();
        if text.is_empty() || text.len() > MAX_ITEM_BYTES {
            return Err(bad(cmd, format!("items must contain 1 to {MAX_ITEM_BYTES} bytes")));
        }
        total = total.saturating_add(text.len());
        if total > MAX_TOTAL_BYTES {
            return Err(bad(cmd, "the list is too large"));
        }
        out.push(text.to_string());
    }
    Ok(out)
}

fn add(s: &mut Session, p: &Json) -> Result<Json> {
    let items = parse_items(p, "options.addCustomList")?;
    check_collection(&s.prefs.custom_lists, None, &items, "options.addCustomList")?;
    s.prefs.custom_lists.push(items);
    Ok(json!({"index": s.prefs.custom_lists.len()}))
}

fn replace(s: &mut Session, p: &Json) -> Result<Json> {
    let index = index(s, p, "options.replaceCustomList")?;
    let items = parse_items(p, "options.replaceCustomList")?;
    check_collection(&s.prefs.custom_lists, Some(index), &items, "options.replaceCustomList")?;
    s.prefs.custom_lists[index] = items;
    ok()
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let index = index(s, p, "options.deleteCustomList")?;
    s.prefs.custom_lists.remove(index);
    ok()
}

fn index(s: &Session, p: &Json, cmd: &str) -> Result<usize> {
    let i = p
        .get("index")
        .and_then(Json::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| i.checked_sub(1))
        .ok_or_else(|| bad(cmd, "`index` must be a 1-based list index"))?;
    if i >= s.prefs.custom_lists.len() {
        return Err(bad(cmd, "no Custom List at that index"));
    }
    Ok(i)
}

fn check_collection(lists: &[Vec<String>], replacing: Option<usize>, items: &[String], cmd: &str) -> Result<()> {
    if replacing.is_none() && lists.len() >= MAX_LISTS {
        return Err(bad(cmd, format!("at most {MAX_LISTS} Custom Lists are supported")));
    }
    let existing = lists
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != replacing)
        .flat_map(|(_, list)| list)
        .fold(0usize, |total, item| total.saturating_add(item.len()));
    let new = items.iter().fold(0usize, |total, item| total.saturating_add(item.len()));
    if existing.saturating_add(new) > MAX_TOTAL_BYTES {
        return Err(bad(cmd, "saved Custom Lists may use at most 1 MiB of text"));
    }
    Ok(())
}

fn import(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    if ranges.len() != 1 {
        return Err(bad("options.importCustomList", "select one row or column"));
    }
    let range = ranges.first().copied().unwrap_or_default();
    if range.height() != 1 && range.width() != 1 {
        return Err(bad("options.importCustomList", "select one row or column"));
    }
    if range.count() > MAX_ITEMS as u64 {
        return Err(bad("options.importCustomList", format!("the selection may contain at most {MAX_ITEMS} cells")));
    }
    let sheet = target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let mut items = Vec::new();
    let mut total = 0usize;
    for row in range.start.row..=range.end.row {
        for col in range.start.col..=range.end.col {
            match sh.value(CellRef::new(row, col)) {
                Value::Empty => {}
                Value::Text(text) => {
                    let text = text.trim();
                    total = total.saturating_add(text.len());
                    if text.is_empty() || text.len() > MAX_ITEM_BYTES || total > MAX_TOTAL_BYTES {
                        return Err(bad("options.importCustomList", "the selected list contains an empty or oversized item"));
                    }
                    items.push(text.to_string());
                }
                _ => return Err(bad("options.importCustomList", "selected cells must contain text or be blank")),
            }
        }
    }
    let items = parse_items(&json!({"items": items}), "options.importCustomList")?;
    check_collection(&s.prefs.custom_lists, None, &items, "options.importCustomList")?;
    s.prefs.custom_lists.push(items);
    Ok(json!({"index": s.prefs.custom_lists.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_lists_can_be_added_replaced_deleted_and_imported() {
        let mut s = Session::new();
        s.execute("options.addCustomList", json!({"items": ["Bronze", "Silver", "Gold"]})).unwrap();
        assert_eq!(s.prefs.custom_lists[0], ["Bronze", "Silver", "Gold"]);
        s.execute("options.replaceCustomList", json!({"index": 1, "items": ["One", "Two"]})).unwrap();
        assert_eq!(s.prefs.custom_lists[0], ["One", "Two"]);
        assert!(s.execute("options.deleteCustomList", json!({"index": 2})).is_err());
        s.execute("options.deleteCustomList", json!({"index": 1})).unwrap();
        assert!(s.prefs.custom_lists.is_empty());
    }

    #[test]
    fn custom_list_inputs_are_bounded_and_validated() {
        let mut s = Session::new();
        for params in [json!({}), json!({"items": []}), json!({"items": [" "]}), json!({"items": [1]})] {
            assert!(s.execute("options.addCustomList", params).is_err());
        }
        assert!(s.execute("options.addCustomList", json!({"items": ["x".repeat(MAX_ITEM_BYTES + 1)]})).is_err());
        assert!(s.execute("options.addCustomList", json!({"items": ["line one\rline two"]})).is_err());
        s.execute("options.addCustomList", json!({"items": ["line one\nline two"]})).unwrap();
        assert_eq!(s.prefs.custom_lists[0], ["line one\nline two"]);
    }

    #[test]
    fn selected_text_can_be_imported_and_preferences_round_trip() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Bronze"], ["Silver"], ["Gold"]]})).unwrap();
        s.execute("options.importCustomList", json!({"range": "A1:A3"})).unwrap();
        let saved = serde_json::to_string(&s.prefs).unwrap();
        let restored: crate::Prefs = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.custom_lists, [["Bronze", "Silver", "Gold"].map(str::to_string).to_vec()]);
        let older: crate::Prefs = serde_json::from_str(r#"{"userName":"Ada"}"#).unwrap();
        assert!(older.custom_lists.is_empty());
    }
}
