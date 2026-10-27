use super::*;
use crate::i18n::Language;

#[test]
/// Loaded script-fallback faces end each role chain just before the craft fonts, in the language's order.
fn fallback_faces_are_appended_to_every_family() {
    // Structural: whatever loaded, the non-Latin fallback keys must sit at the end of each chain
    // (after the Latin/default fonts) so Latin still wins for Latin text, in the language's order.
    for han in [HanOrder::Chinese, HanOrder::Japanese] {
        let language = if han == HanOrder::Japanese { Language::Ja } else { Language::Zh };
        let fonts = font_definitions_for_language(language);
        let fb_keys: Vec<String> = fallback_faces(han).into_iter().map(|(k, _)| k.to_string()).filter(|k| fonts.font_data.contains_key(k)).collect();
        if fb_keys.is_empty() {
            return; // no fallback face on this machine (minimal Linux)
        }
        for role in [UI, UI_BOLD, CELL, CELL_BOLD, CELL_ITALIC, CELL_BOLD_ITALIC, SERIF, MONO] {
            let chain = fonts.families.get(&FontFamily::Name(role.into())).map(Vec::as_slice).unwrap_or_default();
            // The optional craft-fonts faces follow the system fallbacks at the very end.
            let end = chain.iter().rposition(|k| !k.starts_with("craft-")).map_or(0, |i| i + 1);
            assert!(end >= fb_keys.len(), "{role} chain too short for its fallback");
            assert_eq!(&chain[end - fb_keys.len()..end], fb_keys.as_slice(), "{role} must end its non-craft chain with the fallback faces");
        }
    }
}

#[test]
/// Every font family ends with exactly the permitted craft faces in manifest order.
fn craft_fonts_are_the_last_fallback_in_every_family() {
    // Ordering: app/bundled faces, Hack/emoji, system script fallbacks, permitted craft faces.
    // A craft face must never displace a system face that already covers a glyph.
    let fonts = font_definitions_for_language(Language::En);
    let expected: Vec<String> = craft_font_faces().into_iter().map(|(key, _)| key).collect();
    if expected.is_empty() {
        return; // optional input absent
    }
    for (family, chain) in &fonts.families {
        let first = chain.iter().position(|k| k.starts_with("craft-")).unwrap_or(chain.len());
        assert_eq!(&chain[first..], expected.as_slice(), "{family:?} must end with exactly the permitted craft faces in manifest order");
    }
}

#[test]
/// Skips on wasm or when required mode is off; a required native build must embed LXGW with `Hani`/`Hans`.
fn a_required_native_build_embeds_the_chinese_face() {
    if cfg!(target_arch = "wasm32") || !craft_fonts_required() {
        return;
    }
    assert!(
        chinese_font_loaded(),
        "CRAFT_FONTS_REQUIRED=1 native builds must embed the LXGW Chinese face from fonts/simplified-chinese-manifest.txt"
    );
    let face = craft_font_scripts().find(|(f, s, _)| *f == "LXGW WenKai Lite" && *s == "Regular");
    assert_eq!(face.map(|(_, _, scripts)| scripts), Some(["Hani", "Hans"].as_slice()), "the Chinese manifest's script tags are asserted");
}

#[test]
/// No embedded craft face belongs to the barred Noto CJK, Adobe or Source families.
fn barred_faces_never_enter_the_font_set() {
    for (family, _, _) in craft_font_scripts() {
        assert!(!family.contains("CJK"), "the barred Noto CJK collection must never be embedded: {family}");
        assert!(!family.starts_with("Source ") && !family.starts_with("Adobe "), "a barred Adobe/Source face was embedded: {family}");
    }
}

#[test]
/// Skips when craft fonts are absent unless required mode is set; otherwise the required faces and their exact script tags are embedded.
fn the_required_craft_faces_keep_their_exact_scripts() {
    if !craft_fonts_loaded() {
        assert!(!craft_fonts_required(), "CRAFT_FONTS_REQUIRED=1 but no craft-fonts faces were embedded");
        return;
    }
    let faces: Vec<_> = craft_font_scripts().collect();
    for (family, style, scripts) in [
        ("BIZ UDPGothic", "Regular", ["Jpan", "Latn"].as_slice()),
        ("BIZ UDPGothic", "Bold", ["Jpan", "Latn"].as_slice()),
        ("NanumGothic", "Regular", ["Hang", "Kore"].as_slice()),
        ("Noto Sans Symbols", "Regular", ["Zsym"].as_slice()),
        ("Noto Sans Symbols 2", "Regular", ["Zsym"].as_slice()),
    ] {
        assert!(
            faces.iter().any(|(f, s, declared)| *f == family && *s == style && *declared == scripts),
            "{family} {style} with {scripts:?} must be embedded; got {faces:?}"
        );
    }
}

