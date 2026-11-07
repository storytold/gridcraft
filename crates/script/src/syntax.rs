//! A syntax sanity check: strings, comments and brackets are balanced, and the source is not
//! empty. It does not parse TypeScript and never runs anything: the runtime reports real
//! grammar errors with their line. This catches the damage that makes a script fail before its
//! first statement, and it costs no dependency.

use std::fmt;

/// What is wrong with a script and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    /// 1-based line of the problem.
    pub line: u32,
    /// 1-based column of the problem, in characters.
    pub column: u32,
}

impl SyntaxError {
    pub(crate) fn new(message: impl Into<String>, line: u32, column: u32) -> Self {
        SyntaxError { message: message.into(), line, column }
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (line {}, column {})", self.message, self.line, self.column)
    }
}

impl std::error::Error for SyntaxError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Single,
    Double,
    Template,
    LineComment,
    BlockComment,
    Regex,
}

#[derive(Clone, Copy, Debug)]
struct Open {
    ch: char,
    line: u32,
    column: u32,
    /// `{` that opened a `${…}` inside a template literal.
    template: bool,
    /// The string mode the `${…}` expression returns to.
    return_mode: Mode,
}

struct Scan {
    chars: Vec<char>,
    at: usize,
    line: u32,
    column: u32,
    mode: Mode,
    /// The mode a template literal returns to when its closing backtick is read.
    template_return: Vec<Mode>,
    open: Vec<Open>,
    /// Where the current string, comment or regex started.
    opened_at: (u32, u32),
    /// The last character that was not whitespace and not part of a comment, for the `/`
    /// regular-expression heuristic.
    previous: Option<char>,
    saw_code: bool,
}

/// Checks `code` for balanced strings, comments and brackets, and for being non-empty.
///
/// The check is deliberately conservative: it never accepts an unterminated string, comment or
/// bracket, and it does not claim a script to be valid TypeScript. A regular-expression literal
/// is recognised by the character before its slash, as JavaScript's grammar does; the one case
/// that is ambiguous (`a / b / c` read as a regex) never rejects working code.
pub fn check(code: &str) -> Result<(), SyntaxError> {
    if code.chars().count() > super::MAX_CODE_CHARS {
        return Err(SyntaxError::new(format!("the script is longer than the {}-character Office Scripts limit", super::MAX_CODE_CHARS), 1, 1));
    }
    let mut s = Scan {
        chars: code.chars().collect(),
        at: 0,
        line: 1,
        column: 1,
        mode: Mode::Normal,
        template_return: Vec::new(),
        open: Vec::new(),
        opened_at: (1, 1),
        previous: None,
        saw_code: false,
    };
    s.run()?;
    if !s.saw_code {
        return Err(SyntaxError::new("the script is empty", 1, 1));
    }
    Ok(())
}

impl Scan {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.at.saturating_add(1)).copied()
    }

    /// Consumes the current character, keeping line and column.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at = self.at.saturating_add(1);
        if c == '\n' {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else {
            self.column = self.column.saturating_add(1);
        }
        Some(c)
    }

    fn run(&mut self) -> Result<(), SyntaxError> {
        while let Some(c) = self.peek() {
            match self.mode {
                Mode::Normal => self.normal(c)?,
                Mode::LineComment => {
                    self.bump();
                    if c == '\n' {
                        self.mode = Mode::Normal;
                    }
                }
                Mode::BlockComment => {
                    self.bump();
                    if c == '*' && self.peek() == Some('/') {
                        self.bump();
                        self.mode = Mode::Normal;
                    }
                }
                Mode::Single | Mode::Double => {
                    self.bump();
                    if c == '\\' {
                        self.bump();
                    } else if (self.mode == Mode::Single && c == '\'') || (self.mode == Mode::Double && c == '"') {
                        self.mode = Mode::Normal;
                        self.previous = Some(c);
                    } else if c == '\n' {
                        return Err(SyntaxError::new("unterminated string", self.opened_at.0, self.opened_at.1));
                    }
                }
                Mode::Template => {
                    if c == '`' {
                        self.bump();
                        self.mode = self.template_return.pop().unwrap_or(Mode::Normal);
                        self.previous = Some('`');
                    } else if c == '\\' {
                        self.bump();
                        self.bump();
                    } else if c == '$' && self.peek_next() == Some('{') {
                        let (line, column) = (self.line, self.column);
                        self.bump();
                        self.bump();
                        self.open.push(Open { ch: '{', line, column, template: true, return_mode: Mode::Template });
                        self.mode = Mode::Normal;
                        self.previous = Some('{');
                    } else {
                        self.bump();
                    }
                }
                Mode::Regex => {
                    self.bump();
                    if c == '\\' {
                        self.bump();
                    } else if c == '[' {
                        self.regex_class()?;
                    } else if c == '/' {
                        self.mode = Mode::Normal;
                        self.previous = Some('/');
                    } else if c == '\n' {
                        return Err(SyntaxError::new("unterminated regular expression", self.opened_at.0, self.opened_at.1));
                    }
                }
            }
        }
        self.finish()
    }

    /// Skips to the end of a `[…]` character class inside a regex literal.
    fn regex_class(&mut self) -> Result<(), SyntaxError> {
        while let Some(c) = self.peek() {
            self.bump();
            match c {
                '\\' => {
                    self.bump();
                }
                ']' => return Ok(()),
                '\n' => return Err(SyntaxError::new("unterminated regular expression", self.opened_at.0, self.opened_at.1)),
                _ => {}
            }
        }
        Err(SyntaxError::new("unterminated regular expression", self.opened_at.0, self.opened_at.1))
    }

    fn normal(&mut self, c: char) -> Result<(), SyntaxError> {
        let (line, column) = (self.line, self.column);
        match c {
            '/' if self.peek_next() == Some('/') => {
                self.bump();
                self.bump();
                self.mode = Mode::LineComment;
            }
            '/' if self.peek_next() == Some('*') => {
                self.bump();
                self.bump();
                self.mode = Mode::BlockComment;
                self.opened_at = (line, column);
            }
            '/' if regex_starts(self.previous) => {
                self.bump();
                self.mode = Mode::Regex;
                self.opened_at = (line, column);
            }
            '\'' | '"' => {
                let mode = if c == '\'' { Mode::Single } else { Mode::Double };
                self.bump();
                self.mode = mode;
                self.opened_at = (line, column);
            }
            '`' => {
                self.bump();
                self.template_return.push(Mode::Normal);
                self.mode = Mode::Template;
                self.opened_at = (line, column);
            }
            '(' | '[' | '{' => {
                self.bump();
                self.open.push(Open { ch: c, line, column, template: false, return_mode: Mode::Normal });
                self.saw_code = true;
                self.previous = Some(c);
            }
            ')' | ']' | '}' => {
                self.bump();
                self.saw_code = true;
                self.close(c, line, column)?;
                self.previous = Some(c);
            }
            _ => {
                self.bump();
                if !c.is_whitespace() {
                    self.saw_code = true;
                    self.previous = Some(c);
                }
            }
        }
        Ok(())
    }

    fn close(&mut self, c: char, line: u32, column: u32) -> Result<(), SyntaxError> {
        let Some(top) = self.open.pop() else {
            return Err(SyntaxError::new(format!("`{c}` closes nothing"), line, column));
        };
        let expected = match top.ch {
            '(' => ')',
            '[' => ']',
            _ => '}',
        };
        if expected != c {
            return Err(SyntaxError::new(format!("`{}` is closed by `{c}`", top.ch), line, column));
        }
        if top.template {
            self.mode = top.return_mode;
            self.previous = Some('}');
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), SyntaxError> {
        match self.mode {
            Mode::Single | Mode::Double => {
                return Err(SyntaxError::new("unterminated string", self.opened_at.0, self.opened_at.1));
            }
            Mode::Template => {
                return Err(SyntaxError::new("unterminated template literal", self.opened_at.0, self.opened_at.1));
            }
            Mode::Regex => {
                return Err(SyntaxError::new("unterminated regular expression", self.opened_at.0, self.opened_at.1));
            }
            Mode::BlockComment => {
                return Err(SyntaxError::new("unterminated comment", self.opened_at.0, self.opened_at.1));
            }
            Mode::Normal | Mode::LineComment => {}
        }
        if let Some(top) = self.open.last() {
            return Err(SyntaxError::new(format!("`{}` is never closed", top.ch), top.line, top.column));
        }
        Ok(())
    }
}

