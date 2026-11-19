//! A function's parameters as the Function Arguments dialog lists them: read from its signature
//! (which ones are optional, which run a `...` repeats), with a description of each parameter of
//! the common functions.

/// The most arguments a call takes, as in Excel.
pub const MAX_ARGS: usize = 255;

/// One parameter, as the signature writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    /// With its index, if any: `number1`, `value_if_true`, `criteria_range2`.
    pub name: String,
    pub optional: bool,
}

impl Param {
    /// The name without its index (`number2` is `number`), as descriptions are keyed.
    pub fn base(&self) -> &str {
        base(&self.name)
    }
}

/// A function's parameters, and the run of them a `...` repeats (start and length in `params`).
/// Parameters after that run (LET's `calculation`) stay last however often it repeats.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Params {
    pub params: Vec<Param>,
    pub repeat: Option<(usize, usize)>,
}

impl Params {
    /// Reads `SUMIFS(sum_range, criteria_range1, criteria1, ...)`: brackets mark optional
    /// parameters, and may span several (`[field1, item1]`); `...` repeats the parameters before
    /// it that share the last one's index. A signature without parentheses has no parameters.
    pub fn parse(signature: &str) -> Params {
        let Some(inner) = signature.split_once('(').and_then(|(_, rest)| rest.trim_end().strip_suffix(')')) else {
            return Params::default();
        };
        let mut params = Vec::new();
        let mut depth = 0usize;
        let mut ellipsis = None;
        for raw in inner.split(',') {
            let mut arg = raw.trim();
            if let Some(rest) = arg.strip_prefix('[') {
                depth += 1;
                arg = rest;
            }
            let optional = depth > 0;
            if let Some(rest) = arg.strip_suffix(']') {
                depth = depth.saturating_sub(1);
                arg = rest;
            }
            let arg = arg.trim();
            if arg == "..." {
                ellipsis.get_or_insert(params.len());
            } else if !arg.is_empty() {
                params.push(Param { name: arg.to_string(), optional });
            }
        }
        let repeat = ellipsis.filter(|&at| at > 0).map(|at| repeated_run(&params, at));
        Params { params, repeat }
    }

    /// Parameters for a function the catalog doesn't describe (a user-defined one): `arg1`,
    /// `arg2`… for as many arguments as it is given.
    pub fn generic() -> Params {
        Params { params: vec![Param { name: "arg1".into(), optional: true }], repeat: Some((0, 1)) }
    }

    /// The parameters of `count` fields: those written, with the repeating run given as many
    /// times as the fields need (`number3`, `number4`…, optional), the parameters after it last.
    /// Fields beyond what a function takes are `arg5`, `arg6`…, so no argument given is lost.
    pub fn fields(&self, count: usize) -> Vec<Param> {
        let count = count.min(MAX_ARGS);
        let Some((start, len)) = self.repeat.filter(|&(_, len)| len > 0) else {
            let extra = (self.params.len()..count).map(|i| Param { name: format!("arg{}", i + 1), optional: true });
            return self.params.iter().cloned().chain(extra).collect();
        };
        let tail = self.params.len().saturating_sub(start + len);
        let instances = count.saturating_sub(start + tail).div_ceil(len).max(1);
        let head = self.params.iter().take(start).cloned();
        let run = self.params.get(start..start + len).unwrap_or_default();
        let repeated = (0..instances).flat_map(|j| run.iter().map(move |p| instance(p, j)));
        head.chain(repeated).chain(self.params.iter().skip(start + len).cloned()).collect()
    }