#[test]
/// Interface text maps unbundled `⌃`, `✓` and `✕` to readable chrome fallbacks; document text is not rewritten.
fn available_text_maps_unbundled_chrome_symbols() {
    // No bundled face has ⌃ ✓ ✕; chrome text must show ^ ✔ × instead of tofu. Document text is
    // never rewritten: this helper is only used for interface tooltips.
    let ctx = egui::Context::default();
    ctx.set_fonts(FontDefinitions::default());
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        assert_eq!(available_text(ui, "Show Formulas (⌃`) ✓ ✕"), "Show Formulas (^`) ✔ ×");
    });
    out.textures_delta.clear();
}

#[test]
fn interface_families_fall_back_to_the_cell_font() {
    // Structural (#349): the formula bar, name box and dialogs draw every script the cells can,
    // so each interface chain carries the cell face, ahead of the script fallback faces.
    let fonts = font_definitions_for_language(Language::En);
    let cell = format!("sys-{CELL}");
    if !fonts.font_data.contains_key(&cell) {
        return; // no system cell font on this machine
    }
    let fb: Vec<String> = fallback_faces(HanOrder::Chinese).into_iter().map(|(k, _)| k.to_string()).collect();
    for family in [FontFamily::Proportional, FontFamily::Name(UI.into()), FontFamily::Name(UI_BOLD.into()), FontFamily::Name(MONO.into())] {
        let chain = fonts.families.get(&family).cloned().unwrap_or_default();
        let at = chain.iter().position(|k| *k == cell);
        assert!(at.is_some(), "{family:?} lacks the cell face: {chain:?}");
        let first_fb = chain.iter().position(|k| fb.contains(k)).unwrap_or(chain.len());
        assert!(at.unwrap_or(usize::MAX) < first_fb, "{family:?}: cell face must precede the script faces");
    }
}

#[test]
fn language_reorders_only_the_han_faces() {
    let keys = |han| fallback_faces(han).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
    assert_eq!(
        keys(HanOrder::Japanese),
        ["sys-cjk-han-ja", "sys-cjk-han-zh", "sys-cjk-hangul", "sys-thai", "sys-lao", "sys-hebrew", "sys-symbols", "sys-symbols-2"]
    );
    assert_eq!(
        keys(HanOrder::Chinese),
        ["sys-cjk-han-zh", "sys-cjk-han-ja", "sys-cjk-hangul", "sys-thai", "sys-lao", "sys-hebrew", "sys-symbols", "sys-symbols-2"]
    );
    assert_eq!(HanOrder::of(Language::Ja), HanOrder::Japanese);
    for l in [Language::En, Language::Zh, Language::Ko, Language::Ru, Language::PtBr, Language::De, Language::Cs, Language::Fr, Language::Tr] {
        assert_eq!(HanOrder::of(l), HanOrder::Chinese, "{l:?} shares the default order (no font rebuild)");
    }
}

#[test]
fn cjk_fallback_probes_flatpak_fonts_with_the_correct_locale_faces() {
    // Exercise the candidates the fallback loader actually consumes, not just the directory
    // constant. This must work on CI without an installed CJK font or a Flatpak runtime.
    for (candidates, face) in [(cjk_han_japanese(), 0), (cjk_han_simplified(), 2), (cjk_hangul(), 1)] {
        for directory in [
            "/run/host/fonts/opentype/noto",
            "/run/host/fonts/google-noto-cjk",
            "/run/host/fonts/noto-cjk",
            "/run/host/fonts/truetype/noto",
            "/run/host/fonts/noto",
            "/run/host/fonts/truetype",
            "/run/host/fonts/TTF",
            "/run/host/fonts",
            "/run/host/local-fonts",
            "/run/host/local-fonts/noto-cjk",
            "/run/host/user-fonts",
        ] {
            let path = format!("{directory}/NotoSansCJK-Regular.ttc");
            assert!(candidates.contains(&(path.clone(), face)), "missing Flatpak CJK candidate {path}, face {face}");
        }
    }
    let chinese = cjk_han_simplified();
    assert!(chinese.contains(&("/run/host/fonts/truetype/wqy/wqy-microhei.ttc".into(), 0)));
    assert!(chinese.contains(&("/run/host/fonts/wenquanyi/wqy-microhei.ttc".into(), 0)));
}

