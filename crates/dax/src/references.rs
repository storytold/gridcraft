//! Model references as DAX text, and the formulas of Excel's implicit measures.
//!
//! The model window lists every visible column with a summarization, and a model PivotTable
//! summarizes a column the same way. Both spell the column the same way so the evaluator reads
//! them identically: this module owns the one spelling.

/// Reserved words the lexer or parser turns into operators and statement keywords: a table with
/// one of these names must be quoted, or `SUM(In[Amount])` would not parse.
const RESERVED: [&str; 7] = ["NOT", "IN", "EVALUATE", "VAR", "RETURN", "TRUE", "FALSE"];

/// A DAX table reference for a model name, quoted when the name is not a plain identifier or is a
/// reserved word.
pub fn table_ref(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !RESERVED.iter().any(|word| word.eq_ignore_ascii_case(name));
    if plain { name.to_string() } else { format!("'{}'", name.replace('\'', "''")) }
}

/// A DAX column reference for a model name; a `]` in the name is escaped as `]]`.
pub fn column_ref(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

/// The formula Excel's implicit measure over `table[column]` evaluates, or `None` when
/// `aggregate` is not one of Excel's five summarizations.
///
/// `Count` is `COUNT`, not `COUNTROWS`: Excel counts the column's non-blank values, so the model
/// window and a model PivotTable show the same number under `Count of <column>`.
pub fn implicit_measure_formula(aggregate: &str, table: &str, column: &str) -> Option<String> {
    let function = match aggregate.trim().to_ascii_lowercase().as_str() {
        "sum" => "SUM",
        "average" => "AVERAGE",
        "count" => "COUNT",
        "min" => "MIN",
        "max" => "MAX",
        _ => return None,
    };
    Some(format!("{function}({}{})", table_ref(table), column_ref(column)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_quote_what_dax_needs_quoted() {
        assert_eq!(table_ref("Sales"), "Sales");
        assert_eq!(table_ref("Sales Data"), "'Sales Data'");
        assert_eq!(table_ref("O'Brien"), "'O''Brien'");
        assert_eq!(table_ref("In"), "'In'");
        assert_eq!(table_ref("VAR"), "'VAR'");
        // A bare `TRUE`/`FALSE` would parse as a Boolean literal, not as a table.
        assert_eq!(table_ref("TRUE"), "'TRUE'");
        assert_eq!(table_ref("False"), "'False'");
        assert_eq!(column_ref("Amount"), "[Amount]");
        assert_eq!(column_ref("a]b"), "[a]]b]");
    }

    #[test]
    fn implicit_measures_count_non_blank_values() {
        assert_eq!(implicit_measure_formula("Sum", "Sales", "Amount"), Some("SUM(Sales[Amount])".to_string()));
        assert_eq!(implicit_measure_formula("count", "Sales Data", "Region"), Some("COUNT('Sales Data'[Region])".to_string()));
        assert_eq!(implicit_measure_formula("Average", "Sales", "Amount"), Some("AVERAGE(Sales[Amount])".to_string()));
        assert_eq!(implicit_measure_formula("Min", "Sales", "Amount"), Some("MIN(Sales[Amount])".to_string()));
        assert_eq!(implicit_measure_formula("Max", "Sales", "Amount"), Some("MAX(Sales[Amount])".to_string()));
        assert_eq!(implicit_measure_formula("None", "Sales", "Amount"), None);
    }
}