    /// Lays argument texts out on the fields: missing fields are blank, a repeating run is padded
    /// to whole instances before the parameters that follow it, and once its last instance holds
    /// anything one blank instance follows, as the dialog offers `number3` once `number2` is
    /// filled in.
    pub fn arrange(&self, values: &mut Vec<String>) {
        values.truncate(MAX_ARGS);
        if values.len() < self.params.len() {
            values.resize(self.params.len(), String::new());
        }
        let Some((start, len)) = self.repeat.filter(|&(_, len)| len > 0) else { return };
        let tail = self.params.len().saturating_sub(start + len);
        let run_len = values.len().saturating_sub(start + tail);
        let missing = run_len.div_ceil(len) * len - run_len;
        let at = values.len() - tail;
        values.splice(at..at, std::iter::repeat_n(String::new(), missing));
        let at = values.len() - tail;
        let last_filled = values.get(at.saturating_sub(len)..at).is_some_and(|last| last.iter().any(|v| !v.trim().is_empty()));
        if last_filled && values.len() + len <= MAX_ARGS {
            values.splice(at..at, std::iter::repeat_n(String::new(), len));
        }
    }

    /// The arguments a call is written with, from the fields: blank fields at the end are left
    /// out (before the parameters after a repeating run, and at the very end), blank fields
    /// between given ones stay as empty arguments.
    pub fn written<'a>(&self, values: &'a [String]) -> Vec<&'a str> {
        let tail = match self.repeat {
            Some((start, len)) if values.len() >= self.params.len() => self.params.len().saturating_sub(start + len),
            _ => 0,
        };
        let split = values.len() - tail;
        let mut out: Vec<&str> = values.iter().take(split).map(|v| v.trim()).collect();
        trim_blank_end(&mut out);
        let mut last: Vec<&str> = values.iter().skip(split).map(|v| v.trim()).collect();
        trim_blank_end(&mut last);
        if !last.is_empty() {
            out.extend(last);
        }
        out
    }
}

fn trim_blank_end(args: &mut Vec<&str>) {
    while args.last().is_some_and(|a| a.is_empty()) {
        args.pop();
    }
}

