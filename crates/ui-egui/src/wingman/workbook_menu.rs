//! Generated from the same sources as docs/wingman-commands.md. Data, not a dispatch table:
//! a control's native ID is never accepted from the model as an engine command.
use super::{context, ribbon};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, sync::OnceLock};

type Row = [String; 6]; // tab, group, label, ID, access, adapter
fn rows() -> Result<&'static [Row], String> {
    static INDEX: OnceLock<Result<Vec<Row>, serde_json::Error>> = OnceLock::new();
    INDEX
        .get_or_init(|| serde_json::from_str(include_str!("control-inventory.json")))
        .as_deref()
        .map_err(|e| format!("Invalid installed ribbon inventory: {e}"))
}
pub(in crate::wingman) fn sections() -> Result<&'static std::collections::BTreeSet<&'static str>, String> {
    static SECTIONS: OnceLock<std::collections::BTreeSet<&'static str>> = OnceLock::new();
    let entries = rows()?;
    Ok(SECTIONS.get_or_init(|| entries.iter().map(|r| r[0].as_str()).collect()))
}
pub(super) fn groups(tab: &str) -> Result<BTreeMap<&'static str, usize>, String> {
    let mut result = BTreeMap::new();
    for [section, group, ..] in rows()? {
        if section.eq_ignore_ascii_case(tab) {
            *result.entry(group.as_str()).or_default() += 1;
        }
    }
    Ok(result)
}
pub(super) fn group_tools(tab: &str, group: &str) -> Result<std::collections::BTreeSet<&'static str>, String> {
    let mut tools = std::collections::BTreeSet::new();
    for row in rows()?.iter().filter(|r| r[0].eq_ignore_ascii_case(tab) && r[1].eq_ignore_ascii_case(group) && !r[5].is_empty()) {
        tools.insert(row[5].as_str());
        if let Some((members, _)) = recipe(row) {
            tools.extend(members.iter().copied());
        }
    }
    Ok(tools)
}

/// An equivalent menu action can take several real controls. Expose the same members both
/// on opening its group (argument schemas) and in discovery (the ordered recipe).
fn recipe(row: &Row) -> Option<(&'static [&'static str], &'static str)> {
    if row[5] != "apply_edits" {
        return None;
    }
    Some(match (row[3].as_str(), row[2].as_str()) {
        ("home.formatAsTable", _) | ("", "Format as Table") => (
            &["create_table", "set_table_style"],
            "Use an explicit table name when creating. On an existing table use only set_table_style. For a gallery swatch use its displayed TableStyle name.",
        ),
        ("home.borders", _) => {
            (&["set_borders", "set_borders"], "Apply the top border then the bottom border, using the bottom line style named on the button.")
        }
        ("home.mergeCenter", _) => (&["merge_cells", "format_range"], "Merge the rectangle, then apply horizontal Center alignment."),
        ("home.mergeAcross", _) => {
            (&["merge_cells"], "Merge each row as a separate rectangle, not the entire selection. Use at most 16 actions per approval.")
        }
        _ => return None,
    })
}
pub(in crate::wingman) fn find(args: &Json) -> Result<Json, String> {
    let search = args["search"].as_str().unwrap_or("").trim().to_lowercase();
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let section = args["section"].as_str();
    let group = args["group"].as_str();
    let matches = rows()?.iter().filter(|row| {
        section.is_none_or(|s| row[0].eq_ignore_ascii_case(s))
            && group.is_none_or(|g| row[1].eq_ignore_ascii_case(g))
            && (search.is_empty() || row.iter().any(|s| s.to_lowercase().contains(&search)))
    });
    // Preserve menu order when browsing. For a query, surface the exact native button
    // or tool first; a match in a group caption should not bury it under an entire tab.
    let mut hits = matches.collect::<Vec<_>>();
    if !search.is_empty() {
        hits.sort_by_key(|row| {
            let score = if [&row[2], &row[3], &row[5]].iter().any(|s| s.eq_ignore_ascii_case(&search)) {
                0
            } else if row[2].to_lowercase().contains(&search) {
                1
            } else {
                2
            };
            (score, row[5].is_empty())
        });
    }
    Ok(context::page(
        hits.into_iter().skip(offset).map(|row| {
            let [tab,group,label,id,access,tool]=row;
            let native = gridcraft_engine::find_command(id);
            let adapter=super::lookup(tool).ok();
            json!({"section":tab,"group":group,"label":label,"command":(!id.is_empty()).then_some(id),
            "access":access,"wingmanTool":(!tool.is_empty()).then_some(tool),"toolTab":ribbon::tab(tool),
            "nativeParameters":native.map(|s|s.params),"toolArguments":adapter.map(|a|json!({"required":a.required,"optional":a.optional})),
            "recipe":recipe(row).map(|(members,hint)|json!({"hint":hint,"steps":members.iter().filter_map(|m|super::lookup(m).ok()).map(|t|json!({"tool":t.name,"required":t.required,"optional":t.optional})).collect::<Vec<_>>()}))})
        }),
        offset,
        16,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_compound_menu_supplies_its_component_schemas() {
        for row in rows().unwrap().iter().filter(|r| r[5] == "apply_edits") {
            let (members, _) = recipe(row).unwrap();
            let group = group_tools(&row[0], &row[1]).unwrap();
            assert!(members.iter().all(|t| group.contains(t) && super::super::lookup(t).is_ok()));
        }
    }
}
