use super::super::{context, object, string};
use gridcraft_engine::{
    DocState,
    core::{CellRef, MAX_ROWS, RangeRef, Value, col_to_letters},
    model::{Sheet, Table, features::CfRule},
};
use serde_json::{Value as Json, json};

pub(super) fn field(key: &str) -> Option<Json> {
    let choice = |values: &[&str]| json!({"type":"string","enum":values});
    Some(match key {
        "on" => json!({"type":"boolean"}),
        "headerName" => string(255),
        "tableStyle" => json!({"type":"string","description":"TableStyleLight1–21, TableStyleMedium1–28, or TableStyleDark1–11","maxLength":32}),
        "aggregation" => choice(&["sum", "average", "count", "countNums", "max", "min", "stdDev", "var", "none"]),
        "validationRule" => object(
            json!({
                "type":choice(&["list","whole","decimal","textLength","date","time"]),
                "entries":{"type":"array","minItems":1,"maxItems":128,"items":string(255)},
                "source":string(42),"operator":choice(&["between","notBetween","equal","notEqual","less","lessOrEqual","greater","greaterOrEqual"]),
                "value":{"type":"number"},"value2":{"type":"number"},"ignoreBlank":{"type":"boolean"},"dropdown":{"type":"boolean"},
                "inputTitle":string(32),"inputMessage":string(255),"errorTitle":string(32),"errorMessage":string(255),"errorStyle":choice(&["stop","warning","information"])
            }),
            &["type"],
        ),
        _ => return None,
    })
}

fn named<'a>(d: &'a DocState, sh: &Sheet, p: &Json) -> Result<&'a Table, String> {
    let (si, ti) = p["table"].as_str().and_then(|n| d.wb.table(n)).ok_or("No such table.")?;
    d.wb.sheet(si).filter(|s| s.uid == sh.uid).and_then(|s| s.tables.get(ti)).ok_or_else(|| "The table does not belong to that sheet.".into())
}
fn totals_ok(sh: &Sheet, t: &Table) -> Result<(), String> {
    if t.columns.len() != t.range.width() as usize {
        return Err("Table column metadata does not match the range.".into());
    }
    for (i, col) in t.columns.iter().enumerate() {
        let at = CellRef::new(t.range.end.row, t.range.start.col + i as u32);
        let Some(cell) = sh.cell(at).filter(|c| c.has_content()) else { continue };
        let native = if let Some(code) = col.totals.subtotal_code() {
            cell.formula.as_ref().is_some_and(|f| f.text == format!("SUBTOTAL({code},{}[{}])", t.name, col.name))
        } else {
            cell.formula.is_none() && matches!(&cell.value,Value::Text(v) if col.totals_label.as_deref()==Some(v.as_ref()))
        };
        if !native {
            return Err(format!("{} has a manually edited total. Move it before changing the native totals row.", at.a1()));
        }
    }
    Ok(())
}
pub(super) fn table(d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    let t = named(d, sh, p)?;
    if name == "table_total_function" || (name == "table_totals" && !t.totals_row && p["on"] == true) {
        // write_totals emits Table[Column] for every native SUBTOTAL. Imported headers using
        // brackets, @ or # can parse as structured *selectors*, or as another expression.
        for col in &t.columns {
            if col.name.contains(['[', ']', '#', '@', '\'']) {
                let text = format!("{}[{}]", t.name, col.name);
                if !matches!(gridcraft_engine::formula::parse(&text),Ok(gridcraft_engine::formula::Expr::Struct(s))
                    if s.table==t.name && s.specifiers.is_empty() && s.col_start.as_deref()==Some(col.name.as_str()) && s.col_end.is_none())
                {
                    return Err(format!("The native SUBTOTAL syntax cannot unambiguously address the header {}.", context::clip(&col.name, 128)));
                }
            }
        }
    }
    if name == "table_filter_buttons" && p["on"] == false && !t.filter.is_empty() {
        return Err("This table has active filter criteria; hiding the buttons would discard them.".into());
    }
    if name == "table_totals" && p["on"].as_bool() != Some(t.totals_row) {
        if t.totals_row {
            totals_ok(sh, t)?;
        } else {
            let row = t.range.end.row.checked_add(1).filter(|v| *v < MAX_ROWS).ok_or("No space below the table.")?;
            let r = RangeRef::new(CellRef::new(row, t.range.start.col), CellRef::new(row, t.range.end.col));
            if sh.cells.iter_range(r).next().is_some()
                || sh.tables.iter().any(|u| u.range.intersects(&r))
                || sh.merges.iter().any(|m| m.intersects(&r))
                || sh.spill_ranges.values().any(|m| m.intersects(&r))
                || sh.pivots.iter().any(|pv| r.contains(pv.anchor) || pv.last_range.is_some_and(|m| m.intersects(&r)))
                || sh.comments.range(r.start..=r.end).next().is_some()
                || sh.hyperlinks.range(r.start..=r.end).next().is_some()
                || sh.cell_pictures.range(r.start..=r.end).next().is_some()
            {
                return Err("Make room for the totals row below this table; existing data or layout would be overwritten.".into());
            }
        }
    }
    if name == "set_table_style" {
        let s = p["tableStyle"].as_str().ok_or("Missing gallery style name.")?;
        let valid = [("TableStyleLight", 21), ("TableStyleMedium", 28), ("TableStyleDark", 11)].into_iter().any(|(prefix, max)| {
            s.strip_prefix(prefix).is_some_and(|n| n.parse::<usize>().ok().is_some_and(|v| (1..=max).contains(&v) && v.to_string() == n))
        });
        if !valid {
            return Err("Use one of the Table Design gallery styles: Light1–21, Medium1–28 or Dark1–11 (prefix TableStyle).".into());
        }
        *p = json!({"table":t.name,"style":s});
    } else if name == "table_total_function" {
        let col = p["headerName"]
            .as_str()
            .and_then(|h| t.column_index(h))
            .and_then(|i| t.columns.get(i))
            .ok_or("No column with that header on this table.")?;
        if !t.totals_row {
            return Err("Enable the table totals row first.".into());
        }
        totals_ok(sh, t)?;
        *p = json!({"table":t.name,"column":col.name,"function":p["aggregation"]});
    } else {
        *p = json!({"table":t.name,"on":p["on"]});
    }
    Ok(())
}

