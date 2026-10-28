//! Which script a character belongs to, and the runs of one script in a string.
//!
//! Font fallback is per script: a Latin face rarely covers Han, Thai or Hebrew, so the shaping layer
//! has to know where one script ends and the next begins, and the startup code has to know which
//! scripts a face can draw at all. Both answers come from here, with no font access and no platform
//! dependency, so the same classification works on every target (including wasm) and in tests.
//!
//! Only [`ScriptClass::Common`] is special: digits, spaces and punctuation are drawn by whichever
//! face comes first, so they join the run beside them instead of starting one. Everything the base
//! proportional font covers (Greek, Cyrillic, Armenian, Georgian, ...) is [`ScriptClass::Latin`]:
//! it needs no fallback face of its own.

use std::ops::Range;

/// The scripts GridCraft routes fonts by. One class per fallback face family, plus the catch-all
/// [`ScriptClass::Latin`] and the neutral [`ScriptClass::Common`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptClass {
    /// Latin, Greek, Cyrillic and the other scripts the base proportional font covers.
    Latin,
    /// Han ideographs. Chinese and Japanese share them; the face order picks the shapes.
    Han,
    /// Kana. The only reliable marker of Japanese text.
    Japanese,
    /// Hangul. The only reliable marker of Korean text.
    Korean,
    Thai,
    Lao,
    Hebrew,
    Arabic,
    Devanagari,
    Khmer,
    Myanmar,
    Tibetan,
    Yi,
    /// Arrows, maths, dingbats and emoji: drawn by a symbol face, never by a text face.
    Symbols,
    /// Digits, spaces and punctuation: inherit the script of the run beside them.
    Common,
}

impl ScriptClass {
    /// Every class, [`ScriptClass::Common`] included.
    pub const ALL: [ScriptClass; 15] = [
        ScriptClass::Latin,
        ScriptClass::Han,
        ScriptClass::Japanese,
        ScriptClass::Korean,
        ScriptClass::Thai,
        ScriptClass::Lao,
        ScriptClass::Hebrew,
        ScriptClass::Arabic,
        ScriptClass::Devanagari,
        ScriptClass::Khmer,
        ScriptClass::Myanmar,
        ScriptClass::Tibetan,
        ScriptClass::Yi,
        ScriptClass::Symbols,
        ScriptClass::Common,
    ];

    /// The bit this class uses in a [`ScriptSet`]; `None` for `Common`, which is never tracked.
    pub const fn bit(self) -> Option<u32> {
        Some(match self {
            ScriptClass::Latin => 0,
            ScriptClass::Han => 1,
            ScriptClass::Japanese => 2,
            ScriptClass::Korean => 3,
            ScriptClass::Thai => 4,
            ScriptClass::Lao => 5,
            ScriptClass::Hebrew => 6,
            ScriptClass::Arabic => 7,
            ScriptClass::Devanagari => 8,
            ScriptClass::Khmer => 9,
            ScriptClass::Myanmar => 10,
            ScriptClass::Tibetan => 11,
            ScriptClass::Yi => 12,
            ScriptClass::Symbols => 13,
            ScriptClass::Common => return None,
        })
    }

    /// A short stable name, for tests and log messages.
    pub const fn name(self) -> &'static str {
        match self {
            ScriptClass::Latin => "latin",
            ScriptClass::Han => "han",
            ScriptClass::Japanese => "japanese",
            ScriptClass::Korean => "korean",
            ScriptClass::Thai => "thai",
            ScriptClass::Lao => "lao",
            ScriptClass::Hebrew => "hebrew",
            ScriptClass::Arabic => "arabic",
            ScriptClass::Devanagari => "devanagari",
            ScriptClass::Khmer => "khmer",
            ScriptClass::Myanmar => "myanmar",
            ScriptClass::Tibetan => "tibetan",
            ScriptClass::Yi => "yi",
            ScriptClass::Symbols => "symbols",
            ScriptClass::Common => "common",
        }
    }
}

