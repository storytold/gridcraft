//! `gridcraft-cli dax`: format or dump the syntax tree of DAX text.

use std::io::Read;

use gridcraft_dax::{Expr, Query, TokenKind, parse_expression, parse_query, print_expression, print_query, tokenize};

use super::{Args, USAGE};

pub fn main(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).ok_or_else(|| format!("missing dax action\n{USAGE}"))?;
    let rest = args.get(1..).unwrap_or_default();
    match action {
        "format" => format(rest),
        "parse" => parse(rest),
        other => Err(format!("unknown dax action `{other}`\n{USAGE}")),
    }
}

fn format(args: &[String]) -> Result<(), String> {
    let text = read_input(args)?;
    match parse_any(&text)? {
        Parsed::Query(query) => outln!("{}", print_query(&query)),
        Parsed::Expression(expr) => outln!("{}", print_expression(&expr)),
    }
    Ok(())
}

fn parse(args: &[String]) -> Result<(), String> {
    let text = read_input(args)?;
    match parse_any(&text)? {
        Parsed::Query(query) => outln!("{query:#?}"),
        Parsed::Expression(expr) => outln!("{expr:#?}"),
    }
    Ok(())
}

enum Parsed {
    Query(Query),
    Expression(Expr),
}

/// Reads `TEXT`, `--file PATH`, or stdin (in that order of preference). Multiple positionals
/// are joined with spaces, so an unquoted `dax format 1 + 2` works.
fn read_input(args: &[String]) -> Result<String, String> {
    let parsed = Args::parse(args, &["file"], &[])?;
    if let Some(path) = parsed.opt("file") {
        if !parsed.positional.is_empty() {
            return Err("give either TEXT or --file, not both".into());
        }
        return std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"));
    }
    if !parsed.positional.is_empty() {
        return Ok(parsed.positional.join(" "));
    }
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|e| format!("can't read stdin: {e}"))?;
    Ok(text)
}

/// Queries are tried first; a malformed `EVALUATE` reports its query error instead of a
/// misleading expression error.
fn parse_any(text: &str) -> Result<Parsed, String> {
    match parse_query(text) {
        Ok(query) => Ok(Parsed::Query(query)),
        Err(query_error) => {
            if starts_with_evaluate(text) {
                return Err(query_error.to_string());
            }
            parse_expression(text).map(Parsed::Expression).map_err(|e| e.to_string())
        }
    }
}

fn starts_with_evaluate(text: &str) -> bool {
    tokenize(text).is_ok_and(|tokens| matches!(tokens.first().map(|token| &token.kind), Some(TokenKind::Evaluate)))
}
