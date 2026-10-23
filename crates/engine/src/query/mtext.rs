//! A readable M-like rendering of a query, for the Power Query Editor's formula bar and Advanced
//! Editor. It is generated from the typed steps for transparency; GridCraft never parses or runs
//! it (there is no M interpreter).

use std::fmt::Write as _;

use gridcraft_model::query::{
    AggOp, ColType, FilterCondition, FilterOp, JoinKind, Literal, Output, Query, QuerySource, SplitMode, StepOp, TextFn, WorkbookItemKind,
};

const KEYWORDS: &[&str] = &[
    "and",
    "as",
    "each",
    "else",
    "error",
    "false",
    "if",
    "in",
    "is",
    "let",
    "meta",
    "not",
    "null",
    "or",
    "otherwise",
    "section",
    "shared",
    "then",
    "true",
    "try",
    "type",
];

/// An M string literal.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\"\""),
            '\r' => out.push_str("#(cr)"),
            '\n' => out.push_str("#(lf)"),
            '\t' => out.push_str("#(tab)"),
            c if c.is_control() => {
                let _ = write!(out, "#({:04x})", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An identifier: plain when it can be, else `#"quoted"`.
pub fn ident(name: &str) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&name);
    if plain { name.to_string() } else { format!("#{}", string(name)) }
}

fn list(items: &[String]) -> String {
    format!("{{{}}}", items.iter().map(|s| string(s)).collect::<Vec<_>>().join(", "))
}

fn literal(l: Option<&Literal>) -> String {
    match l {
        None => "null".into(),
        Some(Literal::Bool(b)) => b.to_string(),
        Some(Literal::Number(n)) => gridcraft_core::number_to_text(*n),
        Some(Literal::Text(t)) => string(t),
    }
}

fn type_name(t: ColType) -> &'static str {
    match t {
        ColType::Any => "type any",
        ColType::Text => "type text",
        ColType::Number => "type number",
        ColType::Currency => "Currency.Type",
        ColType::Whole => "Int64.Type",
        ColType::Percentage => "Percentage.Type",
        ColType::Date => "type date",
        ColType::DateTime => "type datetime",
        ColType::Time => "type time",
        ColType::Logical => "type logical",
    }
}

fn file(path: &str) -> String {
    format!("File.Contents({})", string(path))
}

fn encoding(e: Option<&str>) -> &'static str {
    match e.map(str::to_ascii_lowercase).as_deref() {
        Some("utf-16" | "utf-16le" | "1200") => "1200",
        Some("utf-16be" | "1201") => "1201",
        Some("windows-1252" | "1252" | "latin1" | "iso-8859-1" | "cp1252") => "1252",
        _ => "65001",
    }
}