/// The script of one character.
///
/// Han ideographs are [`ScriptClass::Han`] whatever the language: Unicode cannot tell Chinese from
/// Japanese from Korean without context, so the face order ([`crate::theme::HanOrder`]) decides the
/// shapes. Kana and Hangul are unambiguous and get their own classes.
pub fn classify(c: char) -> ScriptClass {
    match c {
        '\u{0000}'..='\u{0040}' | '\u{005B}'..='\u{0060}' | '\u{007B}'..='\u{00BF}' => ScriptClass::Common,
        'a'..='z' | 'A'..='Z' | '\u{00C0}'..='\u{02AF}' | '\u{0370}'..='\u{058F}' | '\u{1E00}'..='\u{1EFF}' => ScriptClass::Latin,
        '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}' => ScriptClass::Hebrew,
        '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}' => {
            ScriptClass::Arabic
        }
        '\u{0900}'..='\u{097F}' | '\u{A8E0}'..='\u{A8FF}' => ScriptClass::Devanagari,
        '\u{0E00}'..='\u{0E7F}' => ScriptClass::Thai,
        '\u{0E80}'..='\u{0EFF}' => ScriptClass::Lao,
        '\u{0F00}'..='\u{0FFF}' => ScriptClass::Tibetan,
        '\u{1000}'..='\u{109F}' | '\u{A9E0}'..='\u{A9FF}' | '\u{AA60}'..='\u{AA7F}' => ScriptClass::Myanmar,
        '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{A960}'..='\u{A97F}' | '\u{AC00}'..='\u{D7AF}' => ScriptClass::Korean,
        '\u{1780}'..='\u{17FF}' | '\u{19E0}'..='\u{19FF}' => ScriptClass::Khmer,
        '\u{2000}'..='\u{206F}' | '\u{3000}'..='\u{303F}' | '\u{FE00}'..='\u{FE0F}' => ScriptClass::Common,
        '\u{2190}'..='\u{2BFF}' | '\u{1F000}'..='\u{1FAFF}' => ScriptClass::Symbols,
        '\u{2E80}'..='\u{2FDF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{20000}'..='\u{3FFFF}' => {
            ScriptClass::Han
        }
        '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => ScriptClass::Japanese,
        '\u{A000}'..='\u{A4CF}' => ScriptClass::Yi,
        _ => ScriptClass::Common,
    }
}

/// The runs of one script in `text`, as byte ranges into `text`.
///
/// Ranges are contiguous, do not overlap, are cut on character boundaries and together cover the
/// whole string. [`ScriptClass::Common`] characters join the run before them (a leading run of them
/// joins the run after it, and text that is entirely Common is one Latin run), because punctuation
/// is drawn by the same face as its neighbours.
pub fn split_runs(text: &str) -> Vec<(Range<usize>, ScriptClass)> {
    // One run per stretch of the same class, Common runs included.
    let mut raw: Vec<(Range<usize>, ScriptClass)> = Vec::new();
    for (at, c) in text.char_indices() {
        let class = classify(c);
        match raw.last_mut() {
            Some((range, last)) if *last == class => range.end = at + c.len_utf8(),
            _ => raw.push((at..at + c.len_utf8(), class)),
        }
    }
    // Each Common run takes the class beside it: the run before when there is one, otherwise the
    // run after (a leading stretch), and Latin when the whole text is Common.
    let first_real = raw.iter().find(|(_, class)| *class != ScriptClass::Common).map(|(_, class)| *class);
    let mut beside = first_real.unwrap_or(ScriptClass::Latin);
    let mut resolved: Vec<(Range<usize>, ScriptClass)> = Vec::with_capacity(raw.len());
    for (range, class) in raw {
        let class = if class == ScriptClass::Common {
            beside
        } else {
            beside = class;
            class
        };
        match resolved.last_mut() {
            Some((last, last_class)) if *last_class == class => last.end = range.end,
            _ => resolved.push((range, class)),
        }
    }
    resolved
}

/// A set of [`ScriptClass`]es, one bit per class (`Common` has no bit).
///
/// Used twice: a face reports which scripts it can draw, and a workbook reports which scripts it
/// needs, so startup can load only the fallback faces that can be asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptSet(u16);

