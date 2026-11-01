//! Advance widths of the standard 14 PDF fonts, in 1/1000 em, for the printable ASCII range.
//!
//! Source: the glyph advance widths ("WX" values) of Adobe's Core 14 AFM font metrics files
//! (Helvetica, Helvetica-Bold, Times-Roman, Times-Bold, Courier). Adobe distributes those metrics
//! files with permission to use, copy and distribute them for any purpose ("This file and the 14
//! PostScript(R) AFM files it accompanies may be used, copied, and distributed for any purpose and
//! without charge, with or without modification, provided that all copyright notices are retained;
//! that the AFM files are not distributed without this file; that all modifications to this file
//! or any of the AFM files are prominently noted in the modified file(s); and that this paragraph
//! is not modified."). Only the numeric widths are reproduced here, typed in from the public
//! metrics; no AFM file is included. Oblique/italic faces reuse the upright widths (an
//! approximation that is exact for Helvetica and close for Times), and characters outside ASCII
//! are approximated by their base letter.

/// Helvetica and Helvetica-Oblique, chars 32..=126.
pub(crate) const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // ' '..'/'
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // '0'..'9'
    278, 278, 584, 584, 584, 556, 1015, // ':'..'@'
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // 'A'..'M'
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // 'N'..'Z'
    278, 278, 278, 469, 556, 333, // '['..'`'
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // 'a'..'m'
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // 'n'..'z'
    334, 260, 334, 584, // '{'..'~'
];

/// Helvetica-Bold and Helvetica-BoldOblique, chars 32..=126.
pub(crate) const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, // ' '..'/'
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // '0'..'9'
    333, 333, 584, 584, 584, 611, 975, // ':'..'@'
    722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, // 'A'..'M'
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // 'N'..'Z'
    333, 278, 333, 584, 556, 333, // '['..'`'
    556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, // 'a'..'m'
    611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, // 'n'..'z'
    389, 280, 389, 584, // '{'..'~'
];

/// Times-Roman and Times-Italic (approximation), chars 32..=126.
pub(crate) const TIMES: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, // ' '..'/'
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, // '0'..'9'
    278, 278, 564, 564, 564, 444, 921, // ':'..'@'
    722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, // 'A'..'M'
    722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, // 'N'..'Z'
    333, 278, 333, 469, 500, 333, // '['..'`'
    444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, // 'a'..'m'
    500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, // 'n'..'z'
    480, 200, 480, 541, // '{'..'~'
];

/// Times-Bold and Times-BoldItalic (approximation), chars 32..=126.
pub(crate) const TIMES_BOLD: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278, // ' '..'/'
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, // '0'..'9'
    333, 333, 570, 570, 570, 500, 930, // ':'..'@'
    722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, // 'A'..'M'
    722, 778, 611, 778, 722, 556, 667, 722, 722, 1000, 722, 722, 667, // 'N'..'Z'
    333, 278, 333, 581, 500, 333, // '['..'`'
    500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556, 278, 833, // 'a'..'m'
    556, 500, 556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, // 'n'..'z'
    394, 220, 394, 520, // '{'..'~'
];