/// The Source step's expression.
pub fn source(src: &QuerySource) -> String {
    match src {
        QuerySource::Csv { path, delimiter, encoding: enc } => format!(
            "Csv.Document({},[Delimiter={}, Encoding={}, QuoteStyle=QuoteStyle.Csv])",
            file(path),
            string(delimiter.as_deref().unwrap_or(",")),
            encoding(enc.as_deref())
        ),
        QuerySource::Text { text, delimiter } => {
            format!("Csv.Document({},[Delimiter={}])", string(text), string(delimiter.as_deref().unwrap_or(",")))
        }
        QuerySource::Workbook { path, item, item_kind } => {
            let kind = match item_kind {
                WorkbookItemKind::Sheet => "Sheet",
                WorkbookItemKind::Table => "Table",
                WorkbookItemKind::DefinedName => "DefinedName",
            };
            format!("Excel.Workbook({}, null, true){{[Item={},Kind=\"{kind}\"]}}[Data]", file(path), string(item))
        }
        QuerySource::Xml { path, item } => match item {
            Some(i) => format!("Xml.Tables({}){{[Name={}]}}[Table]", file(path), string(i.rsplit('/').next().unwrap_or(i))),
            None => format!("Xml.Tables({})", file(path)),
        },
        QuerySource::Json { path, item } => {
            let mut s = format!("Json.Document({})", file(path));
            for part in item.iter().flat_map(|i| i.split('.')).filter(|p| !p.is_empty()) {
                let _ = write!(s, "[{}]", ident(part));
            }
            s
        }
        QuerySource::Folder { path, combine, extension } => {
            let files = format!("Folder.Files({})", string(path));
            if *combine {
                let ext = extension.as_deref().unwrap_or("");
                format!(
                    "Table.Combine(Table.SelectRows({files}, each [Extension] = {})[Content])",
                    string(&format!(".{}", ext.trim_start_matches('.')))
                )
            } else {
                files
            }
        }
        QuerySource::Web { url, item } => match item.as_deref().filter(|i| !i.is_empty()) {
            Some(i) if i.starts_with("Table ") => {
                let n = i.trim_start_matches("Table ").trim().parse::<usize>().unwrap_or(1).saturating_sub(1);
                format!("Web.Page(Web.Contents({})){{{n}}}[Data]", string(url))
            }
            Some(i) => format!("Json.Document(Web.Contents({}))[{}]", string(url), ident(i)),
            None => format!("Web.Contents({})", string(url)),
        },
        QuerySource::OData { url, item } => match item.as_deref().filter(|i| !i.is_empty()) {
            Some(i) => format!("OData.Feed({}){{[Name={}]}}[Data]", string(url), string(i)),
            None => format!("OData.Feed({})", string(url)),
        },
        QuerySource::Odbc { connection, table, sql } => match (sql.as_deref().filter(|s| !s.trim().is_empty()), table) {
            (Some(q), _) => format!("Odbc.Query({}, {})", string(connection), string(q)),
            (None, Some(t)) => format!("Odbc.DataSource({}){{[Name={}]}}[Data]", string(connection), string(t)),
            (None, None) => format!("Odbc.DataSource({})", string(connection)),
        },
        QuerySource::CurrentWorkbook { name } => format!("Excel.CurrentWorkbook(){{[Name={}]}}[Content]", string(name)),
        QuerySource::Query { name } => ident(name),
        QuerySource::Blank => "#table({}, {})".into(),
    }
}

/// A field access `[Name]` (a generalized identifier: spaces allowed), quoted when it must be.
fn field(col: &str) -> String {
    if col.is_empty() || col.contains(['[', ']', '"']) || col.starts_with(' ') || col.ends_with(' ') || col.chars().any(char::is_control) {
        format!("[#{}]", string(col))
    } else {
        format!("[{col}]")
    }
}

fn condition(col: &str, c: &FilterCondition) -> String {
    let field = field(col);
    let v = literal(c.value.as_ref());
    match c.op {
        FilterOp::Equals => format!("{field} = {v}"),
        FilterOp::NotEquals => format!("{field} <> {v}"),
        FilterOp::BeginsWith => format!("Text.StartsWith({field}, {v})"),
        FilterOp::NotBeginsWith => format!("not Text.StartsWith({field}, {v})"),
        FilterOp::EndsWith => format!("Text.EndsWith({field}, {v})"),
        FilterOp::NotEndsWith => format!("not Text.EndsWith({field}, {v})"),
        FilterOp::Contains => format!("Text.Contains({field}, {v})"),
        FilterOp::NotContains => format!("not Text.Contains({field}, {v})"),
        FilterOp::GreaterThan => format!("{field} > {v}"),
        FilterOp::GreaterOrEqual => format!("{field} >= {v}"),
        FilterOp::LessThan => format!("{field} < {v}"),
        FilterOp::LessOrEqual => format!("{field} <= {v}"),
        FilterOp::IsNull => format!("{field} = null"),
        FilterOp::IsNotNull => format!("{field} <> null"),
    }
}

fn output(o: &Output) -> String {
    match &o.column {
        Some(c) => field(c),
        None => literal(o.value.as_ref()),
    }
}