fn threshold(v: &Json) -> Result<f64, String> {
    let n = v.as_f64().ok_or("A numeric rule needs a threshold.")?;
    let integer = v.as_i64().map(i128::from).or_else(|| v.as_u64().map(i128::from));
    // Decimal/exponent tokens already parsed as f64 have lost their spelling.
    // Outside the safe-integer range we cannot tell adjacent integers apart.
    if integer.map_or(n.abs() >= (1_u64 << 53) as f64, |exact| n as i128 != exact) {
        return Err("Threshold cannot be represented exactly as a worksheet number.".into());
    }
    Ok(n)
}

pub(super) fn validation(name: &str, sh: &Sheet, r: RangeRef, p: &mut Json) -> Result<(), String> {
    if name == "clear_validation" {
        *p = json!({"sheet":sh.name,"range":r.a1(),"clear":true});
        return Ok(());
    }
    let v = &p["validationRule"];
    let mut args = json!({"sheet":sh.name,"range":r.a1(),"type":v["type"]});
    for key in ["ignoreBlank", "inputTitle", "inputMessage", "errorTitle", "errorMessage", "errorStyle"] {
        if let Some(val) = v.get(key) {
            args[key] = val.clone();
        }
    }
    if v["type"] == "list" {
        if ["operator", "value", "value2"].iter().any(|k| v.get(k).is_some()) {
            return Err("List validation accepts entries or source, not numeric comparisons.".into());
        }
        let formula = match (v["entries"].as_array(), v["source"].as_str()) {
            (Some(entries), None) => {
                let items = entries
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .filter(|s| !s.trim().is_empty() && !s.contains([',', '\n', '\r', '"']))
                            .ok_or("An inline list needs plain nonempty entries; use a source range for commas or quotes.")
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                if items.encode_utf16().count() > 255 {
                    return Err("Inline validation list exceeds 255 characters; use a local source range.".into());
                }
                format!("\"{items}\"")
            }
            (None, Some(source)) => {
                let src = RangeRef::parse(source)
                    .filter(|r| (r.height() == 1 || r.width() == 1) && r.count() <= 100_000)
                    .ok_or("Use one local worksheet row or column for the list source.")?;
                let abs = |c: CellRef| format!("${}${}", col_to_letters(c.col), c.row + 1);
                format!("={}:{}", abs(src.start), abs(src.end))
            }
            _ => return Err("Choose exactly one of entries and source for the list.".into()),
        };
        args["formula1"] = json!(formula);
        if let Some(dropdown) = v.get("dropdown") {
            args["dropdown"] = dropdown.clone();
        }
    } else {
        if ["entries", "source", "dropdown"].iter().any(|k| v.get(k).is_some()) {
            return Err("Entries, source, and dropdown belong to list validation.".into());
        }
        let op = v["operator"].as_str().ok_or("A numeric rule needs an operator.")?;
        let f1 = threshold(&v["value"])?;
        let f2 = v.get("value2").map(threshold).transpose()?;
        if matches!(op, "between" | "notBetween") != f2.is_some() {
            return Err("Supply value2 only for between or notBetween; both bounds are required.".into());
        }
        if f2.is_some_and(|hi| hi < f1) {
            return Err("The upper bound is below the lower bound.".into());
        }
        if std::iter::once(f1).chain(f2).any(|n| match v["type"].as_str() {
            Some("whole") => n.fract() != 0.0,
            Some("textLength") => n < 0.0 || n.fract() != 0.0,
            Some("time") => !(0.0..1.0).contains(&n),
            _ => false,
        }) {
            return Err("Use bounds appropriate to this validation type.".into());
        }
        args["operator"] = json!(op);
        args["formula1"] = json!(v["value"].to_string());
        if let Some(bound) = v.get("value2") {
            args["formula2"] = json!(bound.to_string());
        }
    }
    *p = args;
    Ok(())
}

