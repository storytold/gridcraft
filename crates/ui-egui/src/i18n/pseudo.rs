//! The pseudo-locale: the interface with every string accented, padded and bracketed.
//!
//! It is not offered in the switcher and it translates nothing. Its point is to make a screen that
//! reads plain English stand out: those words never went through a key, so no catalog — present or
//! future — can translate them. The brackets show where a label is cut short, and the padding
//! stands in for the longer words most languages need.

use std::cell::RefCell;
use std::collections::HashMap;

/// An accented stand-in per Latin letter, in `a`..`z` order.
const LETTERS: [char; 26] =
    ['á', 'ƀ', 'ç', 'ð', 'é', 'ƒ', 'ĝ', 'ĥ', 'í', 'ĵ', 'ķ', 'ļ', 'ɱ', 'ñ', 'ó', 'þ', 'ǫ', 'ŕ', 'š', 'ţ', 'ú', 'ṽ', 'ŵ', 'ẋ', 'ý', 'ž'];

/// The pseudo-locale's rendering of `english`.
///
/// `{slots}` are copied as they are: they are filled after this, and a renamed slot would show as
/// a missing value. The interface hands back borrowed text, so each label is made once and kept
/// for the life of the process — the set of labels is fixed, so that is a bounded amount.
pub(super) fn text(english: &str) -> &'static str {
    thread_local! {
        static MADE: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
    }
    MADE.with(|made| {
        if let Some(&text) = made.borrow().get(english) {
            return text;
        }
        let text: &'static str = Box::leak(accented(english).into_boxed_str());
        made.borrow_mut().insert(english.to_owned(), text);
        text
    })
}

/// `english` with its letters accented, every word padded the way a longer language extends it,
/// and the whole in angle brackets.
fn accented(english: &str) -> String {
    let mut out = String::with_capacity(english.len() * 3 / 2 + 2);
    out.push('⟦');
    let mut word = 0;
    let mut chars = english.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                out.push('{');
                for slot in chars.by_ref() {
                    out.push(slot);
                    if slot == '}' {
                        break;
                    }
                }
            }
            ch if ch.is_ascii_alphabetic() => {
                word += 1;
                let letter = LETTERS[ch.to_ascii_lowercase() as usize - 'a' as usize];
                if ch.is_ascii_uppercase() {
                    out.extend(letter.to_uppercase());
                } else {
                    out.push(letter);
                }
            }
            ch => {
                pad(&mut out, word);
                word = 0;
                out.push(ch);
            }
        }
    }
    pad(&mut out, word);
    out.push('⟧');
    out
}

/// The tail that stretches a `word` of `word` letters to a length a wordy language would reach.
fn pad(out: &mut String, letters: usize) {
    for _ in 0..letters.div_ceil(2) {
        out.push('·');
    }
}

#[cfg(test)]
mod tests {
    use super::{accented, text};

    #[test]
    fn every_letter_is_accented_and_the_label_is_bracketed() {
        let made = accented("Cancel");
        assert!(made.starts_with('⟦') && made.ends_with('⟧'), "{made}");
        assert!(!made.contains(|ch: char| ch.is_ascii_alphabetic()), "{made} still reads as English");
        assert!(made.chars().count() > "Cancel".len(), "{made} is not longer than its source");
    }

    #[test]
    fn slots_survive_for_the_caller_to_fill() {
        let made = accented("Saved to {path}");
        assert!(made.contains("{path}"), "{made}");
        assert!(accented("— {n} step").contains("{n}"));
    }

    #[test]
    fn a_label_is_made_once_and_kept() {
        let first = text("Cancel");
        assert_eq!(first, text("Cancel"));
        assert!(std::ptr::eq(first, text("Cancel")), "the same label is made again");
    }
}
