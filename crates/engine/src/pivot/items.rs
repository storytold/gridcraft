use super::{CellError, HashMap, Ordering, PivotDateGroup, PivotField, PivotFunc, PivotSort, Source, Value, Workbook};

// ---------------------------------------------------------------- items

#[derive(Clone, Debug)]
pub(super) struct Item {
    pub(super) value: Value,
    pub(super) label: String,
    class: u8,
    num: f64,
    text: String,
    first: usize,
    /// The value is a number shown with the source format.
    pub(super) formatted: bool,
}

#[derive(Default)]
pub(super) struct FieldItems {
    items: Vec<Item>,
    index: HashMap<String, u32>,
}

fn cmp_items(a: &Item, b: &Item) -> Ordering {
    a.class.cmp(&b.class).then(a.num.total_cmp(&b.num)).then_with(|| gridcraft_core::compare_text(&a.text, &b.text))
}

/// (identity key, item) for a source value under a field's date grouping.
fn make_item(wb: &Workbook, v: &Value, group: PivotDateGroup, fmt: &str, first: usize) -> (String, Item) {
    let it = |value: Value, label: String, class: u8, num: f64, formatted: bool| Item {
        text: label.to_lowercase(),
        value,
        label,
        class,
        num,
        first,
        formatted,
    };
    // The label is the model's, which file writers list items by too. It takes the value as the
    // cell holds it and reads an array's first element the same way as the match below.
    let label = gridcraft_model::pivot_source::item_label(v, group, &crate::display::parsed(fmt).into(), wb.date_system);
    let v = &v.scalar();
    match v {
        Value::Empty => ("b".into(), it(Value::text("(blank)"), label, 4, 0.0, false)),
        Value::Number(n) => match gridcraft_model::pivot_source::date_group_item(wb.date_system, *n, group) {
            Some((_, key)) => (format!("g{key}"), it(Value::text(label.as_str()), label, 0, key, false)),
            None => (format!("n{}", n.to_bits()), it(v.clone(), label, 0, *n, true)),
        },
        Value::Text(t) => (format!("t{}", t.to_lowercase()), it(v.clone(), label, 1, 0.0, false)),
        Value::Bool(b) => (format!("l{label}"), it(Value::Bool(*b), label, 2, *b as u8 as f64, false)),
        Value::Error(e) => (format!("e{}", e.as_str()), it(v.clone(), label, 3, 0.0, false)),
        // An array in an array: no value an item can hold.
        Value::Array(_) => ("b".into(), it(Value::text("(blank)"), label, 4, 0.0, false)),
    }
}

impl FieldItems {
    pub(super) fn intern(&mut self, wb: &Workbook, v: &Value, group: PivotDateGroup, fmt: &str, first: usize) -> u32 {
        let (key, item) = make_item(wb, v, group, fmt, first);
        if let Some(i) = self.index.get(&key) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(item);
        self.index.insert(key, i);
        i
    }
    pub(super) fn get(&self, i: u32) -> Option<&Item> {
        self.items.get(i as usize)
    }
    pub(super) fn label(&self, i: u32) -> String {
        self.get(i).map(|x| x.label.clone()).unwrap_or_default()
    }
}

/// The label of a source value under a date grouping (for filters and field lists).
pub fn item_label(wb: &Workbook, v: &Value, group: PivotDateGroup, fmt: &str) -> String {
    make_item(wb, v, group, fmt, 0).1.label
}

/// Distinct item labels of a source column, sorted like the field list shows them.
pub fn distinct_labels(wb: &Workbook, src: &Source, col: usize, group: PivotDateGroup, limit: usize) -> Vec<String> {
    let mut fi = FieldItems::default();
    let fmt = src.formats.get(col).map(String::as_str).unwrap_or("General");
    for (i, r) in src.rows.iter().enumerate() {
        if let Some(v) = r.get(col) {
            fi.intern(wb, v, group, fmt, i);
        }
        if fi.items.len() > 100_000 {
            break;
        }
    }
    let mut items = fi.items;
    items.sort_by(cmp_items);
    items.into_iter().take(limit).map(|i| i.label).collect()
}

// ---------------------------------------------------------------- aggregation

#[derive(Clone, Debug, Default)]
pub(super) struct Acc {
    count: u64,
    nums: u64,
    sum: f64,
    prod: f64,
    min: f64,
    max: f64,
    mean: f64,
    m2: f64,
    err: Option<CellError>,
}

