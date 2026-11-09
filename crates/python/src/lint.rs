//! Static checks on a Python cell's code: [`lint`] warns a person, in the editor and before anything
//! runs, about code the sandbox will refuse, in the `lint.*` kinds of the [`crate::diagnostics`]
//! catalog. It runs in Rust with no Python process, so it works with Python off, on systems with no
//! sandbox and on the web.
//!
//! This is a tokenizer and a linear pass over its tokens, not a parser: code can be any text, and a
//! line still being typed must get an answer too. Text inside a comment or a string is never read
//! as code.

use std::collections::HashSet;

use crate::diagnostics::{Kind, kind};

// Tokens.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    /// A name or a keyword.
    Name,
    Number,
    Str(Literal),
    /// One byte of punctuation (always ASCII).
    Punct(u8),
    /// The end of a statement: a line break outside brackets. Blank lines give one token.
    Newline,
}

/// What the lexer knows about a string literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Literal {
    /// How many prefix letters come before the quote.
    prefix: u8,
    triple: bool,
    closed: bool,
    /// An f-string or a t-string, whose value is only known when it runs.
    interpolated: bool,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Tok,
    start: usize,
    end: usize,
    /// For an opening bracket: the index of the token that ends its first element, a comma or the
    /// closing bracket. 0 until one is seen, so also when the bracket is never closed.
    first_end: usize,
}

fn tokenize(code: &str) -> Vec<Token> {
    let mut lexer = Lexer { bytes: code.as_bytes(), i: 0, tokens: Vec::new(), open: Vec::new() };
    while let Some(&b) = lexer.bytes.get(lexer.i) {
        lexer.step(b);
    }
    lexer.tokens
}

struct Lexer<'a> {
    bytes: &'a [u8],
    i: usize,
    tokens: Vec<Token>,
    /// The opening brackets not yet closed, as token indexes. Line breaks inside them don't end a
    /// statement.
    open: Vec<usize>,
}

impl Lexer<'_> {
    /// Reads what starts at `i`, which is `b`, and always moves `i` forward. Token edges fall on
    /// ASCII bytes or at the end of the code, so they are char boundaries.
    fn step(&mut self, b: u8) {
        match b {
            b'#' => self.i = line_end(self.bytes, self.i),
            b'\n' | b'\r' => self.line_break(),
            b'\\' => self.backslash(),
            b' ' | b'\t' | 0x0c => self.i += 1,
            b'"' | b'\'' => self.string(self.i, self.i, false),
            b'(' | b'[' | b'{' => {
                self.open.push(self.tokens.len());
                self.punct(b);
            }
            b')' | b']' | b'}' => {
                self.end_first_element();
                self.open.pop();
                self.punct(b);
            }
            b',' => {
                self.end_first_element();
                self.punct(b);
            }
            b if is_word_byte(b) => self.word(),
            b => self.punct(b),
        }
    }

    /// The token about to be pushed is a comma or a closing bracket of the innermost open bracket.
    fn end_first_element(&mut self) {
        let here = self.tokens.len();
        if let Some(bracket) = self.open.last().and_then(|at| self.tokens.get_mut(*at))
            && bracket.first_end == 0
        {
            bracket.first_end = here;
        }
    }

    fn push(&mut self, kind: Tok, start: usize, end: usize) {
        self.tokens.push(Token { kind, start, end, first_end: 0 });
        self.i = end;
    }

    fn punct(&mut self, b: u8) {
        self.push(Tok::Punct(b), self.i, self.i + 1);
    }

    fn line_break(&mut self) {
        let end = after_line_break(self.bytes, self.i);
        let ended_a_statement = self.tokens.last().is_some_and(|t| t.kind != Tok::Newline);
        if self.open.is_empty() && ended_a_statement {
            self.push(Tok::Newline, self.i, end);
        } else {
            self.i = end;
        }
    }

    /// A backslash before a line break joins the two lines. Anywhere else it means nothing.
    fn backslash(&mut self) {
        if matches!(self.bytes.get(self.i + 1), Some(b'\n' | b'\r')) {
            self.i = skip_escape(self.bytes, self.i);
        } else {
            self.punct(b'\\');
        }
    }

    /// A name or a number, or a string when these are prefix letters right before a quote.
    fn word(&mut self) {
        let start = self.i;
        let end = word_end(self.bytes, start);
        let word = self.bytes.get(start..end).unwrap_or_default();
        if let Some(interpolated) = string_prefix(self.bytes, word, end) {
            self.string(start, end, interpolated);
        } else if word.first().is_some_and(u8::is_ascii_digit) {
            self.push(Tok::Number, start, end);
        } else {
            self.push(Tok::Name, start, end);
        }
    }

    fn string(&mut self, start: usize, quote_at: usize, interpolated: bool) {
        let (_, triple, _) = opening_quote(self.bytes, quote_at);
        let (end, closed) = scan_string(self.bytes, quote_at, interpolated);
        let prefix = u8::try_from(quote_at - start).unwrap_or_default();
        self.push(Tok::Str(Literal { prefix, triple, closed, interpolated }), start, end);
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn word_end(bytes: &[u8], from: usize) -> usize {
    let rest = bytes.get(from..).unwrap_or_default();
    from + rest.iter().take_while(|b| is_word_byte(**b)).count()
}

/// Where the line holding `from` ends, before its line break.
fn line_end(bytes: &[u8], from: usize) -> usize {
    let rest = bytes.get(from..).unwrap_or_default();
    from + rest.iter().take_while(|b| !matches!(b, b'\n' | b'\r')).count()
}

/// Just past the line break at `at`: `\r\n` is one, and so is a `\r` on its own, as Python reads it.
fn after_line_break(bytes: &[u8], at: usize) -> usize {
    match (bytes.get(at), bytes.get(at + 1)) {
        (Some(b'\r'), Some(b'\n')) => at + 2,
        _ => at + 1,
    }
}

/// Just past what the backslash at `at` escapes. It can land inside a multi-byte character, which
/// the string's scan walks through byte by byte: it only stops on ASCII.
fn skip_escape(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at + 1) {
        Some(b'\r' | b'\n') => after_line_break(bytes, at + 1),
        Some(_) => at + 2,
        None => at + 1,
    }
}

/// Whether `word`, followed by the byte at `end`, starts a string: `Some(true)` when the string has
/// replacement fields (f-strings, and t-strings from Python 3.14), in any letter case.
fn string_prefix(bytes: &[u8], word: &[u8], end: usize) -> Option<bool> {
    if word.len() > 2 || !matches!(bytes.get(end), Some(b'"' | b'\'')) {
        return None;
    }
    match word.to_ascii_lowercase().as_slice() {
        b"r" | b"u" | b"b" | b"br" | b"rb" => Some(false),
        b"f" | b"t" | b"fr" | b"rf" | b"tr" | b"rt" => Some(true),
        _ => None,
    }
}

/// The quote at `at`, whether it is tripled, and where the text after it starts.
fn opening_quote(bytes: &[u8], at: usize) -> (u8, bool, usize) {
    let quote = bytes.get(at).copied().unwrap_or(b'"');
    let triple = bytes.get(at + 1) == Some(&quote) && bytes.get(at + 2) == Some(&quote);
    (quote, triple, at + if triple { 3 } else { 1 })
}

/// Whether the string's closing quote is at `at`, which holds its quote byte.
fn closes(bytes: &[u8], at: usize, quote: u8, triple: bool) -> bool {
    !triple || (bytes.get(at + 1) == Some(&quote) && bytes.get(at + 2) == Some(&quote))
}

/// Just past the string whose opening quote is at `quote_at`, and whether it was closed. A string
/// not closed ends at its line break (triple quoted: at the end of the code).
fn scan_string(bytes: &[u8], quote_at: usize, interpolated: bool) -> (usize, bool) {
    let (quote, triple, text) = opening_quote(bytes, quote_at);
    if interpolated { scan_fstring(bytes, text, quote, triple) } else { scan_text(bytes, text, quote, triple) }
}

fn scan_text(bytes: &[u8], from: usize, quote: u8, triple: bool) -> (usize, bool) {
    let mut i = from;
    while let Some(&c) = bytes.get(i) {
        match c {
            b'\\' => i = skip_escape(bytes, i),
            b'\n' | b'\r' if !triple => return (i, false),
            c if c == quote && closes(bytes, i, quote, triple) => return (i + if triple { 3 } else { 1 }, true),
            _ => i += 1,
        }
    }
    (bytes.len(), false)
}

/// What an f-string's scan is inside, innermost last.
#[derive(Clone, Copy)]
enum Frame {
    /// An f-string's text.
    Text { quote: u8, triple: bool },
    /// A replacement field's expression, which is code, with how many brackets are open in it. Its
    /// strings may use the outer quote (Python 3.12), so the outer string doesn't end there.
    Field { brackets: u32 },
    /// A replacement field's format spec: text, in which a quote is a fill character.
    Spec,
}

/// An f-string's scan. The stack is on the heap and each round moves forward or closes a frame, so
/// nesting can't overflow anything and the work stays linear.
struct FString<'a> {
    bytes: &'a [u8],
    i: usize,
    stack: Vec<Frame>,
    /// Whether the outermost string is triple quoted. If it isn't, a line break in a field ends the
    /// whole string, so that `f"{x` being typed doesn't swallow the code below it. A field that
    /// spans lines in a single-quoted f-string (Python 3.12) is read from its second line as code.
    multiline: bool,
    closed: bool,
}

fn scan_fstring(bytes: &[u8], from: usize, quote: u8, triple: bool) -> (usize, bool) {
    let mut f = FString { bytes, i: from, stack: vec![Frame::Text { quote, triple }], multiline: triple, closed: false };
    while let (Some(&frame), Some(&c)) = (f.stack.last(), bytes.get(f.i)) {
        match frame {
            Frame::Text { quote, triple } => f.text(c, quote, triple),
            Frame::Field { brackets } => f.field(c, brackets),
            Frame::Spec => f.spec(c),
        }
    }
    (f.i.min(bytes.len()), f.closed)
}

