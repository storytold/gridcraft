//! Fixed lookup tables: built-in number formats, the legacy indexed colour palette, paper sizes,
//! functions that need the `_xlfn.` prefix in files, and unit conversions.

/// Built-in number format codes (en-US) by id. The locale-specific ids read as the one Excel shows
/// them as ([`LOCALE_FORMATS`]); any other id not listed (23–26, 82 and up) reads as General.
const BUILTIN_FORMATS: [(u32, &str); 36] = [
    (0, "General"),
    (1, "0"),
    (2, "0.00"),
    (3, "#,##0"),
    (4, "#,##0.00"),
    (5, "\"$\"#,##0_);\\(\"$\"#,##0\\)"),
    (6, "\"$\"#,##0_);[Red]\\(\"$\"#,##0\\)"),
    (7, "\"$\"#,##0.00_);\\(\"$\"#,##0.00\\)"),
    (8, "\"$\"#,##0.00_);[Red]\\(\"$\"#,##0.00\\)"),
    (9, "0%"),
    (10, "0.00%"),
    (11, "0.00E+00"),
    (12, "# ?/?"),
    (13, "# ??/??"),
    (14, "m/d/yyyy"),
    (15, "d-mmm-yy"),
    (16, "d-mmm"),
    (17, "mmm-yy"),
    (18, "h:mm AM/PM"),
    (19, "h:mm:ss AM/PM"),
    (20, "h:mm"),
    (21, "h:mm:ss"),
    (22, "m/d/yyyy h:mm"),
    (37, "#,##0 ;(#,##0)"),
    (38, "#,##0 ;[Red](#,##0)"),
    (39, "#,##0.00;(#,##0.00)"),
    (40, "#,##0.00;[Red](#,##0.00)"),
    (41, "_(* #,##0_);_(* \\(#,##0\\);_(* \"-\"_);_(@_)"),
    (42, "_(\"$\"* #,##0_);_(\"$\"* \\(#,##0\\);_(\"$\"* \"-\"_);_(@_)"),
    (43, "_(* #,##0.00_);_(* \\(#,##0.00\\);_(* \"-\"??_);_(@_)"),
    (44, "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)"),
    (45, "mm:ss"),
    (46, "[h]:mm:ss"),
    (47, "mmss.0"),
    (48, "##0.0E+0"),
    (49, "@"),
];

/// Locale-specific built-in ids and the en-US built-in each one shows as. ECMA-376 §18.8.30 only
/// defines 27–36 and 50–81 for the East Asian and Thai locales, and files from other locales use
/// them too (an `es-MX` workbook stored its dates as 58). Observed black-box in Excel for Mac with
/// the es-MX region on a synthetic workbook, one cell per id: the dates show as the short date
/// (14), the times as `h:mm:ss` (21), and 59–81 as the matching number, percent, fraction, date
/// and time formats. Without this they read as General and dates showed as serial numbers.
const LOCALE_FORMATS: [(u32, u32); 42] = [
    (27, 14),
    (28, 14),
    (29, 14),
    (30, 14),
    (31, 14),
    (32, 21),
    (33, 21),
    (34, 21),
    (35, 21),
    (36, 14),
    (50, 14),
    (51, 14),
    (52, 14),
    (53, 14),
    (54, 14),
    (55, 14),
    (56, 14),
    (57, 14),
    (58, 14),
    (59, 1),
    (60, 2),
    (61, 3),
    (62, 4),
    (63, 5),
    (64, 6),
    (65, 7),
    (66, 8),
    (67, 9),
    (68, 10),
    (69, 12),
    (70, 13),
    (71, 14),
    (72, 14),
    (73, 15),
    (74, 16),
    (75, 17),
    (76, 20),
    (77, 21),
    (78, 22),
    (79, 45),
    (80, 46),
    (81, 47),
];