/// `number2` -> `number`.
fn base(name: &str) -> &str {
    name.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// `number2` -> 2.
fn index(name: &str) -> Option<usize> {
    name.get(base(name).len()..).and_then(|digits| digits.parse().ok())
}

/// The run of parameters ending before `at` that share the index of the one just before it:
/// `[number2]` of SUM, `criteria_range1, criteria1` of SUMIFS.
fn repeated_run(params: &[Param], at: usize) -> (usize, usize) {
    let last = at.checked_sub(1).and_then(|i| params.get(i)).and_then(|p| index(&p.name));
    let mut start = at.saturating_sub(1);
    while last.is_some() && start > 0 && params.get(start - 1).and_then(|p| index(&p.name)) == last {
        start -= 1;
    }
    (start, at - start)
}

/// The `j`th instance of a repeated parameter (0 is the one written).
fn instance(p: &Param, j: usize) -> Param {
    if j == 0 {
        return p.clone();
    }
    Param { name: format!("{}{}", base(&p.name), index(&p.name).unwrap_or(1).saturating_add(j)), optional: true }
}

const CRITERIA: &str = "The condition a cell must meet: a number, an expression or text, such as 32, \">32\", B4 or \"apples\".";
const CRITERIA_RANGE: &str = "A range whose cells are tested against the criteria that follows it.";

/// What each parameter of the common functions is for, by function and parameter name without
/// its index.
const HELP: &[(&str, &[(&str, &str)])] = &[
    ("SUM", &[("number", "A number, cell, range or expression to add. Text and logical values in ranges are skipped.")]),
    ("AVERAGE", &[("number", "A number, cell or range to average. Text, logical values and blank cells in ranges are skipped.")]),
    ("MAX", &[("number", "A number, cell or range among which to find the largest number.")]),
    ("MIN", &[("number", "A number, cell or range among which to find the smallest number.")]),
    ("COUNT", &[("value", "A value, cell or range whose numbers are counted.")]),
    ("COUNTA", &[("value", "A value, cell or range whose non-blank entries are counted.")]),
    (
        "IF",
        &[
            ("logical_test", "The condition to check: any value or expression that is TRUE or FALSE."),
            ("value_if_true", "The result when the condition is TRUE."),
            ("value_if_false", "The result when the condition is FALSE. Left out, IF returns FALSE."),
        ],
    ),
    (
        "IFERROR",
        &[
            ("value", "The value, reference or expression to check for an error."),
            ("value_if_error", "The result when the value is an error, such as #N/A, #VALUE! or #DIV/0!."),
        ],
    ),
    ("IFNA", &[("value", "The value, reference or expression to check for #N/A."), ("value_if_na", "The result when the value is #N/A.")]),
    (
        "IFS",
        &[
            ("logical_test", "A condition that is TRUE or FALSE; the first one that is TRUE gives the result."),
            ("value_if_true", "The result when the condition before it is the first that is TRUE."),
        ],
    ),
    ("AND", &[("logical", "A condition, value or range that is TRUE or FALSE. AND is TRUE when all of them are.")]),
    ("OR", &[("logical", "A condition, value or range that is TRUE or FALSE. OR is TRUE when any of them is.")]),
    ("NOT", &[("logical", "A value or expression that is TRUE or FALSE, to turn the other way round.")]),
    (
        "VLOOKUP",
        &[
            ("lookup_value", "The value to find in the first column of the table."),
            ("table_array", "The table to search, with the values looked up in its first column."),
            ("col_index_num", "Which column of the table to return a value from; its first column is 1."),
            (
                "range_lookup",
                "FALSE finds an exact match. TRUE or left out finds the closest smaller value, which needs the first column sorted ascending.",
            ),
        ],
    ),
    (
        "HLOOKUP",
        &[
            ("lookup_value", "The value to find in the first row of the table."),
            ("table_array", "The table to search, with the values looked up in its first row."),
            ("row_index_num", "Which row of the table to return a value from; its first row is 1."),
            (
                "range_lookup",
                "FALSE finds an exact match. TRUE or left out finds the closest smaller value, which needs the first row sorted ascending.",
            ),
        ],
    ),
    (
        "XLOOKUP",
        &[
            ("lookup_value", "The value to search for."),
            ("lookup_array", "The row, column or array to search."),
            ("return_array", "The range or array to return a value from, as long as the lookup array."),
            ("if_not_found", "What to return when nothing matches. Left out, the result is #N/A."),
            (
                "match_mode",
                "0 or left out: exact match. -1: exact match or the next smaller value. 1: exact match or the next larger value. 2: wildcards *, ? and ~.",
            ),
            (
                "search_mode",
                "1 or left out: first to last. -1: last to first. 2: binary search of ascending values. -2: binary search of descending values.",
            ),
        ],
    ),
    (
        "XMATCH",
        &[
            ("lookup_value", "The value to search for."),
            ("lookup_array", "The row, column or array to search."),
            (
                "match_mode",
                "0 or left out: exact match. -1: exact match or the next smaller value. 1: exact match or the next larger value. 2: wildcards *, ? and ~.",
            ),
            (
                "search_mode",
                "1 or left out: first to last. -1: last to first. 2: binary search of ascending values. -2: binary search of descending values.",
            ),
        ],
    ),
    (
        "MATCH",
        &[
            ("lookup_value", "The value to find."),
            ("lookup_array", "The single row or column to search."),
            (
                "match_type",
                "1 or left out: the largest value up to the lookup value (sorted ascending). 0: an exact match. -1: the smallest value from the lookup value up (sorted descending).",
            ),
        ],
    ),
    (
        "INDEX",
        &[
            ("array", "The range or array to take a value from."),
            ("row_num", "Which row to return, counting from 1; 0 returns the whole column."),
            ("column_num", "Which column to return, counting from 1; 0 returns the whole row."),
            ("area_num", "For a reference made of several areas, which area to use; the first is 1."),
        ],
    ),
    ("CHOOSE", &[("index_num", "Which value to return, counting from 1."), ("value", "A value, reference or expression to choose from.")]),
    ("COUNTIF", &[("range", "The cells to count in."), ("criteria", CRITERIA)]),
    ("COUNTIFS", &[("criteria_range", CRITERIA_RANGE), ("criteria", CRITERIA)]),
    (
        "SUMIF",
        &[
            ("range", "The cells tested against the criteria."),
            ("criteria", CRITERIA),
            ("sum_range", "The cells to add. Left out, the cells of the range are added."),
        ],
    ),
    ("SUMIFS", &[("sum_range", "The cells to add."), ("criteria_range", CRITERIA_RANGE), ("criteria", CRITERIA)]),
    (
        "AVERAGEIF",
        &[
            ("range", "The cells tested against the criteria."),
            ("criteria", CRITERIA),
            ("average_range", "The cells to average. Left out, the cells of the range are averaged."),
        ],
    ),
    ("AVERAGEIFS", &[("average_range", "The cells to average."), ("criteria_range", CRITERIA_RANGE), ("criteria", CRITERIA)]),
    (
        "ROUND",
        &[
            ("number", "The number to round."),
            ("num_digits", "How many decimal places to round to: 0 rounds to a whole number, a negative number to tens, hundreds and so on."),
        ],
    ),
    (
        "ROUNDUP",
        &[
            ("number", "The number to round away from zero."),
            ("num_digits", "How many decimal places to round to: 0 rounds to a whole number, a negative number to tens, hundreds and so on."),
        ],
    ),
    (
        "ROUNDDOWN",
        &[
            ("number", "The number to round toward zero."),
            ("num_digits", "How many decimal places to round to: 0 rounds to a whole number, a negative number to tens, hundreds and so on."),
        ],
    ),
    (
        "TEXT",
        &[
            ("value", "The number, date or time to format."),
            ("format_text", "A number format as text, such as \"0.00\", \"#,##0\" or \"yyyy-mm-dd\"."),
        ],
    ),
    (
        "DATE",
        &[
            ("year", "The year. A year from 0 to 1899 is counted from 1900."),
            ("month", "The month, 1 to 12. Months beyond that carry into the years after or before."),
            ("day", "The day of the month. Days beyond the month carry into the months after or before."),
        ],
    ),
    ("CONCAT", &[("text", "A text, number, cell or range to join.")]),
    ("CONCATENATE", &[("text", "A text, number or cell to join.")]),
    (
        "TEXTJOIN",
        &[
            ("delimiter", "The text put between the joined values; it may be empty."),
            ("ignore_empty", "TRUE leaves empty cells out; FALSE joins them too."),
            ("text", "A text, number, cell or range to join."),
        ],
    ),
    ("LEFT", &[("text", "The text to take characters from."), ("num_chars", "How many characters to take from the start. Left out, 1.")]),
    ("RIGHT", &[("text", "The text to take characters from."), ("num_chars", "How many characters to take from the end. Left out, 1.")]),
    (
        "MID",
        &[
            ("text", "The text to take characters from."),
            ("start_num", "The position of the first character to take; the text starts at 1."),
            ("num_chars", "How many characters to take."),
        ],
    ),
    ("LEN", &[("text", "The text whose characters are counted.")]),
    ("TRIM", &[("text", "The text to remove extra spaces from.")]),
    ("SUMPRODUCT", &[("array", "A range or array whose items are multiplied with those of the others at the same position, then added.")]),
];

/// What parameter `param` (with or without its index) of `function` is for, when the catalog says.
pub fn describe(function: &str, param: &str) -> Option<&'static str> {
    let (_, params) = HELP.iter().find(|(name, _)| name.eq_ignore_ascii_case(function))?;
    params.iter().find(|(name, _)| *name == base(param)).map(|(_, text)| *text)
}

