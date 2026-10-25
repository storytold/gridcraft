//! Pure host-mode rules, shared by the bridge ([`crate::host`]) and testable without a browser
//! (like [`crate::prefs`]): what opts a page into hosting GridCraft in its own document, and the
//! name the bridge fills in when a host's open request has none.

/// The one `data-host` value that opts a page in: the exact string `"page"`.
pub const PAGE_HOST: &str = "page";

/// The file name used when an open request carries no usable one.
pub const DEFAULT_OPEN_NAME: &str = "Book.xlsx";

/// Does this `data-host` value ask for page host mode? Only the exact value [`PAGE_HOST`] counts;
/// anything else is ignored, so the page host mode stays opt-in.
pub fn page_host_requested(data_host: Option<&str>) -> bool {
    data_host == Some(PAGE_HOST)
}

/// A usable file name for an open request: the host's name when it isn't blank, else the
/// [`DEFAULT_OPEN_NAME`].
pub fn open_name(name: Option<&str>) -> String {
    name.filter(|n| !n.trim().is_empty()).unwrap_or(DEFAULT_OPEN_NAME).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_page_value_opts_in() {
        assert!(page_host_requested(Some("page")));
        assert!(!page_host_requested(Some("Page")));
        assert!(!page_host_requested(Some(" page")));
        assert!(!page_host_requested(Some("parent")));
        assert!(!page_host_requested(Some("")));
        assert!(!page_host_requested(None));
    }

    #[test]
    fn open_names_fall_back_to_the_default_when_blank() {
        assert_eq!(open_name(Some("Budget.xlsx")), "Budget.xlsx");
        assert_eq!(open_name(Some(" Sheet2!Book.xlsx ")), " Sheet2!Book.xlsx ");
        assert_eq!(open_name(Some("")), DEFAULT_OPEN_NAME);
        assert_eq!(open_name(Some("   ")), DEFAULT_OPEN_NAME);
        assert_eq!(open_name(None), DEFAULT_OPEN_NAME);
    }
}