/// Two sample characters per script, used to read a face's coverage. They are ordinary characters
/// every font of that script has; a font that lacks them would be unusual enough to be worth
/// skipping.
pub const SAMPLES: &[(ScriptClass, [char; 2])] = &[
    (ScriptClass::Latin, ['A', 'é']),
    (ScriptClass::Han, ['中', '漢']),
    (ScriptClass::Japanese, ['あ', 'ア']),
    (ScriptClass::Korean, ['한', '글']),
    (ScriptClass::Thai, ['ก', 'า']),
    (ScriptClass::Lao, ['ກ', 'ຳ']),
    (ScriptClass::Hebrew, ['א', 'ב']),
    (ScriptClass::Arabic, ['ا', 'ب']),
    (ScriptClass::Devanagari, ['अ', 'क']),
    (ScriptClass::Khmer, ['ក', 'ខ']),
    (ScriptClass::Myanmar, ['က', 'ခ']),
    (ScriptClass::Tibetan, ['ཀ', 'ཁ']),
    (ScriptClass::Yi, ['ꀀ', 'ꀁ']),
    (ScriptClass::Symbols, ['★', '😀']),
];

impl ScriptSet {
    /// The empty set.
    pub const EMPTY: Self = Self(0);

    /// Whether `script` is in the set. `Common` is never in it.
    pub const fn has(self, script: ScriptClass) -> bool {
        match script.bit() {
            Some(bit) => self.0 & (1 << bit) != 0,
            None => false,
        }
    }

    /// The set with `script` added.
    pub const fn with(self, script: ScriptClass) -> Self {
        match script.bit() {
            Some(bit) => Self(self.0 | (1 << bit)),
            None => self,
        }
    }

    /// Adds `script` in place.
    pub fn insert(&mut self, script: ScriptClass) {
        *self = self.with(script);
    }

    /// All scripts at once, for a caller that knows nothing about the text and needs every face.
    pub const fn all() -> Self {
        Self((1 << ScriptClass::ALL.len()) - 1)
    }