/// Every described parameter, by function, so a test can hold the descriptions to the signatures.
pub fn described() -> impl Iterator<Item = (&'static str, &'static str)> {
    HELP.iter().flat_map(|(function, params)| params.iter().map(move |(param, _)| (*function, *param)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(p: &[Param]) -> Vec<String> {
        p.iter().map(|p| if p.optional { format!("[{}]", p.name) } else { p.name.clone() }).collect()
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn signatures_give_optional_and_repeating_parameters() {
        let sum = Params::parse("SUM(number1, [number2], ...)");
        assert_eq!(names(&sum.params), ["number1", "[number2]"]);
        assert_eq!(sum.repeat, Some((1, 1)));
        assert_eq!(names(&sum.fields(4)), ["number1", "[number2]", "[number3]", "[number4]"]);
        let sumifs = Params::parse("SUMIFS(sum_range, criteria_range1, criteria1, ...)");
        assert_eq!(sumifs.repeat, Some((1, 2)));
        assert_eq!(names(&sumifs.fields(4)), ["sum_range", "criteria_range1", "criteria1", "[criteria_range2]", "[criteria2]"]);
        let iff = Params::parse("IF(logical_test, [value_if_true], [value_if_false])");
        assert_eq!(names(&iff.params), ["logical_test", "[value_if_true]", "[value_if_false]"]);
        assert_eq!(iff.repeat, None);
        let let_ = Params::parse("LET(name1, value1, ..., calculation)");
        assert_eq!(let_.repeat, Some((0, 2)));
        assert_eq!(names(&let_.fields(5)), ["name1", "value1", "[name2]", "[value2]", "calculation"]);
        let pivot = Params::parse("GETPIVOTDATA(data_field, pivot_table, [field1, item1], ...)");
        assert_eq!(names(&pivot.params), ["data_field", "pivot_table", "[field1]", "[item1]"]);
        assert_eq!(pivot.repeat, Some((2, 2)));
        let lambda = Params::parse("LAMBDA([parameter1, ...], calculation)");
        assert_eq!(names(&lambda.params), ["[parameter1]", "calculation"]);
        assert_eq!(lambda.repeat, Some((0, 1)));
        assert_eq!(Params::parse("TODAY()"), Params::default());
        assert_eq!(names(&Params::parse("ROUND(number, num_digits)").fields(3)), ["number", "num_digits", "[arg3]"]);
        assert_eq!(names(&Params::generic().fields(2)), ["[arg1]", "[arg2]"]);
    }

    #[test]
    fn a_filled_last_instance_offers_another() {
        let sum = Params::parse("SUM(number1, [number2], ...)");
        let mut v = Vec::new();
        sum.arrange(&mut v);
        assert_eq!(v, strings(&["", ""]));
        v[1] = "B1".into();
        sum.arrange(&mut v);
        assert_eq!(v, strings(&["", "B1", ""]));
        sum.arrange(&mut v);
        assert_eq!(v.len(), 3, "a blank last field stays the only one");
        let let_ = Params::parse("LET(name1, value1, ..., calculation)");
        let mut v = strings(&["x", "1", "y", "x+y"]);
        let_.arrange(&mut v);
        assert_eq!(v, strings(&["x", "1", "y", "", "", "", "x+y"]), "a run padded to whole instances, then one blank instance");
        let mut many = vec!["1".to_string(); 300];
        sum.arrange(&mut many);
        assert_eq!(many.len(), MAX_ARGS);
    }

    #[test]
    fn blank_fields_at_the_end_are_left_out() {
        let iff = Params::parse("IF(logical_test, [value_if_true], [value_if_false])");
        assert_eq!(iff.written(&strings(&["A1>0", " ", "3"])), ["A1>0", "", "3"]);
        assert_eq!(iff.written(&strings(&["A1>0", "1", ""])), ["A1>0", "1"]);
        let let_ = Params::parse("LET(name1, value1, ..., calculation)");
        assert_eq!(let_.written(&strings(&["x", "1", "", "", "x*2"])), ["x", "1", "x*2"]);
        assert_eq!(let_.written(&strings(&["x", "1", "", "", ""])), ["x", "1"]);
    }

    #[test]
    fn descriptions_ignore_the_index() {
        assert!(describe("sum", "number3").is_some_and(|d| d.contains("add")));
        assert_eq!(describe("SUMIFS", "criteria2"), describe("COUNTIFS", "criteria1"));
        assert_eq!(describe("SUM", "nothing"), None);
        assert_eq!(describe("NOSUCH", "number"), None);
    }
}
