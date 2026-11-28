//! Static metadata only: never open a workbook, start the UI, dispatch commands or probe model tools.
#[path = "../../crates/ui-egui/src/wingman/workbook_routes.rs"]
mod routes;
mod surface;

use gridcraft_engine::{catalog::CATALOG, command_specs};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

type Row = [String; 6];
type Rows = BTreeSet<Row>;
#[derive(Default)]
struct Inventory {
    rows: Rows,
    adapters: BTreeMap<String, BTreeSet<String>>,
    ids: BTreeSet<String>,
}

impl Inventory {
    fn add(&mut self, tab: &str, group: &str, label: &str, id: &str) {
        let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty() || label == "-" {
            return;
        }
        let adapter = self.adapters.get(id).and_then(|set| set.first()).map(String::as_str);
        let (access, tool) = match (id, group, label.as_str()) {
            ("data.goalSeek" | "dialog:goalSeek", _, _) => ("inspect", Some("preview_goal_seek")),
            (_, _, "Remove Duplicates") if tab == "Table Design" => ("guide", None),
            ("table.removeDuplicates", _, _) => ("guide", None),
            ("dialog:removeDuplicates", _, _) => ("equivalent", Some("remove_duplicates")),
            ("home.conditionalFormat", _, "Data Bars") => ("direct", Some("data_bars")),
            ("home.conditionalFormat", _, "Find Conditional Formatting") | ("edit.goToSpecial", _, "Conditional Formatting" | "Data Validation") => {
                ("inspect", Some("inspect_rules"))
            }
            ("edit.goToSpecial", _, "Notes") => ("inspect", Some("list_objects")),
            ("edit.goToSpecial", _, "Formulas") => ("inspect", Some("grep")),
            // Some menu items send the same ID with incompatible fixed parameters. Never
            // choose an alphabetically first adapter (e.g. clear instead of set validation).
            ("data.validation", _, "Clear Validation") => ("direct", Some("clear_validation")),
            ("data.validation", _, _) => ("direct", Some("set_validation")),
            ("dialog:comment", _, v) if v.contains("Note") => ("equivalent", Some("set_note")),
            ("dialog:comment", _, _) => ("equivalent", Some("set_comment")),
            ("dialog:macroEditor", _, "New Macro" | "Create") => ("equivalent", Some("save_script")),
            ("dialog:macroEditor", _, "Edit") => ("equivalent", Some("edit_macro_draft")),
            ("chart.addElement", _, v) if v.starts_with("Data Table:") => ("guide", None),
            ("chart.set", _, "Horizontal Borders" | "Vertical Borders" | "Outline" | "Show Legend Keys") => ("guide", None),
            ("chart.set", _, "Chart Styles") => ("inspect", Some("inspect_chart")),
            ("home.borders", _, "Top and Double Bottom Border" | "Top and Thick Bottom Border") => ("equivalent", Some("apply_edits")),
            ("", _, "Add Chart Element" | "Format Pane") if tab == "Chart Design" => ("inspect", Some("inspect_chart")),
            ("", _, "Change Chart Type") if tab == "Chart Design" => ("equivalent", Some("update_chart")),
            ("", _, "More Table Styles") if tab == "Table Design" => ("equivalent", Some("set_table_style")),
            ("", _, "Data Validation") if tab == "Data" => ("equivalent", Some("set_validation")),
            ("", _, "Data Bars") if tab == "Home" => ("equivalent", Some("data_bars")),
            ("", _, "Icon Sets" | "Color Scales" | "Top/Bottom Rules" | "Highlight Cells Rules") if tab == "Home" => {
                ("equivalent", Some("conditional_format"))
            }
            ("", _, "Clear Rules") if tab == "Home" => ("equivalent", Some("clear_format_rules")),
            ("", _, "Format as Table") if tab == "Home" => ("equivalent", Some("apply_edits")),
            ("", _, "Trace Precedents" | "Trace Dependents" | "Trace Error") if tab == "Formulas" => ("inspect", Some("inspect_formula")),
            ("", _, "Error Checking" | "Circular References") if tab == "Formulas" => ("inspect", Some("formula_diagnostics")),
            ("", _, "AutoSum") if tab == "Formulas" => ("equivalent", Some("set_cell")),
            ("", _, "Notes") if tab == "Review" => ("inspect", Some("list_objects")),
            ("table.style", "Keytips", l) if l.starts_with("New ") => ("guide", None),
            ("home.formatAsTable", g, _) if g.starts_with("Format as Table / ") && self.adapters.contains_key("table.style") => {
                ("equivalent", Some("apply_edits"))
            }
            ("home.conditionalFormat", group, _) if group.ends_with("Data Bars") => ("direct", Some("data_bars")),
            ("home.conditionalFormat", group, _) if group.ends_with("Icon Sets") && adapter != Some("conditional_format") => ("guide", None),
            ("dialog:cfQuick", _, _) if self.adapters.contains_key("home.conditionalFormat") => ("equivalent", Some("conditional_format")),
            ("home.clearRules", _, "Clear Rules from Entire Sheet") => ("guide", None),
            ("edit.pasteSpecial", _, "Transpose") => ("equivalent", Some("transfer_range")),
            ("", _, _) => ("guide", None),
            _ => routes::route(id, adapter),
        };
        self.rows.insert([tab.into(), group.into(), label, id.into(), access.into(), tool.unwrap_or_default().into()]);
    }
}