/// Whether a `/` after `previous` starts a regular expression rather than a division. The
/// character list is the set of places JavaScript's grammar allows an expression.
fn regex_starts(previous: Option<char>) -> bool {
    match previous {
        None => true,
        Some(c) => "([{=,:;!&|?+-*%<>~^".contains(c),
    }
}

/// Rejects names that are empty, only whitespace, too long, or hold control characters.
pub(crate) fn check_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("a script needs a name".into());
    }
    if trimmed.chars().count() > super::MAX_NAME_CHARS {
        return Err(format!("a script name can be at most {} characters long", super::MAX_NAME_CHARS));
    }
    if trimmed.chars().any(char::is_control) {
        return Err("a script name can't hold control characters".into());
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(code: &str) {
        assert_eq!(check(code), Ok(()), "{code}");
    }

    fn line(code: &str) -> u32 {
        match check(code) {
            Ok(()) => panic!("expected an error for {code}"),
            Err(e) => e.line,
        }
    }

    #[test]
    fn accepts_ordinary_scripts() {
        ok("function main(workbook: ExcelScript.Workbook) {\n  workbook.getActiveWorksheet().getRange(\"A1\").setValue(1);\n}");
        ok("const x = a / b / c;");
        ok("const re = /[\"']+/g;");
        ok("const t = `a ${b + `c ${d}`} e`;");
        ok("// only a comment is not enough, but this line is code\nconsole.log(1);");
        ok("if (a < b) { /* } */ }");
    }

    #[test]
    fn rejects_unbalanced_brackets_with_the_opening_line() {
        assert_eq!(line("function main() {\n  if (true) {\n"), 2);
        assert_eq!(line("const a = (1 + 2;\n"), 1);
        assert_eq!(line("]\n"), 1);
        assert_eq!(line("const a = (1 + 2];\n"), 1);
    }

    #[test]
    fn rejects_unterminated_literals_and_comments() {
        assert_eq!(line("const a = \"x;\n"), 1);
        assert_eq!(line("const a = `x;\n"), 1);
        assert_eq!(line("/* x\n"), 1);
        assert_eq!(line("const a = /x;\n"), 1);
    }

    #[test]
    fn rejects_empty_and_comment_only_scripts() {
        assert!(check("").is_err());
        assert!(check("   \n\t").is_err());
        assert!(check("// nothing\n/* here */").is_err());
    }

    #[test]
    fn caps_length() {
        let long = "a".repeat(super::super::MAX_CODE_CHARS + 1);
        assert!(check(&long).is_err());
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(check_name("  Band  "), Ok("Band"));
        assert!(check_name("").is_err());
        assert!(check_name("   ").is_err());
        assert!(check_name("a\nb").is_err());
        assert!(check_name(&"a".repeat(super::super::MAX_NAME_CHARS + 1)).is_err());
    }
}
