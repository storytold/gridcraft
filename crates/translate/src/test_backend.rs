//! A deterministic test-only backend: a fixed English↔Spanish word map. It never ships (the
//! `test-backend` feature is enabled only by dev-dependencies), and it lets the engine test
//! load → run → insert without committing a model.

use std::sync::OnceLock;

use crate::accelerator::Accelerator;
use crate::backend::{Backend, BackendError, BatchRequest, Cancel};
use crate::tag::LangTag;

pub const MODEL_ID: &str = "test-fixed-en-es";

/// English → Spanish, lower-case. Unknown words pass through unchanged, which keeps names,
/// markers and punctuation intact.
const EN_ES: &[(&str, &str)] = &[
    ("a", "un"),
    ("and", "y"),
    ("are", "son"),
    ("before", "antes"),
    ("cell", "celda"),
    ("close", "cerrar"),
    ("cost", "coste"),
    ("date", "fecha"),
    ("dollars", "dólares"),
    ("file", "archivo"),
    ("friday", "viernes"),
    ("good", "bueno"),
    ("hello", "hola"),
    ("in", "en"),
    ("is", "es"),
    ("meeting", "reunión"),
    ("morning", "mañana"),
    ("name", "nombre"),
    ("need", "necesitamos"),
    ("number", "número"),
    ("of", "de"),
    ("open", "abrir"),
    ("price", "precio"),
    ("report", "informe"),
    ("review", "revisar"),
    ("test", "prueba"),
    ("thank", "gracias"),
    ("that", "ese"),
    ("the", "el"),
    ("this", "este"),
    ("to", "a"),
    ("tomorrow", "mañana"),
    ("total", "total"),
    ("translation", "traducción"),
    ("user", "usuario"),
    ("we", "nosotros"),
    ("with", "con"),
    ("world", "mundo"),
    ("you", "tú"),
    ("for", "para"),
];

fn reverse() -> &'static Vec<(&'static str, &'static str)> {
    static REVERSE: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    REVERSE.get_or_init(|| EN_ES.iter().map(|(en, es)| (*es, *en)).collect())
}

/// A fixed-backend instance. `model_id` is checked against the requested pair only through
/// [`Backend::supports`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedBackend;

impl FixedBackend {
    pub fn new() -> FixedBackend {
        FixedBackend
    }
}

impl Backend for FixedBackend {
    fn model_id(&self) -> &str {
        MODEL_ID
    }

    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }

    fn supports(&self, from: &LangTag, to: &LangTag) -> bool {
        let pair = (from.language(), to.language());
        pair == ("en", "es") || pair == ("es", "en")
    }

    fn translate(&self, req: &BatchRequest<'_>, cancel: &Cancel) -> Result<Vec<String>, BackendError> {
        if !self.supports(req.from, req.to) {
            return Err(BackendError::UnsupportedPair { from: req.from.canonical(), to: req.to.canonical() });
        }
        let table = if req.from.language() == "en" { EN_ES } else { reverse().as_slice() };
        let mut out = Vec::with_capacity(req.texts.len());
        for text in req.texts {
            cancel.check()?;
            out.push(words(text, table));
        }
        Ok(out)
    }
}

fn words(text: &str, table: &[(&'static str, &'static str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut current = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        let lower = word.to_lowercase();
        match table.iter().find(|(from, _)| *from == lower) {
            Some((_, to)) => {
                if word.chars().next().is_some_and(char::is_uppercase) {
                    let mut chars = to.chars();
                    if let Some(first) = chars.next() {
                        out.extend(first.to_uppercase());
                    }
                    out.push_str(chars.as_str());
                } else {
                    out.push_str(to);
                }
            }
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '\'' || c == '-' {
            current.push(c);
        } else {
            flush(&mut current, &mut out);
            out.push(c);
        }
    }
    flush(&mut current, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> LangTag {
        LangTag::parse(code).expect("tag")
    }

    #[test]
    fn translates_words_both_ways_and_keeps_the_rest() {
        let b = FixedBackend;
        let cancel = Cancel::new();
        let texts = vec!["Hello, world! The total is 42 dollars.".to_string()];
        let out = b.translate(&BatchRequest { from: &lang("en"), to: &lang("es"), texts: &texts }, &cancel).expect("translate");
        assert_eq!(out[0], "Hola, mundo! El total es 42 dólares.");
        let back = b.translate(&BatchRequest { from: &lang("es"), to: &lang("en"), texts: &out }, &cancel).expect("back");
        assert_eq!(back[0], "Hello, world! The total is 42 dollars.");
    }

    #[test]
    fn refuses_other_pairs() {
        let b = FixedBackend;
        assert!(!b.supports(&lang("en"), &lang("de")));
        let texts = vec!["x".to_string()];
        assert!(matches!(
            b.translate(&BatchRequest { from: &lang("en"), to: &lang("de"), texts: &texts }, &Cancel::new()),
            Err(BackendError::UnsupportedPair { .. })
        ));
    }

    #[test]
    fn honours_cancellation() {
        let cancel = Cancel::new();
        cancel.cancel();
        let texts = vec!["Hello".to_string()];
        assert_eq!(
            FixedBackend.translate(&BatchRequest { from: &lang("en"), to: &lang("es"), texts: &texts }, &cancel),
            Err(BackendError::Cancelled)
        );
    }
}
