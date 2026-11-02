//! A small, strict JSON reader for FILTERJSON.
//!
//! It reads RFC 8259 exactly: no comments, no trailing commas, no single-quoted strings, no
//! `NaN`/`Infinity`, no leading zeros, no unescaped control characters, no lone surrogates.
//! Input comes from cells, so nesting and node counts are capped and every access is checked.

/// Values one document may hold.
pub(crate) const MAX_NODES: usize = 100_000;
/// Arrays and objects one document may nest.
const MAX_DEPTH: usize = 256;

#[derive(Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The member named `key`: the first match, because JSON permits repeated names.
    pub(crate) fn member(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The items of an array; an object has no positional children.
    pub(crate) fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => &[],
        }
    }
}

/// Reads one JSON value. `None` when the text is not valid JSON, too large or too deep.
pub(crate) fn parse(input: &str) -> Option<Json> {
    let mut p = Parser { b: input.as_bytes(), i: 0, nodes: 0 };
    p.ws();
    let value = p.value(0)?;
    p.ws();
    if p.i == p.b.len() { Some(value) } else { None }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    nodes: usize,
}

impl Parser<'_> {
    /// peek.
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// bump.
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i += 1;
        Some(c)
    }

    /// ws.
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    /// node.
    fn node(&mut self) -> Option<()> {
        self.nodes = self.nodes.checked_add(1)?;
        if self.nodes > MAX_NODES {
            return None;
        }
        Some(())
    }

    /// value.
    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.node()?;
        match self.peek()? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.string().map(Json::Text),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    /// literal.
    fn literal(&mut self, word: &str, value: Json) -> Option<Json> {
        let end = self.i.checked_add(word.len())?;
        if self.b.get(self.i..end)? == word.as_bytes() {
            self.i = end;
            Some(value)
        } else {
            None
        }
    }

    /// object.
    fn object(&mut self, depth: usize) -> Option<Json> {
        self.bump(); // '{'
        let mut items = Vec::new();
        self.ws();
        if self.peek()? == b'}' {
            self.bump();
            return Some(Json::Object(items));
        }
        loop {
            self.ws();
            if self.peek()? != b'"' {
                return None;
            }
            let key = self.string()?;
            self.ws();
            if self.bump()? != b':' {
                return None;
            }
            self.ws();
            items.push((key, self.value(depth + 1)?));
            self.ws();
            match self.bump()? {
                b',' => continue,
                b'}' => return Some(Json::Object(items)),
                _ => return None,
            }
        }
    }

    /// array.
    fn array(&mut self, depth: usize) -> Option<Json> {
        self.bump(); // '['
        let mut items = Vec::new();
        self.ws();
        if self.peek()? == b']' {
            self.bump();
            return Some(Json::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.bump()? {
                b',' => continue,
                b']' => return Some(Json::Array(items)),
                _ => return None,
            }
        }
    }

    /// string.
    fn string(&mut self) -> Option<String> {
        self.bump(); // '"'
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.bump()? {
                b'"' => return String::from_utf8(out).ok(),
                0x00..=0x1f => return None,
                b'\\' => self.escape(&mut out)?,
                c => out.push(c),
            }
        }
    }

    /// escape.
    fn escape(&mut self, out: &mut Vec<u8>) -> Option<()> {
        match self.bump()? {
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'/' => out.push(b'/'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'u' => {
                let ch = self.unicode()?;
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            _ => return None,
        }
        Some(())
    }

    /// Reads `\uXXXX`, joining a UTF-16 surrogate pair. A lone surrogate is invalid JSON.
    fn unicode(&mut self) -> Option<char> {
        let high = self.hex4()?;
        let code = if (0xD800..=0xDBFF).contains(&high) {
            if self.bump()? != b'\\' || self.bump()? != b'u' {
                return None;
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return None;
            }
            0x1_0000 + ((high - 0xD800) << 10) + (low - 0xDC00)
        } else if (0xDC00..=0xDFFF).contains(&high) {
            return None;
        } else {
            high
        };
        char::from_u32(code)
    }

    /// hex4.
    fn hex4(&mut self) -> Option<u32> {
        let mut n: u32 = 0;
        for _ in 0..4 {
            let d = match self.bump()? {
                c @ b'0'..=b'9' => u32::from(c - b'0'),
                c @ b'a'..=b'f' => u32::from(c - b'a' + 10),
                c @ b'A'..=b'F' => u32::from(c - b'A' + 10),
                _ => return None,
            };
            n = n.checked_mul(16)?.checked_add(d)?;
        }
        Some(n)
    }

    /// RFC 8259 numbers: no leading zeros, no leading `+`, no bare `.5` or `5.`, finite in `f64`.
    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek()? {
            b'0' => self.i += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            self.digits();
        }
        let text = std::str::from_utf8(self.b.get(start..self.i)?).ok()?;
        let n: f64 = text.parse().ok()?;
        if !n.is_finite() {
            return None;
        }
        Some(Json::Number(n))
    }

    /// digits.
    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// text.
    fn text(s: &str) -> Json {
        Json::Text(s.to_string())
    }

    #[test]
    /// reads_values.
    fn reads_values() {
        assert_eq!(parse("null"), Some(Json::Null));
        assert_eq!(parse(" true "), Some(Json::Bool(true)));
        assert_eq!(parse("-12.5e2"), Some(Json::Number(-1250.0)));
        assert_eq!(parse("0"), Some(Json::Number(0.0)));
        assert_eq!(parse("\"hola\""), Some(text("hola")));
        assert_eq!(parse("[]"), Some(Json::Array(vec![])));
        assert_eq!(parse("{}"), Some(Json::Object(vec![])));
    }

    #[test]
    /// reads_nesting.
    fn reads_nesting() {
        let expected = Json::Object(vec![
            ("a".to_string(), Json::Array(vec![Json::Number(1.0), Json::Object(vec![("b".to_string(), Json::Null)])])),
            ("c".to_string(), Json::Bool(true)),
        ]);
        assert_eq!(parse(r#"{"a":[1,{"b":null}],"c":true}"#), Some(expected));
    }

    #[test]
    /// reads_escapes.
    fn reads_escapes() {
        assert_eq!(parse(r#""a\"b\\c\/d\t""#), Some(text("a\"b\\c/d\t")));
        assert_eq!(parse(r#""\u00e9""#), Some(text("é")));
        assert_eq!(parse(r#""\ud83d\ude00""#), Some(text("😀")));
        assert_eq!(parse("\"ñandú\""), Some(text("ñandú")));
    }

    #[test]
    /// rejects_malformed.
    fn rejects_malformed() {
        for bad in [
            "",
            " ",
            "{",
            "[",
            "]",
            "{\"a\":1,}",
            "[1,]",
            "{'a':1}",
            "{a:1}",
            "{\"a\" 1}",
            "01",
            "+1",
            ".5",
            "5.",
            "1e",
            "NaN",
            "Infinity",
            "tru",
            "nul",
            "1 2",
            "\"sin cerrar",
            "\"tab\taquí\"",
            "\"\\x\"",
            "\"\\ud83d\"",
            "\"\\udc00\"",
        ] {
            assert_eq!(parse(bad), None, "debió rechazar {bad:?}");
        }
    }

    #[test]
    /// rejects_overflow_and_depth.
    fn rejects_overflow_and_depth() {
        assert_eq!(parse("1e400"), None);
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert_eq!(parse(&deep), None);
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_some());
    }

    #[test]
    /// rejects_node_flood.
    fn rejects_node_flood() {
        let many = format!("[{}]", vec!["0"; MAX_NODES].join(","));
        assert_eq!(parse(&many), None);
    }

    #[test]
    /// member_takes_the_first_match.
    fn member_takes_the_first_match() {
        let doc = parse(r#"{"a":1,"a":2}"#);
        assert_eq!(doc.as_ref().and_then(|d| d.member("a")), Some(&Json::Number(1.0)));
        assert_eq!(doc.as_ref().and_then(|d| d.member("b")), None);
    }

    // Robustness: thousands of generated inputs must never panic the reader.
    #[test]
    /// fuzz_parse_never_panics.
    fn fuzz_parse_never_panics() {
        /// rng.
        fn rng(s: &mut u64) -> u64 {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            *s
        }
        /// gen_json.
        fn gen_json(s: &mut u64, depth: usize) -> String {
            if depth == 0 || rng(s).is_multiple_of(5) {
                match rng(s) % 7 {
                    0 => "null".to_string(),
                    1 => "true".to_string(),
                    2 => "false".to_string(),
                    3 => format!("{}", (rng(s) % 200) as i64 - 100),
                    4 => {
                        let n = (rng(s) % 8) as usize;
                        let mut out = String::from("\"");
                        for _ in 0..n {
                            let c = (b'a' + (rng(s) % 26) as u8) as char;
                            out.push(c);
                        }
                        out.push('"');
                        out
                    }
                    5 => format!("{}", rng(s) % 1000),
                    _ => {
                        // occasionally malformed
                        let r = rng(s) % 4;
                        match r {
                            0 => "{".to_string(),
                            1 => "\"unterminated".to_string(),
                            2 => "1e".to_string(),
                            _ => "NaN".to_string(),
                        }
                    }
                }
            } else if rng(s).is_multiple_of(2) {
                let n = (rng(s) % 5) as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(gen_json(s, depth - 1));
                }
                format!("[{}]", v.join(","))
            } else {
                let n = (rng(s) % 4) as usize;
                let mut v = Vec::with_capacity(n);
                for i in 0..n {
                    v.push(format!("\"k{}\":{}", i, gen_json(s, depth - 1)));
                }
                format!("{{{}}}", v.join(","))
            }
        }
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..50_000 {
            let input = gen_json(&mut s, 5);
            let _ = parse(&input);
        }
    }
}
