use super::{
    CellError, HashMap, MONTHS, Ordering, PivotDateGroup, PivotField, PivotFunc, PivotSort, Source, Value, Workbook, datetime_from_serial,
    number_to_text,
};

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
    /// The item's custom name (a renamed item).
    pub(super) caption: Option<String>,
    /// Position in the field's manual item order.
    rank: Option<usize>,
}

impl Item {
    /// The value shown: the custom name, else the item's value.
    pub(super) fn shown_value(&self) -> Value {
        self.caption.as_deref().map(Value::text).unwrap_or_else(|| self.value.clone())
    }
    pub(super) fn shown_label(&self) -> &str {
        self.caption.as_deref().unwrap_or(&self.label)
    }
}

#[derive(Default)]
pub(super) struct FieldItems {
    items: Vec<Item>,
    index: HashMap<String, u32>,
}

/// Sort classes: numbers, text, logicals, errors, dates (a number whose own cell has a date
/// format, which Excel caches as `<d>` and sorts after errors), blanks.
fn cmp_items(a: &Item, b: &Item) -> Ordering {
    a.class.cmp(&b.class).then(a.num.total_cmp(&b.num)).then_with(|| a.text.cmp(&b.text))
}

/// (identity key, item) for a source value under a field's date grouping; `date` marks a number
/// whose cell has a date format.
fn make_item(wb: &Workbook, v: &Value, group: PivotDateGroup, fmt: &str, date: bool, first: usize) -> (String, Item) {
    let it = |value: Value, label: String, class: u8, num: f64, formatted: bool| Item {
        text: label.to_lowercase(),
        value,
        label,
        class,
        num,
        first,
        formatted,
        caption: None,
        rank: None,
    };
    match v {
        Value::Empty => ("b".into(), it(Value::text("(blank)"), "(blank)".into(), 5, 0.0, false)),
        Value::Number(n) => {
            if group != PivotDateGroup::None
                && let Some(dt) = datetime_from_serial(wb.date_system, *n)
            {
                let mon = MONTHS.get(dt.month.saturating_sub(1) as usize).copied().unwrap_or("Jan");
                let (label, key) = match group {
                    PivotDateGroup::Years => (dt.year.to_string(), dt.year as f64),
                    PivotDateGroup::Quarters => {
                        let q = (dt.month.saturating_sub(1)) / 3 + 1;
                        (format!("Qtr{q}"), q as f64)
                    }
                    PivotDateGroup::Months => (mon.to_string(), dt.month as f64),
                    _ => (format!("{}-{mon}", dt.day), (dt.month * 100 + dt.day) as f64),
                };
                return (format!("g{key}"), it(Value::text(label.as_str()), label, 0, key, false));
            }
            let shown = crate::display::format(v, fmt, wb);
            let class = if date { 4 } else { 0 };
            (format!("n{}", n.to_bits()), it(v.clone(), shown.text, class, *n, true))
        }
        Value::Text(t) => (format!("t{}", t.to_lowercase()), it(v.clone(), t.to_string(), 1, 0.0, false)),
        Value::Bool(b) => {
            let l = if *b { "TRUE" } else { "FALSE" };
            (format!("l{l}"), it(Value::Bool(*b), l.into(), 2, *b as u8 as f64, false))
        }
        Value::Error(e) => (format!("e{}", e.as_str()), it(v.clone(), e.as_str().into(), 3, 0.0, false)),
        Value::Array(_) => make_item(wb, &v.scalar(), group, fmt, date, first),
    }
}

