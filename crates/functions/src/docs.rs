//! Documentation for every function the workbook supports: the native library (see [`all`]) and
//! the special forms the calculation engine evaluates itself because they need references,
//! laziness or hidden-row information.

use std::sync::LazyLock;

use crate::{Category, all};

/// One entry of the function list.
#[derive(Clone, Copy, Debug)]
pub struct FnDoc {
    /// Canonical upper-case name, without any `_xlfn.` prefix.
    pub name: &'static str,
    pub category: Category,
    pub signature: &'static str,
    pub description: &'static str,
    /// Evaluated by the calculation engine rather than by [`call`](crate::call). A special form
    /// may also have a native implementation (`INDEX`); its documentation then comes from the
    /// native entry.
    pub special: bool,
}

const fn special(name: &'static str, category: Category, signature: &'static str, description: &'static str) -> FnDoc {
    FnDoc { name, category, signature, description, special: true }
}

/// The special forms.
const SPECIALS: &[FnDoc] = &[
    special(
        "IF",
        Category::Logical,
        "IF(logical_test, [value_if_true], [value_if_false])",
        "Returns one value if a condition is true and another if it's false.",
    ),
    special("IFS", Category::Logical, "IFS(test1, value1, ...)", "Returns the value of the first true condition."),
    special("IFERROR", Category::Logical, "IFERROR(value, value_if_error)", "Returns a fallback when a value is an error."),
    special("IFNA", Category::Logical, "IFNA(value, value_if_na)", "Returns a fallback when a value is #N/A."),
    special("CHOOSE", Category::Lookup, "CHOOSE(index_num, value1, [value2], ...)", "Picks a value from a list by position."),
    special("SWITCH", Category::Logical, "SWITCH(expression, value1, result1, ..., [default])", "Matches a value against a list of cases."),
    special("ROW", Category::Lookup, "ROW([reference])", "Row number of a reference."),
    special("COLUMN", Category::Lookup, "COLUMN([reference])", "Column number of a reference."),
    special("ROWS", Category::Lookup, "ROWS(array)", "Number of rows in a reference or array."),
    special("COLUMNS", Category::Lookup, "COLUMNS(array)", "Number of columns in a reference or array."),
    special("OFFSET", Category::Lookup, "OFFSET(reference, rows, cols, [height], [width])", "A reference shifted from a starting point."),
    special("INDIRECT", Category::Lookup, "INDIRECT(ref_text, [a1])", "The reference named by a text string."),
    special("INDEX", Category::Lookup, "INDEX(array, row_num, [column_num], [area_num])", "The value or reference at a position."),
    special("SUBTOTAL", Category::MathTrig, "SUBTOTAL(function_num, ref1, ...)", "A subtotal that can skip hidden rows."),
    special("AGGREGATE", Category::MathTrig, "AGGREGATE(function_num, options, ref1, ...)", "An aggregate that can skip hidden rows and errors."),
    special("TEXT", Category::Text, "TEXT(value, format_text)", "Formats a number as text."),
    special("LET", Category::Logical, "LET(name1, value1, ..., calculation)", "Names intermediate results."),
    special("LAMBDA", Category::Logical, "LAMBDA([parameter1, ...], calculation)", "Creates a reusable custom function."),
    special("MAP", Category::Logical, "MAP(array1, ..., lambda)", "Applies a LAMBDA to each value."),
    special("REDUCE", Category::Logical, "REDUCE(initial_value, array, lambda)", "Accumulates an array into one value."),
    special("SCAN", Category::Logical, "SCAN(initial_value, array, lambda)", "Running accumulation of an array."),
    special("BYROW", Category::Logical, "BYROW(array, lambda)", "Applies a LAMBDA to each row."),
    special("BYCOL", Category::Logical, "BYCOL(array, lambda)", "Applies a LAMBDA to each column."),
    special("MAKEARRAY", Category::Logical, "MAKEARRAY(rows, cols, lambda)", "Builds an array from a LAMBDA."),
    special("CELL", Category::Information, "CELL(info_type, [reference])", "Information about a cell."),
    special("ISREF", Category::Information, "ISREF(value)", "TRUE for references."),
    special("ISFORMULA", Category::Information, "ISFORMULA(reference)", "TRUE when the cell has a formula."),
    special("FORMULATEXT", Category::Lookup, "FORMULATEXT(reference)", "The formula of a cell as text."),
    special("SHEET", Category::Information, "SHEET([value])", "Sheet number."),
    special("SHEETS", Category::Information, "SHEETS([reference])", "Number of sheets."),
    special("AREAS", Category::Lookup, "AREAS(reference)", "Number of areas in a reference."),
    special("ISOMITTED", Category::Information, "ISOMITTED(argument)", "TRUE when a LAMBDA argument was left out."),
];

fn build() -> Vec<FnDoc> {
    let mut v: Vec<FnDoc> = all()
        .iter()
        .map(|f| FnDoc {
            name: f.name,
            category: f.category,
            signature: f.signature,
            description: f.description,
            special: SPECIALS.iter().any(|s| s.name == f.name),
        })
        .collect();
    for s in SPECIALS {
        if !v.iter().any(|d| d.name == s.name) {
            v.push(*s);
        }
    }
    v.sort_by(|a, b| a.name.cmp(b.name));
    v
}

/// Every native function and special form, sorted by name.
pub fn docs() -> &'static [FnDoc] {
    static DOCS: LazyLock<Vec<FnDoc>> = LazyLock::new(build);
    &DOCS
}

/// Whether `name` (upper-case) is a special form the calculation engine evaluates itself.
pub fn is_special(name: &str) -> bool {
    let d = docs();
    d.binary_search_by(|x| x.name.cmp(name)).ok().and_then(|i| d.get(i)).is_some_and(|x| x.special)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_cover_natives_and_specials() {
        let d = docs();
        assert!(d.windows(2).all(|w| w[0].name < w[1].name), "sorted, no duplicates");
        for f in all() {
            assert!(d.iter().any(|x| x.name == f.name), "{} missing", f.name);
        }
        for s in SPECIALS {
            assert!(is_special(s.name), "{} not special", s.name);
        }
        assert_eq!(d.iter().filter(|x| x.special).count(), SPECIALS.len());
        assert!(!is_special("SUM"));
        assert!(!is_special("NOSUCH"));
    }

    #[test]
    fn special_forms_that_are_also_native_keep_the_native_documentation() {
        let index = docs().iter().find(|x| x.name == "INDEX");
        assert!(index.is_some_and(|x| x.special && x.signature.starts_with("INDEX(")));
    }
}