/// Format code for a built-in id, reading a locale-specific id as the en-US built-in Excel shows it
/// as. Saving writes that en-US id ([`builtin_id`]), which Excel shows the same way.
pub fn builtin_format(id: u32) -> Option<&'static str> {
    let id = LOCALE_FORMATS.iter().find(|(l, _)| *l == id).map_or(id, |(_, en)| *en);
    BUILTIN_FORMATS.iter().find(|(i, _)| *i == id).map(|(_, c)| *c)
}

/// Id to write for a format code when it is a locale-independent built-in (currency formats 5–8
/// are written as custom formats because their meaning depends on the reader's locale).
pub fn builtin_id(code: &str) -> Option<u32> {
    BUILTIN_FORMATS.iter().find(|(i, c)| *c == code && !(5..=8).contains(i)).map(|(i, _)| *i)
}

/// The 64-entry legacy palette (`indexed` colours), RRGGBB.
pub const INDEXED: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, // 0-7
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, // 8-15
    0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, // 16-23
    0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, // 24-31
    0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, // 32-39
    0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, // 40-47
    0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699, 0x969696, // 48-55
    0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333, // 56-63
];

/// Paper sizes (`pageSetup paperSize`) we name.
const PAPERS: [(u32, &str); 14] = [
    (1, "Letter"),
    (3, "Tabloid"),
    (4, "Ledger"),
    (5, "Legal"),
    (6, "Statement"),
    (7, "Executive"),
    (8, "A3"),
    (9, "A4"),
    (11, "A5"),
    (12, "B4"),
    (13, "B5"),
    (20, "Envelope #10"),
    (27, "Envelope DL"),
    (34, "Envelope B5"),
];

pub fn paper_name(id: u32) -> Option<&'static str> {
    PAPERS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

pub fn paper_id(name: &str) -> Option<u32> {
    PAPERS.iter().find(|(_, n)| n.eq_ignore_ascii_case(name.trim())).map(|(i, _)| *i)
}

