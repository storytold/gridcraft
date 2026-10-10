//! A basic VBA macro interpreter.
//!
//! This is intentionally not a full VBA implementation — there is no `vbaProject.bin` *writer*,
//! no object browser, no UserForms, no error handling (`On Error`), no arrays, no calling one Sub
//! from another, and no object model beyond `Range`/`Cells`/`ActiveSheet`/`MsgBox`. It covers the
//! common shape of small, button-run macros: looping over cells, simple conditionals, and reading
//! or writing cell values.
//!
//! [`extract`] pulls the module source text out of a `vbaProject.bin` package (via the `ovba`
//! crate, which handles the `[MS-CFB]` container and `[MS-OVBA]` source-code decompression).
//! [`Macro::parse`] parses one `Sub`'s source into a [`Stmt`] block, and [`Macro::run`] executes
//! it against a [`Host`] — the thing that actually owns the spreadsheet's cells.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod ast;
mod extract;
mod interp;
mod lexer;
mod parser;

pub use ast::{Stmt, Value};
pub use extract::{ModuleSource, extract};
pub use interp::{Host, RunError};
pub use parser::ParseError;

/// One `Sub ... End Sub` found in a VBA module, parsed and ready to run.
#[derive(Debug, Clone)]
pub struct Macro {
    pub name: String,
    pub body: Vec<Stmt>,
}

impl Macro {
    /// Finds and parses every zero-argument `Sub` in `source`. Subs that take parameters, or that
    /// fail to parse, are skipped rather than failing the whole module — one broken or unsupported
    /// macro shouldn't hide the others.
    pub fn parse_all(source: &str) -> Vec<Macro> {
        parser::subs(source).into_iter().filter_map(|(name, body_src)| parser::parse_block(&body_src).ok().map(|body| Macro { name, body })).collect()
    }

    /// Runs this macro's statements against `host`.
    pub fn run(&self, host: &mut dyn Host) -> Result<(), RunError> {
        interp::exec_block(&self.body, host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MockHost {
        cells: HashMap<(u32, u32), Value>,
        messages: Vec<String>,
    }

    impl Host for MockHost {
        fn get_cell(&self, _sheet: Option<&str>, row: u32, col: u32) -> Value {
            self.cells.get(&(row, col)).cloned().unwrap_or(Value::Empty)
        }
        fn set_cell(&mut self, _sheet: Option<&str>, row: u32, col: u32, value: Value) {
            self.cells.insert((row, col), value);
        }
        fn msg_box(&mut self, text: &str) {
            self.messages.push(text.to_string());
        }
    }

    #[test]
    fn runs_a_for_loop_filling_cells() {
        let src = r#"
            Sub FillNumbers()
                Dim i As Integer
                For i = 1 To 5
                    Cells(i, 1).Value = i * 2
                Next i
            End Sub
        "#;
        let macros = Macro::parse_all(src);
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].name, "FillNumbers");
        let mut host = MockHost::default();
        macros[0].run(&mut host).unwrap();
        // `Cells(i, 1)` is 1-based; the host sees 0-based `(row, col)`.
        for i in 1..=5u32 {
            assert_eq!(host.get_cell(None, i - 1, 0), Value::Number(f64::from(i) * 2.0));
        }
    }

    #[test]
    fn runs_if_else_against_a_cell() {
        let src = r#"
            Sub Classify()
                If Range("A1").Value > 100 Then
                    Range("B1").Value = "High"
                Else
                    Range("B1").Value = "Low"
                End If
            End Sub
        "#;
        let macros = Macro::parse_all(src);
        let mut host = MockHost::default();
        host.set_cell(None, 0, 0, Value::Number(200.0));
        macros[0].run(&mut host).unwrap();
        assert_eq!(host.get_cell(None, 0, 1), Value::Str("High".into()));

        let mut host = MockHost::default();
        host.set_cell(None, 0, 0, Value::Number(5.0));
        macros[0].run(&mut host).unwrap();
        assert_eq!(host.get_cell(None, 0, 1), Value::Str("Low".into()));
    }

    #[test]
    fn msg_box_and_string_concat() {
        let src = r#"
            Sub Greet()
                Dim name As String
                name = "World"
                MsgBox "Hello, " & name & "!"
            End Sub
        "#;
        let macros = Macro::parse_all(src);
        let mut host = MockHost::default();
        macros[0].run(&mut host).unwrap();
        assert_eq!(host.messages, vec!["Hello, World!".to_string()]);
    }

    #[test]
    fn skips_subs_with_parameters() {
        let src = r#"
            Sub WithArgs(x As Integer)
                MsgBox "never runs"
            End Sub
            Sub NoArgs()
                MsgBox "runs"
            End Sub
        "#;
        let macros = Macro::parse_all(src);
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].name, "NoArgs");
    }
}