    /// Whether the two sets share a script.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Both sets together.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The classes in the set, in [`ScriptClass::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = ScriptClass> {
        ScriptClass::ALL.into_iter().filter(move |script| self.has(*script))
    }

    /// Whether nothing is in the set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The scripts a set of characters covers, classifying each character.
    pub fn of_text(text: &str) -> Self {
        let mut set = Self::EMPTY;
        for c in text.chars() {
            set.insert(classify(c));
        }
        set
    }

    /// The scripts a face covers, asked per sample character. `covers` is the face's `cmap` test
    /// (or a fontconfig charset lookup): a script counts when either sample character is mapped.
    pub fn of_coverage(covers: impl Fn(char) -> bool) -> Self {
        let mut set = Self::EMPTY;
        for (script, samples) in SAMPLES {
            if samples.iter().any(|c| covers(*c)) {
                set.insert(*script);
            }
        }
        set
    }

    /// The scripts a fontconfig `%{charset}` string covers: hexadecimal code point ranges separated
    /// by spaces (`"20-7e a0-ff"`). A malformed or empty string yields the empty set.
    pub fn of_charset(charset: &str) -> Self {
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for token in charset.split_whitespace() {
            let (from, to) = match token.split_once('-') {
                Some((from, to)) => (from, to),
                None => (token, token),
            };
            let (Ok(from), Ok(to)) = (u32::from_str_radix(from, 16), u32::from_str_radix(to, 16)) else { continue };
            if from > to {
                continue;
            }
            ranges.push((from, to));
        }
        if ranges.is_empty() {
            return Self::EMPTY;
        }
        ranges.sort_unstable();
        let covers = |c: char| {
            let at = c as u32;
            ranges
                .binary_search_by(|(from, to)| {
                    if at < *from {
                        std::cmp::Ordering::Greater
                    } else if at > *to {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .is_ok()
        };
        Self::of_coverage(covers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_classified_by_block() {
        assert_eq!(classify('A'), ScriptClass::Latin);
        assert_eq!(classify('é'), ScriptClass::Latin);
        // Greek and Cyrillic need no fallback face: the base font draws them.
        assert_eq!(classify('Ω'), ScriptClass::Latin);
        assert_eq!(classify('Ж'), ScriptClass::Latin);
        assert_eq!(classify('中'), ScriptClass::Han);
        assert_eq!(classify('𠀀'), ScriptClass::Han);
        assert_eq!(classify('あ'), ScriptClass::Japanese);
        assert_eq!(classify('ア'), ScriptClass::Japanese);
        assert_eq!(classify('한'), ScriptClass::Korean);
        assert_eq!(classify('ก'), ScriptClass::Thai);
        assert_eq!(classify('ກ'), ScriptClass::Lao);
        assert_eq!(classify('א'), ScriptClass::Hebrew);
        assert_eq!(classify('ا'), ScriptClass::Arabic);
        assert_eq!(classify('अ'), ScriptClass::Devanagari);
        assert_eq!(classify('ក'), ScriptClass::Khmer);
        assert_eq!(classify('က'), ScriptClass::Myanmar);
        assert_eq!(classify('ཀ'), ScriptClass::Tibetan);
        assert_eq!(classify('ꀀ'), ScriptClass::Yi);
        assert_eq!(classify('★'), ScriptClass::Symbols);
        assert_eq!(classify('😀'), ScriptClass::Symbols);
        // Digits, ASCII and CJK punctuation, spaces and variation selectors are shared.
        assert_eq!(classify('7'), ScriptClass::Common);
        assert_eq!(classify(','), ScriptClass::Common);
        assert_eq!(classify('，'), ScriptClass::Common);
        assert_eq!(classify(' '), ScriptClass::Common);
        assert_eq!(classify('\u{200B}'), ScriptClass::Common);
    }

    #[test]
    fn runs_cover_the_whole_text_without_gaps() {
        for text in ["", "abc", "中文", "abc中文", "  ", "中", "한국어 텍스트", "😀a中", "a，b"] {
            let runs = split_runs(text);
            let mut at = 0;
            for (range, _) in &runs {
                assert_eq!(range.start, at, "{text:?} has a gap or an overlap");
                assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
                assert!(range.end > range.start);
                at = range.end;
            }
            assert_eq!(at, text.len(), "{text:?} is not fully covered");
        }
    }

    #[test]
    fn common_text_joins_the_run_beside_it() {
        // Punctuation inside one script does not split it.
        assert_eq!(split_runs("中文，abc"), [(0..9, ScriptClass::Han), (9..12, ScriptClass::Latin)]);
        // A leading stretch joins the run after it, not the run before it (there is none).
        assert_eq!(split_runs("«Привет"), [(0..14, ScriptClass::Latin)]);
        // All-Common text is one Latin run: the base font draws it.
        assert_eq!(split_runs("12.5%"), [(0..5, ScriptClass::Latin)]);
        // Two scripts meeting through punctuation keep their own runs.
        let runs = split_runs("中 a");
        assert_eq!(runs, [(0..4, ScriptClass::Han), (4..5, ScriptClass::Latin)]);
    }

    #[test]
    fn kana_and_hangul_are_their_own_scripts() {
        assert_eq!(split_runs("日本"), [(0..6, ScriptClass::Han)]);
        assert_eq!(split_runs("日本語です"), [(0..9, ScriptClass::Han), (9..15, ScriptClass::Japanese)]);
        assert_eq!(split_runs("한국"), [(0..6, ScriptClass::Korean)]);
    }

    #[test]
    fn a_charset_string_becomes_a_script_set() {
        // Latin only: the classic DejaVu range set.
        let latin = ScriptSet::of_charset("20-7e a0-17f");
        assert!(latin.has(ScriptClass::Latin));
        assert!(!latin.has(ScriptClass::Han));
        // A Han-only face, plus emoji for the symbol bit.
        let han = ScriptSet::of_charset("4e00-9fff 1f600-1f64f");
        assert!(han.has(ScriptClass::Han));
        assert!(han.has(ScriptClass::Symbols));
        assert!(!han.has(ScriptClass::Latin));
        // Damage is not a failure.
        assert!(ScriptSet::of_charset("").is_empty());
        assert!(ScriptSet::of_charset("zz-yy").is_empty());
    }

    #[test]
    fn a_coverage_closure_fills_the_set() {
        let set = ScriptSet::of_coverage(|c| c == 'A' || c == '中');
        assert!(set.has(ScriptClass::Latin));
        assert!(set.has(ScriptClass::Han));
        assert!(!set.has(ScriptClass::Japanese));
        assert!(set.iter().count() == 2);
    }

    #[test]
    fn a_texts_scripts_are_collected() {
        let set = ScriptSet::of_text("Total 总計 12");
        assert!(set.has(ScriptClass::Latin));
        assert!(set.has(ScriptClass::Han));
        assert!(!set.has(ScriptClass::Common), "Common is never in a set");
    }
}
