//! Locate matched math delimiters in prose. Ranges are byte offsets into the untouched message.
use std::ops::Range;

pub(super) struct Span {
    pub outer: Range<usize>,
    pub body: Range<usize>,
    pub display: bool,
}

#[derive(Clone, Copy)]
enum Delimiter {
    Dollar,
    CodeDollar,
    DoubleDollar,
    Parenthesis,
    Bracket,
}

impl Delimiter {
    fn display(self) -> bool {
        matches!(self, Self::DoubleDollar | Self::Bracket)
    }

    fn close(self) -> &'static [u8] {
        match self {
            Self::Dollar => b"$",
            Self::CodeDollar => b"`$",
            Self::DoubleDollar => b"$$",
            Self::Parenthesis => br"\)",
            Self::Bracket => br"\]",
        }
    }
}

pub(super) fn spans(text: &str, limit: usize, offset: usize, refs: &[Range<usize>]) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut pending: Option<(Delimiter, usize, usize)> = None;
    let mut code = 0;
    let mut i = 0;
    let mut reference = refs.partition_point(|s| s.end <= offset);
    while i < bytes.len() && found.len() < limit {
        if let Some(s) = refs.get(reference) {
            if s.end <= offset + i {
                reference += 1;
                continue;
            }
            if s.start <= offset + i {
                // A quoted sheet name can contain math delimiters. Workbook references are opaque.
                pending = None;
                i = (s.end - offset).min(bytes.len());
                reference += 1;
                continue;
            }
        }
        let rest = &bytes[i..];
        if let Some((kind, start, body)) = pending {
            let close = kind.close();
            if rest.starts_with(close) {
                // A dollar following whitespace or introducing a price is not a closing delimiter.
                if i > body
                    && (!matches!(kind, Delimiter::Dollar) || (!bytes[i - 1].is_ascii_whitespace() && !rest.get(1).is_some_and(u8::is_ascii_digit)))
                {
                    found.push(Span { outer: start..i + close.len(), body: body..i, display: kind.display() });
                    i += close.len();
                    pending = None;
                    continue;
                }
                pending = None;
            } else if !kind.display() && (rest[0] == b'\n' || (rest[0] == b'`' && !matches!(kind, Delimiter::CodeDollar))) {
                pending = None;
            } else {
                i += if rest[0] == b'\\' && rest.get(1).is_some_and(|b| b.is_ascii_punctuation()) { 2 } else { 1 };
                continue;
            }
        }
        if rest[0] == b'`' {
            let n = rest.iter().take_while(|&&b| b == b'`').count();
            if code == 0 {
                code = n
            } else if code == n {
                code = 0
            }
            i += n;
        } else if code != 0 {
            i += 1;
        } else {
            let opener = if rest.starts_with(b"$$") {
                Some((Delimiter::DoubleDollar, 2))
            } else if rest.starts_with(b"$`") {
                Some((Delimiter::CodeDollar, 2))
            } else if rest.starts_with(br"\[") {
                Some((Delimiter::Bracket, 2))
            } else if rest.starts_with(br"\(") {
                Some((Delimiter::Parenthesis, 2))
            } else if rest[0] == b'$' && rest.get(1).is_some_and(|b| !b.is_ascii_whitespace()) {
                // The dollar can also be in the middle of a mixed address, e.g. B$1:$C2.
                if i > 0 && bytes[i - 1].is_ascii_alphabetic() && rest[1].is_ascii_digit() {
                    let digits = rest[1..].iter().take_while(|b| b.is_ascii_digit()).count();
                    if rest.get(1 + digits) != Some(&b'$') {
                        i += 1 + digits;
                        continue;
                    }
                }
                // In a spreadsheet chat, $AB$12 and $AB1:$CD2 must remain addresses.
                let col = rest[1..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
                if (1..=3).contains(&col) {
                    let row = 1 + col + usize::from(rest.get(1 + col) == Some(&b'$'));
                    let digits = rest[row..].iter().take_while(|b| b.is_ascii_digit()).count();
                    let tail = &rest[row + digits..];
                    // A plain $x2 also starts a valid equation such as "$x2 + 1$".
                    // Claim it as a cell only on an explicit row marker or a range/operator boundary.
                    let address = row == col + 2 || tail.starts_with(b":") || (tail.get(1) == Some(&b'$') && tail[0].is_ascii_punctuation());
                    if digits > 0 && address {
                        i += row + digits;
                        continue;
                    }
                }
                Some((Delimiter::Dollar, 1))
            } else {
                None
            };
            if let Some((kind, n)) = opener {
                pending = Some((kind, i, i + n));
                i += n;
            } else {
                i += if rest[0] == b'\\' && rest.get(1).is_some_and(|b| b.is_ascii_punctuation()) { 2 } else { 1 };
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::spans;

    #[test]
    fn math_does_not_consume_cell_references_currency_or_literal_code() {
        let text = r#"@$AB$12 costs $5 to $10; `$x$` and \$9. Read $x^2$, $`a_b`$, \(β\) and \[β^2\]."#;
        let math: Vec<_> = spans(text, 32, 0, &[]).into_iter().map(|s| (&text[s.body], s.display)).collect();
        assert_eq!(math, [("x^2", false), ("a_b", false), ("β", false), ("β^2", true)]);
    }

    #[test]
    fn mixed_addresses_and_quoted_sheet_names_are_literal() {
        let text = r#"Inspect $A1:$B2, B$1:$C2 and @'$α$ sales'!$C4; then use $x2 + 1$."#;
        let start = text.find('@').unwrap();
        let end = text.find(';').unwrap();
        let spans = spans(text, 32, 0, std::slice::from_ref(&(start..end)));
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].body.clone()], "x2 + 1");
    }

    #[test]
    fn a_partial_display_stays_literal_and_math_collection_is_bounded() {
        let text = "$$\r\nx + y\r\n$$\r\n$z$ and $$partial";
        let math = spans(text, 1, 0, &[]);
        assert_eq!(&text[math[0].outer.clone()], "$$\r\nx + y\r\n$$");
        assert_eq!(spans(text, 32, 0, &[]).len(), 2);
    }
}
