use super::format::{cell_format_code, r1c1_to_a1};
use super::{Area, Arg, Array, CellError, Ev, Evaluator, Expr, FnCtx, MAX_CELLS, Value, err};
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use std::sync::Arc;

impl<'h> Evaluator<'h> {
    /// Whole rows/columns trimmed for ROW()/COLUMN() arrays.
    pub(super) fn trim_lines(&mut self, a: &Area) -> RangeRef {
        if a.range.is_full_cols() || a.range.is_full_rows() { self.trim(a) } else { a.range }
    }

    pub(super) fn offset(&mut self, args: &[Expr]) -> Ev {
        self.host.note_volatile();
        if args.len() < 3 || args.len() > 5 {
            return err(CellError::Value);
        }
        let base = match self.eval(&args[0]) {
            Ev::R(a) if a.len() == 1 => a[0],
            Ev::V(Value::Error(e)) => return err(e),
            _ => return err(CellError::Value),
        };
        let rows = match self.arg_num(args, 1, 0.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let cols = match self.arg_num(args, 2, 0.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let h = match self.arg_num(args, 3, base.range.height() as f64) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let w = match self.arg_num(args, 4, base.range.width() as f64) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        if h == 0 || w == 0 {
            return err(CellError::Ref);
        }
        let r0 = (base.range.start.row as i64).saturating_add(rows);
        let c0 = (base.range.start.col as i64).saturating_add(cols);
        let (r0, r1) = if h > 0 { (r0, r0.saturating_add(h - 1)) } else { (r0.saturating_add(h + 1), r0) };
        let (c0, c1) = if w > 0 { (c0, c0.saturating_add(w - 1)) } else { (c0.saturating_add(w + 1), c0) };
        if r0 < 0 || c0 < 0 || r1 >= MAX_ROWS as i64 || c1 >= MAX_COLS as i64 {
            return err(CellError::Ref);
        }
        Ev::R(vec![Area { sheet: base.sheet, range: RangeRef::new(CellRef::new(r0 as u32, c0 as u32), CellRef::new(r1 as u32, c1 as u32)) }])
    }

    pub(super) fn indirect(&mut self, args: &[Expr]) -> Ev {
        self.host.note_volatile();
        if args.is_empty() || args.len() > 2 {
            return err(CellError::Value);
        }
        let text = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t,
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let a1 = match args.get(1) {
            None | Some(Expr::Missing) => true,
            Some(e) => {
                let v = self.value(e);
                v.scalar().to_bool().unwrap_or(true)
            }
        };
        // With a1 = FALSE, text that isn't R1C1 can still be a name, but never an A1 reference.
        let (text, a1_ok) = if a1 {
            (text, true)
        } else {
            match r1c1_to_a1(&text, self.at) {
                Ok(Some(a1_text)) => (a1_text, true),
                Ok(None) => (text, false),
                Err(e) => return err(e),
            }
        };
        match gridcraft_formula::parse(&text) {
            Ok(Expr::Ref(_)) if !a1_ok => err(CellError::Ref),
            Ok(e @ (Expr::Ref(_) | Expr::Name(_) | Expr::Struct(_))) => match self.eval(&e) {
                r @ Ev::R(_) => r,
                _ => err(CellError::Ref),
            },
            _ => err(CellError::Ref),
        }
    }

    pub(super) fn index(&mut self, args: &[Expr]) -> Ev {
        if args.is_empty() || args.len() > 4 {
            return err(CellError::Value);
        }
        let src = self.eval(&args[0]);
        let mut arg = |i: usize| match args.get(i) {
            None | Some(Expr::Missing) => None,
            Some(e) => Some(self.value(e)),
        };
        let (rows, cols) = (arg(1), arg(2));
        let area_n = match self.arg_num(args, 3, 1.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let two = args.len() == 2;
        let num = |v: &Option<Value>| v.as_ref().map_or(Ok(0.0), |v| v.to_number()).map(|n| n.trunc() as i64);
        // Arrays of positions pick a value for each: INDEX(A1:C10,{1;2},{1,3}) is 2×2.
        if let (Some(Value::Array(_)), _) | (_, Some(Value::Array(_))) = (&rows, &cols) {
            let as_array = |v: &Option<Value>| match v {
                Some(Value::Array(a)) => Arc::clone(a),
                Some(v) => Arc::new(Array::scalar(v.clone())),
                None => Arc::new(Array::scalar(Value::Number(0.0))),
            };
            let (ra, ca) = (as_array(&rows), as_array(&cols));
            let (h, w) = (ra.rows.max(ca.rows), ra.cols.max(ca.cols));
            if (h as u64).saturating_mul(w as u64) > MAX_CELLS {
                return err(CellError::Num);
            }
            let mut out = Vec::with_capacity(h * w);
            for r in 0..h {
                for c in 0..w {
                    let pos = (num(&Some(ra.get_broadcast(r, c))), num(&Some(ca.get_broadcast(r, c))));
                    let v = match pos {
                        // A position that picks several cells is #CALC!. That is decided from the
                        // shape: reading the block first, for every position, let a short formula
                        // read one large range over and over only to discard it.
                        (Ok(row), Ok(col)) if index_picks_several_values(&src, row, col, two) => Value::Error(CellError::Calc),
                        (Ok(row), Ok(col)) => {
                            let one = self.index_one(src.clone(), row, col, area_n, two);
                            let cells = match &one {
                                Ev::R(areas) => match areas.as_slice() {
                                    [a] => self.area_cells(a),
                                    _ => 0,
                                },
                                _ => 0,
                            };
                            if cells > MAX_CELLS {
                                // As reading it would answer.
                                Value::Error(CellError::Num)
                            } else if cells > 1 {
                                Value::Error(CellError::Calc)
                            } else {
                                match self.deref(one) {
                                    Value::Array(a) if a.rows * a.cols > 1 => Value::Error(CellError::Calc),
                                    v => v.scalar(),
                                }
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => Value::Error(e),
                    };
                    out.push(v);
                }
            }
            return Ev::V(Array::new(h, w, out).map(Value::from).unwrap_or(Value::Error(CellError::Value)));
        }
        let (row, col) = match (num(&rows), num(&cols)) {
            (Ok(r), Ok(c)) => (r, c),
            (Err(e), _) | (_, Err(e)) => return err(e),
        };
        self.index_one(src, row, col, area_n, two)
    }

    /// Cells that reading `a` gives: its own size, or for a large block the part inside the
    /// sheet's used range (what [`Evaluator::area_value`] reads).
    fn area_cells(&mut self, a: &Area) -> u64 {
        if a.range.is_single() {
            return 1;
        }
        let r = self.read_range(a);
        u64::from(r.height()) * u64::from(r.width())
    }

    /// INDEX for one row and column position (0 = the whole column or row).
    fn index_one(&mut self, src: Ev, row: i64, col: i64, area_n: i64, two_args: bool) -> Ev {
        if row < 0 || col < 0 {
            return err(CellError::Value);
        }
        match src {
            Ev::R(areas) => {
                let area = usize::try_from((area_n as u64).saturating_sub(1)).ok();
                let Some(a) = area.and_then(|i| areas.get(i)).copied() else { return err(CellError::Ref) };
                let r = a.range;
                // A single row or column with one index picks along it. An area with several rows
                // and columns needs both: `INDEX(A1:C3,2)` is #REF! in Excel (an empty third
                // argument, `INDEX(A1:C3,2,)`, still means the whole row). Arrays differ:
                // `INDEX({1,2;3,4},2)` is the row.
                if two_args && r.height() > 1 && r.width() > 1 {
                    return err(CellError::Ref);
                }
                let (row, col) = if two_args && r.height() == 1 { (1, row) } else { (row, col) };
                if row as u64 > r.height() as u64 || col as u64 > r.width() as u64 {
                    return err(CellError::Ref);
                }
                let (r0, r1) = if row == 0 { (r.start.row, r.end.row) } else { (r.start.row + row as u32 - 1, r.start.row + row as u32 - 1) };
                let (c0, c1) = if col == 0 { (r.start.col, r.end.col) } else { (r.start.col + col as u32 - 1, r.start.col + col as u32 - 1) };
                Ev::R(vec![Area { sheet: a.sheet, range: RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)) }])
            }
            Ev::V(Value::Array(arr)) => {
                let (row, col) = if two_args && arr.rows == 1 { (1, row) } else { (row, col) };
                if row as usize > arr.rows || col as usize > arr.cols {
                    return err(CellError::Ref);
                }
                match (row, col) {
                    (0, 0) => Ev::V(Value::Array(arr)),
                    (0, c) => Ev::V(Array::column((0..arr.rows).map(|r| arr.get(r, c as usize - 1).cloned().unwrap_or_default()).collect()).into()),
                    (r, 0) => Ev::V(Array::row((0..arr.cols).map(|c| arr.get(r as usize - 1, c).cloned().unwrap_or_default()).collect()).into()),
                    (r, c) => Ev::V(arr.get(r as usize - 1, c as usize - 1).cloned().unwrap_or(Value::Error(CellError::Ref))),
                }
            }
            Ev::V(Value::Error(e)) => err(e),
            Ev::V(v) => {
                if row <= 1 && col <= 1 {
                    Ev::V(v)
                } else {
                    err(CellError::Ref)
                }
            }
            Ev::L(_) => err(CellError::Value),
        }
    }

    /// XLOOKUP whose result range is a reference returns a reference, as in Excel, so that it can
    /// end a range (`SUM(XLOOKUP(a,…):XLOOKUP(b,…))`) or feed CELL. Everything else (arrays of
    /// lookup values, array arguments, errors) goes to the library function.
    pub(super) fn xlookup(&mut self, args: &[Expr]) -> Ev {
        if (3..=6).contains(&args.len())
            && let (Ev::R(look), Ev::R(ret)) = (self.eval(&args[1]), self.eval(&args[2]))
            && let ([l], [r]) = (look.as_slice(), ret.as_slice())
            && let Some(spec) = gridcraft_functions::lookup("XMATCH")
        {
            let key = self.value(&args[0]);
            let (l, r) = (*l, *r);
            let vertical = l.range.width() == 1 && l.range.height() > 1;
            let horizontal = l.range.height() == 1 && l.range.width() > 1;
            let same_len = if vertical { r.range.height() == l.range.height() } else { r.range.width() == l.range.width() };
            if !matches!(key, Value::Array(_)) && (vertical || horizontal) && same_len {
                let mut xargs = vec![Arg::val(key), Arg::reference(self.area_value(&l))];
                // An empty match_mode before a given search_mode is the default (0), not the end.
                let given = |i: usize| args.get(i).is_some_and(|e| !matches!(e, Expr::Missing));
                let last = if given(5) {
                    5
                } else if given(4) {
                    4
                } else {
                    3
                };
                for i in 4..=last {
                    match args.get(i) {
                        Some(Expr::Missing) | None => xargs.push(Arg::val(Value::Number(0.0))),
                        Some(e) => {
                            let value = self.value(e);
                            let blank_value = value == Value::Empty;
                            xargs.push(Arg { value, from_ref: false, blank_value });
                        }
                    }
                }
                let mut ctx = FnCtx { ev: self, criteria: None, lambda: None };
                return match gridcraft_functions::call(spec, &xargs, &mut ctx) {
                    Value::Number(p) if p >= 1.0 => {
                        let k = p as u32 - 1;
                        let range = if vertical {
                            RangeRef::new(
                                CellRef::new(r.range.start.row + k, r.range.start.col),
                                CellRef::new(r.range.start.row + k, r.range.end.col),
                            )
                        } else {
                            RangeRef::new(
                                CellRef::new(r.range.start.row, r.range.start.col + k),
                                CellRef::new(r.range.end.row, r.range.start.col + k),
                            )
                        };
                        Ev::R(vec![Area { sheet: r.sheet, range }])
                    }
                    Value::Error(CellError::NA) if args.get(3).is_some_and(|e| !matches!(e, Expr::Missing)) => self.eval(&args[3]),
                    Value::Error(e) => err(e),
                    _ => err(CellError::Value),
                };
            }
        }
        self.builtin("XLOOKUP", args)
    }

    /// `VLOOKUP` and `HLOOKUP` over a single-area reference: evaluates only the key column (or
    /// row) and the matched result cell(s), never unused columns or unmatched rows of
    /// `table_array`. This avoids both O(rows * cols) array copies on wide tables and false
    /// `#CIRC!` cycles through columns `col_index_num` never reads.
    pub(super) fn vh_lookup(&mut self, vertical: bool, args: &[Expr]) -> Ev {
        if (3..=4).contains(&args.len())
            && let Ev::R(areas) = self.eval(&args[1])
            && let [area] = areas.as_slice()
        {
            let area = *area;
            let v0 = self.value(&args[0]);
            let v2 = self.value(&args[2]);
            let v3 = args.get(3).map(|e| self.value(e));
            let max_len = if vertical { area.range.width() } else { area.range.height() } as usize;
            let eff = self.read_range(&area);
            let key_area = Area {
                sheet: area.sheet,
                range: if vertical {
                    RangeRef::new(eff.start, CellRef::new(eff.end.row, eff.start.col))
                } else {
                    RangeRef::new(eff.start, CellRef::new(eff.start.row, eff.end.col))
                },
            };
            let keys_val = self.area_value(&key_area);
            let read_hit = |this: &mut Self, pos: usize, idx: usize| -> Value {
                let c = if vertical {
                    CellRef::new(area.range.start.row + pos as u32, area.range.start.col + idx as u32)
                } else {
                    CellRef::new(area.range.start.row + idx as u32, area.range.start.col + pos as u32)
                };
                this.host.cell_value(area.sheet, c)
            };
            let lifted = matches!(v0, Value::Array(_)) || matches!(v2, Value::Array(_)) || matches!(v3, Some(Value::Array(_)));
            if !lifted {
                return Ev::V(match gridcraft_functions::vh_lookup_pos(&v0, &keys_val, &v2, v3.as_ref(), max_len) {
                    Ok((pos, idx)) => read_hit(self, pos, idx),
                    Err(e) => Value::Error(e),
                });
            }
            let as_arr = |v: &Value| match v {
                Value::Array(a) => Arc::clone(a),
                v => Arc::new(Array::scalar(v.clone())),
            };
            let a0 = as_arr(&v0);
            let a2 = as_arr(&v2);
            let a3 = v3.as_ref().map(as_arr);
            let mut h = a0.rows.max(a2.rows);
            let mut w = a0.cols.max(a2.cols);
            if let Some(a) = &a3 {
                h = h.max(a.rows);
                w = w.max(a.cols);
            }
            if (h as u64).saturating_mul(w as u64) > MAX_CELLS {
                return err(CellError::Num);
            }
            let mut out = Vec::with_capacity(h * w);
            for r in 0..h {
                for c in 0..w {
                    let n0 = a0.get_broadcast(r, c);
                    let n2 = a2.get_broadcast(r, c);
                    let n3 = a3.as_ref().map(|a| a.get_broadcast(r, c));
                    let v = match gridcraft_functions::vh_lookup_pos(&n0, &keys_val, &n2, n3.as_ref(), max_len) {
                        Ok((pos, idx)) => read_hit(self, pos, idx),
                        Err(e) => Value::Error(e),
                    };
                    out.push(v);
                }
            }
            return Ev::V(Array::new(h, w, out).map(Value::from).unwrap_or(Value::Error(CellError::Value)));
        }
        self.builtin(if vertical { "VLOOKUP" } else { "HLOOKUP" }, args)
    }

    /// `LOOKUP` over reference vector/array arguments: evaluates only the search vector and the
    /// single matched result cell.
    pub(super) fn lookup_call(&mut self, args: &[Expr]) -> Ev {
        if args.len() == 2
            && let Ev::R(areas) = self.eval(&args[1])
            && let [area] = areas.as_slice()
        {
            let area = *area;
            let v0 = self.value(&args[0]);
            if !matches!(v0, Value::Array(_)) {
                let eff = self.read_range(&area);
                let wide = area.range.width() > area.range.height();
                let key_area = Area {
                    sheet: area.sheet,
                    range: if wide {
                        RangeRef::new(eff.start, CellRef::new(eff.start.row, eff.end.col))
                    } else {
                        RangeRef::new(eff.start, CellRef::new(eff.end.row, eff.start.col))
                    },
                };
                let keys_val = self.area_value(&key_area);
                return Ev::V(match gridcraft_functions::lookup_vec_pos(&v0, &keys_val) {
                    Ok(pos) => {
                        let c = if wide {
                            CellRef::new(area.range.end.row, area.range.start.col + pos as u32)
                        } else {
                            CellRef::new(area.range.start.row + pos as u32, area.range.end.col)
                        };
                        self.host.cell_value(area.sheet, c)
                    }
                    Err(e) => Value::Error(e),
                });
            }
        } else if args.len() == 3
            && let (Ev::R(la), Ev::R(ra)) = (self.eval(&args[1]), self.eval(&args[2]))
            && let ([l], [r]) = (la.as_slice(), ra.as_slice())
        {
            let (l, r) = (*l, *r);
            let v0 = self.value(&args[0]);
            if !matches!(v0, Value::Array(_)) {
                let eff_l = self.read_range(&l);
                let key_range = if l.range.height() == 1 || l.range.width() == 1 {
                    eff_l
                } else {
                    RangeRef::new(eff_l.start, CellRef::new(eff_l.end.row, eff_l.start.col))
                };
                let key_len = key_range.count() as usize;
                let keys_val = self.area_value(&Area { sheet: l.sheet, range: key_range });
                return Ev::V(match gridcraft_functions::lookup_vec_pos(&v0, &keys_val) {
                    Ok(pos) => {
                        let target = if r.range.height() == 1 {
                            (pos < r.range.width() as usize).then(|| CellRef::new(r.range.start.row, r.range.start.col + pos as u32))
                        } else if r.range.width() == 1 || r.range.height() as usize >= key_len {
                            (pos < r.range.height() as usize).then(|| CellRef::new(r.range.start.row + pos as u32, r.range.start.col))
                        } else {
                            None
                        };
                        match target {
                            Some(c) => self.host.cell_value(r.sheet, c),
                            None => Value::Error(CellError::NA),
                        }
                    }
                    Err(e) => Value::Error(e),
                });
            }
        }
        self.builtin("LOOKUP", args)
    }

    pub(super) fn cell_info(&mut self, args: &[Expr]) -> Ev {
        let kind = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t.to_ascii_lowercase(),
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let area = match args.get(1).map(|a| self.eval(a)) {
            Some(Ev::R(a)) => a.first().copied(),
            None => Some(Area { sheet: self.sheet, range: RangeRef::cell(self.at) }),
            _ => return err(CellError::Value),
        };
        let Some(a) = area else { return err(CellError::Ref) };
        let c = a.range.start;
        let wb = self.host.workbook();
        let sheet = wb.sheet(a.sheet);
        match kind.as_str() {
            "address" => Ev::V(Value::text(format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1))),
            "row" => Ev::V(Value::number(c.row as f64 + 1.0)),
            "col" => Ev::V(Value::number(c.col as f64 + 1.0)),
            "contents" => Ev::V(self.host.cell_value(a.sheet, c)),
            "type" => {
                let v = self.host.cell_value(a.sheet, c);
                Ev::V(Value::text(match v {
                    Value::Empty => "b",
                    Value::Text(_) => "l",
                    _ => "v",
                }))
            }
            "width" => Ev::V(Value::number(sheet.map_or(8.43, |s| gridcraft_model::points_to_chars(s.col_width(c.col) as f64)).round())),
            // Excel's `<folder>/[<file>]<sheet>`, which the classic sheet-name formula splits on
            // the `]`. Empty until the workbook has a file of its own, as in Excel (#1489).
            "filename" => Ev::V(Value::text(match &wb.source_path {
                Some(file) => {
                    let name = file.file_name().unwrap_or_default().to_string_lossy();
                    let on = sheet.map_or("", |s| s.name.as_str());
                    file.parent().unwrap_or(std::path::Path::new("")).join(format!("[{name}]{on}")).to_string_lossy().into_owned()
                }
                None => String::new(),
            })),
            "sheetname" => Ev::V(Value::text(sheet.map(|s| s.name.clone()).unwrap_or_default())),
            "protect" => Ev::V(Value::number(if sheet.is_some_and(|s| wb.styles.get(s.style_id(c)).protection.locked) { 1.0 } else { 0.0 })),
            "format" => {
                let code = sheet.map(|s| wb.styles.get(s.style_id(c)).num_fmt.as_str().to_string()).unwrap_or_default();
                Ev::V(Value::text(cell_format_code(&code)))
            }
            "color" | "parentheses" => Ev::V(Value::Number(0.0)),
            "prefix" => Ev::V(Value::text("")),
            _ => err(CellError::Value),
        }
    }

    pub(super) fn subtotal(&mut self, name: &str, args: &[Expr]) -> Ev {
        // SUBTOTAL needs function_num and ref1; AGGREGATE needs function_num, options and ref1.
        let min_args = if name == "AGGREGATE" { 3 } else { 2 };
        if args.len() < min_args {
            return err(CellError::Value);
        }
        let code = match self.arg_num(args, 0, 0.0) {
            Ok(n) => n.trunc() as u32,
            Err(e) => return err(e),
        };
        let (func, options, rest) = if name == "AGGREGATE" {
            let opt = match self.arg_num(args, 1, 0.0) {
                Ok(n) => n.trunc() as u32,
                Err(e) => return err(e),
            };
            (code, opt, &args[2..])
        } else {
            let (f, ignore_hidden) = if code > 100 { (code - 100, true) } else { (code, false) };
            (f, if ignore_hidden { 5 } else { 0 }, &args[1..])
        };
        if !(1..=19).contains(&func) || (name == "SUBTOTAL" && func > 11) {
            return err(CellError::Value);
        }
        let skip_hidden = matches!(options, 1 | 3 | 5 | 7);
        // SUBTOTAL leaves out rows a filter hides whatever the function number (Microsoft's
        // SUBTOTAL documentation); 101-111 also leave out rows hidden by hand.
        let skip_filtered = name == "SUBTOTAL";
        let skip_errors = matches!(options, 2 | 3 | 6 | 7);
        // SUBTOTAL always leaves out nested SUBTOTAL/AGGREGATE cells (101-111 too); AGGREGATE does so for options 0-3.
        let skip_nested = name == "SUBTOTAL" || options <= 3;
        let mut values = Vec::new();
        let mut k = None;
        let refs = if name == "AGGREGATE" && func >= 14 { &rest[..1.min(rest.len())] } else { rest };
        if name == "AGGREGATE" && func >= 14 {
            k = rest.get(1).map(|e| self.value(e));
        }
        // Every cell visited (even one skipped as hidden, nested or an error) and every value
        // counts against one budget for the whole call. All arguments are evaluated (references
        // stay lazy) and charged before any cell is read: a value stored in XFD1048576 makes
        // `A:XFD` trim to the whole sheet, ~17 billion cells (#2175).
        enum Input {
            Cells(usize, RangeRef),
            Values(Value),
        }
        let mut inputs = Vec::new();
        let mut budget = MAX_CELLS;
        for e in refs {
            match self.eval(e) {
                Ev::R(areas) => {
                    for a in areas {
                        let range = self.trim(&a);
                        budget = match budget.checked_sub(range.count()) {
                            Some(left) => left,
                            None => return err(CellError::Num),
                        };
                        inputs.push(Input::Cells(a.sheet, range));
                    }
                }
                Ev::V(v) => {
                    let n = match &v {
                        Value::Array(a) => u64::try_from(a.data.len()).unwrap_or(u64::MAX),
                        _ => 1,
                    };
                    budget = match budget.checked_sub(n) {
                        Some(left) => left,
                        None => return err(CellError::Num),
                    };
                    inputs.push(Input::Values(v));
                }
                Ev::L(_) => return err(CellError::Value),
            }
        }
        for input in inputs {
            match input {
                Input::Cells(sheet, range) => {
                    for c in range.iter() {
                        let (hidden, nested) = {
                            let wb = self.host.workbook();
                            let s = wb.sheet(sheet);
                            let hidden =
                                s.is_some_and(|s| if skip_hidden { s.is_row_hidden(c.row) } else { skip_filtered && s.is_row_filtered(c.row) });
                            let nested = skip_nested
                                && s.and_then(|s| s.cell(c)).and_then(|cell| cell.formula.as_ref()).is_some_and(|f| {
                                    let u = f.text.to_ascii_uppercase();
                                    u.contains("SUBTOTAL(") || u.contains("AGGREGATE(")
                                });
                            (hidden, nested)
                        };
                        if hidden || nested {
                            continue;
                        }
                        let v = self.host.cell_value(sheet, c);
                        if skip_errors && v.is_error() {
                            continue;
                        }
                        values.push(v);
                    }
                }
                Input::Values(Value::Array(a)) => values.extend(a.data.iter().filter(|v| !(skip_errors && v.is_error())).cloned()),
                Input::Values(v) => values.push(v),
            }
        }
        if let Some(k) = k {
            return Ev::V(gridcraft_functions::aggregate_values_k(func, &values, &k));
        }
        Ev::V(gridcraft_functions::aggregate_values(func, &values))
    }
}

/// Whether INDEX on an array picks a whole row or column of several values at position
/// (`row`, `col`), which the array-of-positions form answers with #CALC!.
fn index_picks_several_values(src: &Ev, row: i64, col: i64, two_args: bool) -> bool {
    let Ev::V(Value::Array(arr)) = src else { return false };
    let (row, col) = if two_args && arr.rows == 1 { (1, row) } else { (row, col) };
    if row < 0 || col < 0 || row as u64 > arr.rows as u64 || col as u64 > arr.cols as u64 {
        return false;
    }
    (row == 0 && arr.rows > 1) || (col == 0 && arr.cols > 1)
}

#[cfg(test)]
mod tests {
    use super::super::Host;
    use super::*;
    use gridcraft_model::Workbook;

    /// A sheet of blanks that counts the blocks it is asked for.
    struct Counting {
        wb: Workbook,
        cells_read: usize,
    }
    impl Host for Counting {
        fn workbook(&self) -> &Workbook {
            &self.wb
        }
        fn cell_value(&mut self, _sheet: usize, _c: CellRef) -> Value {
            Value::Empty
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
        fn range_values(&mut self, _sheet: usize, range: RangeRef) -> Vec<Value> {
            self.cells_read += range.count() as usize;
            vec![Value::Empty; range.count() as usize]
        }
    }

    fn eval(src: &str) -> (Value, usize) {
        let mut host = Counting { wb: Workbook::default(), cells_read: 0 };
        let mut ev = Evaluator::new(&mut host, 0, CellRef::new(0, 0));
        let v = ev.value(&gridcraft_formula::parser::parse(src).unwrap_or_else(|e| panic!("{e}")));
        (v, host.cells_read)
    }

    fn all_calc(v: &Value, n: usize) -> bool {
        matches!(v, Value::Array(a) if a.data.len() == n && a.data.iter().all(|x| *x == Value::Error(CellError::Calc)))
    }

    #[test]
    fn index_positions_that_pick_several_cells_are_calc_without_reading_them() {
        // Each zero row position picks the whole of A1:A50000: #CALC!, decided from the shape. It
        // used to read the 50,000 cells once per position before discarding them.
        let (v, read) = eval("=INDEX(A1:A50000,{0,0,0,0},1)");
        assert!(all_calc(&v, 4), "{v:?}");
        assert_eq!(read, 0);
        // The same for an array source: a whole column of a 3x3 array.
        let (v, _) = eval("=INDEX({1,2,3;4,5,6;7,8,9},{0,0},2)");
        assert!(all_calc(&v, 2), "{v:?}");
        // Single cells are still read.
        let (v, _) = eval("=INDEX({1,2,3;4,5,6;7,8,9},{1,3},2)");
        assert!(matches!(&v, Value::Array(a) if a.data == vec![Value::number(2.0), Value::number(8.0)]), "{v:?}");
    }
}
