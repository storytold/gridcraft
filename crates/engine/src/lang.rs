//! The language new workbooks are made in. Excel names them after its interface language
//! (`Book1`/`Sheet1` in English, `Mappe1`/`Tabelle1` in German) and its sample text is in that
//! language too. The app sets [`Session::lang`](crate::Session::lang) from its interface language;
//! headless sessions (CLI, MCP) stay English, so scripts see the same names everywhere.

/// A language with workbook names and samples of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    English,
    German,
}

impl Lang {
    /// A language tag (`de`, `de-AT`, `en-US` …). Languages without names of their own get the
    /// English ones.
    pub fn from_tag(tag: &str) -> Lang {
        if tag.split(['-', '_']).next().is_some_and(|primary| primary.eq_ignore_ascii_case("de")) { Lang::German } else { Lang::English }
    }

    /// The untitled workbook's name before its number (`Book1`).
    pub fn book_base(self) -> &'static str {
        match self {
            Lang::English => "Book",
            Lang::German => "Mappe",
        }
    }

    /// A new sheet's name before its number (`Sheet1`).
    pub fn sheet_base(self) -> &'static str {
        match self {
            Lang::English => "Sheet",
            Lang::German => "Tabelle",
        }
    }

    /// The English or the German text.
    pub(crate) fn pick(self, en: &'static str, de: &'static str) -> &'static str {
        match self {
            Lang::English => en,
            Lang::German => de,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_resolve_to_a_language() {
        assert_eq!(Lang::from_tag("de"), Lang::German);
        assert_eq!(Lang::from_tag("de-AT"), Lang::German);
        assert_eq!(Lang::from_tag("de_CH.UTF-8"), Lang::German);
        assert_eq!(Lang::from_tag("en-US"), Lang::English);
        assert_eq!(Lang::from_tag("ja"), Lang::English);
        assert_eq!(Lang::from_tag("dex"), Lang::English);
        assert_eq!(Lang::from_tag(""), Lang::English);
    }
}