/// A step's expression, given the previous step's name.
pub fn step(op: &StepOp, prev: &str) -> String {
    let p = ident(prev);
    match op {
        StepOp::PromoteHeaders => format!("Table.PromoteHeaders({p}, [PromoteAllScalars=true])"),
        StepOp::DemoteHeaders => format!("Table.DemoteHeaders({p})"),
        StepOp::ChangeType { columns, locale } => {
            let pairs: Vec<String> = columns.iter().map(|c| format!("{{{}, {}}}", string(&c.column), type_name(c.col_type))).collect();
            match locale {
                Some(l) => format!("Table.TransformColumnTypes({p}, {{{}}}, {})", pairs.join(", "), string(l)),
                None => format!("Table.TransformColumnTypes({p},{{{}}})", pairs.join(", ")),
            }
        }
        StepOp::ChooseColumns { columns } => format!("Table.SelectColumns({p},{})", list(columns)),
        StepOp::RemoveColumns { columns } => format!("Table.RemoveColumns({p},{})", list(columns)),
        StepOp::RenameColumns { renames } => {
            let pairs: Vec<String> = renames.iter().map(|r| format!("{{{}, {}}}", string(&r.from), string(&r.to))).collect();
            format!("Table.RenameColumns({p},{{{}}})", pairs.join(", "))
        }
        StepOp::ReorderColumns { columns } => format!("Table.ReorderColumns({p},{})", list(columns)),
        StepOp::DuplicateColumn { column, name } => format!("Table.DuplicateColumn({p}, {}, {})", string(column), string(name)),
        StepOp::KeepTopRows { count } => format!("Table.FirstN({p},{count})"),
        StepOp::KeepBottomRows { count } => format!("Table.LastN({p}, {count})"),
        StepOp::KeepRange { first, count } => format!("Table.Range({p},{},{count})", first.saturating_sub(1)),
        StepOp::RemoveTopRows { count } => format!("Table.Skip({p},{count})"),
        StepOp::RemoveBottomRows { count } => format!("Table.RemoveLastN({p},{count})"),
        StepOp::RemoveDuplicates { columns } if columns.is_empty() => format!("Table.Distinct({p})"),
        StepOp::RemoveDuplicates { columns } => format!("Table.Distinct({p}, {})", list(columns)),
        StepOp::RemoveBlankRows => {
            format!("Table.SelectRows({p}, each not List.IsEmpty(List.RemoveMatchingItems(Record.FieldValues(_), {{\"\", null}})))")
        }
        StepOp::RemoveErrors { columns } if columns.is_empty() => format!("Table.RemoveRowsWithErrors({p})"),
        StepOp::RemoveErrors { columns } => format!("Table.RemoveRowsWithErrors({p}, {})", list(columns)),
        StepOp::FilterRows { column, values, conditions, or } => {
            let mut parts = Vec::new();
            if let Some(vals) = values {
                let any: Vec<String> = vals.iter().map(|v| condition(column, &FilterCondition { op: FilterOp::Equals, value: v.clone() })).collect();
                parts.push(if any.len() == 1 { any.join("") } else { format!("({})", any.join(" or ")) });
            }
            if !conditions.is_empty() {
                let cs: Vec<String> = conditions.iter().map(|c| condition(column, c)).collect();
                let joined = cs.join(if *or { " or " } else { " and " });
                parts.push(if cs.len() > 1 && parts.is_empty() {
                    joined
                } else if cs.len() > 1 {
                    format!("({joined})")
                } else {
                    joined
                });
            }
            let body = if parts.is_empty() { "true".to_string() } else { parts.join(" and ") };
            format!("Table.SelectRows({p}, each {body})")
        }
        StepOp::Sort { keys } => {
            let ks: Vec<String> = keys
                .iter()
                .map(|k| format!("{{{}, {}}}", string(&k.column), if k.descending { "Order.Descending" } else { "Order.Ascending" }))
                .collect();
            format!("Table.Sort({p},{{{}}})", ks.join(", "))
        }
        StepOp::ReplaceValues { columns, find, replace, whole } => {
            let text_find = matches!(find, Some(Literal::Text(_)));
            let replacer = if text_find && !whole { "Replacer.ReplaceText" } else { "Replacer.ReplaceValue" };
            format!("Table.ReplaceValue({p},{},{},{replacer},{})", literal(find.as_ref()), literal(replace.as_ref()), list(columns))
        }
        StepOp::SplitColumn { column, delimiter, mode, count } => {
            let splitter = match mode {
                SplitMode::Each => format!("Splitter.SplitTextByDelimiter({}, QuoteStyle.Csv)", string(delimiter)),
                SplitMode::LeftMost => format!("Splitter.SplitTextByEachDelimiter({{{}}}, QuoteStyle.Csv, false)", string(delimiter)),
                SplitMode::RightMost => format!("Splitter.SplitTextByEachDelimiter({{{}}}, QuoteStyle.Csv, true)", string(delimiter)),
            };
            let n = match mode {
                SplitMode::Each => count.map(|c| c as usize),
                _ => Some(2),
            };
            match n {
                Some(n) => {
                    let names: Vec<String> = (1..=n.min(64)).map(|k| format!("{column}.{k}")).collect();
                    format!("Table.SplitColumn({p}, {}, {}, {})", string(column), splitter, list(&names))
                }
                None => format!("Table.SplitColumn({p}, {}, {})", string(column), splitter),
            }
        }
        StepOp::TransformText { columns, function } => {
            let f = match function {
                TextFn::Lower => "Text.Lower",
                TextFn::Upper => "Text.Upper",
                TextFn::Proper => "Text.Proper",
                TextFn::Trim => "Text.Trim",
                TextFn::Clean => "Text.Clean",
            };
            let pairs: Vec<String> = columns.iter().map(|c| format!("{{{}, {f}, type text}}", string(c))).collect();
            format!("Table.TransformColumns({p},{{{}}})", pairs.join(", "))
        }
        StepOp::FillDown { columns } => format!("Table.FillDown({p},{})", list(columns)),
        StepOp::FillUp { columns } => format!("Table.FillUp({p},{})", list(columns)),
        StepOp::GroupBy { keys, aggregations } => {
            let aggs: Vec<String> = aggregations
                .iter()
                .map(|a| {
                    let col = a.column.as_deref().map(field).unwrap_or_default();
                    let (f, t) = match a.op {
                        AggOp::Count => ("Table.RowCount(_)".to_string(), "Int64.Type"),
                        AggOp::CountDistinct => (format!("List.Count(List.Distinct({col}))"), "Int64.Type"),
                        AggOp::Sum => (format!("List.Sum({col})"), "type number"),
                        AggOp::Average => (format!("List.Average({col})"), "type number"),
                        AggOp::Median => (format!("List.Median({col})"), "type number"),
                        AggOp::Min => (format!("List.Min({col})"), "type any"),
                        AggOp::Max => (format!("List.Max({col})"), "type any"),
                    };
                    format!("{{{}, each {f}, {t}}}", string(&a.name))
                })
                .collect();
            format!("Table.Group({p}, {}, {{{}}})", list(keys), aggs.join(", "))
        }
        StepOp::Unpivot { columns, attribute, value } => format!("Table.Unpivot({p}, {}, {}, {})", list(columns), string(attribute), string(value)),
        StepOp::Transpose => format!("Table.Transpose({p})"),
        StepOp::MergeQueries { query, left, right, join, columns, prefix } => {
            let kind = match join {
                JoinKind::LeftOuter => "JoinKind.LeftOuter",
                JoinKind::RightOuter => "JoinKind.RightOuter",
                JoinKind::FullOuter => "JoinKind.FullOuter",
                JoinKind::Inner => "JoinKind.Inner",
                JoinKind::LeftAnti => "JoinKind.LeftAnti",
                JoinKind::RightAnti => "JoinKind.RightAnti",
            };
            let nested = format!("Table.NestedJoin({p}, {}, {}, {}, {}, {kind})", list(left), ident(query), list(right), string(query));
            let new_names: Vec<String> = columns.iter().map(|c| prefix.as_deref().map_or(c.clone(), |pre| format!("{pre}.{c}"))).collect();
            format!("Table.ExpandTableColumn({nested}, {}, {}, {})", string(query), list(columns), list(&new_names))
        }
        StepOp::AppendQueries { queries } => {
            let mut all = vec![p];
            all.extend(queries.iter().map(|q| ident(q)));
            format!("Table.Combine({{{}}})", all.join(", "))
        }
        StepOp::AddIndexColumn { name, start, step } => format!("Table.AddIndexColumn({p}, {}, {start}, {step}, Int64.Type)", string(name)),
        StepOp::AddConditionalColumn { name, rules, otherwise } => {
            let mut body = String::new();
            for (i, r) in rules.iter().enumerate() {
                let cond = condition(&r.column, &FilterCondition { op: r.op, value: r.value.clone() });
                let _ = write!(body, "{}if {cond} then {} ", if i == 0 { "" } else { "else " }, output(&r.output));
            }
            let _ = write!(body, "else {}", output(otherwise));
            if rules.is_empty() {
                body = output(otherwise);
            }
            format!("Table.AddColumn({p}, {}, each {body})", string(name))
        }
    }
}