impl FieldItems {
    pub(super) fn intern(&mut self, wb: &Workbook, v: &Value, group: PivotDateGroup, fmt: &str, date: bool, first: usize) -> u32 {
        let (key, item) = make_item(wb, v, group, fmt, date, first);
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
    pub(super) fn shown_label(&self, i: u32) -> String {
        self.get(i).map(|x| x.shown_label().to_string()).unwrap_or_default()
    }
    /// The item a setting names by label (or, for a number item, its unformatted number text).
    pub(super) fn find(&self, label: &str) -> Option<u32> {
        let l = label.to_lowercase();
        let i = self.items.iter().position(|it| it.text == l || (it.formatted && number_to_text(it.num).to_lowercase() == l))?;
        u32::try_from(i).ok()
    }
    /// Applies a field's renamed items and manual item order. Settings name items by label; a
    /// number item also matches its unformatted number text (how a file's cache names it).
    pub(super) fn apply(&mut self, f: &PivotField) {
        if f.item_captions.is_empty() && f.item_order.is_empty() {
            return;
        }
        let mut order: HashMap<String, usize> = HashMap::new();
        for (i, l) in f.item_order.iter().enumerate() {
            order.entry(l.to_lowercase()).or_insert(i);
        }
        let mut captions: HashMap<String, &str> = HashMap::new();
        for (l, c) in &f.item_captions {
            captions.entry(l.to_lowercase()).or_insert(c.as_str());
        }
        for it in &mut self.items {
            let raw = if it.formatted { Some(number_to_text(it.num).to_lowercase()) } else { None };
            it.rank = order.get(&it.text).or_else(|| raw.as_ref().and_then(|r| order.get(r))).copied();
            it.caption = captions.get(&it.text).or_else(|| raw.as_ref().and_then(|r| captions.get(r))).map(|c| c.to_string());
        }
    }
}

/// The label of a source value under a date grouping (for filters and field lists).
pub fn item_label(wb: &Workbook, v: &Value, group: PivotDateGroup, fmt: &str) -> String {
    make_item(wb, v, group, fmt, false, 0).1.label
}

/// Distinct item labels of a source column, sorted like the field list shows them.
pub fn distinct_labels(wb: &Workbook, src: &Source, col: usize, group: PivotDateGroup, limit: usize) -> Vec<String> {
    let mut fi = FieldItems::default();
    let fmt = src.formats.get(col).map(String::as_str).unwrap_or("General");
    let dates = date_flags(wb, src, col, group);
    for (i, r) in src.rows.iter().enumerate() {
        if let Some(v) = r.get(col) {
            fi.intern(wb, v, group, fmt, dates.get(i).copied().unwrap_or(false), i);
        }
        if fi.items.len() > 100_000 {
            break;
        }
    }
    let mut items = fi.items;
    items.sort_by(cmp_items);
    items.into_iter().take(limit).map(|i| i.label).collect()
}

/// [`Source::date_cells`] for an ungrouped field; a date-grouped field doesn't use them.
pub(super) fn date_flags(wb: &Workbook, src: &Source, col: usize, group: PivotDateGroup) -> Vec<bool> {
    if group == PivotDateGroup::None { src.date_cells(wb, col) } else { Vec::new() }
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

/// Excel's caption verb for a function ("Sum of Sales").
pub fn func_caption(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count Numbers",
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

/// Sorting of an axis by value fields: per axis field, the value field it sorts by and the path
/// on the other axis whose values it uses (empty = the total); and the value of an axis path at
/// an other-axis path.
pub(super) type ValueLookup<'a> = &'a dyn Fn(&[u32], &[u32], usize) -> Option<f64>;

pub(super) struct ByValue<'a> {
    pub(super) fields: Vec<Option<(usize, Vec<u32>)>>,
    pub(super) value: ValueLookup<'a>,
}

pub(super) fn build_tree(paths: &[&[u32]], depth: usize, fields: &[&PivotField], items: &[FieldItems], by_value: &ByValue) -> Vec<Node> {
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
    let vi = by_value.fields.get(depth).and_then(Option::as_ref).filter(|_| field.sort != PivotSort::None);
    let mut nodes: Vec<(Option<f64>, Node)> = order
        .into_iter()
        .map(|it| {
            let ps = groups.remove(&it).unwrap_or_default();
            let key = vi.and_then(|(vi, other)| ps.first().and_then(|p| p.get(..=depth)).and_then(|p| (by_value.value)(p, other, *vi)));
            (key, Node { item: it, children: build_tree(&ps, depth + 1, fields, items, by_value) })
        })
        .collect();
    let by_label = |a: &Node, b: &Node| match (fi.get(a.item), fi.get(b.item)) {
        (Some(x), Some(y)) => match field.sort {
            PivotSort::Asc => cmp_items(x, y),
            PivotSort::Desc => cmp_items(y, x),
            PivotSort::None => x.rank.unwrap_or(usize::MAX).cmp(&y.rank.unwrap_or(usize::MAX)).then_with(|| {
                // Date groups are in date order, like the groups Excel's cache lists.
                if field.date_group == PivotDateGroup::None { x.first.cmp(&y.first) } else { cmp_items(x, y) }
            }),
        },
        _ => Ordering::Equal,
    };
    // By value: numbers in the sort direction, then items without a number; ties by label.
    let by_key = |a: &Option<f64>, b: &Option<f64>| match (a, b) {
        (Some(x), Some(y)) if field.sort == PivotSort::Desc => y.total_cmp(x),
        (Some(x), Some(y)) => x.total_cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    nodes.sort_by(|(ka, a), (kb, b)| by_key(ka, kb).then_with(|| by_label(a, b)));
    nodes.into_iter().map(|(_, n)| n).collect()
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