fn markdown(v: &str) -> String {
    v.replace('\\', "\\\\").replace('|', "\\|").replace('<', "&lt;").replace('>', "&gt;").replace('`', "&#96;")
}

fn doc(rows: &Rows) -> String {
    let mut s = String::from(
        "# Wingman command and control index\n\nGenerated by `cargo xtask wingman-commands`. The desktop assistant uses the same index for command discovery.\n\n**Direct** names a scoped native tool, not permission to supply arbitrary engine arguments. **Equivalent** achieves the worksheet result through another tool; it does not press the control. **Inspect** reads related workbook state. **Guide** leaves the action to the user. Desktop dialogs, OS clipboard, network, and host operations are not exposed merely by listing them. Dynamic choices such as workbook names, installed fonts and file picker entries depend on the active document or host.\n",
    );
    s.push_str("\nA few controls share an engine command but have different routes:\n\n\
        - **Data Validation** sets a rule; **Clear Validation** removes one. Clearing a rule is never a substitute for creating it.\n\
        - **Text to Columns** can split a literal delimiter into a specified destination via `reshape_delimited`; the interactive fixed-width wizard remains manual.\n\
        - **Copy / Cut** can transfer between explicit worksheet ranges without using the system clipboard. **Paste** from another application is manual.\n\
        - **Format as Table** uses `create_table` and, if a gallery style was requested, `set_table_style`. Existing tables need only the latter. `apply_edits` can group them into one approval and Undo.\n\
        - **Draw pens** map to an explicit ink path, color and width rather than toggling the pointer. **Lasso Select** can inspect identified shapes; it does not select them for the user.\n\
        - **Edit Macro** can edit an open native macro draft; **Step Into**, execution, trust and imported VBA binaries remain outside that tool.\n");
    let mut counts = BTreeMap::<&str, [usize; 4]>::new();
    for [section, _, _, _, access, _] in rows {
        if let Some(i) = ["direct", "equivalent", "inspect", "guide"].iter().position(|key| key == access) {
            counts.entry(section).or_default()[i] += 1;
        }
    }
    s += &format!(
        "\n## Coverage by section\n\n{} entries in {} sections. A command can appear in more than one menu.\n\n| Section | Direct | Equivalent | Inspect | Guide |\n|---|---:|---:|---:|---:|\n",
        rows.len(),
        counts.len()
    );
    for (section, [direct, equivalent, inspect, guide]) in counts {
        s += &format!("| {} | {direct} | {equivalent} | {inspect} | {guide} |\n", markdown(section));
    }
    let mut last = String::new();
    for [tab, group, label, id, access, tool] in rows {
        if *tab != last {
            s += &format!("\n## {}\n\n| Group / menu | Control | Command | Access | Wingman tool |\n|---|---|---|---|---|\n", markdown(tab));
            last = tab.clone();
        }
        let code = |s: &str| if s.is_empty() { "—".into() } else { format!("`{}`", markdown(s)) };
        s += &format!("| {} | {} | {} | {} | {} |\n", markdown(group), markdown(label), code(id), access, code(tool));
    }
    s
}