impl FString<'_> {
    fn text(&mut self, c: u8, quote: u8, triple: bool) {
        match c {
            b'\\' => self.i = self.text_escape(),
            b'{' if self.bytes.get(self.i + 1) == Some(&b'{') => self.i += 2,
            b'{' => self.open_field(),
            // The line break stays, for the frame below or for the lexer.
            b'\n' | b'\r' if !triple => {
                self.stack.pop();
            }
            c if c == quote && closes(self.bytes, self.i, quote, triple) => {
                self.stack.pop();
                self.i += if triple { 3 } else { 1 };
                self.closed = self.stack.is_empty();
            }
            _ => self.i += 1,
        }
    }

    fn field(&mut self, c: u8, brackets: u32) {
        match c {
            b'\\' => self.i = skip_escape(self.bytes, self.i),
            b'"' | b'\'' => self.nested_string(self.i, false),
            b'#' => self.i = line_end(self.bytes, self.i),
            b'\n' | b'\r' => self.line_break(),
            b'(' | b'[' | b'{' => self.set_brackets(brackets.saturating_add(1)),
            b')' | b']' => self.set_brackets(brackets.saturating_sub(1)),
            b'}' if brackets > 0 => self.set_brackets(brackets - 1),
            b'}' => self.close_frame(),
            b':' if brackets == 0 => self.open_spec(),
            c if is_word_byte(c) => self.word(),
            _ => self.i += 1,
        }
    }

    fn spec(&mut self, c: u8) {
        match c {
            b'\\' => self.i = self.text_escape(),
            b'{' => self.open_field(),
            b'}' => self.close_frame(),
            b'\n' | b'\r' => self.line_break(),
            _ => self.i += 1,
        }
    }

    /// In a field's code, a string's prefix counts, and a name can't hold a string's end.
    fn word(&mut self) {
        let end = word_end(self.bytes, self.i);
        let word = self.bytes.get(self.i..end).unwrap_or_default();
        match string_prefix(self.bytes, word, end) {
            Some(interpolated) => self.nested_string(end, interpolated),
            None => self.i = end,
        }
    }

    fn nested_string(&mut self, quote_at: usize, interpolated: bool) {
        let (quote, triple, text) = opening_quote(self.bytes, quote_at);
        if interpolated {
            self.stack.push(Frame::Text { quote, triple });
            self.i = text;
        } else {
            self.i = scan_text(self.bytes, text, quote, triple).0;
        }
    }

    /// A backslash escapes nothing in front of a brace: `f"\{x}"` holds a field.
    fn text_escape(&self) -> usize {
        match self.bytes.get(self.i + 1) {
            Some(b'{' | b'}') => self.i + 1,
            _ => skip_escape(self.bytes, self.i),
        }
    }

    fn open_field(&mut self) {
        self.stack.push(Frame::Field { brackets: 0 });
        self.i += 1;
    }

    fn open_spec(&mut self) {
        self.stack.pop();
        self.stack.push(Frame::Spec);
        self.i += 1;
    }

    fn close_frame(&mut self) {
        self.stack.pop();
        self.i += 1;
    }

    fn set_brackets(&mut self, brackets: u32) {
        self.stack.pop();
        self.stack.push(Frame::Field { brackets });
        self.i += 1;
    }

    fn line_break(&mut self) {
        if self.multiline {
            self.i += 1;
        } else {
            self.stack.clear();
        }
    }
}

// Import statements.

/// Python's reserved words, which can't name a module.
const KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// A module an import statement names.
struct Imported<'a> {
    /// Its dotted path.
    path: String,
    start: usize,
    end: usize,
    /// What a `from` statement takes from it.
    names: Vec<Taken<'a>>,
}

/// A name a `from` statement takes from a module, or `*`.
struct Taken<'a> {
    name: &'a str,
    start: usize,
    end: usize,
}

/// The code with its tokens, read by index: an index past the end reads as nothing.
struct Source<'a> {
    code: &'a str,
    tokens: Vec<Token>,
}

impl<'a> Source<'a> {
    fn new(code: &'a str) -> Source<'a> {
        Source { code, tokens: tokenize(code) }
    }

    fn text(&self, i: usize) -> &'a str {
        self.tokens.get(i).and_then(|t| self.code.get(t.start..t.end)).unwrap_or_default()
    }

    fn is_punct(&self, i: usize, b: u8) -> bool {
        self.tokens.get(i).is_some_and(|t| t.kind == Tok::Punct(b))
    }

    /// Whether the code defines `name` itself: `def name` or `name = ...` at any depth of block,
    /// not an attribute (`x.name = 1`), a comparison (`name == 1`) or a keyword argument.
    fn rebinds(&self, name: &str) -> bool {
        let mut depth = 0u32;
        for i in 0..self.tokens.len() {
            if self.is_punct(i, b'(') || self.is_punct(i, b'[') || self.is_punct(i, b'{') {
                depth = depth.saturating_add(1);
            } else if self.is_punct(i, b')') || self.is_punct(i, b']') || self.is_punct(i, b'}') {
                depth = depth.saturating_sub(1);
            }
            if !self.is_word(i, name) || self.after_dot(i) {
                continue;
            }
            let previous = i.checked_sub(1);
            if previous.is_some_and(|p| self.is_word(p, "def")) {
                return true;
            }
            let assigned = self.is_punct(i + 1, b'=') && !self.is_punct(i + 2, b'=');
            if depth == 0 && assigned {
                return true;
            }
        }
        false
    }

    fn is_word(&self, i: usize, word: &str) -> bool {
        self.tokens.get(i).is_some_and(|t| t.kind == Tok::Name) && self.text(i) == word
    }

    /// The name at `i`, when it is one and not a reserved word.
    fn name(&self, i: usize) -> Option<&'a str> {
        let text = self.text(i);
        (self.tokens.get(i)?.kind == Tok::Name && !KEYWORDS.contains(&text)).then_some(text)
    }

    /// The token before `i` is a dot, so `i` is an attribute.
    fn after_dot(&self, i: usize) -> bool {
        i.checked_sub(1).is_some_and(|before| self.is_punct(before, b'.'))
    }

    fn starts_import(&self, i: usize) -> bool {
        (self.is_word(i, "import") || self.is_word(i, "from")) && !self.after_dot(i)
    }

    /// The modules named by the statement at `i`, which [`Source::starts_import`] said is one, and
    /// the token to read next. `from` also reads `raise E from x`, which names nothing.
    fn import_statement(&self, i: usize) -> (Vec<Imported<'a>>, usize) {
        if self.is_word(i, "from") { self.import_from(i) } else { self.import_plain(i) }
    }

    fn import_plain(&self, i: usize) -> (Vec<Imported<'a>>, usize) {
        let mut found = Vec::new();
        let mut at = i + 1;
        while let Some((path, last)) = self.dotted(at) {
            found.push(self.imported(path, at, last, Vec::new()));
            at = self.skip_alias(last + 1);
            if !self.is_punct(at, b',') {
                break;
            }
            at += 1;
        }
        (found, at)
    }

    fn import_from(&self, i: usize) -> (Vec<Imported<'a>>, usize) {
        let mut at = i + 1;
        while self.is_punct(at, b'.') {
            at += 1;
        }
        let first = at;
        let relative = first > i + 1;
        let module = self.dotted(first);
        if let Some((_, last)) = &module {
            at = last + 1;
        }
        if !self.is_word(at, "import") {
            return (Vec::new(), if relative { at } else { i + 1 });
        }
        let (names, next) = self.taken_names(at + 1);
        // A relative import names a module of the code's own package, not one to find or install.
        let found = module.filter(|_| !relative).map(|(path, last)| self.imported(path, first, last, names));
        (found.into_iter().collect(), next)
    }

    fn imported(&self, path: String, first: usize, last: usize, names: Vec<Taken<'a>>) -> Imported<'a> {
        let (start, end) = self.span(first, last);
        Imported { path, start, end, names }
    }

    /// Where the tokens `first` to `last` are in the code.
    fn span(&self, first: usize, last: usize) -> (usize, usize) {
        let start = self.tokens.get(first).map_or(0, |t| t.start);
        (start, self.tokens.get(last).map_or(start, |t| t.end))
    }

    /// The dotted name at `i` and the index of its last token. `import a . b` is valid Python.
    fn dotted(&self, i: usize) -> Option<(String, usize)> {
        let mut path = self.name(i)?.to_string();
        let mut last = i;
        while self.is_punct(last + 1, b'.') {
            let Some(part) = self.name(last + 2) else { break };
            path.push('.');
            path.push_str(part);
            last += 2;
        }
        Some((path, last))
    }

    fn skip_alias(&self, at: usize) -> usize {
        if self.is_word(at, "as") { at + 2 } else { at }
    }

    /// The names of a `from` statement, which start at token `start`, and the token after them.
    fn taken_names(&self, start: usize) -> (Vec<Taken<'a>>, usize) {
        let mut names = Vec::new();
        let mut at = start;
        if self.is_punct(at, b'(') {
            at += 1;
        }
        while self.is_punct(at, b'*') || self.name(at).is_some() {
            let (name_start, name_end) = self.span(at, at);
            names.push(Taken { name: self.text(at), start: name_start, end: name_end });
            at = self.skip_alias(at + 1);
            if !self.is_punct(at, b',') {
                break;
            }
            at += 1;
        }
        if self.is_punct(at, b')') {
            at += 1;
        }
        (names, at)
    }
}

fn top_level(path: &str) -> &str {
    path.split_once('.').map_or(path, |(top, _)| top)
}

/// The top-level modules `code` imports, in order, without duplicates. Relative imports aren't
/// included: they name the code's own package.
pub fn imported_modules(code: &str) -> Vec<String> {
    let source = Source::new(code);
    let mut seen = HashSet::new();
    let mut modules = Vec::new();
    let mut i = 0;
    while i < source.tokens.len() {
        if !source.starts_import(i) {
            i += 1;
            continue;
        }
        let (imported, next) = source.import_statement(i);
        for module in &imported {
            let top = top_level(&module.path);
            if seen.insert(top.to_string()) {
                modules.push(top.to_string());
            }
        }
        i = next;
    }
    modules
}

// Packages.

/// Modules whose distribution has another name.
const INSTALL_NAMES: [(&str, &str); 20] = [
    ("sklearn", "scikit-learn"),
    ("skimage", "scikit-image"),
    ("PIL", "pillow"),
    ("cv2", "opencv-python"),
    ("yaml", "pyyaml"),
    ("bs4", "beautifulsoup4"),
    ("dateutil", "python-dateutil"),
    ("dotenv", "python-dotenv"),
    ("attr", "attrs"),
    ("serial", "pyserial"),
    ("jwt", "pyjwt"),
    ("dns", "dnspython"),
    ("git", "gitpython"),
    ("docx", "python-docx"),
    ("pptx", "python-pptx"),
    ("Crypto", "pycryptodome"),
    ("OpenSSL", "pyopenssl"),
    ("zmq", "pyzmq"),
    ("magic", "python-magic"),
    ("fitz", "pymupdf"),
];

/// The name to install for `module`.
pub fn install_name(module: &str) -> String {
    let top = top_level(module);
    INSTALL_NAMES.iter().find(|(m, _)| *m == top).map_or(module, |(_, distribution)| distribution).to_string()
}

