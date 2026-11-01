//! Parse DAX from the command line (or stdin), print its syntax tree and canonical text.
//!
//! ```text
//! cargo run -p gridcraft-dax --example parse -- "1 + 2 * 3"
//! echo "EVALUATE SUM('Sales'[Amount])" | cargo run -p gridcraft-dax --example parse
//! ```

use std::io::{self, Read};

use gridcraft_dax::{TokenKind, parse_expression, parse_query, print_expression, print_query, tokenize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        io::stdin().read_to_string(&mut text)?;
    }
    let text = text.trim();
    if text.is_empty() {
        return Err("no DAX expression supplied; pass it as an argument or on stdin".into());
    }
    print!("{}", render(text)?);
    Ok(())
}

/// Parses and renders one input. Queries are preferred; when the query parser fails, a valid
/// expression is still accepted (`DEFINE` is a legal bare name), and the query diagnostic is kept
/// only when both parsers fail for input that starts with a query keyword.
fn render(text: &str) -> Result<String, String> {
    match parse_query(text) {
        Ok(query) => Ok(format!("Query\n{query:#?}\nCanonical: {}\n", print_query(&query))),
        Err(query_error) => {
            let expr = match parse_expression(text) {
                Ok(expr) => expr,
                Err(expr_error) => {
                    if starts_with_query_keyword(text) {
                        return Err(query_error.to_string());
                    }
                    return Err(expr_error.to_string());
                }
            };
            Ok(format!("AST\n{expr:#?}\nCanonical: {}\n", print_expression(&expr)))
        }
    }
}

/// Whether the input starts with a query-only keyword.
fn starts_with_query_keyword(text: &str) -> bool {
    tokenize(text).is_ok_and(|tokens| match tokens.first().map(|token| &token.kind) {
        Some(TokenKind::Evaluate) => true,
        Some(TokenKind::Ident(word)) => word.eq_ignore_ascii_case("DEFINE"),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_only_keywords_stay_on_the_query_parser() {
        assert!(starts_with_query_keyword("EVALUATE Sales"));
        assert!(starts_with_query_keyword("DEFINE MEASURE Sales[M] = 1 EVALUATE Sales"));
        assert!(starts_with_query_keyword("  define measure Sales[M] = 1"));
        assert!(!starts_with_query_keyword("SUM(Sales[Amount])"));
        // `DEFINE` alone is a valid expression, so the expression parser wins.
        assert!(render("DEFINE").expect("bare name expression").starts_with("AST"));
        let error = render("DEFINE MEASURE").expect_err("malformed query");
        assert!(error.contains("measure name"), "{error}");
    }
}