fn emit(path: &Path, value: &str, check: bool) -> Result<(), String> {
    if check {
        let old = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !crate::parity::up_to_date(&old, value) {
            return Err(format!("{} is stale; run cargo xtask wingman-commands", path.display()));
        }
    } else {
        fs::write(path, value).map_err(|e| format!("{}: {e}", path.display()))?
    }
    Ok(())
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    if args.iter().any(|a| *a != "--check") {
        return Err("usage: cargo xtask wingman-commands [--check]".into());
    }
    let check = args.contains(&"--check");
    let ui = root.join("crates/ui-egui/src");
    let mut index = Inventory::default();
    surface::adapters(&ui.join("wingman"), &mut index)?;
    for c in command_specs() {
        index.ids.insert(c.id.into());
    }
    for &(id, t, g, name) in CATALOG {
        index.ids.insert(id.into());
        index.add(t, g, name, id)
    }
    for c in command_specs() {
        if !CATALOG.iter().any(|x| x.0 == c.id) {
            index.add(c.menu.first().unwrap_or(&"Workbook"), c.menu.get(1..).unwrap_or_default().join(" / ").as_str(), c.label, c.id);
        }
    }
    surface::controls(&ui, &mut index)?;
    // The thumbnails are painted at runtime; their names are static upstream metadata.
    for (name, _) in gridcraft_engine::math::GALLERY {
        index.add("Insert", "Equation gallery", name, "insert.equation");
    }
    for style in gridcraft_engine::cmd::wordart::STYLES {
        index.add("Insert", "Decorative Text gallery", style.name, "insert.wordArt");
    }
    let keytips: Value =
        serde_json::from_str(&fs::read_to_string(ui.join("keytips/keytips.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for k in keytips["sequences"].as_array().ok_or("missing keytip sequences")? {
        let (Some(id), Some(label)) = (k["target"].as_str(), k["excel"].as_str()) else { continue };
        if k["pending"] == true || k["kind"] == "tab" {
            continue;
        }
        let tab = CATALOG.iter().find(|r| r.0 == id).map_or("Keyboard", |r| r.1);
        index.add(tab, "Keytips", label, id);
    }
    // A split-button and its handler can expose the same label; keep the associated
    // command in preference to the pass which saw only the widget's title.
    let bound: BTreeSet<_> = index.rows.iter().filter(|r| !r[3].is_empty()).map(|r| (r[0].clone(), r[1].clone(), r[2].clone())).collect();
    index.rows.retain(|r| !r[3].is_empty() || !bound.contains(&(r[0].clone(), r[1].clone(), r[2].clone())));
    let mut data = String::from("[\n");
    for (i, row) in index.rows.iter().enumerate() {
        if i > 0 {
            data += ",\n";
        }
        data += &serde_json::to_string(row).map_err(|e| e.to_string())?;
    }
    data += "\n]\n";
    emit(&ui.join("wingman/control-inventory.json"), &data, check)?;
    emit(&root.join("docs/wingman-commands.md"), &doc(&index.rows), check)?;
    println!("{} Wingman discovery entries", index.rows.len());
    Ok(())
}