#[test]
/// A font file shared by several fallback scripts is loaded once, with one shared font blob rather than a copy per script.
fn fallback_faces_load_each_file_once() {
    // The region-merged Noto Sans CJK .ttc serves Japanese, Chinese and Hangul; it must be loaded
    // once, not once per script (tens of MB each).
    for language in [Language::En, Language::Ja] {
        let fonts = font_definitions_for_language(language);
        let present: Vec<&Arc<FontData>> = fallback_faces(HanOrder::of(language)).into_iter().filter_map(|(k, _)| fonts.font_data.get(k)).collect();
        for (i, a) in present.iter().enumerate() {
            for b in present.iter().skip(i + 1) {
                assert!(!std::sync::Arc::ptr_eq(&a.font, &b.font), "the same font file is loaded for two fallback scripts");
            }
        }
    }
}

#[test]
/// Checks that the proportional and cell families have glyphs for every present script sample; samples with no system or craft face are printed as skipped.
fn fallback_covers_its_script() {
    // Real glyph check, per script: the proportional family (what cells and the UI use) must
    // have a glyph for every non-space character in the sample. A script runs when a system
    // face or the optional craft-fonts input provides one; the printed skip is a recorded
    // gate, and a present face that is not wired in fails.
    let cases: [(&str, Vec<(String, u32)>, &str); 10] = [
        ("sys-cjk-han-ja", cjk_han_japanese(), "カテゴリ"),
        // Chinese uses a system Han face or the native-only permitted LXGW craft face; the
        // barred Noto CJK collection never supplies it.
        ("sys-cjk-han-zh", cjk_han_simplified(), "页面布局 客户端配置"),
        ("sys-cjk-hangul", cjk_hangul(), "한글"),
        ("sys-thai", thai(), "ทดสอบ"),
        ("sys-lao", lao(), "ພາສາລາວ"),
        ("sys-hebrew", hebrew(), "עברית"),
        ("craft-noto-sans-arabic", Vec::new(), "مرحبا"),
        ("craft-noto-sans-georgian", Vec::new(), "ქართული"),
        ("ui", candidates(UI), "Русский"),
        // Czech interface labels: every Czech accented letter, in both cases.
        ("ui", candidates(UI), "Příliš žluťoučký kůň úpěl ďábelské ódy ŘŠČŽÝÁÍÉÚŮĚŤĎŇÓ"),
    ];
    for language in [Language::En, Language::Ja] {
        let ctx = egui::Context::default();
        ctx.set_fonts(font_definitions_for_language(language));
        let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        out.textures_delta.clear(); // nothing consumes the atlas in a headless test
        for family in [FontFamily::Proportional, FontFamily::Name(CELL.into())] {
            let font = FontId::new(14.0, family.clone());
            for (key, cands, sample) in &cases {
                // A script runs when a system face or the optional craft-fonts input provides
                // one. The skip is printed: a missing fixture is a recorded gate, not a pass.
                let system = cands.iter().any(|(p, _)| std::path::Path::new(p).exists());
                if !system && !craft_fonts_cover(sample) {
                    eprintln!("skipped {key}: no system face and no craft-fonts face for {sample:?}");
                    continue;
                }
                let missing: String = sample.chars().filter(|c| !c.is_whitespace()).filter(|&c| !ctx.fonts_mut(|f| f.has_glyph(&font, c))).collect();
                assert!(missing.is_empty(), "{key} is present but {sample:?} has no glyph for {missing:?} ({language:?}, {family:?})");
            }
        }
    }
}