impl Acc {
    pub(super) fn add(&mut self, v: &Value) {
        match v {
            Value::Empty => {}
            Value::Number(n) => {
                let n = *n;
                self.count += 1;
                self.nums += 1;
                self.sum += n;
                if self.nums == 1 {
                    self.prod = n;
                    self.min = n;
                    self.max = n;
                } else {
                    self.prod *= n;
                    self.min = self.min.min(n);
                    self.max = self.max.max(n);
                }
                let d = n - self.mean;
                self.mean += d / self.nums as f64;
                self.m2 += d * (n - self.mean);
            }
            Value::Error(e) => {
                self.count += 1;
                if self.err.is_none() {
                    self.err = Some(*e);
                }
            }
            _ => self.count += 1,
        }
    }
    pub(super) fn result(&self, f: PivotFunc) -> Value {
        let n = self.nums as f64;
        match f {
            PivotFunc::Count => return Value::number(self.count as f64),
            PivotFunc::CountNumbers => return Value::number(n),
            _ => {}
        }
        if let Some(e) = self.err {
            return Value::Error(e);
        }
        let div0 = Value::Error(CellError::Div0);
        match f {
            PivotFunc::Sum => Value::number(self.sum),
            PivotFunc::Average => {
                if self.nums == 0 {
                    div0
                } else {
                    Value::number(self.sum / n)
                }
            }
            PivotFunc::Max => Value::number(if self.nums == 0 { 0.0 } else { self.max }),
            PivotFunc::Min => Value::number(if self.nums == 0 { 0.0 } else { self.min }),
            PivotFunc::Product => Value::number(if self.nums == 0 { 0.0 } else { self.prod }),
            PivotFunc::StdDev | PivotFunc::Var => {
                if self.nums < 2 {
                    div0
                } else {
                    let var = (self.m2 / (n - 1.0)).max(0.0);
                    Value::number(if f == PivotFunc::Var { var } else { var.sqrt() })
                }
            }
            PivotFunc::StdDevP | PivotFunc::VarP => {
                if self.nums < 1 {
                    div0
                } else {
                    let var = (self.m2 / n).max(0.0);
                    Value::number(if f == PivotFunc::VarP { var } else { var.sqrt() })
                }
            }
            PivotFunc::Count | PivotFunc::CountNumbers => Value::Empty,
        }
    }
}

/// Excel's caption verb for a function ("Sum of Sales"). Count Numbers is captioned "Count of
/// Sales", as Excel names it and as the XLSX reader names a value field saved without a name.
pub fn func_caption(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count",
        PivotFunc::StdDev => "StdDev",
        PivotFunc::StdDevP => "StdDevp",
        PivotFunc::Var => "Var",
        PivotFunc::VarP => "Varp",
    }
}

// ---------------------------------------------------------------- trees

#[derive(Clone, Debug)]
pub(super) struct Node {
    pub(super) item: u32,
    pub(super) children: Vec<Node>,
}

pub(super) fn build_tree(paths: &[&[u32]], depth: usize, fields: &[&PivotField], items: &[FieldItems]) -> Vec<Node> {
    let (Some(field), Some(fi)) = (fields.get(depth), items.get(depth)) else { return vec![] };
    let mut order: Vec<u32> = Vec::new();
    let mut groups: HashMap<u32, Vec<&[u32]>> = HashMap::new();
    for p in paths {
        if let Some(&it) = p.get(depth) {
            let g = groups.entry(it).or_default();
            if g.is_empty() {
                order.push(it);
            }
            g.push(p);
        }
    }
    let mut nodes: Vec<Node> = order
        .into_iter()
        .map(|it| {
            let ps = groups.remove(&it).unwrap_or_default();
            Node { item: it, children: build_tree(&ps, depth + 1, fields, items) }
        })
        .collect();
    let by = |a: &Node, b: &Node| match (fi.get(a.item), fi.get(b.item)) {
        (Some(x), Some(y)) => match field.sort {
            PivotSort::Asc => cmp_items(x, y),
            PivotSort::Desc => cmp_items(y, x),
            PivotSort::None => x.first.cmp(&y.first),
        },
        _ => Ordering::Equal,
    };
    nodes.sort_by(by);
    nodes
}

pub(super) fn is_collapsed(field: &PivotField, label: &str) -> bool {
    field.collapsed_items.iter().any(|c| c.eq_ignore_ascii_case(label))
}

pub(super) fn collect_siblings(nodes: &[Node], prefix: &mut Vec<u32>, out: &mut HashMap<Vec<u32>, Vec<u32>>) {
    out.insert(prefix.clone(), nodes.iter().map(|n| n.item).collect());
    for n in nodes {
        prefix.push(n.item);
        collect_siblings(&n.children, prefix, out);
        prefix.pop();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gridcraft_core::Array;

    use super::*;

    #[test]
    fn array_items_agree_in_value_key_and_label() {
        let wb = Workbook::new();
        let one = |v: Value| Value::Array(Arc::new(Array::scalar(v)));
        // An array is its first element.
        let (key, item) = make_item(&wb, &one(Value::Number(5.0)), PivotDateGroup::None, "General", 0);
        assert_eq!(key, format!("n{}", 5f64.to_bits()));
        assert_eq!((item.label.as_str(), &item.value), ("5", &Value::Number(5.0)));
        // An array inside that is no value an item can hold: blank throughout.
        let (key, item) = make_item(&wb, &one(one(Value::Number(5.0))), PivotDateGroup::None, "General", 0);
        assert_eq!(key, "b");
        assert_eq!((item.label.as_str(), &item.value), ("(blank)", &Value::text("(blank)")));
    }
}