/// The public top-level modules of Python's standard library, 3.11 to 3.15, in byte order. Names
/// that a later version removed stay: this Python may be one that has them.
const STANDARD_LIBRARY: [&str; 220] = [
    "abc",
    "aifc",
    "annotationlib",
    "antigravity",
    "argparse",
    "array",
    "ast",
    "asynchat",
    "asyncio",
    "asyncore",
    "atexit",
    "audioop",
    "base64",
    "bdb",
    "binascii",
    "bisect",
    "builtins",
    "bz2",
    "cProfile",
    "calendar",
    "cgi",
    "cgitb",
    "chunk",
    "cmath",
    "cmd",
    "code",
    "codecs",
    "codeop",
    "collections",
    "colorsys",
    "compileall",
    "compression",
    "concurrent",
    "configparser",
    "contextlib",
    "contextvars",
    "copy",
    "copyreg",
    "crypt",
    "csv",
    "ctypes",
    "curses",
    "dataclasses",
    "datetime",
    "dbm",
    "decimal",
    "difflib",
    "dis",
    "distutils",
    "doctest",
    "email",
    "encodings",
    "ensurepip",
    "enum",
    "errno",
    "faulthandler",
    "fcntl",
    "filecmp",
    "fileinput",
    "fnmatch",
    "fractions",
    "ftplib",
    "functools",
    "gc",
    "genericpath",
    "getopt",
    "getpass",
    "gettext",
    "glob",
    "graphlib",
    "grp",
    "gzip",
    "hashlib",
    "heapq",
    "hmac",
    "html",
    "http",
    "idlelib",
    "imaplib",
    "imghdr",
    "imp",
    "importlib",
    "inspect",
    "io",
    "ipaddress",
    "itertools",
    "json",
    "keyword",
    "lib2to3",
    "linecache",
    "locale",
    "logging",
    "lzma",
    "mailbox",
    "mailcap",
    "marshal",
    "math",
    "mimetypes",
    "mmap",
    "modulefinder",
    "msilib",
    "msvcrt",
    "multiprocessing",
    "netrc",
    "nis",
    "nntplib",
    "nt",
    "ntpath",
    "nturl2path",
    "numbers",
    "opcode",
    "operator",
    "optparse",
    "os",
    "ossaudiodev",
    "pathlib",
    "pdb",
    "pickle",
    "pickletools",
    "pipes",
    "pkgutil",
    "platform",
    "plistlib",
    "poplib",
    "posix",
    "posixpath",
    "pprint",
    "profile",
    "profiling",
    "pstats",
    "pty",
    "pwd",
    "py_compile",
    "pyclbr",
    "pydoc",
    "pydoc_data",
    "pyexpat",
    "queue",
    "quopri",
    "random",
    "re",
    "readline",
    "reprlib",
    "resource",
    "rlcompleter",
    "runpy",
    "sched",
    "secrets",
    "select",
    "selectors",
    "shelve",
    "shlex",
    "shutil",
    "signal",
    "site",
    "smtpd",
    "smtplib",
    "sndhdr",
    "socket",
    "socketserver",
    "spwd",
    "sqlite3",
    "sre_compile",
    "sre_constants",
    "sre_parse",
    "ssl",
    "stat",
    "statistics",
    "string",
    "stringprep",
    "struct",
    "subprocess",
    "sunau",
    "symtable",
    "sys",
    "sysconfig",
    "syslog",
    "tabnanny",
    "tarfile",
    "telnetlib",
    "tempfile",
    "termios",
    "textwrap",
    "this",
    "threading",
    "time",
    "timeit",
    "tkinter",
    "token",
    "tokenize",
    "tomllib",
    "trace",
    "traceback",
    "tracemalloc",
    "tty",
    "turtle",
    "turtledemo",
    "types",
    "typing",
    "unicodedata",
    "unittest",
    "urllib",
    "uu",
    "uuid",
    "venv",
    "warnings",
    "wave",
    "weakref",
    "webbrowser",
    "winreg",
    "winsound",
    "wsgiref",
    "xdrlib",
    "xml",
    "xmlrpc",
    "zipapp",
    "zipfile",
    "zipimport",
    "zlib",
    "zoneinfo",
];

/// Whether `module` comes with Python. Names starting with an underscore (`__future__`, `_thread`)
/// are its private modules, or at any rate nothing a person would be told to install. Import names
/// are case sensitive, so this comparison is too.
fn in_standard_library(module: &str) -> bool {
    module.starts_with('_') || STANDARD_LIBRARY.binary_search(&module).is_ok()
}

/// A distribution or module name as the comparison reads it: in any case, with `-`, `_` and `.` the same.
fn normalize(name: &str) -> String {
    name.to_lowercase().replace(['-', '.'], "_")
}

/// The packages a Python has, ready to compare with module names.
struct Installed(HashSet<String>);

impl Installed {
    /// Each distribution counts under its name and, for namespace packages, under the first part of
    /// its name: `google-cloud-storage` provides `import google.cloud.storage`.
    fn new(distributions: &[String]) -> Installed {
        let mut known = HashSet::new();
        for distribution in distributions {
            let name = normalize(distribution);
            if let Some((first, _)) = name.split_once('_') {
                known.insert(first.to_string());
            }
            known.insert(name);
        }
        Installed(known)
    }

    fn provides(&self, module: &str) -> bool {
        self.0.contains(&normalize(module)) || self.0.contains(&normalize(&install_name(module)))
    }
}

/// Packages that draw in a web browser, which a Python cell can't open.
const BROWSER_PLOT_MODULES: [&str; 3] = ["plotly", "bokeh", "altair"];

fn draws_in_a_browser(path: &str) -> bool {
    BROWSER_PLOT_MODULES.contains(&top_level(path))
}

/// Whether a person should be told to install the top-level `module`. Not a package that draws in a
/// browser: with one installed, a cell still can't show what it draws. Not `excel`, Excel's own
/// module: no package gives it, and what is on the package index under that name is something else.
fn is_missing(module: &str, installed: &Installed) -> bool {
    !draws_in_a_browser(module) && top_level(module) != "excel" && !in_standard_library(module) && !installed.provides(module)
}

/// The modules `code` imports that are neither in Python's standard library nor among the
/// distributions in `installed`, leaving out packages that draw in a browser. Only import statements
/// count: the names a cell has without importing anything (`pd`, `np`, `plt`) ask for nothing here.
pub fn missing_modules(code: &str, installed: &[String]) -> Vec<String> {
    let installed = Installed::new(installed);
    imported_modules(code).into_iter().filter(|m| is_missing(m, &installed)).collect()
}

/// [`missing_modules`] for each of `codes`, reading the package list once.
pub fn missing_modules_each<'a>(codes: impl IntoIterator<Item = &'a str>, installed: &[String]) -> Vec<Vec<String>> {
    let installed = Installed::new(installed);
    codes.into_iter().map(|code| imported_modules(code).into_iter().filter(|m| is_missing(m, &installed)).collect()).collect()
}

// What runs into the sandbox.

/// What a module needs that Python cells don't have.
#[derive(Clone, Copy)]
enum Needs {
    Network,
    Programs,
}

impl Needs {
    fn words(self) -> &'static str {
        match self {
            Needs::Network => "needs the network",
            Needs::Programs => "needs other programs",
        }
    }
}

/// Modules the sandbox refuses, with everything under them.
const BLOCKED: [(&str, Needs); 22] = [
    ("socket", Needs::Network),
    ("ssl", Needs::Network),
    ("http.client", Needs::Network),
    ("http.server", Needs::Network),
    ("urllib.request", Needs::Network),
    ("urllib3", Needs::Network),
    ("requests", Needs::Network),
    ("httpx", Needs::Network),
    ("aiohttp", Needs::Network),
    ("ftplib", Needs::Network),
    ("smtplib", Needs::Network),
    ("poplib", Needs::Network),
    ("imaplib", Needs::Network),
    ("telnetlib", Needs::Network),
    ("xmlrpc", Needs::Network),
    ("websockets", Needs::Network),
    ("paramiko", Needs::Network),
    ("subprocess", Needs::Programs),
    ("multiprocessing", Needs::Programs),
    ("concurrent.futures.process", Needs::Programs),
    // Not a module, but how that one is usually reached: `from concurrent.futures import ...`.
    ("concurrent.futures.ProcessPoolExecutor", Needs::Programs),
    ("pty", Needs::Programs),
];

fn is_or_under(path: &str, module: &str) -> bool {
    path.strip_prefix(module).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// What keeps `path` from working in a Python cell, if something does.
fn blocked(path: &str) -> Option<Needs> {
    // `multiprocessing.dummy` is threads under the same interface.
    if is_or_under(path, "multiprocessing.dummy") {
        return None;
    }
    // On Linux, asyncio's event loop can't open the local socket it needs.
    if cfg!(target_os = "linux") && is_or_under(path, "asyncio") {
        return Some(Needs::Network);
    }
    BLOCKED.iter().find(|(module, _)| is_or_under(path, module)).map(|(_, needs)| *needs)
}

/// Whether a call of `name` touches a file through its first argument. `open` as a method, or
/// `Path` of something else, is someone else's.
fn takes_a_path(name: &str, attribute: bool, receiver: Option<&str>) -> bool {
    match name {
        "open" => !attribute,
        "Path" => !attribute || receiver == Some("pathlib"),
        "connect" => receiver == Some("sqlite3"),
        "read_csv" | "read_excel" | "read_parquet" | "read_json" | "read_table" | "read_pickle" | "to_csv" | "to_excel" | "to_parquet"
        | "to_json" | "to_pickle" => true,
        _ => false,
    }
}

/// Whether the literal `text`, as the first argument of `name`, is a path or a URL. SQLite keeps a
/// `:memory:` database in memory, and `read_json` also takes JSON text.
fn is_a_path(name: &str, text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    match name {
        "connect" => text != ":memory:" && !text.starts_with("file::memory:"),
        "read_json" => !text.starts_with(['{', '[']),
        _ => true,
    }
}

/// Whether `text` is a URL of something on the network. `file://` isn't, and a drive letter isn't
/// a scheme.
fn is_url(text: &str) -> bool {
    let Some((scheme, _)) = text.trim().split_once("://") else { return false };
    let shaped = scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    shaped && scheme.len() > 1 && scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && !scheme.eq_ignore_ascii_case("file")
}

/// Whether a literal's text can be a reference: not empty, and escaping nothing but a quote or a
/// backslash, which a sheet name may need.
fn is_reference_text(text: &str) -> bool {
    let mut bytes = text.bytes();
    while let Some(b) = bytes.next() {
        if b == b'\\' && !matches!(bytes.next(), Some(b'\\' | b'\'' | b'"')) {
            return false;
        }
    }
    !text.is_empty()
}

impl<'a> Source<'a> {
    fn literal(&self, i: usize) -> Option<Literal> {
        match self.tokens.get(i)?.kind {
            Tok::Str(literal) => Some(literal),
            _ => None,
        }
    }

    /// The text between the quotes of the literal at `i`.
    fn literal_text(&self, i: usize) -> Option<&'a str> {
        let (literal, token) = (self.literal(i)?, self.tokens.get(i)?);
        let quote = if literal.triple { 3 } else { 1 };
        let end = if literal.closed { token.end.saturating_sub(quote) } else { token.end };
        self.code.get(token.start + usize::from(literal.prefix) + quote..end)
    }

    /// Whether everything between tokens `a` and `b` is `allowed`. A comment isn't, whatever else is.
    fn only_between(&self, a: usize, b: usize, allowed: fn(&u8) -> bool) -> bool {
        let between = self.tokens.get(a).zip(self.tokens.get(b)).and_then(|(a, b)| self.code.as_bytes().get(a.end..b.start));
        between.is_some_and(|gap| gap.iter().all(allowed))
    }

    /// Whether the call of `xl` at `name` has a reference the app can read from the code, which is
    /// all the worker is given. This is the rule of `gridcraft_model::python::xl_calls`, so that this
    /// warning and the failure when the cell runs are the same thing: a literal with no prefix, not
    /// triple quoted, closed on its own line with nothing after it there but `,` or `)`.
    fn plain_reference(&self, name: usize) -> bool {
        let (open, literal, after) = (name + 1, name + 2, name + 3);
        let same_line = |b: &u8| matches!(b, b' ' | b'\t');
        self.literal(literal).is_some_and(|l| l.prefix == 0 && !l.triple && l.closed)
            && (self.is_punct(after, b',') || self.is_punct(after, b')'))
            && self.only_between(name, open, same_line)
            && self.only_between(open, literal, u8::is_ascii_whitespace)
            && self.only_between(literal, after, same_line)
            && self.literal_text(literal).is_some_and(is_reference_text)
    }

    /// The first and last token of the first element of the bracket at `open`. A bracket never
    /// closed has only the next token to go by. `None` when there's nothing in it.
    fn first_element(&self, open: usize) -> Option<(usize, usize)> {
        let first = open + 1;
        match self.tokens.get(open)?.first_end {
            0 => (first < self.tokens.len()).then_some((first, first)),
            end => (end > first).then_some((first, end - 1)),
        }
    }

    /// When the first argument of the call at `open` is a string literal and nothing more: its text
    /// and where it is. Literals side by side are one literal, read by the first one's text.
    fn literal_argument(&self, open: usize) -> Option<(&'a str, (usize, usize))> {
        let fixed = |i: usize| self.literal(i).is_some_and(|l| l.closed && !l.interpolated);
        let first = open + 1;
        let mut last = first;
        if !fixed(first) {
            return None;
        }
        while fixed(last + 1) {
            last += 1;
        }
        if !self.is_punct(last + 1, b',') && !self.is_punct(last + 1, b')') {
            return None;
        }
        Some((self.literal_text(first)?, self.span(first, last)))
    }

    /// The name before the dot before `i`.
    fn receiver(&self, i: usize) -> Option<&'a str> {
        if !self.after_dot(i) {
            return None;
        }
        self.name(i.checked_sub(2)?)
    }

    /// `def` or `class` is before `i`, so it is being defined and not called.
    fn is_defined(&self, i: usize) -> bool {
        i.checked_sub(1).is_some_and(|before| self.is_word(before, "def") || self.is_word(before, "class"))
    }
}

