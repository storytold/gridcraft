/// The number region, including macOS numeric short-date customizations.
pub fn system_number_locale() -> gridcraft_engine::numfmt::Locale {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let locale = system_region_tag().map(|tag| gridcraft_engine::numfmt::Locale::from_tag(&tag)).unwrap_or_default();
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSDateFormatter, NSDateFormatterStyle, NSLocale};
            let formatter = NSDateFormatter::new();
            formatter.setLocale(Some(&NSLocale::currentLocale()));
            formatter.setDateStyle(NSDateFormatterStyle::ShortStyle);
            formatter.setTimeStyle(NSDateFormatterStyle::NoStyle);
            locale.with_icu_short_date(&formatter.dateFormat().to_string())
        }
        #[cfg(not(target_os = "macos"))]
        {
            locale
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        gridcraft_engine::numfmt::Locale::default()
    }
}

/// The locale tag whose region the system formats numbers and dates for. Not the interface
/// language, which can name another region (English (US) with the Region set to Switzerland):
/// macOS's current locale follows the Region setting, Linux has `LC_NUMERIC` (or `LC_ALL`, `LANG`),
/// and Windows's user locale is its format locale.
#[cfg(not(target_arch = "wasm32"))]
fn system_region_tag() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        Some(objc2_foundation::NSLocale::currentLocale().localeIdentifier().to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        region_tag_from_env(|k| std::env::var(k).ok())
    }
    #[cfg(target_os = "windows")]
    {
        // sys-locale reads GetUserPreferredUILanguages on Windows. Numbers and dates
        // instead follow the user's format locale, even with an English interface.
        current_locale::current_locale().ok()
    }
    #[cfg(all(not(unix), not(target_os = "windows")))]
    {
        sys_locale::get_locale()
    }
}

/// The POSIX locale numbers are formatted in: the first of `LC_ALL`, `LC_NUMERIC` and `LANG` that is
/// set, as the C library picks it; none for the C locale.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn region_tag_from_env(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let tag = ["LC_ALL", "LC_NUMERIC", "LANG"].into_iter().filter_map(var).find(|v| !v.is_empty())?;
    let name = tag.split(['.', '@']).next().unwrap_or("");
    (name != "C" && name != "POSIX").then_some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Numeric locale environment settings take precedence over interface-language settings.
    #[test]
    fn numeric_region_ignores_interface_language() {
        let pick = |vars: &[(&str, &str)]| region_tag_from_env(|k| vars.iter().find(|(name, _)| *name == k).map(|(_, value)| value.to_string()));
        assert_eq!(pick(&[("LC_MESSAGES", "fr_FR"), ("LANGUAGE", "de"), ("LC_NUMERIC", "ro_RO")]).as_deref(), Some("ro_RO"));
        assert_eq!(pick(&[("LC_ALL", "de_CH"), ("LC_NUMERIC", "ro_RO")]).as_deref(), Some("de_CH"));
        assert_eq!(pick(&[("LC_ALL", ""), ("LANG", "ro_RO")]).as_deref(), Some("ro_RO"));
        for tag in ["C", "C.UTF-8", "POSIX"] {
            assert_eq!(pick(&[("LC_ALL", tag), ("LANG", "ro_RO")]), None);
        }
    }
}