/// Functions introduced after Excel 2007 that files store with the `_xlfn.` prefix. Entries with
/// `_xlws.` get `_xlfn._xlws.`.
const FUTURE_FUNCTIONS: &[&str] = &[
    "ACOT",
    "ACOTH",
    "AGGREGATE",
    "ARABIC",
    "ARRAYTOTEXT",
    "BASE",
    "BETA.DIST",
    "BETA.INV",
    "BINOM.DIST",
    "BINOM.DIST.RANGE",
    "BINOM.INV",
    "BITAND",
    "BITLSHIFT",
    "BITOR",
    "BITRSHIFT",
    "BITXOR",
    "BYCOL",
    "BYROW",
    "CEILING.MATH",
    "CEILING.PRECISE",
    "CHISQ.DIST",
    "CHISQ.DIST.RT",
    "CHISQ.INV",
    "CHISQ.INV.RT",
    "CHISQ.TEST",
    "CHOOSECOLS",
    "CHOOSEROWS",
    "COMBINA",
    "CONCAT",
    "CONFIDENCE.NORM",
    "CONFIDENCE.T",
    "COT",
    "COTH",
    "COVARIANCE.P",
    "COVARIANCE.S",
    "CSC",
    "CSCH",
    "DAYS",
    "DECIMAL",
    "DROP",
    "ENCODEURL",
    "ERF.PRECISE",
    "ERFC.PRECISE",
    "EXPAND",
    "EXPON.DIST",
    "F.DIST",
    "F.DIST.RT",
    "F.INV",
    "F.INV.RT",
    "F.TEST",
    "FIELDVALUE",
    "FILTERXML",
    "FLOOR.MATH",
    "FLOOR.PRECISE",
    "FORECAST.ETS",
    "FORECAST.ETS.CONFINT",
    "FORECAST.ETS.SEASONALITY",
    "FORECAST.ETS.STAT",
    "FORECAST.LINEAR",
    "FORMULATEXT",
    "GAMMA",
    "GAMMA.DIST",
    "GAMMA.INV",
    "GAMMALN.PRECISE",
    "GAUSS",
    "GROUPBY",
    "HSTACK",
    "HYPGEOM.DIST",
    "IFNA",
    "IFS",
    "IMAGE",
    "IMCOSH",
    "IMCOT",
    "IMCSC",
    "IMCSCH",
    "IMSEC",
    "IMSECH",
    "IMSINH",
    "IMTAN",
    "ISFORMULA",
    "ISOMITTED",
    "ISOWEEKNUM",
    "LAMBDA",
    "LET",
    "LOGNORM.DIST",
    "LOGNORM.INV",
    "MAKEARRAY",
    "MAP",
    "MAXIFS",
    "MINIFS",
    "MODE.MULT",
    "MODE.SNGL",
    "MUNIT",
    "NEGBINOM.DIST",
    "NETWORKDAYS.INTL",
    "NORM.DIST",
    "NORM.INV",
    "NORM.S.DIST",
    "NORM.S.INV",
    "NUMBERVALUE",
    "PDURATION",
    "PERCENTILE.EXC",
    "PERCENTILE.INC",
    "PERCENTOF",
    "PERCENTRANK.EXC",
    "PERCENTRANK.INC",
    "PERMUTATIONA",
    "PHI",
    "PIVOTBY",
    "POISSON.DIST",
    "QUARTILE.EXC",
    "QUARTILE.INC",
    "RANDARRAY",
    "RANK.AVG",
    "RANK.EQ",
    "REDUCE",
    "REGEXEXTRACT",
    "REGEXREPLACE",
    "REGEXTEST",
    "RRI",
    "SCAN",
    "SEC",
    "SECH",
    "SEQUENCE",
    "SHEET",
    "SHEETS",
    "SINGLE",
    "SKEW.P",
    "SORTBY",
    "STDEV.P",
    "STDEV.S",
    "SWITCH",
    "T.DIST",
    "T.DIST.2T",
    "T.DIST.RT",
    "T.INV",
    "T.INV.2T",
    "T.TEST",
    "TAKE",
    "TEXTAFTER",
    "TEXTBEFORE",
    "TEXTJOIN",
    "TEXTSPLIT",
    "TOCOL",
    "TOROW",
    "TRIMRANGE",
    "UNICHAR",
    "UNICODE",
    "UNIQUE",
    "VALUETOTEXT",
    "VAR.P",
    "VAR.S",
    "VSTACK",
    "WEBSERVICE",
    "WEIBULL.DIST",
    "WORKDAY.INTL",
    "WRAPCOLS",
    "WRAPROWS",
    "XLOOKUP",
    "XMATCH",
    "XOR",
    "Z.TEST",
    "_xlws.FILTER",
    "_xlws.SORT",
];

/// The name to write for a function (`XLOOKUP` → `_xlfn.XLOOKUP`, `FILTER` → `_xlfn._xlws.FILTER`).
pub fn file_function_name(name: &str) -> Option<String> {
    let u = name.to_ascii_uppercase();
    for f in FUTURE_FUNCTIONS {
        if let Some(ws) = f.strip_prefix("_xlws.") {
            if ws == u {
                return Some(format!("_xlfn._xlws.{u}"));
            }
        } else if *f == u {
            return Some(format!("_xlfn.{u}"));
        }
    }
    None
}

// ---------------------------------------------------------------- units

/// Column width as stored in a file (characters including padding) → model width (px).
pub fn col_width_to_px(w: f64) -> f32 {
    if !w.is_finite() || w <= 0.0 {
        return 0.0;
    }
    let w = w.min(255.0);
    (((256.0 * w + (128.0f64 / 7.0).trunc()) / 256.0) * 7.0).trunc() as f32
}