/// The step names a query shows in Applied Steps: `Source`, then each step's own.
pub fn step_names(q: &Query) -> Vec<String> {
    let mut names = vec!["Source".to_string()];
    for s in &q.steps {
        names.push(if s.name.is_empty() { s.op.default_name().to_string() } else { s.name.clone() });
    }
    names
}

/// The formula bar's text for applied step `i` (0 = Source).
pub fn step_formula(q: &Query, i: usize) -> Option<String> {
    let names = step_names(q);
    if i == 0 {
        return Some(format!("= {}", source(&q.source)));
    }
    let s = q.steps.get(i - 1)?;
    Some(format!("= {}", step(&s.op, names.get(i - 1)?)))
}

/// The whole query as an M `let` expression (the Advanced Editor's text).
pub fn query(q: &Query) -> String {
    let names = step_names(q);
    let mut out = String::from("let\n");
    let _ = write!(out, "    Source = {}", source(&q.source));
    for (i, s) in q.steps.iter().enumerate() {
        let prev = names.get(i).map(String::as_str).unwrap_or("Source");
        let name = names.get(i + 1).map(String::as_str).unwrap_or("Step");
        let _ = write!(out, ",\n    {} = {}", ident(name), step(&s.op, prev));
    }
    let _ = write!(out, "\nin\n    {}", ident(names.last().map(String::as_str).unwrap_or("Source")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_model::query::{ColumnType, QueryStep};

    #[test]
    fn query_renders_as_a_let_expression() {
        let mut q = Query::new("Sales", QuerySource::Csv { path: "C:\\data\\sales \"q1\".csv".into(), delimiter: None, encoding: None });
        q.steps = vec![
            QueryStep { name: "Promoted Headers".into(), op: StepOp::PromoteHeaders },
            QueryStep {
                name: "Changed Type".into(),
                op: StepOp::ChangeType { columns: vec![ColumnType { column: "Units".into(), col_type: ColType::Whole }], locale: None },
            },
            QueryStep {
                name: "Filtered Rows".into(),
                op: StepOp::FilterRows {
                    column: "Region".into(),
                    values: None,
                    conditions: vec![FilterCondition { op: FilterOp::Equals, value: Some(Literal::Text("North".into())) }],
                    or: false,
                },
            },
        ];
        let m = query(&q);
        assert_eq!(
            m,
            "let\n    Source = Csv.Document(File.Contents(\"C:\\data\\sales \"\"q1\"\".csv\"),[Delimiter=\",\", Encoding=65001, QuoteStyle=QuoteStyle.Csv]),\n    \
             #\"Promoted Headers\" = Table.PromoteHeaders(Source, [PromoteAllScalars=true]),\n    \
             #\"Changed Type\" = Table.TransformColumnTypes(#\"Promoted Headers\",{{\"Units\", Int64.Type}}),\n    \
             #\"Filtered Rows\" = Table.SelectRows(#\"Changed Type\", each [Region] = \"North\")\nin\n    #\"Filtered Rows\""
        );
        assert_eq!(step_formula(&q, 1).as_deref(), Some("= Table.PromoteHeaders(Source, [PromoteAllScalars=true])"));
        assert_eq!(step_formula(&q, 9), None);
        assert_eq!(ident("each"), "#\"each\"");
        assert_eq!(string("a\nb"), "\"a#(lf)b\"");
    }
}