#[test]
fn chromeos_font_layout_is_probed() {
    // #1762: Crostini mounts the host's fonts under /usr/share/fonts/chromeos, one folder per
    // family set; a script whose list misses it renders as tofu with the font installed.
    let probes = |cands: Vec<(String, u32)>, path: &str| cands.iter().any(|(p, _)| p == path);
    let cros = "/usr/share/fonts/chromeos";
    assert!(probes(thai(), &format!("{cros}/noto/NotoSansThai-Regular.ttf")));
    assert!(probes(hebrew(), &format!("{cros}/noto/NotoSansHebrew-Regular.ttf")));
    assert!(probes(cjk_han_japanese(), &format!("{cros}/notocjk/NotoSansCJK-Regular.ttc")));
    assert!(probes(cjk_han_simplified(), &format!("{cros}/notocjk/NotoSansCJK-Regular.ttc")));
    assert!(probes(cjk_hangul(), &format!("{cros}/notocjk/NotoSansCJK-Regular.ttc")));
    assert!(probes(symbols_more(), &format!("{cros}/noto/NotoSansSymbols2-Regular.ttf")));
    assert!(probes(candidates(UI), &format!("{cros}/noto/NotoSans-Regular.ttf")));
    assert!(probes(candidates(CELL), &format!("{cros}/crosextra/Carlito-Regular.ttf")));
}