fn summary(rule: &CfRule) -> Json {
    let cut = |s: &str| context::clip(s, 255);
    match rule {
        CfRule::CellIs { op, a, b, .. } => json!({"type":"cellIs","operator":op,"value":cut(a),"value2":b.as_deref().map(cut)}),
        CfRule::Expression { formula, .. } => json!({"type":"expression","formula":cut(formula)}),
        CfRule::ContainsText { text, .. } => json!({"type":"containsText","text":cut(text)}),
        CfRule::NotContainsText { text, .. } => json!({"type":"notContainsText","text":cut(text)}),
        CfRule::BeginsWith { text, .. } => json!({"type":"beginsWith","text":cut(text)}),
        CfRule::EndsWith { text, .. } => json!({"type":"endsWith","text":cut(text)}),
        CfRule::Blanks { .. } => json!({"type":"blanks"}),
        CfRule::NoBlanks { .. } => json!({"type":"noBlanks"}),
        CfRule::Errors { .. } => json!({"type":"errors"}),
        CfRule::NoErrors { .. } => json!({"type":"noErrors"}),
        CfRule::Duplicate { .. } => json!({"type":"duplicate"}),
        CfRule::Unique { .. } => json!({"type":"unique"}),
        CfRule::Top10 { rank, bottom, percent, .. } => json!({"type":"top10","rank":rank,"bottom":bottom,"percent":percent}),
        CfRule::AboveAverage { below, equal, std_dev, .. } => json!({"type":"aboveAverage","below":below,"equal":equal,"stdDev":std_dev}),
        CfRule::TimePeriod { period, .. } => json!({"type":"timePeriod","period":cut(period)}),
        CfRule::ColorScale { stops } => json!({"type":"colorScale","colors":stops.iter().take(3).map(|s|s.1).collect::<Vec<_>>()}),
        CfRule::DataBar { color, gradient, show_value, .. } => json!({"type":"dataBar","color":color,"gradient":gradient,"showValue":show_value}),
        CfRule::IconSet { set, reverse, show_value, .. } => json!({"type":"iconSet","set":cut(set),"reverse":reverse,"showValue":show_value}),
    }
}

pub(in super::super) fn read(d: &DocState, name: &str, p: &Json) -> Result<Json, String> {
    let sh = context::sheet(d, p)?;
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    if name == "inspect_table" {
        let t = named(d, sh, p)?;
        let cols=context::page(t.columns.iter().skip(offset).map(|col|json!({"name":context::clip(&col.name,255),"totals":col.totals,"totalsLabel":col.totals_label.as_deref().map(|s|context::clip(s,255))})),offset,32);
        return Ok(
            json!({"sheet":sh.name,"table":t.name,"range":t.range.a1(),"style":t.style,"headerRow":t.header_row,"filterButtons":t.filter_button,"filters":t.filter.len(),
            "bandedRows":t.banded_rows,"bandedColumns":t.banded_cols,"firstColumn":t.first_col,"lastColumn":t.last_col,"totalsRow":t.totals_row,"columns":cols}),
        );
    }
    let r =
        p["range"].as_str().and_then(RangeRef::parse).filter(|r| r.count() <= 100_000).ok_or("Supply an A1 rectangle of at most 100,000 cells.")?;
    enum Area<'a> {
        Format(&'a gridcraft_engine::model::features::CondFormat),
        Validation(&'a gridcraft_engine::model::features::Validation),
    }
    let formats = sh.cond_formats.iter().flat_map(|cf| cf.ranges.iter().map(move |a| (a, Area::Format(cf))));
    let validations = sh.validations.iter().flat_map(|v| v.ranges.iter().map(move |a| (a, Area::Validation(v))));
    let entries=formats.chain(validations).filter(|(a,_)|a.intersects(&r)).skip(offset).map(|(area,rule)|match rule{
        Area::Format(cf) if sh.is_protected()=>json!({"kind":"conditionalFormat","range":area.a1(),"priority":cf.priority,"detailsHidden":true}),
        Area::Validation(v) if sh.is_protected()=>json!({"kind":"validation","range":area.a1(),"type":v.kind,"detailsHidden":true}),
        Area::Format(cf)=>json!({"kind":"conditionalFormat","range":area.a1(),"priority":cf.priority,"stopIfTrue":cf.stop_if_true,"rule":summary(&cf.rule)}),
        Area::Validation(v)=>json!({"kind":"validation","range":area.a1(),"type":v.kind,"operator":v.op,"formula1":context::clip(&v.f1,255),
            "formula2":v.f2.as_deref().map(|s|context::clip(s,255)),"ignoreBlank":v.allow_blank,"dropdown":v.in_cell_dropdown,"errorStyle":v.error_style}),
    });
    Ok(json!({"sheet":sh.name,"range":r.a1(),"page":context::page(entries,offset,32)}))
}
