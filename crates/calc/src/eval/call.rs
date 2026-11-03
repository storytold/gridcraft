use super::array::{Acc, Agg, zip2, zip3};
use super::format::same_kind;
use super::special::{ev_shape, resized_sum_range, resizes_sum_range};
use super::*;

impl<'h> Evaluator<'h> {
    pub(super) fn call(&mut self, name: &str, args: &[Expr]) -> Ev {
        match name {
            "IF" => {
                if args.is_empty() || args.len() > 3 {
                    return err(CellError::Value);
                }
                let c = self.arg_val(args, 0).unwrap_or_default();
                if let Value::Array(arr) = &c {
                    // Array condition: element-wise.
                    let t = self.arg_val(args, 1).unwrap_or(Value::Bool(true));
                    let f = if args.len() > 2 { self.arg_val(args, 2).unwrap_or(Value::Bool(false)) } else { Value::Bool(false) };
                    return Ev::V(zip3(arr, &t, &f, |c, t, f| match c.to_bool() {
                        Ok(true) => t.clone(),
                        Ok(false) => f.clone(),
                        Err(e) => Value::Error(e),
                    }));
                }
                match c.to_bool() {
                    Ok(true) => match args.get(1) {
                        Some(Expr::Missing) => Ev::V(Value::Number(0.0)),
                        Some(e) => self.eval(e),
                        None => Ev::V(Value::Bool(true)),
                    },
                    Ok(false) => match args.get(2) {
                        Some(Expr::Missing) => Ev::V(Value::Number(0.0)),
                        Some(e) => self.eval(e),
                        None => Ev::V(Value::Bool(false)),
                    },
                    Err(e) => err(e),
                }
            }
            "IFS" => {
                if args.len() < 2 || !args.len().is_multiple_of(2) {
                    return err(CellError::Value);
                }
                for pair in args.chunks(2) {
                    let [c, v] = pair else { break };
                    let cv = self.value(c);
                    match cv.scalar().to_bool() {
                        Ok(true) => return self.eval(v),
                        Ok(false) => {}
                        Err(e) => return err(e),
                    }
                }
                err(CellError::NA)
            }
            "IFERROR" | "IFNA" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let v = self.arg_ev(args, 0).unwrap_or(Ev::V(Value::Empty));
                let na_only = name == "IFNA";
                let catch = |e: CellError| if na_only { e == CellError::NA } else { true };
                match &v {
                    Ev::V(Value::Error(e)) if catch(*e) => self.eval(&args[1]),
                    Ev::V(Value::Array(a)) if a.data.iter().any(|x| x.as_error().is_some_and(catch)) => {
                        let alt = self.arg_val(args, 1).unwrap_or_default();
                        let data = a.data.iter().map(|x| if x.as_error().is_some_and(catch) { alt.scalar() } else { x.clone() }).collect();
                        Ev::V(Array::new(a.rows, a.cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
                    }
                    Ev::R(_) => {
                        let val = self.deref(v.clone());
                        match val {
                            Value::Error(e) if catch(e) => self.eval(&args[1]),
                            Value::Array(ref a) if a.data.iter().any(|x| x.as_error().is_some_and(catch)) => {
                                let alt = self.arg_val(args, 1).unwrap_or_default();
                                let data = a.data.iter().map(|x| if x.as_error().is_some_and(catch) { alt.scalar() } else { x.clone() }).collect();
                                Ev::V(Array::new(a.rows, a.cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
                            }
                            _ => v,
                        }
                    }
                    _ => v,
                }
            }
            "CHOOSE" => {
                if args.len() < 2 {
                    return err(CellError::Value);
                }
                let index = self.value(&args[0]);
                if let Value::Array(_) = index {
                    // An array of indexes picks element by element: `CHOOSE({1,2},A1:A3,B1:B3)`
                    // is a two-column array. The library CHOOSE lifts over its arguments.
                    let Some(spec) = gridcraft_functions::lookup(name) else { return err(CellError::Name) };
                    let mut evaluated = vec![Arg { value: index, from_ref: false }];
                    for a in &args[1..] {
                        let value = self.value(a);
                        evaluated.push(Arg { value, from_ref: false });
                    }
                    let mut ctx = FnCtx { ev: self, criteria: None, lambda: None };
                    return gridcraft_functions::call(spec, &evaluated, &mut ctx).pipe(Ev::V);
                }
                let i = match index.to_number() {
                    Ok(n) => n.trunc() as i64,
                    Err(e) => return err(e),
                };
                if i < 1 || i as usize >= args.len() {
                    return err(CellError::Value);
                }
                self.eval(&args[i as usize])
            }
            "SWITCH" => {
                if args.len() < 3 {
                    return err(CellError::Value);
                }
                let v = self.arg_val(args, 0).unwrap_or_default().scalar();
                if let Value::Error(e) = v {
                    return err(e);
                }
                let rest = &args[1..];
                let mut i = 0;
                while i + 1 < rest.len() {
                    let cand = self.value(&rest[i]).scalar();
                    if !cand.is_error() && compare(&v, &cand) == std::cmp::Ordering::Equal && same_kind(&v, &cand) {
                        return self.eval(&rest[i + 1]);
                    }
                    i += 2;
                }
                if rest.len() % 2 == 1 { rest.last().map_or(err(CellError::NA), |d| self.eval(d)) } else { err(CellError::NA) }
            }
            "ROW" | "COLUMN" => {
                let is_row = name == "ROW";
                if args.is_empty() || matches!(args.first(), Some(Expr::Missing)) {
                    return Ev::V(Value::number(if is_row { self.at.row + 1 } else { self.at.col + 1 } as f64));
                }
                match self.eval(&args[0]) {
                    Ev::R(a) => {
                        let Some(a) = a.first() else { return err(CellError::Ref) };
                        let r = self.trim_lines(a);
                        if is_row {
                            let rows: Vec<Value> = (r.start.row..=r.end.row).map(|x| Value::number(x as f64 + 1.0)).collect();
                            if rows.len() == 1 { Ev::V(rows.into_iter().next().unwrap_or_default()) } else { Ev::V(Array::column(rows).into()) }
                        } else {
                            let cols: Vec<Value> = (r.start.col..=r.end.col).map(|x| Value::number(x as f64 + 1.0)).collect();
                            if cols.len() == 1 { Ev::V(cols.into_iter().next().unwrap_or_default()) } else { Ev::V(Array::row(cols).into()) }
                        }
                    }
                    Ev::V(Value::Error(e)) => err(e),
                    _ => err(CellError::Value),
                }
            }
            "ROWS" | "COLUMNS" => {
                if args.len() != 1 {
                    return err(CellError::Value);
                }
                let is_rows = name == "ROWS";
                match self.eval(&args[0]) {
                    Ev::R(a) => match a.as_slice() {
                        [a] => Ev::V(Value::number(if is_rows { a.range.height() } else { a.range.width() } as f64)),
                        _ => err(CellError::Ref),
                    },
                    Ev::V(Value::Array(arr)) => Ev::V(Value::number(if is_rows { arr.rows } else { arr.cols } as f64)),
                    Ev::V(Value::Error(e)) => err(e),
                    Ev::V(_) => Ev::V(Value::Number(1.0)),
                    Ev::L(_) => err(CellError::Value),
                }
            }
            "AREAS" => match args.first().map(|a| self.eval(a)) {
                Some(Ev::R(a)) => Ev::V(Value::number(a.len() as f64)),
                Some(Ev::V(Value::Error(e))) => err(e),
                _ => err(CellError::Value),
            },
            "ISREF" => Ev::V(Value::Bool(matches!(args.first().map(|a| self.eval(a)), Some(Ev::R(_))))),
            "ISFORMULA" | "FORMULATEXT" => {
                let Some(Ev::R(a)) = args.first().map(|a| self.eval(a)) else { return err(CellError::NA) };
                let Some(a) = a.first() else { return err(CellError::NA) };
                let f = self.host.workbook().sheet(a.sheet).and_then(|s| s.cell(a.range.start)).and_then(|c| c.formula.clone());
                if name == "ISFORMULA" {
                    Ev::V(Value::Bool(f.is_some()))
                } else {
                    match f {
                        Some(f) => Ev::V(Value::text(format!("={}", f.text))),
                        None => err(CellError::NA),
                    }
                }
            }
            "SHEET" | "SHEETS" => {
                let wb = self.host.workbook();
                if args.is_empty() {
                    return Ev::V(Value::number(if name == "SHEET" { self.sheet + 1 } else { wb.sheets.len() } as f64));
                }
                match self.eval(&args[0]) {
                    Ev::R(a) => {
                        if name == "SHEET" {
                            a.first().map_or(err(CellError::Ref), |a| Ev::V(Value::number(a.sheet as f64 + 1.0)))
                        } else {
                            let mut s: Vec<usize> = a.iter().map(|x| x.sheet).collect();
                            s.dedup();
                            Ev::V(Value::number(s.len() as f64))
                        }
                    }
                    Ev::V(Value::Text(t)) if name == "SHEET" => {
                        let wb = self.host.workbook();
                        wb.sheet_index(&t).map_or(err(CellError::NA), |i| Ev::V(Value::number(i as f64 + 1.0)))
                    }
                    _ => err(CellError::NA),
                }
            }
            "OFFSET" => self.offset(args),
            "INDIRECT" => self.indirect(args),
            "INDEX" => self.index(args),
            "XLOOKUP" => self.xlookup(args),
            "CELL" => self.cell_info(args),
            "TRIMRANGE" => self.trimrange(args),
            "SUBTOTAL" | "AGGREGATE" => self.subtotal(name, args),
            "TEXT" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let v = self.arg_val(args, 0).unwrap_or_default();
                let f = self.arg_val(args, 1).unwrap_or_default();
                let sys = self.host.workbook().date_system;
                let one = |v: &Value, f: &Value| -> Value {
                    if let Value::Error(e) = v {
                        return Value::Error(*e);
                    }
                    let code = match f.to_text() {
                        Ok(c) => c,
                        Err(e) => return Value::Error(e),
                    };
                    match gridcraft_numfmt::text_function(v, &code, sys) {
                        Ok(s) => Value::text(s),
                        Err(e) => Value::Error(e),
                    }
                };
                Ev::V(zip2(&v, &f, one))
            }
            "LET" => self.let_(args),
            "LAMBDA" => {
                if args.is_empty() {
                    return err(CellError::Value);
                }
                let mut params = Vec::new();
                for p in &args[..args.len() - 1] {
                    match p {
                        Expr::Name(n) => params.push(n.trim_start_matches('[').trim_end_matches(']').to_string()),
                        _ => return err(CellError::Value),
                    }
                }
                let Some(body) = args.last() else { return err(CellError::Value) };
                Ev::L(Arc::new(Lambda { params, body: body.clone(), env: self.env.clone() }))
            }
            "ISOMITTED" => {
                let r = matches!(args.first(), Some(Expr::Name(n)) if self.env.iter().rev().find(|(k, _)| gridcraft_formula::same_name(k, n)).is_some_and(|(_, v)| matches!(v, Ev::V(Value::Empty))));
                Ev::V(Value::Bool(r))
            }
            "MAP" | "REDUCE" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" => self.higher_order(name, args),
            "GROUPBY" | "PIVOTBY" => {
                // The aggregation may be a function given by name (`SUM`): pass it as its name.
                let i = if name == "GROUPBY" { 2 } else { 3 };
                match args.get(i).and_then(|a| self.function_by_name(a)) {
                    Some(f) => {
                        let mut args = args.to_vec();
                        if let Some(a) = args.get_mut(i) {
                            *a = Expr::Text(f.into());
                        }
                        self.builtin(name, &args)
                    }
                    None => self.builtin(name, args),
                }
            }
            "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" => {
                self.host.note_volatile();
                self.builtin(name, args)
            }
            _ => self.builtin(name, args),
        }
    }

    pub(super) fn builtin(&mut self, name: &str, args: &[Expr]) -> Ev {
        let Some(spec) = gridcraft_functions::lookup(name) else {
            // A defined name holding a LAMBDA, or a LET-bound lambda.
            return match self.name(name) {
                Ev::L(l) => self.apply_lambda(&l, args),
                Ev::V(Value::Error(x)) => err(x),
                _ => err(CellError::Name),
            };
        };
        if args.len() < spec.min_args || args.len() > spec.max_args {
            return err(CellError::Value);
        }
        if let Some(kind) = Agg::of(spec.name) {
            return self.aggregate(kind, args);
        }
        let mut evaluated = Vec::with_capacity(args.len());
        // SUMIF/AVERAGEIF: the size of the criteria range (argument 1), which the sum range takes.
        let mut criteria_shape = None;
        // Database functions: the criteria range (argument 2), whose formulas are computed criteria.
        let mut criteria = None;
        // GROUPBY/PIVOTBY: the LAMBDA given as their function.
        let mut lambda = None;
        let lambda_at = match name {
            "GROUPBY" => Some(2),
            "PIVOTBY" => Some(3),
            _ => None,
        };
        for (i, a) in args.iter().enumerate() {
            let mut ev = self.eval(a);
            if lambda_at == Some(i)
                && let Ev::L(l) = &ev
            {
                lambda = Some(l.clone());
                ev = Ev::V(Value::Empty);
            }
            if spec.category == gridcraft_functions::Category::Database
                && i == 2
                && let Ev::R(areas) = &ev
                && let [area] = areas.as_slice()
            {
                criteria = Some(*area);
            }
            if resizes_sum_range(name, args.len()) {
                if i == 0 {
                    criteria_shape = ev_shape(&ev);
                } else if i == 2
                    && let (Some((h, w)), Ev::R(areas)) = (criteria_shape, &ev)
                    && let [area] = areas.as_slice()
                {
                    ev = Ev::R(vec![Area { sheet: area.sheet, range: resized_sum_range(area.range, h, w) }]);
                }
            }
            let from_ref = matches!(ev, Ev::R(_));
            let value = match ev {
                Ev::R(areas) if areas.len() > 1 => {
                    // Multi-area references (unions): flatten into one row for aggregates.
                    let mut vals = Vec::new();
                    for ar in &areas {
                        match self.area_value(ar) {
                            Value::Array(a) => vals.extend(a.data.iter().cloned()),
                            v => vals.push(v),
                        }
                    }
                    Value::from(Array::row(vals))
                }
                ev => self.deref(ev),
            };
            evaluated.push(Arg { value, from_ref });
        }
        let mut ctx = FnCtx { ev: self, criteria, lambda };
        gridcraft_functions::call(spec, &evaluated, &mut ctx).pipe(Ev::V)
    }

    /// Performs aggregation operations without copying its ranges: blocks are folded over the stored
    /// cells, so no array is built and there is no `MAX_CELLS` limit. Arguments are evaluated and read one
    /// after another, as the array path does, so volatile formulas (like `RAND`) see the same order.
    /// Only `SUM`, `COUNT` and `AVG` are supported currently.
    fn aggregate(&mut self, kind: Agg, args: &[Expr]) -> Ev {
        let mut acc = Acc::default();
        let mut first_error = None;
        for a in args {
            let ev = self.eval(a);
            let steps = match ev {
                Ev::R(areas) => areas.iter().map(|area| self.aggregate_area(kind, area, &mut acc)).collect(),
                ev => {
                    let value = self.deref(ev);
                    vec![self.aggregate_value(kind, value, false, &mut acc)]
                }
            };
            if first_error.is_none() {
                first_error = steps.into_iter().find_map(Result::err);
            }
        }
        first_error.unwrap_or_else(|| Ev::V(acc.finish(kind)))
    }

    /// Adds one area to `acc`; `Err` is the result the function ends with.
    fn aggregate_area(&mut self, kind: Agg, a: &Area, acc: &mut Acc) -> Result<(), Ev> {
        if !a.range.is_single() {
            let mut error = None;
            let walked = self.host.visit_range(a.sheet, a.range, &mut |v| match acc.add_cell(kind, v) {
                Ok(()) => true,
                Err(e) => {
                    error = Some(e);
                    false
                }
            });
            if walked {
                return error.map_or(Ok(()), |e| Err(err(e)));
            }
        }
        let value = self.area_value(a);
        self.aggregate_value(kind, value, true, acc)
    }

    /// Adds one argument's value by calling `SUM` and `COUNT` on it, so coercion and error rules
    /// stay in one place (`AVERAGE` of an argument is its `SUM` over its `COUNT`).
    fn aggregate_value(&mut self, kind: Agg, value: Value, from_ref: bool, acc: &mut Acc) -> Result<(), Ev> {
        let (Some(sum), Some(count)) = (gridcraft_functions::lookup("SUM"), gridcraft_functions::lookup("COUNT")) else {
            return Err(err(CellError::Name));
        };
        let args = [Arg { value, from_ref }];
        if kind != Agg::Count {
            match gridcraft_functions::call(sum, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => acc.sum += n,
                other => return Err(Ev::V(other)),
            }
        }
        if kind != Agg::Sum {
            match gridcraft_functions::call(count, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => acc.count = acc.count.saturating_add(n as u64),
                other => return Err(Ev::V(other)),
            }
        }
        Ok(())
    }

    /// A worksheet function given by name where a function is expected (`BYROW(A1:C3,SUM)`,
    /// `GROUPBY(A2:A9,B2:B9,SUM)`): its name, unless a LET/LAMBDA variable or a defined name has
    /// that name.
    pub(super) fn function_by_name(&self, e: &Expr) -> Option<String> {
        let Expr::Name(n) = e else { return None };
        if self.env.iter().any(|(k, _)| gridcraft_formula::same_name(k, n)) || self.host.workbook().name(n, self.sheet).is_some() {
            return None;
        }
        let u = n.to_ascii_uppercase();
        gridcraft_functions::lookup(&u).map(|_| u)
    }
}
