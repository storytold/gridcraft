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

    match parse_query(text) {
        Ok(query) => {
            println!("Query");
            println!("{query:#?}");
            println!("Canonical: {}", print_query(&query));
        }
        Err(query_error) => {
            if starts_with_evaluate(text) {
                return Err(query_error.into());
            }
            let expr = parse_expression(text)?;
            println!("AST");
            println!("{expr:#?}");
            println!("Canonical: {}", print_expression(&expr));
        }
    }
    Ok(())
}

fn starts_with_evaluate(text: &str) -> bool {
    tokenize(text).is_ok_and(|tokens| matches!(tokens.first().map(|token| &token.kind), Some(TokenKind::Evaluate)))
}