/// One thing in the code that the sandbox will refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: &'static Kind,
    /// The 1-based line of `start`.
    pub line: u32,
    /// Byte offsets into the code, on char boundaries.
    pub start: usize,
    pub end: usize,
    /// The specifics: the module and what it needs, the path, or the name to install.
    pub detail: String,
}

/// How many findings [`lint`] returns: the first ones in the code.
const MAX_FINDINGS: usize = 100;

/// How much of the code's own text a finding's detail holds.
const MAX_DETAIL_CHARS: usize = 200;

/// The code's text as a finding's detail: one line, of a length that can be shown.
fn detail(text: &str) -> String {
    let mut chars = text.chars().map(|c| if c.is_control() { ' ' } else { c });
    let mut out: String = chars.by_ref().take(MAX_DETAIL_CHARS).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

/// What the sandbox will refuse in `code`, in order of position, at most 100. `installed` is the
/// distribution names of the packages this Python has, or `None` when not known yet, in which case
/// `lint.importMissing` isn't reported.
pub fn lint(code: &str, installed: Option<&[String]>) -> Vec<Finding> {
    let source = Source::new(code);
    let (rebinds_open, rebinds_input) = (source.rebinds("open"), source.rebinds("input"));
    let mut linter = Linter { source, installed: installed.map(Installed::new), rebinds_open, rebinds_input, found: Vec::new() };
    let mut i = 0;
    while i < linter.source.tokens.len() && linter.found.len() < MAX_FINDINGS {
        i = linter.step(i);
    }
    let mut found = linter.found;
    found.sort_by_key(|f| f.start);
    number_lines(code, &mut found);
    found
}

/// Sets each finding's line, for findings in order of position. `\r\n` and `\n` end a line, and so
/// does a `\r` on its own, as Python reads it.
fn number_lines(code: &str, found: &mut [Finding]) {
    let bytes = code.as_bytes();
    let ends_a_line = |i: &usize| match bytes.get(*i) {
        Some(b'\n') => true,
        Some(b'\r') => bytes.get(*i + 1) != Some(&b'\n'),
        _ => false,
    };
    let (mut at, mut line) = (0, 1u32);
    for finding in found {
        let breaks = (at..finding.start).filter(ends_a_line).count();
        line = line.saturating_add(u32::try_from(breaks).unwrap_or(u32::MAX));
        at = finding.start;
        finding.line = line;
    }
}

struct Linter<'a> {
    source: Source<'a>,
    installed: Option<Installed>,
    /// The code gives `open` or `input` a meaning of its own, so calling it says nothing.
    rebinds_open: bool,
    rebinds_input: bool,
    found: Vec<Finding>,
}

