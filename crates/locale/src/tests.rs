use super::*;

#[test]
fn every_local_name_maps_back_to_its_canonical_name() {
    for l in LANGUAGES {
        for (canonical, local) in l.functions {
            assert_eq!(l.local_function(canonical), Some(*local), "{} {canonical}", l.tag);
            assert_eq!(l.canonical_function(local), Some(*canonical), "{} {local}", l.tag);
            assert_eq!(l.canonical_function(&local.to_lowercase()), Some(*canonical), "{} lower-case {local}", l.tag);
        }
        for (canonical, local) in l.errors {
            assert_eq!(l.canonical_error(local), Some(*canonical), "{}", l.tag);
            assert_eq!(l.local_error(canonical), *local, "{}", l.tag);
        }
        assert_eq!(l.parse_bool(&l.bool_true.to_lowercase()), Some(true), "{}", l.tag);
        assert_eq!(l.parse_bool(l.bool_false), Some(false), "{}", l.tag);
    }
}

#[test]
fn unknown_names_and_canonical_lookups() {
    let en = &LANG_EN_US;
    assert_eq!(en.local_function("sum"), Some("SUM"));
    assert_eq!(en.canonical_function("Sum"), Some("SUM"));
    assert_eq!(en.canonical_function("NOSUCHFUNCTION"), None);
    assert_eq!(en.local_function("NOSUCHFUNCTION"), None);
    assert_eq!(en.local_error("#CIRC!"), "#CIRC!");
}

#[test]
fn error_prefix_takes_the_longest_literal() {
    let en = &LANG_EN_US;
    assert_eq!(en.error_prefix("#n/a+1"), Some(("#N/A", 4)));
    assert_eq!(en.error_prefix("#DIV/0!"), Some(("#DIV/0!", 7)));
    assert_eq!(en.error_prefix("#NOPE"), None);
    for l in LANGUAGES {
        for (canonical, local) in l.errors {
            let text = format!("{local}+1");
            assert_eq!(l.error_prefix(&text), Some((*canonical, local.len())), "{} {local}", l.tag);
        }
    }
}

#[test]
fn table_items_are_case_insensitive() {
    let en = &LANG_EN_US;
    assert_eq!(en.table_item(" #this row "), Some(TABLE_ITEM_THIS_ROW));
    assert_eq!(en.table_item("#ALL"), Some(TABLE_ITEM_ALL));
    assert_eq!(en.table_item("#Everything"), None);
}

#[test]
fn negotiation_handles_os_and_browser_tags() {
    assert_eq!(negotiate_language("pt_BR.UTF-8").tag, "pt-BR");
    assert_eq!(negotiate_language("C").tag, "en-US");
    assert_eq!(negotiate_language("").tag, "en-US");
    assert_eq!(negotiate_language("xx-YY").tag, "en-US");
    assert_eq!(negotiate_region("pt-PT").tag, "pt-PT");
    assert_eq!(negotiate_region("pt").tag, "pt-BR");
    assert_eq!(negotiate_region("de-AT").tag, "de-DE");
    assert_eq!(negotiate_region("zh-Hant-HK").tag, "zh-TW");
    assert_eq!(negotiate_region("zh-SG").tag, "zh-CN");
    assert_eq!(negotiate_language("ja").tag, "ja-JP");
}

#[test]
fn negotiation_follows_cldr_parents_and_scripts() {
    let all = ["de-DE", "en-US", "es-ES", "fr-FR", "it-IT", "ja-JP", "ko-KR", "pt-BR", "pt-PT", "zh-CN", "zh-TW"];
    let pick = |req: &str| negotiate_index(req, &all, |t| *t).and_then(|i| all.get(i)).copied();
    for (req, want) in [
        ("pt", "pt-BR"),
        ("pt-BR", "pt-BR"),
        ("pt_PT.UTF-8", "pt-PT"),
        ("pt-AO", "pt-PT"),
        ("pt-MZ", "pt-PT"),
        ("pt-CH", "pt-PT"),
        ("pt-US", "pt-BR"),
        ("zh", "zh-CN"),
        ("zh-SG", "zh-CN"),
        ("zh-HK", "zh-TW"),
        ("zh_MO", "zh-TW"),
        ("zh-Hant", "zh-TW"),
        ("zh-Hans-HK", "zh-CN"),
        ("zh-Hans-TW", "zh-CN"),
        ("de-AT", "de-DE"),
        ("es-419", "es-ES"),
        ("en-GB", "en-US"),
        (" ja ", "ja-JP"),
    ] {
        assert_eq!(pick(req), Some(want), "{req}");
    }
    for req in ["", "C", "POSIX", "xx-YY"] {
        assert_eq!(pick(req), None, "{req}");
    }
    // Without the preferred variant, the other one of the same language.
    let few = ["en-US", "pt-BR", "zh-CN"];
    assert_eq!(negotiate_index("pt-AO", &few, |t| *t), Some(1));
    assert_eq!(negotiate_index("zh-HK", &few, |t| *t), Some(2));
}

#[test]
fn custom_separators_never_collide() {
    for r in REGIONS {
        for (d, g) in [('.', ','), (',', '.'), (',', ' '), ('.', '.'), (',', ','), ('.', '\''), (';', '.')] {
            let x = r.with_separators(d, g);
            assert_eq!(x.decimal, d);
            assert_ne!(x.decimal, x.group, "{} {d}{g}", r.tag);
            assert_ne!(x.decimal, x.list, "{} {d}{g}", r.tag);
            assert_ne!(x.array_col, x.decimal, "{} {d}{g}", r.tag);
            assert_ne!(x.array_col, x.array_row, "{} {d}{g}", r.tag);
            assert_ne!(x.array_row, x.decimal, "{} {d}{g}", r.tag);
        }
    }
}

#[test]
fn applying_a_regions_own_separators_changes_nothing() {
    for r in REGIONS {
        assert_eq!(r.with_separators(r.decimal, r.group), **r, "{}", r.tag);
    }
}

#[test]
fn regions_by_lcid_and_content_fallback() {
    assert_eq!(region_by_lcid(0x0409).map(|r| r.tag), Some("en-US"));
    assert_eq!(LANG_EN_US.content("sheet"), "Sheet");
    assert_eq!(LANG_EN_US.content("no-such-key"), "no-such-key");
    assert!(INVARIANT.is_invariant());
    assert!(Dialect::INVARIANT.is_invariant());
}