/// Model width (px) → stored column width.
pub fn px_to_col_width(px: f32) -> f64 {
    let px = (px.max(0.0) as f64).round().min(1790.0);
    let back = |w: f64| col_width_to_px(w) as f64;
    let chars = if px > 12.0 { ((px - 5.0) / 7.0 * 100.0 + 0.5).trunc() / 100.0 } else { px / 12.0 };
    let w = ((chars * 7.0 + 5.0) / 7.0 * 256.0).trunc() / 256.0;
    if back(w) == px {
        return w;
    }
    // Smallest stored width that reads back as `px`.
    let s = (px * 256.0 / 7.0 - 18.0).ceil().max(0.0);
    s / 256.0
}

/// Row height in typographic points → model px.
pub fn pt_to_px(pt: f64) -> f32 {
    if !pt.is_finite() || pt <= 0.0 { 0.0 } else { (pt.min(409.5) * 4.0 / 3.0) as f32 }
}

pub fn px_to_pt(px: f32) -> f64 {
    ((px.max(0.0) as f64) * 0.75 * 100.0).round() / 100.0
}

/// EMU per px at 96 dpi.
pub const EMU_PER_PX: f64 = 9525.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(col_width_to_px(9.140625), 64.0);
        assert_eq!(px_to_col_width(64.0), 9.140625);
        for px in 0..400 {
            assert_eq!(col_width_to_px(px_to_col_width(px as f32)), px as f32, "px {px}");
        }
        assert_eq!(pt_to_px(15.0), 20.0);
        assert_eq!(px_to_pt(20.0), 15.0);
    }

    #[test]
    fn functions() {
        assert_eq!(file_function_name("xlookup").as_deref(), Some("_xlfn.XLOOKUP"));
        assert_eq!(file_function_name("forecast.ets.confint").as_deref(), Some("_xlfn.FORECAST.ETS.CONFINT"));
        assert_eq!(file_function_name("FILTER").as_deref(), Some("_xlfn._xlws.FILTER"));
        assert_eq!(file_function_name("PERCENTOF").as_deref(), Some("_xlfn.PERCENTOF"));
        assert_eq!(file_function_name("TRIMRANGE").as_deref(), Some("_xlfn.TRIMRANGE"));
        assert_eq!(file_function_name("SUM"), None);
        assert_eq!(builtin_format(14), Some("m/d/yyyy"));
        assert_eq!(builtin_id("0.00%"), Some(10));
        assert_eq!(builtin_id(builtin_format(5).unwrap()), None);
        assert_eq!(paper_name(9), Some("A4"));
        assert_eq!(paper_id("a4"), Some(9));
    }

    #[test]
    fn locale_builtin_ids_read_as_excel_shows_them() {
        // Dates stored as the East Asian ids show as the short date, and save as it.
        for id in [27, 30, 31, 36, 50, 57, 58, 71, 72] {
            assert_eq!(builtin_format(id), Some("m/d/yyyy"), "id {id}");
        }
        assert_eq!(builtin_id(builtin_format(58).unwrap()), Some(14));
        for id in [32, 35, 77] {
            assert_eq!(builtin_format(id), Some("h:mm:ss"), "id {id}");
        }
        // The Thai ids repeat the en-US numbers, currencies, percents, fractions, dates and times.
        for (thai, en) in [(59, 1), (62, 4), (63, 5), (66, 8), (67, 9), (70, 13), (73, 15), (78, 22), (81, 47)] {
            assert_eq!(builtin_format(thai), builtin_format(en), "id {thai}");
            assert!(builtin_format(thai).is_some(), "id {thai}");
        }
        // Ids Excel shows as General, and ids past the table.
        for id in [23, 26, 82, 164, u32::MAX] {
            assert_eq!(builtin_format(id), None, "id {id}");
        }
        // The en-US built-ins are untouched.
        for (id, code) in BUILTIN_FORMATS {
            assert_eq!(builtin_format(id), Some(code), "id {id}");
        }
    }
}