impl Linter<'_> {
    fn report(&mut self, id: &str, (start, end): (usize, usize), detail: String) {
        if let Some(kind) = kind(id)
            && self.found.len() < MAX_FINDINGS
        {
            self.found.push(Finding { kind, line: 1, start, end, detail });
        }
    }

    /// Reads token `i` and says which to read next. Findings come in order of position: a call's
    /// finding is at or before anything in its arguments.
    fn step(&mut self, i: usize) -> usize {
        if self.source.starts_import(i) {
            let (imported, next) = self.source.import_statement(i);
            for module in &imported {
                self.check_import(module);
            }
            return next;
        }
        if self.source.tokens.get(i).is_some_and(|t| t.kind == Tok::Name) {
            self.check_name(i);
        }
        i + 1
    }

    fn check_import(&mut self, module: &Imported) {
        if draws_in_a_browser(&module.path) {
            self.report("lint.browserPlots", (module.start, module.end), detail(&module.path));
        } else if !self.check_blocked(module) {
            // A module that can't work here isn't worth installing, so this comes second.
            self.check_missing(module);
        }
    }

    /// Reports what `module` can't do here, and says whether there was anything. In a `from`
    /// statement, what is blocked can be the name it takes (`from urllib import request`).
    fn check_blocked(&mut self, module: &Imported) -> bool {
        let at_module = (module.start, module.end);
        if module.names.is_empty() {
            let Some(needs) = blocked(&module.path) else { return false };
            self.report_blocked(at_module, &module.path, needs);
            return true;
        }
        let whole_module = blocked(&module.path).is_some();
        let mut any = false;
        for taken in &module.names {
            let path = format!("{}.{}", module.path, taken.name);
            let Some(needs) = blocked(&path) else { continue };
            if !whole_module {
                self.report_blocked((taken.start, taken.end), &path, needs);
            } else if !any {
                self.report_blocked(at_module, &module.path, needs);
            }
            any = true;
        }
        any
    }

    fn report_blocked(&mut self, at: (usize, usize), path: &str, needs: Needs) {
        self.report("lint.importBlocked", at, format!("{} {}", detail(path), needs.words()));
    }

    fn check_missing(&mut self, module: &Imported) {
        let top = top_level(&module.path);
        if self.installed.as_ref().is_some_and(|installed| is_missing(top, installed)) {
            self.report("lint.importMissing", (module.start, module.end), detail(&install_name(top)));
        }
    }

    fn check_name(&mut self, i: usize) {
        let name = self.source.text(i);
        let receiver = self.source.receiver(i);
        if (receiver, name) == (Some("os"), "environ") {
            self.report_name("lint.environ", i);
        } else if self.source.is_punct(i + 1, b'(') && !self.source.is_defined(i) {
            self.check_call(i, name, receiver);
        }
    }

    fn check_call(&mut self, i: usize, name: &str, receiver: Option<&str>) {
        let attribute = self.source.after_dot(i);
        match (name, receiver) {
            ("xl", _) if !attribute => self.check_xl(i),
            ("input", _) if !attribute && !self.rebinds_input => self.report_name("lint.window", i),
            ("open", _) if !attribute && self.rebinds_open => {}
            ("getenv", Some("os")) => self.report_name("lint.environ", i),
            // With a file's name: the picture needs no saving. `savefig(buffer)` works.
            ("savefig", _) => self.check_savefig(i),
            _ if takes_a_path(name, attribute, receiver) => self.check_path(i, name),
            _ => {}
        }
    }

    /// Reports the name at `i`, with the name it is an attribute of, if any.
    fn report_name(&mut self, id: &str, i: usize) {
        let name = self.source.text(i);
        let (first, what) = match self.source.receiver(i) {
            Some(receiver) => (i.saturating_sub(2), format!("{receiver}.{name}")),
            None => (i, name.to_string()),
        };
        self.report(id, self.source.span(first, i), detail(&what));
    }

    /// Points at the file's name, when it is written out.
    fn check_savefig(&mut self, i: usize) {
        if let Some((text, at)) = self.source.literal_argument(i + 1) {
            self.report("lint.savefig", at, detail(text));
        }
    }

    /// Points at the first argument, or at the call when it has none.
    fn check_xl(&mut self, i: usize) {
        if self.source.plain_reference(i) {
            return;
        }
        let open = i + 1;
        let (at, text) = match self.source.first_element(open) {
            Some((first, last)) => {
                let (start, end) = self.source.span(first, last);
                ((start, end), self.source.code.get(start..end).unwrap_or_default())
            }
            None => {
                let closer = self.source.tokens.get(open).map_or(0, |t| t.first_end);
                (self.source.span(i, closer.max(open)), "")
            }
        };
        self.report("lint.xlNotLiteral", at, detail(text));
    }

    fn check_path(&mut self, i: usize, name: &str) {
        let Some((text, at)) = self.source.literal_argument(i + 1) else { return };
        if !is_a_path(name, text) {
            return;
        }
        let pandas = name.starts_with("read_") || name.starts_with("to_");
        let text = if pandas && is_url(text) { format!("{} needs the network", detail(text)) } else { detail(text) };
        self.report("lint.fileAccess", at, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excels_own_module_isnt_a_package_to_install() {
        let installed = ["pandas".to_string()];
        assert_eq!(missing_modules("import excel\nimport excel.foo\nfrom excel import x\nimport nope", &installed), ["nope"]);
        assert!(lint("import excel\n", Some(&installed)).is_empty());
    }

    fn modules(code: &str) -> Vec<String> {
        imported_modules(code)
    }

    fn installed(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    fn ids(code: &str) -> Vec<&'static str> {
        lint(code, None).iter().map(|f| f.kind.id).collect()
    }

    /// The text each finding points at, with its detail.
    fn spans(code: &str) -> Vec<(&str, String)> {
        lint(code, None).into_iter().map(|f| (&code[f.start..f.end], f.detail)).collect()
    }

    /// What every result must be, whatever the code was.
    fn assert_sound(code: &str, findings: &[Finding]) {
        assert!(findings.len() <= 100);
        let mut previous = 0;
        for f in findings {
            assert!(previous <= f.start && f.start <= f.end && f.end <= code.len(), "{f:?}");
            assert!(code.is_char_boundary(f.start) && code.is_char_boundary(f.end), "{f:?}");
            assert!(f.detail.chars().count() <= MAX_DETAIL_CHARS + 30, "{f:?}");
            previous = f.start;
            // Only on short code: this is slow, on purpose, to be plainly right.
            if code.len() < 10_000 {
                let before = code[..f.start].replace("\r\n", "\n").replace('\r', "\n");
                assert_eq!(f.line as usize, before.matches('\n').count() + 1, "{code:?} {f:?}");
            }
        }
    }

    /// Whether the text after `code` is still read as code: the string or comment `code` ends in has
    /// ended where it should.
    fn ends_at_its_line(code: &str) -> bool {
        modules(&format!("{code}\nimport after")) == ["after"]
    }

    #[test]
    fn every_form_of_import_names_its_top_level_module() {
        let code = "import a\nimport b.c\nimport d as e\nimport f, g.h as i, j\nfrom k import l\nfrom m.n import o as p, q\n\
                    from r import (s,\n  t as u,\n)\nfrom v import *\nif x:\n    import w\ntry: import y\nexcept E: pass\nimport z; import aa\n\
                    import bb . cc\nimport dd.\\\n  ee, \\\n  ff\n";
        let expected = ["a", "b", "d", "f", "g", "j", "k", "m", "r", "v", "w", "y", "z", "aa", "bb", "dd", "ff"];
        assert_eq!(modules(code), expected);
    }

    #[test]
    fn a_module_imported_twice_is_listed_once_where_first_seen() {
        assert_eq!(modules("import b\nimport a\nfrom b.c import d\nimport a.e"), ["b", "a"]);
    }

    #[test]
    fn relative_imports_and_other_uses_of_from_name_no_module() {
        for code in ["from . import a", "from .b import c", "from ..d.e import f", "from ... import g", "from . import (h, i)"] {
            assert!(modules(code).is_empty(), "{code}");
        }
        assert!(modules("raise E from h").is_empty());
        assert!(modules("yield from i").is_empty());
        assert_eq!(modules("raise E from h\nimport j"), ["j"]);
    }

    #[test]
    fn unfinished_and_malformed_imports_return_what_they_can() {
        for (code, expected) in [
            ("", vec![]),
            ("import", vec![]),
            ("import ", vec![]),
            ("from", vec![]),
            ("from a", vec![]),
            ("from a import", vec!["a"]),
            ("from a import (", vec!["a"]),
            ("import a as", vec!["a"]),
            ("import a,", vec!["a"]),
            ("import a.", vec!["a"]),
            ("import if", vec![]),
            ("import import a", vec!["a"]),
            ("x.import\nx.from", vec![]),
        ] {
            assert_eq!(modules(code), expected, "{code}");
        }
    }

    #[test]
    fn text_in_comments_and_strings_is_not_code() {
        for code in [
            "# import a",
            "x = 1 # import a",
            "x = 'import a'",
            "x = \"import a\"",
            "x = '''\nimport a\n'''",
            "x = \"\"\"\nimport a\n\"\"\"",
            "x = 'it\\'s; import a'",
            "x = \"\\\"; import a\"",
            "x = '\\\\' # import a",
            "x = 'a\\\nimport b'",
            "x = 'a\\\r\nimport b'",
            "x = \"\"\"a\"b\"\"c\nimport d\"\"\"",
            "x = ''; y = '' # import a",
        ] {
            assert!(ends_at_its_line(code), "{code}");
        }
    }

    #[test]
    fn every_string_prefix_in_any_case_starts_a_string() {
        for prefix in
            ["r", "u", "b", "f", "t", "br", "rb", "fr", "rf", "tr", "rt", "R", "U", "B", "F", "T", "bR", "Rb", "BR", "fR", "Rf", "FR", "tR", "RT"]
        {
            for quote in ["'", "\"", "'''", "\"\"\""] {
                let code = format!("x = {prefix}{quote}\\{quote}; import a {quote}");
                assert!(ends_at_its_line(&code), "{code}");
            }
        }
        // Letters that aren't a prefix are a name, and the string after them is still a string.
        assert!(ends_at_its_line("abc'import a'"));
        assert!(ends_at_its_line("ub'import a'"));
    }

    #[test]
    fn replacement_fields_do_not_end_an_fstring() {
        for code in [
            // The outer quote inside a field (Python 3.12).
            "f\"{x[\"; import a; \"]}\"",
            "f\"{f\"{f\"{y}; import a\"}\"}\"",
            "f\"{x[\"#\"]}; import a\"",
            // A quote in a format spec is a fill character.
            "f\"{x:'>10}; import a\"",
            "f'{x:\">10}; import a'",
            "f\"{x:{y}'}; import a\"",
            "f\"{x!r:>{w}.{p}f}; import a\"",
            // A colon in brackets or a string isn't a format spec.
            "f\"{d['a:\"b']}; import a\"",
            "f\"{x[1:2]['a']}; import a\"",
            "f\"{ {'a': 1}['a'] }; import a\"",
            // Braces that are text.
            "f\"{{; import a\"",
            "f\"}}; import a\"",
            "f\"}; import a\"",
            "rf\"\\d{x}; import a\"",
            "f\"\\{x['a']}; import a\"",
            "f\"\\N{EM DASH}; import a\"",
            "t\"{x['a']}; import a\"",
            // Triple quoted, over several lines.
            "f\"\"\"{x:'>10}\nimport a\n\"\"\"",
            "f\"\"\"{\n  x # import a\n}\nimport a\"\"\"",
            "f\"\"\"{x['''\nimport a\n''']}\"\"\"",
            "f\"\"\"{f'{y}'}\nimport a\"\"\"",
        ] {
            assert!(ends_at_its_line(code), "{code}");
        }
    }

    #[test]
    fn an_unfinished_string_ends_at_its_line_unless_triple_quoted() {
        for code in ["x = 'abc", "x = \"abc\\\"", "x = b'abc", "f\"abc", "f\"{x", "f\"{x\"", "f\"{x:", "f\"{x:{", "f\"{f\"{x", "f\"{x['", "f\"{x # y"]
        {
            assert!(ends_at_its_line(code), "{code}");
            assert_eq!(modules(&format!("{code}\r\nimport after")), ["after"], "{code}");
            assert_eq!(modules(&format!("{code}\rimport after")), ["after"], "{code}");
        }
        for code in ["x = '''abc\nimport a", "x = \"\"\"abc\nimport a\"\"", "x = f\"\"\"{y\nimport a", "x = f\"\"\"{y:\nimport a", "x = '''abc\\"] {
            assert!(modules(code).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_name_runs_through_non_ascii_letters_and_digits() {
        assert_eq!(modules("import é.b"), ["é"]);
        assert_eq!(modules("x = 'é' # é\nimport 模块"), ["模块"]);
        assert_eq!(modules("x = '\\é'\nimport a"), ["a"]);
        assert_eq!(modules("reimport a\nimport_b\nimports c"), Vec::<String>::new());
        assert_eq!(modules("1import a"), Vec::<String>::new());
    }

    #[test]
    fn install_names_differ_only_where_they_have_to() {
        for (module, distribution) in [
            ("sklearn", "scikit-learn"),
            ("PIL", "pillow"),
            ("cv2", "opencv-python"),
            ("yaml", "pyyaml"),
            ("bs4", "beautifulsoup4"),
            ("dateutil", "python-dateutil"),
            ("dotenv", "python-dotenv"),
            ("sklearn.svm", "scikit-learn"),
            ("pandas", "pandas"),
            ("pil", "pil"),
            ("", ""),
        ] {
            assert_eq!(install_name(module), distribution);
        }
    }

    #[test]
    fn the_standard_library_list_can_be_searched() {
        assert!(STANDARD_LIBRARY.windows(2).all(|w| w[0] < w[1]), "the list must stay in byte order");
        for module in ["os", "cProfile", "tomllib", "asyncio", "zoneinfo", "abc", "__future__", "_thread"] {
            assert!(in_standard_library(module), "{module}");
        }
        for module in ["pandas", "OS", "Tkinter", ""] {
            assert!(!in_standard_library(module), "{module}");
        }
    }

    #[test]
    fn missing_modules_are_those_neither_standard_nor_installed() {
        let code = "import os, json\nimport numpy as np\nimport pandas\nfrom __future__ import annotations\nfrom .x import y\nimport zzz.a";
        assert_eq!(missing_modules(code, &[]), ["numpy", "pandas", "zzz"]);
        assert_eq!(missing_modules(code, &installed(&["numpy", "zzz"])), ["pandas"]);
        assert!(missing_modules(code, &installed(&["NumPy", "Pandas", "ZZZ"])).is_empty());
    }

    #[test]
    fn missing_modules_compare_names_as_distributions_do() {
        assert!(missing_modules("import typing_inspect", &installed(&["typing-inspect"])).is_empty());
        assert!(missing_modules("import Foo_Bar", &installed(&["foo-bar"])).is_empty());
        for (module, distribution) in [
            ("sklearn", "scikit-learn"),
            ("sklearn", "Scikit_Learn"),
            ("PIL", "Pillow"),
            ("cv2", "opencv-python"),
            ("yaml", "PyYAML"),
            ("bs4", "beautifulsoup4"),
            ("dateutil", "python-dateutil"),
            ("dotenv", "python-dotenv"),
        ] {
            let code = format!("import {module}");
            assert!(missing_modules(&code, &installed(&[distribution])).is_empty(), "{module}");
            assert_eq!(missing_modules(&code, &[]), [module], "{module}");
        }
        // A namespace package's distributions don't carry the module's bare name.
        assert!(missing_modules("import google.cloud.storage", &installed(&["google-cloud-storage"])).is_empty());
        assert!(missing_modules("import ruamel.yaml", &installed(&["ruamel.yaml"])).is_empty());
        // An installed distribution with another first part provides nothing here.
        assert_eq!(missing_modules("import pandas", &installed(&["geopandas"])), ["pandas"]);
    }

    /// Each returns in far less than the limit: the work is a linear pass whatever the text.
    #[test]
    #[allow(clippy::disallowed_methods)] // the clock, off wasm
    fn hostile_code_returns_quickly() {
        const SIZE: usize = 1 << 20;
        let repeat = |s: &str| s.repeat(SIZE / s.len());
        let hostile = [
            repeat("("),
            repeat(")"),
            repeat("\n"),
            repeat("\r"),
            repeat("\\"),
            repeat("\0"),
            repeat("'"),
            repeat("\"\"\""),
            repeat("f\""),
            repeat("f\"{"),
            repeat("f\"\"\"{"),
            repeat("{"),
            repeat("."),
            repeat("a"),
            repeat("é"),
            repeat("import "),
            repeat("import a."),
            repeat("from "),
            repeat("from a from "),
            repeat("from . "),
            repeat("import a,"),
            format!("from a import {}", repeat("b,")),
            format!("'''{}", repeat("a")),
            format!("f\"\"\"{{{}{}", repeat("x:{"), repeat("\n")),
            format!("f\"\"\"{{'{}", repeat("\n")),
            // Calls that look at what follows them, nested and side by side.
            repeat("xl("),
            repeat("xl(a,"),
            repeat("my_xl("),
            repeat("open("),
            repeat("open(x,"),
            repeat("open(\"a\" "),
            repeat("open(\"\","),
            repeat("\"a\" "),
            repeat("x."),
            repeat("os."),
            repeat("os.environ "),
            repeat("input()\n"),
            repeat("plt."),
            repeat("plt.plot("),
            repeat("my.plot"),
            repeat("my.plot("),
            repeat("plot("),
            repeat(".plot("),
            repeat("savefig("),
            repeat("import matplotlib\n"),
            repeat("from urllib import request\n"),
            format!("from urllib import {}", repeat("request,")),
            format!("from a import {}", repeat("request,")),
            format!("xl({})", repeat("a+")),
            format!("xl(\"{}\")", repeat("\\\\")),
            format!("xl({}\"A1\")", repeat(" ")),
            format!("open(\"{}\")", repeat("a")),
            format!("read_csv(\"https://{}\")", repeat("é")),
            format!("x = 1{}\nimport socket", repeat(" ")),
        ];
        let installed = vec!["a".repeat(SIZE), "_".repeat(SIZE), "a-".repeat(SIZE / 2)];
        for code in &hostile {
            let began = std::time::Instant::now();
            let _ = imported_modules(code);
            let _ = missing_modules(code, &installed);
            assert_sound(code, &lint(code, None));
            assert_sound(code, &lint(code, Some(&installed)));
            assert!(began.elapsed().as_secs() < 10, "{:?}", code.get(..20));
        }
    }

    #[test]
    fn xl_needs_a_plain_literal_as_its_first_argument() {
        for code in [
            "xl(name)",
            "xl(name, headers=True)",
            "xl(f\"A1\")",
            "xl(F'A1')",
            "xl(\"A\" + \"1\")",
            "xl(\"A\" \"1\")",
            "xl()",
            "xl( )",
            "xl(\n)",
            "xl(*refs)",
            "xl(reference=\"A1\")",
            "total = 1 + xl(a)[0]",
            "if a:\n    b = xl(c)",
            "f(g, xl(a))",
            // Still being typed.
            "xl(",
            "xl(\"A1",
            "xl(\"A1\"",
            // The app reads none of these, so they fail when the cell runs.
            "xl(r\"A1\")",
            "xl(u\"A1\")",
            "xl(b\"A1\")",
            "xl('''A1''')",
            "xl(\"\")",
            "xl(\"A\\n1\")",
            "xl(\"A1\\\")",
            "xl((\"A1\"))",
            "xl(\"A1\"\n)",
            "xl(\"A1\" # c\n)",
            "xl(# c\n\"A1\")",
            "xl(\"A\\\n1\")",
            "xl(\"A1\".upper())",
            "xl(\"A1\" if a else \"B1\")",
            "[xl\n(\"A1\")]",
            "xl \\\n(\"A1\")",
        ] {
            assert_eq!(ids(code), ["lint.xlNotLiteral"], "{code}");
        }
    }

    #[test]
    fn xl_with_a_plain_literal_is_fine() {
        for code in [
            "xl(\"A1\")",
            "xl('A1')",
            "xl( \"A1\" )",
            "xl  (\"A1\")",
            "xl\t('A1')",
            "xl(\n  \"A1\",\n  headers=True,\n)",
            "xl(\"A1:B5\", headers=True)",
            "xl(\"A1\" , headers=True)",
            "xl(\"'My Sheet'!A1:B3\")",
            "xl('Bob\\'s!A1')",
            "xl(\"A\\\\1\")",
            "xl(\"Données!A1\")",
            "y = xl(\"A1\") + xl('B1')",
            "f(xl(\"A1\"), 2)",
        ] {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn only_a_call_of_the_name_xl_counts() {
        for code in [
            ".xl(name)",
            "obj.xl(name)",
            "obj . xl(name)",
            "a().xl(name)",
            "my_xl(name)",
            "xl2(name)",
            "_xl(name)",
            "XL(name)",
            "def xl(a): pass",
            "x = xl",
            "map(xl, refs)",
            "from a import xl",
        ] {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn xl_points_at_its_first_argument_or_at_the_call() {
        for (code, span, text) in [
            ("xl(name)", "name", "name"),
            ("xl( name )", "name", "name"),
            ("xl(\"A\" + \"1\", headers=True)", "\"A\" + \"1\"", "\"A\" + \"1\""),
            ("xl(f(a, b), c)", "f(a, b)", "f(a, b)"),
            ("xl([a, b])", "[a, b]", "[a, b]"),
            ("xl(a +\n  b)", "a +\n  b", "a +   b"),
            ("xl()", "xl()", ""),
            ("xl( ,)", "xl( ,", ""),
            ("xl(", "xl(", ""),
            ("xl(name", "name", "name"),
        ] {
            assert_eq!(spans(code), [(span, text.to_string())], "{code}");
        }
    }

    #[test]
    fn imports_of_what_needs_the_network_or_other_programs_are_blocked() {
        for (module, needs) in [
            ("socket", "needs the network"),
            ("ssl", "needs the network"),
            ("http.client", "needs the network"),
            ("urllib.request", "needs the network"),
            ("urllib3", "needs the network"),
            ("requests", "needs the network"),
            ("httpx", "needs the network"),
            ("aiohttp", "needs the network"),
            ("ftplib", "needs the network"),
            ("smtplib", "needs the network"),
            ("poplib", "needs the network"),
            ("imaplib", "needs the network"),
            ("telnetlib", "needs the network"),
            ("xmlrpc", "needs the network"),
            ("websockets", "needs the network"),
            ("paramiko", "needs the network"),
            ("subprocess", "needs other programs"),
            ("multiprocessing", "needs other programs"),
            ("concurrent.futures.process", "needs other programs"),
            ("pty", "needs other programs"),
        ] {
            let expected = [(module, format!("{module} {needs}"))];
            for code in [
                format!("import {module}"),
                format!("import {module} as x"),
                format!("from {module} import y"),
                format!("from {module} import y as z, w"),
                format!("from {module} import (\n  y,\n  z,\n)"),
                format!("from {module} import *"),
                format!("import os, {module}, sys"),
                format!("if a:\n    import {module}"),
                format!("def f():\n\timport {module} as x"),
                format!("try: import {module}\nexcept E: pass"),
                format!("a = 1; import {module}"),
            ] {
                assert_eq!(spans(&code), expected, "{code}");
                assert_eq!(ids(&code), ["lint.importBlocked"], "{code}");
            }
        }
    }

    #[test]
    fn a_module_under_a_blocked_one_is_blocked() {
        assert_eq!(spans("import http.client"), [("http.client", "http.client needs the network".to_string())]);
        assert_eq!(spans("from xmlrpc.client import x"), [("xmlrpc.client", "xmlrpc.client needs the network".to_string())]);
        assert_eq!(spans("import http . client"), [("http . client", "http.client needs the network".to_string())]);
    }

    #[test]
    fn what_a_from_statement_takes_can_be_what_is_blocked() {
        let blocked = |span: &'static str, path: &str, needs: &str| (span, format!("{path} {needs}"));
        assert_eq!(spans("from urllib import parse, request"), [blocked("request", "urllib.request", "needs the network")]);
        assert_eq!(spans("from urllib import request as r"), [blocked("request", "urllib.request", "needs the network")]);
        let code = "from concurrent.futures import ProcessPoolExecutor";
        assert_eq!(spans(code), [blocked("ProcessPoolExecutor", "concurrent.futures.ProcessPoolExecutor", "needs other programs")]);
        let code = "from concurrent.futures import process";
        assert_eq!(spans(code), [blocked("process", "concurrent.futures.process", "needs other programs")]);
        // A statement taking several things from a blocked module is one finding, at the module.
        assert_eq!(spans("from socket import socket, AF_INET"), [blocked("socket", "socket", "needs the network")]);
        assert_eq!(lint("from socket import socket, AF_INET", None)[0].start, 5);
        assert_eq!(spans("from multiprocessing import dummy, Pool"), [blocked("multiprocessing", "multiprocessing", "needs other programs")]);
    }

    #[test]
    fn imports_that_work_are_not_blocked() {
        for code in [
            "import urllib",
            "import urllib.parse",
            "import urllib.parse as p",
            "from urllib.parse import quote",
            "from urllib import parse",
            "import pandas",
            "import pandas as pd",
            "import concurrent.futures",
            "from concurrent.futures import ThreadPoolExecutor",
            "import concurrent.futures.thread",
            "import multiprocessing.dummy",
            "from multiprocessing.dummy import Pool",
            "from multiprocessing import dummy",
            // Names that only look like blocked ones.
            "import sslx",
            "import httpx2",
            "import my_socket",
            "import Socket",
            "import requests_mock",
            // Not imports, or of the code's own package.
            "from . import socket",
            "from .socket import x",
            "socket.socket()",
            "raise E from socket",
            "x.import_socket",
        ] {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn asyncio_is_blocked_only_on_linux() {
        let linux = cfg!(target_os = "linux");
        for code in ["import asyncio", "from asyncio import run", "import asyncio.subprocess"] {
            assert_eq!(ids(code) == ["lint.importBlocked"], linux, "{code}");
            assert_eq!(ids(code).is_empty(), !linux, "{code}");
        }
    }

    #[test]
    fn a_missing_package_is_reported_only_when_what_is_installed_is_known() {
        assert!(lint("import pandas", None).is_empty());
        let found = lint("import pandas", Some(&[]));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].kind.id, found[0].detail.as_str(), found[0].line), ("lint.importMissing", "pandas", 1));
        assert!(lint("import pandas", Some(&installed(&["Pandas"]))).is_empty());
        assert!(lint("import os, json\nfrom __future__ import annotations\nfrom .a import b", Some(&[])).is_empty());
    }

    #[test]
    fn a_missing_package_says_the_name_to_install() {
        for (code, span, install) in [
            ("import sklearn.svm as s", "sklearn.svm", "scikit-learn"),
            ("from PIL import Image", "PIL", "pillow"),
            ("import cv2", "cv2", "opencv-python"),
            ("if a:\n  import yaml", "yaml", "pyyaml"),
            ("from bs4 import BeautifulSoup as B, Tag", "bs4", "beautifulsoup4"),
            ("from dateutil.parser import parse", "dateutil.parser", "python-dateutil"),
            ("import os, dotenv", "dotenv", "python-dotenv"),
            ("import zzz.a.b", "zzz.a.b", "zzz"),
        ] {
            let found = lint(code, Some(&[]));
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!((found[0].kind.id, &code[found[0].start..found[0].end], found[0].detail.as_str()), ("lint.importMissing", span, install));
        }
        assert!(lint("import sklearn", Some(&installed(&["scikit-learn"]))).is_empty());
    }

    #[test]
    fn a_blocked_module_is_not_also_told_to_be_installed() {
        let code = "import requests\nfrom httpx import get\nimport urllib3 as u\nimport zzz";
        let found: Vec<&str> = lint(code, Some(&[])).iter().map(|f| f.kind.id).collect();
        assert_eq!(found, ["lint.importBlocked", "lint.importBlocked", "lint.importBlocked", "lint.importMissing"]);
    }

    #[test]
    fn each_place_a_missing_package_is_imported_is_a_finding() {
        let lines: Vec<u32> = lint("import a\nx = 1\nimport a", Some(&[])).iter().map(|f| f.line).collect();
        assert_eq!(lines, [1, 3]);
    }

    #[test]
    fn a_literal_path_as_the_first_argument_is_file_access() {
        for call in [
            "open",
            "pd.read_csv",
            "pandas.read_csv",
            "read_csv",
            ".read_csv",
            "a().read_csv",
            "pd.read_excel",
            "pd.read_parquet",
            "pd.read_json",
            "pd.read_table",
            "pd.read_pickle",
            "df.to_csv",
            "df.to_excel",
            "df.to_parquet",
            "df.to_json",
            "df.to_pickle",
            "Path",
            "pathlib.Path",
            "sqlite3.connect",
        ] {
            for args in
                ["(\"a.csv\")", "('a.csv')", "( 'a.csv' )", "(\"a.csv\", 'w')", "(\n  \"a.csv\",\n  x=1,\n)", "(# c\n\"a.csv\")", "  (\"a.csv\")"]
            {
                let code = format!("{call}{args}");
                let found = lint(&code, None);
                assert_eq!(found.len(), 1, "{code}");
                assert_eq!((found[0].kind.id, found[0].detail.as_str()), ("lint.fileAccess", "a.csv"), "{code}");
                assert!(code[found[0].start..found[0].end].contains("a.csv"), "{code}");
            }
        }
    }

    #[test]
    fn a_path_can_be_any_plain_literal() {
        for (code, span, text) in [
            ("open(r\"C:\\a.txt\")", "r\"C:\\a.txt\"", "C:\\a.txt"),
            ("open(b'a')", "b'a'", "a"),
            ("open(RB'a')", "RB'a'", "a"),
            ("open(u'a')", "u'a'", "a"),
            ("open('''a''')", "'''a'''", "a"),
            ("open(\"\"\"a\"\"\", 'w')", "\"\"\"a\"\"\"", "a"),
            ("open('a' 'b')", "'a' 'b'", "a"),
            ("with open('a') as f:\n  pass", "'a'", "a"),
            ("x = open('a').read()", "'a'", "a"),
            ("open('/etc/passwd')", "'/etc/passwd'", "/etc/passwd"),
            ("Path('.')", "'.'", "."),
        ] {
            assert_eq!(spans(code), [(span, text.to_string())], "{code}");
        }
    }

    #[test]
    fn what_is_not_a_literal_path_is_not_file_access() {
        for code in [
            // Not known until it runs.
            "open(path)",
            "open(f\"{a}.txt\")",
            "open(F'a.txt')",
            "open(rf'a.txt')",
            "open('a' f'{b}')",
            "open(\"a\" + b)",
            "open(\"a{}\".format(b))",
            "open(\"a%d\" % b)",
            "open(('a'))",
            "open('a' if b else 'c')",
            "open('a'",
            "open('a",
            "pd.read_csv(io.StringIO(\"a,b\"))",
            "df.to_csv(None)",
            // No arguments, or only named ones: these return text.
            "df.to_csv()",
            "df.to_json()",
            "df.to_csv(index=False)",
            "df.to_csv(sep=\",\")",
            "Path()",
            "open()",
            // In memory, or not a path at all.
            "sqlite3.connect(\":memory:\")",
            "sqlite3.connect(':memory:', x=1)",
            "sqlite3.connect('file::memory:?cache=shared', uri=True)",
            "sqlite3.connect('')",
            "open('')",
            "pd.read_json('{\"a\": 1}')",
            "pd.read_json(' [1, 2]')",
            // Someone else's method or function.
            "x.open('a')",
            "obj.open('a')",
            "obj . open('a')",
            "a().open('a')",
            "x.Path('a')",
            "conn.connect('a')",
            "connect('a')",
            "my_open('a')",
            "reopen('a')",
            "open2('a')",
            "read_csv2('a')",
            "my_read_csv('a')",
            "Path.home()",
            // Definitions and bare names.
            "def open(a): pass",
            "def to_csv(a): pass",
            "class Path(a): pass",
            "x = open",
            "map(open, paths)",
            "from a import open",
        ] {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn a_url_in_a_pandas_call_says_it_needs_the_network() {
        for code in [
            "pd.read_csv(\"https://a.example/b.csv\")",
            ".read_csv('http://a.example/b.csv')",
            "pd.read_json('s3://a/b.json')",
            "pd.read_excel('ftp://a/b.xlsx', x=1)",
            "df.to_csv('s3://a/b.csv')",
        ] {
            let found = lint(code, None);
            assert_eq!(found.len(), 1, "{code}");
            assert_eq!(found[0].kind.id, "lint.fileAccess", "{code}");
            assert!(found[0].detail.ends_with(" needs the network"), "{code}");
        }
        for (code, text) in [
            ("pd.read_csv('file:///a/b.csv')", "file:///a/b.csv"),
            ("pd.read_csv('C://a/b.csv')", "C://a/b.csv"),
            ("pd.read_csv('a/b.csv')", "a/b.csv"),
            ("pd.read_csv('://a')", "://a"),
            ("pd.read_csv('1a://b')", "1a://b"),
            ("open('https://a.example')", "https://a.example"),
        ] {
            assert_eq!(lint(code, None)[0].detail, text, "{code}");
        }
    }

    #[test]
    fn a_detail_stays_short_and_on_one_line() {
        let long = "é".repeat(5000);
        let found = lint(&format!("open('{long}')\nimport {long}\nread_csv('http://{long}')\nxl({long})"), Some(&[]));
        assert_eq!(found.len(), 4);
        for f in &found {
            assert!(f.detail.chars().count() < MAX_DETAIL_CHARS + 30, "{}", f.detail.len());
        }
        assert!(found[0].detail.ends_with("é..."));
        assert!(found[2].detail.ends_with("é... needs the network"));
        assert_eq!(lint("open('a\tb\0c\x1b')", None)[0].detail, "a b c ");
        assert_eq!(lint("open('''a\nb''')", None)[0].detail, "a b");
    }

    #[test]
    fn input_is_what_a_window_means_now() {
        for (code, span, text) in [
            ("input()", "input", "input"),
            ("input('Name? ')", "input", "input"),
            ("name = input ()", "input", "input"),
            ("print(input())", "input", "input"),
        ] {
            assert_eq!(spans(code), [(span, text.to_string())], "{code}");
            assert_eq!(ids(code), ["lint.window"], "{code}");
        }
        for code in
            ["fig.show()", "show()", "my_plt.show()", "x.input()", "input", "my_input()", "def input(): pass", "def show(): pass", "input = 1"]
        {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn plotting_packages_are_packages_like_any_other() {
        let code = "import matplotlib\nimport seaborn as sns\nfrom matplotlib.pyplot import plot\nimport zzz";
        assert_eq!(missing_modules(code, &[]), ["matplotlib", "seaborn", "zzz"]);
        assert!(lint("import matplotlib.pyplot as plt\nimport seaborn as sns", None).is_empty());
        let installed = ["matplotlib".to_string(), "seaborn".to_string()];
        assert!(lint(code.trim_end_matches("\nimport zzz"), Some(&installed)).is_empty());
        let found: Vec<&str> = lint(code, Some(&[])).iter().map(|f| f.kind.id).collect();
        assert_eq!(found, ["lint.importMissing", "lint.importMissing", "lint.importMissing", "lint.importMissing"]);
        assert!(missing_modules("plt.plot(x)\nsns.barplot(x)", &[]).is_empty(), "the names a cell has are no imports");
    }

    #[test]
    fn packages_that_draw_in_a_browser_are_reported_and_never_said_to_be_missing() {
        for (code, span) in [
            ("import plotly", "plotly"),
            ("import plotly.express as px", "plotly.express"),
            ("from plotly import graph_objects", "plotly"),
            ("import bokeh", "bokeh"),
            ("import altair as alt", "altair"),
            ("import os, altair, sys", "altair"),
        ] {
            for known in [None, Some(&[][..])] {
                let found = lint(code, known);
                assert_eq!(found.len(), 1, "{code}");
                assert_eq!((found[0].kind.id, &code[found[0].start..found[0].end]), ("lint.browserPlots", span), "{code}");
            }
        }
        assert_eq!(missing_modules("import plotly\nimport zzz", &[]), ["zzz"]);
        for code in ["import plotlyx", "x = 'import altair'", "# import bokeh"] {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn the_default_names_are_not_imports() {
        let code = "df = pd.DataFrame()\nnp.zeros(3)\nsm.OLS(a, b)\nx = pd\ny = np\n";
        assert!(imported_modules(code).is_empty());
        assert!(missing_modules(code, &[]).is_empty());
        assert!(lint(code, Some(&[])).is_empty());
        let plots = "plt.plot(x)\nsns.barplot(x)\nplt.show()\ndf.plot()\ndf.hist()\nax.plot(x)\nplt.rcParams";
        assert!(missing_modules(plots, &[]).is_empty());
        assert!(lint(plots, Some(&[])).is_empty(), "drawing is nothing to warn about");
        // An import of what they stand for is an import like any other.
        assert_eq!(missing_modules("import pandas as pd\nimport numpy as np", &[]), ["pandas", "numpy"]);
    }

    #[test]
    fn savefig_with_a_files_name_is_reported_and_without_one_is_not() {
        for (code, span, text) in [
            ("savefig('a.png')", "'a.png'", "a.png"),
            ("plt.savefig('a.png')", "'a.png'", "a.png"),
            ("fig.savefig(\"b/c.pdf\", dpi=3)", "\"b/c.pdf\"", "b/c.pdf"),
            ("a().savefig('b')", "'b'", "b"),
        ] {
            assert_eq!(spans(code), [(span, text.to_string())], "{code}");
            assert_eq!(ids(code), ["lint.savefig"], "{code}");
        }
        for code in
            ["def savefig(a): pass", "x = fig.savefig", "my_savefig('a')", "fig.savefig(buffer, format='png')", "fig.savefig()", "savefig(path)"]
        {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn the_environment_is_reported() {
        for (code, span, text) in [
            ("os.environ", "os.environ", "os.environ"),
            ("x = os.environ['A']", "os.environ", "os.environ"),
            ("os.environ.get('A')", "os.environ", "os.environ"),
            ("os.getenv('A')", "os.getenv", "os.getenv"),
            ("os . getenv ('A', 'b')", "os . getenv", "os.getenv"),
        ] {
            assert_eq!(spans(code), [(span, text.to_string())], "{code}");
            assert_eq!(ids(code), ["lint.environ"], "{code}");
        }
        for code in ["os.getenv", "environ", "getenv('A')", "my_os.environ", "os.environment", "os.path", "x.getenv('A')", "def getenv(): pass"] {
            assert_eq!(ids(code), Vec::<&str>::new(), "{code}");
        }
    }

    #[test]
    fn every_kind_is_in_the_catalog_with_the_scope_of_a_line() {
        let code = "xl(a)\nimport socket\nimport zzz\nopen('a')\nplt.savefig('b')\nimport plotly\ninput()\nos.environ";
        let found = lint(code, Some(&[]));
        let ids: Vec<&str> = found.iter().map(|f| f.kind.id).collect();
        let expected = [
            "lint.xlNotLiteral",
            "lint.importBlocked",
            "lint.importMissing",
            "lint.fileAccess",
            "lint.savefig",
            "lint.browserPlots",
            "lint.window",
            "lint.environ",
        ];
        assert_eq!(ids, expected);
        assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(found.iter().all(|f| f.kind.scope == crate::diagnostics::Scope::Line));
    }

    #[test]
    fn nothing_in_a_comment_or_a_string_is_reported() {
        const BODY: &str = "open(\"a.csv\"); import socket; xl(a); plt.savefig(\"c\"); input(); os.environ; pd.read_csv(\"b\"); import plotly";
        assert_eq!(lint(BODY, None).len(), 8, "the text would be reported if it were code");
        let other_quotes = BODY.replace('"', "'");
        for code in [
            format!("# {BODY}"),
            format!("x = 1 # {BODY}"),
            format!("'{BODY}'"),
            format!("'''{BODY}'''"),
            format!("'''\n{BODY}\n'''"),
            format!("\"\"\"\n{BODY}\n\"\"\""),
            format!("r'{BODY}'"),
            format!("b'{BODY}'"),
            format!("rb'''{BODY}'''"),
            format!("BR'{BODY}'"),
            format!("u'{BODY}'"),
            format!("f'{BODY}'"),
            format!("F'''{BODY}'''"),
            format!("rf'{BODY}'"),
            format!("f\"\"\"\n{BODY}\n\"\"\""),
            format!("t'{BODY}'"),
            format!("\"{other_quotes}\""),
            format!("f\"{other_quotes}\""),
            // Inside an f-string's replacement field too.
            format!("f'{{{BODY}}}'"),
            format!("f\"{{{BODY}}}\""),
            format!("f\"{{x:{BODY}}}\""),
            format!("f'''\n{{\n{BODY}\n}}\n'''"),
            "f\"{open('a.csv')}\"".to_string(),
            "f\"{open(\"a.csv\")}\"".to_string(),
            "f\"{xl(a)}\"".to_string(),
            "f\"{input()}\"".to_string(),
            "f\"{f'{os.environ}'}\"".to_string(),
            "print(\"open(\")".to_string(),
            "x = \"open(\\\"a.csv\\\")\"".to_string(),
            "'xl(name)'".to_string(),
            "'import socket'".to_string(),
        ] {
            assert!(lint(&code, None).is_empty(), "{code}");
            let after = format!("{code}\nimport socket");
            let found = lint(&after, None);
            assert_eq!(found.len(), 1, "{after}");
            assert_eq!((&after[found[0].start..found[0].end], found[0].line as usize), ("socket", code.matches('\n').count() + 2), "{after}");
        }
    }

    #[test]
    fn offsets_and_lines_are_right_after_non_ascii_text() {
        let code = "名前 = \"日本語\" # コメント é\nopen(\"données.csv\")\nçà = 1; input()\nx = 'é\nimport sócket, socket\nxl(é)\nos.environ # é";
        let found = lint(code, None);
        assert_sound(code, &found);
        let read: Vec<(&str, u32, &str)> = found.iter().map(|f| (&code[f.start..f.end], f.line, f.detail.as_str())).collect();
        assert_eq!(
            read,
            [
                ("\"données.csv\"", 2, "données.csv"),
                ("input", 3, "input"),
                ("socket", 5, "socket needs the network"),
                ("é", 6, "é"),
                ("os.environ", 7, "os.environ"),
            ]
        );
    }

    #[test]
    fn every_line_ending_counts_one_line() {
        let lines = |code: &str| lint(code, None).iter().map(|f| f.line).collect::<Vec<_>>();
        assert_eq!(lines("input()"), [1]);
        assert_eq!(lines("\ninput()"), [2]);
        assert_eq!(lines("a = 1\r\nb = 2\r\ninput()\r\nc = 3\r\ninput()"), [3, 5]);
        assert_eq!(lines("a\rinput()\r\rinput()"), [2, 4]);
        assert_eq!(lines("a\n\r\n\rinput()"), [4]);
        // Line breaks inside strings, comments, brackets and joined lines are lines all the same.
        assert_eq!(lines("x = \"\"\"\r\n\r\n\"\"\"\r\ninput()"), [4]);
        assert_eq!(lines("x = '''\n\n'''\ninput()"), [4]);
        assert_eq!(lines("# a\r\n# b\r\ninput()"), [3]);
        assert_eq!(lines("x = (\r\n1,\r\ninput())"), [3]);
        assert_eq!(lines("x = 1 + \\\r\n  2\r\ninput()"), [3]);
        assert_eq!(lines("x = 'a\\\r\nb'\r\ninput()"), [3]);
    }

    #[test]
    fn a_statement_ends_at_a_windows_line_ending() {
        assert_eq!(ids("import socket\r\nimport ssl\r\n"), ["lint.importBlocked", "lint.importBlocked"]);
        assert_eq!(ids("x = 'import socket\r\ninput()"), ["lint.window"]);
        assert_eq!(ids("# import socket\r\ninput()"), ["lint.window"]);
    }

    #[test]
    fn findings_are_in_order_and_capped() {
        let code = "input()\n".repeat(150);
        let found = lint(&code, None);
        assert_sound(&code, &found);
        assert_eq!(found.len(), 100);
        assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), (1..=100).collect::<Vec<u32>>());

        // A statement with more findings than the cap is cut there as well.
        let code = format!("from urllib import {}", "request, ".repeat(150));
        let found = lint(&code, None);
        assert_sound(&code, &found);
        assert_eq!(found.len(), 100);

        // A call's argument is reported after the call, at or after its position.
        let code = "xl(os.environ)\nxl(open('a'))\nxl(input())\nplt.show(input())";
        let found = lint(code, None);
        assert_sound(code, &found);
        assert_eq!(found.len(), 7);
    }

    #[test]
    fn empty_and_strange_code_finds_nothing_and_does_not_panic() {
        for code in [
            "", " ", "\n", "\r", "\r\n", "\0", "\0\0\0", "\\", "\\\n", "\\\r\n", "'", "\"", "'''", "f'", "f'{", "f'{{", "#", "é", "(", ")", "]", "}",
            ".", ",",
        ] {
            assert!(lint(code, None).is_empty(), "{code:?}");
            assert!(lint(code, Some(&[])).is_empty(), "{code:?}");
            assert!(imported_modules(code).is_empty(), "{code:?}");
        }
        assert_eq!(ids("input\0()"), Vec::<&str>::new());
        assert_eq!(ids("\0input()"), ["lint.window"]);
    }

    /// Pieces of Python put together at random, with the same seed each time: whatever comes out,
    /// nothing panics and every result is sound.
    #[test]
    fn random_code_gives_sound_results() {
        const PIECES: [&str; 50] = [
            "xl",
            "(",
            ")",
            "\"",
            "'",
            "\"\"\"",
            "'''",
            "f\"",
            "rb'",
            "F'''",
            "{",
            "}",
            ":",
            "\\",
            "\n",
            "\r\n",
            "\r",
            "#",
            " ",
            ",",
            ".",
            "é",
            "日本",
            "open",
            "import ",
            "from ",
            "socket",
            " as ",
            "os.environ",
            "plt.show",
            "sns.",
            ".plot(",
            ".hist(",
            "savefig",
            "matplotlib",
            "seaborn",
            "input",
            "read_csv",
            "to_csv",
            "Path",
            "\"a.csv\"",
            "sqlite3.connect",
            "\0",
            "=",
            "+",
            "*",
            "[",
            "]",
            "urllib",
            "request",
        ];
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |below: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % below as u64).unwrap()
        };
        let installed = installed(&["é", "a-b", "socket"]);
        for _ in 0..4000 {
            let mut code = String::new();
            for _ in 0..next(60) {
                code.push_str(PIECES[next(PIECES.len())]);
            }
            assert_sound(&code, &lint(&code, None));
            assert_sound(&code, &lint(&code, Some(&installed)));
            let _ = missing_modules(&code, &installed);
        }
    }

    #[test]
    fn a_redefined_open_or_input_is_someone_elses() {
        assert!(lint("def open(p):\n    return p\nopen('a.csv')", None).is_empty());
        assert!(lint("input = lambda: 1\ninput()", None).is_empty());
        // Comparing, passing or setting an attribute doesn't redefine it.
        assert_eq!(lint("input == 1\ninput()", None).len(), 1);
        assert_eq!(lint("f(open=1)\nopen('a.csv')", None).len(), 1);
        assert_eq!(lint("x.open = 1\nopen('a.csv')", None).len(), 1);
    }

    #[test]
    fn the_http_package_is_only_the_network_in_its_client_and_server() {
        assert!(lint("from http import HTTPStatus\nimport http", None).is_empty());
        for code in ["import http.client", "from http import client", "from http.server import HTTPServer"] {
            assert_eq!(lint(code, None).first().map(|f| f.kind.id), Some("lint.importBlocked"), "{code}");
        }
    }

    #[test]
    fn modules_with_another_install_name_read_as_installed() {
        assert_eq!(install_name("attr"), "attrs");
        assert_eq!(install_name("serial"), "pyserial");
        assert_eq!(install_name("jwt"), "pyjwt");
        let installed = ["attrs".to_string(), "PyJWT".to_string(), "pyserial".to_string()];
        assert!(missing_modules("import attr, jwt, serial", &installed).is_empty());
    }
}
