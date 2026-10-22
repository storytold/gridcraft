//! Parse DAX from the command line (or stdin), print its syntax tree and canonical text.
//!
//! ```text
//! cargo run -p gridcraft-dax --example parse -- "1 + 2 * 3"
//! echo "EVALUATE SUM('Sales'[Amount])" | cargo run -p gridcraft-dax --example parse
//! ```

use std::io::{self, Read};

use gridcraft_dax::{parse_expression, parse_query, print_expression, print_query};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        io::stdin().read_to_string(&mut text)?;
    }
    let text = text.trim();
    if text.is_empty() {
        return Err("no DAX expression supplied; pass it as an argument or on stdin".into());
    }

    if let Ok(query) = parse_query(text) {
        println!("Query");
        println!("{query:#?}");
        println!("Canonical: {}", print_query(&query));
    } else {
        let expr = parse_expression(text)?;
        println!("AST");
        println!("{expr:#?}");
        println!("Canonical: {}", print_expression(&expr));
    }
    Ok(())
}