#[test]
/// Empty, garbage, truncated and bad-index fonts are refused; a bad face is dropped from the definitions and `set_fonts` does not panic.
fn unparseable_font_files_are_rejected_not_fatal() {
    // #226: epaint panics in `set_fonts` on a face it can't parse ("An offset was out of
    // bounds"). Garbage, a truncated real font and a bad collection index must all be refused.
    let real = FontDefinitions::default().font_data.values().next().map(|fd| fd.bytes().to_vec()).unwrap_or_default();
    assert!(font_parses(&real, 0), "egui's own font must parse");
    assert!(!font_parses(b"", 0));
    assert!(!font_parses(&[0xAB; 64], 0));
    assert!(!font_parses(real.get(..20).unwrap_or_default(), 0), "a truncated font must be refused");
    assert!(!font_parses(&real, 3), "a single-face file has no face 3");

    // A bad face that slipped into the definitions is dropped (key and family entries), and the
    // context builds its fonts without panicking.
    let mut fonts = font_definitions();
    fonts.font_data.insert("sys-hebrew".into(), Arc::new(FontData::from_owned(vec![0, 1, 0, 0, 0, 0xFF, 0, 0])));
    for chain in fonts.families.values_mut() {
        chain.push("sys-hebrew".into());
    }
    drop_unparseable_fonts(&mut fonts);
    assert!(!fonts.font_data.contains_key("sys-hebrew"));
    assert!(fonts.families.values().all(|c| !c.iter().any(|k| k == "sys-hebrew")));
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.label("Hebrew עברית");
    });
    out.textures_delta.clear(); // nothing consumes the atlas in a headless test
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
/// A broken existing font candidate is skipped in favor of a later readable candidate.
fn try_load_skips_a_broken_candidate() {
    // A candidate that exists but is not a readable font must not win over a good one behind it.
    let dir = std::env::temp_dir().join(format!("gridcraft-font-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let bad = dir.join("broken.ttf");
    let good = dir.join("good.ttf");
    let real = FontDefinitions::default().font_data.values().next().map(|fd| fd.bytes().to_vec()).unwrap_or_default();
    let _ = std::fs::write(&bad, real.get(..40).unwrap_or_default());
    let _ = std::fs::write(&good, &real);
    let (bad_s, good_s) = (bad.to_string_lossy().into_owned(), good.to_string_lossy().into_owned());
    assert!(try_load(&[(bad_s.as_str(), 0)]).is_none());
    let loaded = try_load(&[(bad_s.as_str(), 0), (good_s.as_str(), 0)]);
    assert_eq!(loaded.map(|fd| fd.bytes().len()), Some(real.len()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
/// Repeated loads reuse one shared font blob, while a face the kept file does not contain is still refused.
fn interface_fonts_are_read_once_and_shared_with_egui() {
    // egui copies owned font bytes into its text engine and keeps the original too; borrowed
    // bytes it shares. A rebuild of the font set reuses the file read the first time.
    let path = std::env::temp_dir().join(format!("gridcraft-font-shared-{}.ttf", std::process::id()));
    std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).unwrap();
    let p = path.to_string_lossy().into_owned();
    let first = try_load(&[(p.as_str(), 0)]).unwrap();
    std::fs::remove_file(&path).unwrap();
    let second = try_load(&[(p.as_str(), 0)]).expect("kept from the first read");
    assert!(
        std::sync::Arc::ptr_eq(&first.font, &second.font),
        "interface font bytes must be one shared blob, so egui shares them instead of copying"
    );
    assert_eq!(first.bytes(), epaint_default_fonts::UBUNTU_LIGHT);
    assert!(try_load(&[(p.as_str(), 3)]).is_none(), "a face the kept file doesn't have is still refused");
}

#[test]
fn macos_bold_interface_face_is_drawn_unhinted() {
    // #1184: Helvetica Neue Bold's own TrueType hints close Cyrillic е into ө at 13 pt on a
    // Retina screen. Core Text never runs them, so on macOS the face goes unhinted like every
    // other app draws it; the regular face and the other platforms keep egui's default.
    let fonts = font_definitions_for_language(Language::En);
    let hinting = |key: &str| fonts.font_data.get(key).map(|fd| fd.tweak.hinting);
    if let Some(bold) = hinting(&format!("sys-{UI_BOLD}")) {
        assert_eq!(bold, if cfg!(target_os = "macos") { Some(false) } else { None });
    }
    if let Some(regular) = hinting(&format!("sys-{UI}")) {
        assert_eq!(regular, None, "only the bold face is affected");
    }
}

#[test]
fn fedora_package_folders_are_searched_after_the_other_linux_layouts() {
    // #1694: Fedora keeps each font family in its own package folder, so without these entries
    // no cell face is found there and bold and italic cells draw as regular text. Each comes
    // after the existing Linux folders for the same font, so wherever one of those matched,
    // nothing changes.
    let paths = |cands: Vec<(String, u32)>| cands.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
    let cases: [(&str, Vec<String>, &str, &str); 9] = [
        (CELL, paths(candidates(CELL)), "google-carlito-fonts/Carlito-Regular.ttf", "/carlito/Carlito-Regular.ttf"),
        (CELL_BOLD, paths(candidates(CELL_BOLD)), "google-carlito-fonts/Carlito-Bold.ttf", "/carlito/Carlito-Bold.ttf"),
        (CELL_ITALIC, paths(candidates(CELL_ITALIC)), "google-carlito-fonts/Carlito-Italic.ttf", "/carlito/Carlito-Italic.ttf"),
        (CELL_BOLD_ITALIC, paths(candidates(CELL_BOLD_ITALIC)), "google-carlito-fonts/Carlito-BoldItalic.ttf", "/carlito/Carlito-BoldItalic.ttf"),
        (CELL_BOLD, paths(candidates(CELL_BOLD)), "liberation-sans-fonts/LiberationSans-Bold.ttf", "/liberation/LiberationSans-Bold.ttf"),
        (SERIF, paths(candidates(SERIF)), "liberation-serif-fonts/LiberationSerif-Regular.ttf", "/liberation/LiberationSerif-Regular.ttf"),
        (MONO, paths(candidates(MONO)), "liberation-mono-fonts/LiberationMono-Regular.ttf", "/dejavu/DejaVuSansMono.ttf"),
        ("thai", paths(thai()), "google-noto-vf/NotoSansThai[wght].ttf", "/NotoSansThai-Regular.ttf"),
        ("hebrew", paths(hebrew()), "google-noto-vf/NotoSansHebrew[wght].ttf", "/NotoSansHebrew-Regular.ttf"),
    ];
    for (role, list, fedora_file, earlier) in &cases {
        let at = list.iter().position(|p| *p == format!("/usr/share/fonts/{fedora_file}"));
        let last_earlier = list.iter().rposition(|p| p.ends_with(earlier));
        assert!(at.is_some(), "{role}: Fedora's {fedora_file} is not a candidate");
        assert!(last_earlier.is_some(), "{role}: no existing {earlier} candidate to compare with");
        assert!(at > last_earlier, "{role}: {fedora_file} must come after every existing {earlier}");
    }
    // Liberation Mono only stands in for DejaVu Sans Mono, wherever that is installed.
    let mono = paths(candidates(MONO));
    let first_liberation = mono.iter().position(|p| p.ends_with("LiberationMono-Regular.ttf"));
    let last_dejavu = mono.iter().rposition(|p| p.ends_with("DejaVuSansMono.ttf"));
    assert!(first_liberation.is_some() && first_liberation > last_dejavu, "Liberation Mono must follow DejaVu Sans Mono: {mono:?}");
    let noto = mono.iter().position(|p| p == "/usr/share/fonts/google-noto-vf/NotoSansMono[wght].ttf");
    assert!(noto.is_some() && noto.unwrap_or(0) + 1 == mono.len(), "Fedora's default Noto Sans Mono must be the last resort: {mono:?}");
}