/// Latin Extended-A (U+0100..=U+017F): the standard (Adobe Glyph List) glyph name of each
/// character and an ASCII character of similar width.
pub(crate) const LATIN_EXT_A: [(&str, char); 128] = [
    ("Amacron", 'A'),
    ("amacron", 'a'),
    ("Abreve", 'A'),
    ("abreve", 'a'),
    ("Aogonek", 'A'),
    ("aogonek", 'a'),
    ("Cacute", 'C'),
    ("cacute", 'c'),
    ("Ccircumflex", 'C'),
    ("ccircumflex", 'c'),
    ("Cdotaccent", 'C'),
    ("cdotaccent", 'c'),
    ("Ccaron", 'C'),
    ("ccaron", 'c'),
    ("Dcaron", 'D'),
    ("dcaron", 'd'),
    ("Dcroat", 'D'),
    ("dcroat", 'd'),
    ("Emacron", 'E'),
    ("emacron", 'e'),
    ("Ebreve", 'E'),
    ("ebreve", 'e'),
    ("Edotaccent", 'E'),
    ("edotaccent", 'e'),
    ("Eogonek", 'E'),
    ("eogonek", 'e'),
    ("Ecaron", 'E'),
    ("ecaron", 'e'),
    ("Gcircumflex", 'G'),
    ("gcircumflex", 'g'),
    ("Gbreve", 'G'),
    ("gbreve", 'g'),
    ("Gdotaccent", 'G'),
    ("gdotaccent", 'g'),
    ("Gcommaaccent", 'G'),
    ("gcommaaccent", 'g'),
    ("Hcircumflex", 'H'),
    ("hcircumflex", 'h'),
    ("Hbar", 'H'),
    ("hbar", 'h'),
    ("Itilde", 'I'),
    ("itilde", 'i'),
    ("Imacron", 'I'),
    ("imacron", 'i'),
    ("Ibreve", 'I'),
    ("ibreve", 'i'),
    ("Iogonek", 'I'),
    ("iogonek", 'i'),
    ("Idotaccent", 'I'),
    ("dotlessi", 'i'),
    ("IJ", 'N'),
    ("ij", 'x'),
    ("Jcircumflex", 'J'),
    ("jcircumflex", 'j'),
    ("Kcommaaccent", 'K'),
    ("kcommaaccent", 'k'),
    ("kgreenlandic", 'k'),
    ("Lacute", 'L'),
    ("lacute", 'l'),
    ("Lcommaaccent", 'L'),
    ("lcommaaccent", 'l'),
    ("Lcaron", 'L'),
    ("lcaron", 'l'),
    ("Ldot", 'L'),
    ("ldot", 'l'),
    ("Lslash", 'L'),
    ("lslash", 'l'),
    ("Nacute", 'N'),
    ("nacute", 'n'),
    ("Ncommaaccent", 'N'),
    ("ncommaaccent", 'n'),
    ("Ncaron", 'N'),
    ("ncaron", 'n'),
    ("napostrophe", 'n'),
    ("Eng", 'N'),
    ("eng", 'n'),
    ("Omacron", 'O'),
    ("omacron", 'o'),
    ("Obreve", 'O'),
    ("obreve", 'o'),
    ("Ohungarumlaut", 'O'),
    ("ohungarumlaut", 'o'),
    ("OE", 'W'),
    ("oe", 'm'),
    ("Racute", 'R'),
    ("racute", 'r'),
    ("Rcommaaccent", 'R'),
    ("rcommaaccent", 'r'),
    ("Rcaron", 'R'),
    ("rcaron", 'r'),
    ("Sacute", 'S'),
    ("sacute", 's'),
    ("Scircumflex", 'S'),
    ("scircumflex", 's'),
    ("Scedilla", 'S'),
    ("scedilla", 's'),
    ("Scaron", 'S'),
    ("scaron", 's'),
    ("Tcommaaccent", 'T'),
    ("tcommaaccent", 't'),
    ("Tcaron", 'T'),
    ("tcaron", 't'),
    ("Tbar", 'T'),
    ("tbar", 't'),
    ("Utilde", 'U'),
    ("utilde", 'u'),
    ("Umacron", 'U'),
    ("umacron", 'u'),
    ("Ubreve", 'U'),
    ("ubreve", 'u'),
    ("Uring", 'U'),
    ("uring", 'u'),
    ("Uhungarumlaut", 'U'),
    ("uhungarumlaut", 'u'),
    ("Uogonek", 'U'),
    ("uogonek", 'u'),
    ("Wcircumflex", 'W'),
    ("wcircumflex", 'w'),
    ("Ycircumflex", 'Y'),
    ("ycircumflex", 'y'),
    ("Ydieresis", 'Y'),
    ("Zacute", 'Z'),
    ("zacute", 'z'),
    ("Zdotaccent", 'Z'),
    ("zdotaccent", 'z'),
    ("Zcaron", 'Z'),
    ("zcaron", 'z'),
    ("longs", 'f'),
];

/// The [`LATIN_EXT_A`] entry of `c`, if it is in that block.
pub(crate) fn latin_ext_a(c: char) -> Option<(&'static str, char)> {
    LATIN_EXT_A.get((c as u32).checked_sub(0x100)? as usize).copied()
}

/// Romanian Ș ș Ț ț with a comma below (U+0218..=U+021B, Latin Extended-B): glyph name and an
/// ASCII character of similar width. The names are the `uniXXXX` form: a viewer that substitutes
/// a font finds the glyph either by that name (TeX Gyre Heros, for one, names them so and has no
/// `scommaaccent`) or by the code point the name spells. The AGL names are not used: it maps
/// `Tcommaaccent`/`tcommaaccent` to the cedilla letters U+0162/U+0163, which Romanian does not use.
pub(crate) const COMMA_BELOW: [(&str, char); 4] = [("uni0218", 'S'), ("uni0219", 's'), ("uni021A", 'T'), ("uni021B", 't')];

/// The [`COMMA_BELOW`] entry of `c`, if it is one of those letters.
pub(crate) fn comma_below(c: char) -> Option<(&'static str, char)> {
    COMMA_BELOW.get((c as u32).checked_sub(0x218)? as usize).copied()
}

/// Maps a non-ASCII character to an ASCII character of similar width, or `None` to use the
/// font's default width.
pub(crate) fn fold(c: char) -> Option<char> {
    if let Some((_, base)) = latin_ext_a(c).or_else(|| comma_below(c)) {
        return Some(base);
    }
    Some(match c {
        'À'..='Å' => 'A',
        'Æ' => 'W',
        'Ç' => 'C',
        'È'..='Ë' => 'E',
        'Ì'..='Ï' => 'I',
        'Ð' => 'D',
        'Ñ' => 'N',
        'Ò'..='Ö' | 'Ø' => 'O',
        '×' => '+',
        'Ù'..='Ü' => 'U',
        'Ý' | 'Ÿ' => 'Y',
        'Þ' => 'P',
        'ß' => 'b',
        'à'..='å' => 'a',
        'æ' => 'm',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ð' => 'o',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        '÷' => '+',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'þ' => 'p',
        'Š' => 'S',
        'š' => 's',
        'Ž' => 'Z',
        'ž' => 'z',
        'Œ' => 'W',
        'œ' => 'm',
        '‘'..='‚' => '\'',
        '“'..='„' => '"',
        '–' | '€' | '£' | '¥' | '¢' | '¤' => '0',
        '\u{a0}' => ' ',
        '·' | '•' => '.',
        '«' | '»' | '‹' | '›' => '<',
        '¡' => '!',
        '¿' => '?',
        '¦' => '|',
        '°' | '¹' | '²' | '³' | 'ª' | 'º' => '*',
        _ => return None,
    })
}
