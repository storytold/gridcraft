use std::collections::{BTreeMap, BTreeSet};

use fluent_syntax::ast::{CallArguments, Entry, Expression, InlineExpression, Pattern, PatternElement};

use super::*;

/// Variables used by a pattern, including nested placeables and select variants.
fn pattern_variables(p: &Pattern<&str>, out: &mut BTreeSet<String>) {
    for e in &p.elements {
        if let PatternElement::Placeable { expression } = e {
            expression_variables(expression, out);
        }
    }
}

fn expression_variables(e: &Expression<&str>, out: &mut BTreeSet<String>) {
    match e {
        Expression::Inline(i) => inline_variables(i, out),
        Expression::Select { selector, variants } => {
            inline_variables(selector, out);
            for v in variants {
                pattern_variables(&v.value, out);
            }
        }
    }
}

fn call_variables(a: &CallArguments<&str>, out: &mut BTreeSet<String>) {
    for p in &a.positional {
        inline_variables(p, out);
    }
    for n in &a.named {
        inline_variables(&n.value, out);
    }
}

fn inline_variables(i: &InlineExpression<&str>, out: &mut BTreeSet<String>) {
    match i {
        InlineExpression::VariableReference { id } => {
            out.insert(id.name.to_string());
        }
        InlineExpression::FunctionReference { arguments, .. } => call_variables(arguments, out),
        InlineExpression::TermReference { arguments: Some(a), .. } => call_variables(a, out),
        InlineExpression::Placeable { expression } => expression_variables(expression, out),
        _ => {}
    }
}

/// message key -> (slot -> variables), where the slot is "" for the value or the attribute name.
type Shape = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

fn shape(source: &LangSource) -> BTreeMap<&'static str, Shape> {
    let mut files = BTreeMap::new();
    for (name, text) in source.files {
        let resource = match fluent_syntax::parser::parse(*text) {
            Ok(r) => r,
            Err((_, errors)) => panic!("{}/{name}.ftl does not parse: {errors:?}", source.tag),
        };
        let mut shape = Shape::new();
        for entry in &resource.body {
            if let Entry::Message(m) = entry {
                let mut slots = BTreeMap::new();
                let mut vars = BTreeSet::new();
                if let Some(v) = &m.value {
                    pattern_variables(v, &mut vars);
                    slots.insert(String::new(), vars);
                }
                for a in &m.attributes {
                    let mut vars = BTreeSet::new();
                    pattern_variables(&a.value, &mut vars);
                    slots.insert(a.id.name.to_string(), vars);
                }
                assert!(shape.insert(m.id.name.to_string(), slots).is_none(), "{}/{name}.ftl: duplicate {}", source.tag, m.id.name);
            }
        }
        files.insert(*name, shape);
    }
    files
}

fn source(tag: &str) -> &'static LangSource {
    LANGS.iter().find(|l| l.tag == tag).unwrap_or_else(|| panic!("language {tag} is not embedded"))
}

#[test]
fn embedded_files_load_without_problems() {
    assert_eq!(load_problems(), Vec::<String>::new());
}

#[test]
fn reference_language_is_present_and_complete() {
    let en = source("en-US");
    let names: Vec<&str> = en.files.iter().map(|f| f.0).collect();
    for required in ["commands", "ribbon", "functions", "errors"] {
        assert!(names.contains(&required), "en-US/{required}.ftl is missing");
    }
    assert!(!en.keymap.is_empty());
}

#[test]
fn every_language_has_the_keys_attributes_and_variables_of_en_us() {
    let reference = shape(source(FALLBACK_TAG));
    for src in LANGS.iter().filter(|l| l.tag != FALLBACK_TAG) {
        let other = shape(src);
        assert_eq!(
            other.keys().collect::<Vec<_>>(),
            reference.keys().collect::<Vec<_>>(),
            "{}: the set of message files differs from en-US",
            src.tag
        );
        for (file, ref_shape) in &reference {
            let got = &other[file];
            let missing: Vec<&String> = ref_shape.keys().filter(|k| !got.contains_key(*k)).collect();
            let extra: Vec<&String> = got.keys().filter(|k| !ref_shape.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{}/{file}.ftl lacks keys of en-US: {missing:?}", src.tag);
            assert!(extra.is_empty(), "{}/{file}.ftl has keys en-US does not define: {extra:?}", src.tag);
            for (key, slots) in ref_shape {
                assert_eq!(&got[key], slots, "{}/{file}.ftl: `{key}` differs from en-US in value/attributes/variables", src.tag);
            }
        }
    }
}

#[test]
fn messages_have_no_untranslated_placeholders() {
    for src in LANGS.iter() {
        for (name, text) in src.files {
            assert!(!text.contains('\u{2068}') && !text.contains('\u{2069}'), "{}/{name}.ftl contains isolating marks", src.tag);
            assert!(!text.contains("TODO"), "{}/{name}.ftl contains a TODO", src.tag);
        }
    }
}

#[test]
fn function_argument_lists_have_the_same_shape_in_every_language() {
    let en = Localizer::new("en-US");
    for tag in available_languages() {
        let loc = Localizer::new(tag);
        let names: Vec<String> = en_function_names();
        for name in names {
            let a = en.function_args(&name);
            let b = loc.function_args(&name);
            assert_eq!(a.len(), b.len(), "{tag}: {name} has {} parameters in en-US and {} here", a.len(), b.len());
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(x.starts_with('['), y.starts_with('['), "{tag}: {name}: optional marker differs for {x} / {y}");
                assert_eq!(x == "...", y == "...", "{tag}: {name}: repetition marker differs for {x} / {y}");
            }
        }
    }
}

fn en_function_names() -> Vec<String> {
    shape(source(FALLBACK_TAG))
        .get("functions")
        .map(|s| s.keys().filter_map(|k| k.strip_prefix("fn-")).map(str::to_string).collect())
        .unwrap_or_default()
}

#[test]
fn function_messages_cover_the_formula_catalog() {
    let catalog: BTreeSet<String> = gridcraft_formula::catalog::BUILTINS.iter().map(|(n, _)| function_key(n)).collect();
    let documented: BTreeSet<String> = en_function_names().into_iter().map(|n| format!("fn-{n}")).collect();
    let missing: Vec<&String> = catalog.difference(&documented).collect();
    let extra: Vec<&String> = documented.difference(&catalog).collect();
    assert!(missing.is_empty(), "functions without documentation: {missing:?}");
    assert!(extra.is_empty(), "documentation for functions that are not in the catalog: {extra:?}");
}

fn is_valid_chord(chord: &str) -> bool {
    let parts: Vec<&str> = chord.split('+').collect();
    let Some((key, mods)) = parts.split_last() else { return false };
    !key.is_empty() && mods.iter().all(|m| ["Cmd", "Ctrl", "Alt", "Shift"].contains(m))
}

fn command_ids() -> BTreeSet<String> {
    let commands = shape(source(FALLBACK_TAG)).remove("commands").unwrap_or_default();
    commands.keys().filter_map(|k| k.strip_prefix("cmd-")).filter_map(|k| k.strip_suffix("-label")).map(str::to_string).collect()
}

#[test]
fn keymap_entries_name_existing_commands_and_carry_provenance() {
    let ids = command_ids();
    assert!(ids.len() > 300, "commands.ftl looks incomplete: {} commands", ids.len());
    for src in LANGS.iter() {
        let keymap: KeymapFile = toml::from_str(src.keymap).unwrap_or_else(|e| panic!("{}/keymap.toml: {e}", src.tag));
        for (id, entry) in &keymap.shortcuts {
            assert!(ids.contains(&id.replace('.', "-")), "{}/keymap.toml: `{id}` is not a command in en-US/commands.ftl", src.tag);
            assert!(is_valid_chord(&entry.keys), "{}/keymap.toml: `{id}`: bad chord {:?}", src.tag, entry.keys);
            if let Some(mac) = &entry.mac {
                assert!(is_valid_chord(mac), "{}/keymap.toml: `{id}`: bad mac chord {mac:?}", src.tag);
            }
            assert!(entry.source.trim().len() > 3, "{}/keymap.toml: `{id}` has no provenance", src.tag);
        }
    }
}

#[test]
fn en_us_keymap_matches_the_engine_chords() {
    let loc = Localizer::new("en-US");
    assert_eq!(loc.shortcut("file.save"), Some("Cmd+S"));
    assert_eq!(loc.shortcut("home.bold"), Some("Cmd+B"));
    assert_eq!(loc.shortcut("edit.beginEdit"), Some("F2"));
    assert_eq!(loc.shortcut("home.insertSheet"), Some("Shift+F11"));
    assert_eq!(loc.shortcut("file.exportCsv"), None);
    assert_eq!(loc.shortcut("no.such.command"), None);
    assert_eq!(loc.shortcuts(Platform::Pc).len(), 64);
}

/// Chord with its modifiers in a canonical order; on the PC layout `Cmd` is Ctrl.
fn normalized_chord(chord: &str, platform: Platform) -> String {
    let mut parts: Vec<&str> = chord.split('+').collect();
    let key = parts.pop().unwrap_or_default();
    let mods: BTreeSet<&str> = parts.into_iter().map(|m| if platform == Platform::Pc && m == "Cmd" { "Ctrl" } else { m }).collect();
    let mut out = mods.into_iter().collect::<Vec<_>>().join("+");
    out.push('+');
    out.push_str(key);
    out
}

/// Pairs of commands that share a chord in the language's effective keymap.
fn conflicting_pairs(loc: &Localizer, platform: Platform) -> BTreeSet<(String, String)> {
    let entries: Vec<(&str, String)> = loc.shortcuts(platform).into_iter().map(|(id, chord)| (id, normalized_chord(chord, platform))).collect();
    let mut pairs = BTreeSet::new();
    for (i, (a, chord_a)) in entries.iter().enumerate() {
        for (b, chord_b) in entries.iter().skip(i + 1) {
            if chord_a == chord_b {
                pairs.insert((a.to_string(), b.to_string()));
            }
        }
    }
    pairs
}

#[test]
fn a_language_adds_no_shortcut_conflict_that_en_us_lacks() {
    let en = Localizer::new("en-US");
    for platform in [Platform::Pc, Platform::Mac] {
        let baseline = conflicting_pairs(&en, platform);
        assert!(!baseline.is_empty(), "{platform:?}: the en-US keymap is expected to have known conflicts, so this check is not vacuous");
        for tag in available_languages() {
            let added: Vec<_> = conflicting_pairs(&Localizer::new(tag), platform).difference(&baseline).cloned().collect();
            assert!(added.is_empty(), "{tag} {platform:?}: new shortcut conflicts versus en-US: {added:?}");
        }
    }
}

#[test]
fn pt_br_shortcuts_follow_the_documented_letters() {
    let pt = Localizer::new("pt-BR");
    assert_eq!(pt.shortcut("home.bold"), Some("Cmd+N"));
    assert_eq!(pt.shortcut("file.save"), Some("Cmd+B"));
    assert_eq!(pt.shortcut("file.open"), Some("Cmd+A"));
    assert_eq!(pt.shortcut("file.new"), Some("Cmd+O"));
    assert_eq!(pt.shortcut("home.underline"), Some("Cmd+S"));
    // macOS keeps the Command-key chords.
    assert_eq!(pt.shortcut_for("home.bold", Platform::Mac), Some("Cmd+B"));
    assert_eq!(pt.shortcut_for("file.save", Platform::Mac), Some("Cmd+S"));
    // Commands the language does not list inherit the en-US chord.
    assert_eq!(pt.shortcut("edit.copy"), Some("Cmd+C"));
    assert!(pt.shortcut_source("home.bold").is_some_and(|s| s.contains("pt-BR")));
    assert!(pt.shortcut_source("edit.copy").is_some_and(|s| !s.contains("pt-BR")));
}

#[test]
fn tags_are_negotiated_against_the_available_languages() {
    assert_eq!(Localizer::new("pt-BR").tag(), "pt-BR");
    assert_eq!(Localizer::new("pt_BR.UTF-8").tag(), "pt-BR");
    assert_eq!(Localizer::new("pt-br").tag(), "pt-BR");
    assert_eq!(Localizer::new("pt").tag(), "pt-BR");
    // European Portuguese gets its own messages when present, else the other Portuguese.
    let pt_pt = if available_languages().contains(&"pt-PT") { "pt-PT" } else { "pt-BR" };
    assert_eq!(Localizer::new("pt-PT").tag(), pt_pt);
    assert_eq!(Localizer::new("pt-AO").tag(), pt_pt);
    let zh_tw = if available_languages().contains(&"zh-TW") { "zh-TW" } else { Localizer::new("zh").tag() };
    assert_eq!(Localizer::new("zh-HK").tag(), zh_tw);
    assert_eq!(Localizer::new("en").tag(), "en-US");
    assert_eq!(Localizer::new("en-GB").tag(), "en-US");
    assert_eq!(Localizer::new("").tag(), "en-US");
    assert_eq!(Localizer::new("C").tag(), "en-US");
    assert_eq!(Localizer::new("POSIX").tag(), "en-US");
    assert_eq!(Localizer::new("xx-YY").tag(), "en-US");
    assert_eq!(Localizer::default().tag(), "en-US");
    assert!(available_languages().contains(&"en-US"));
    assert!(available_languages().contains(&"pt-BR"));
}

#[test]
fn labels_come_from_the_active_language_with_en_us_fallback() {
    let en = Localizer::new("en-US");
    let pt = Localizer::new("pt-BR");
    assert_eq!(en.command_label("file.saveAs").as_deref(), Some("Save As…"));
    assert_eq!(pt.command_label("file.saveAs").as_deref(), Some("Salvar Como…"));
    assert_eq!(pt.command_label("home.mergeCenter").as_deref(), Some("Mesclar e Centralizar"));
    assert_eq!(pt.command_label("no.such.command"), None);
    assert_eq!(en.ribbon_tab("Page Layout").as_deref(), Some("Page Layout"));
    assert_eq!(pt.ribbon_tab("Page Layout").as_deref(), Some("Layout da Página"));
    assert_eq!(pt.ribbon_group("Find & Select").as_deref(), Some("Localizar e Selecionar"));
    assert_eq!(pt.ribbon_tab("Nonexistent"), None);
}

#[test]
fn missing_keys_fall_back_to_en_us_then_to_the_key() {
    let pt = Localizer::new("pt-BR");
    assert_eq!(pt.text("no-such-key", &[]), "no-such-key");
    assert!(pt.get("no-such-key", &[]).is_none());
    assert!(!pt.has("no-such-key"));
    assert!(pt.has("cmd-file-new-label"));
}

#[test]
fn text_formats_variables_without_bidi_marks() {
    let en = Localizer::new("en-US");
    let pt = Localizer::new("pt-BR");
    let args: &[(&str, Arg<'_>)] = &[("command", "file.save".into()), ("reason", "no workbook".into())];
    let e = en.error("command-disabled", args).unwrap_or_default();
    let p = pt.error("command-disabled", args).unwrap_or_default();
    assert_eq!(e, "“file.save” is not available right now: no workbook");
    assert_eq!(p, "“file.save” não está disponível no momento: no workbook");
    assert!(!e.contains('\u{2068}') && !p.contains('\u{2068}'));
    assert_eq!(en.error("no-document", &[]).as_deref(), Some("No workbook is open."));
    assert_eq!(pt.error("other", &[("message", Arg::Str("x"))]).as_deref(), Some("x"));
    assert_eq!(pt.error("not-a-code", &[]), None);
    // A missing variable leaves the placeholder visible instead of failing.
    assert!(en.text("error-bad-params", &[]).contains("command"));
}

#[test]
fn numeric_arguments_convert() {
    assert_eq!(Arg::from(3), Arg::Num(3.0));
    assert_eq!(Arg::from(3usize), Arg::Num(3.0));
    assert_eq!(Arg::from(3i64), Arg::Num(3.0));
    assert_eq!(Arg::from(2.5), Arg::Num(2.5));
    assert_eq!(Arg::from("x"), Arg::Str("x"));
}

#[test]
fn function_documentation_is_translated_with_keys_that_replace_dots() {
    let en = Localizer::new("en-US");
    let pt = Localizer::new("pt-BR");
    assert_eq!(function_key("BETA.DIST"), "fn-BETA-DIST");
    assert_eq!(en.function_description("SUM").as_deref(), Some("Adds all the numbers in the arguments."));
    assert_eq!(pt.function_description("SUM").as_deref(), Some("Soma todos os números dos argumentos."));
    assert_eq!(en.function_args("IF"), ["logical_test", "[value_if_true]", "[value_if_false]"]);
    assert_eq!(pt.function_args("IF"), ["teste_lógico", "[valor_se_verdadeiro]", "[valor_se_falso]"]);
    assert_eq!(en.function_args("SUM"), ["number1", "[number2]", "..."]);
    assert!(en.function_args("PI").is_empty());
    assert!(en.function_args("NOSUCH").is_empty());
    assert!(pt.function_description("BETA.DIST").is_some());
}

#[test]
fn grouped_optional_parameters_stay_one_item() {
    let en = Localizer::new("en-US");
    let pt = Localizer::new("pt-BR");
    assert_eq!(en.function_args("SWITCH"), ["expression", "value1", "result1", "[default_or_value2, result2]", "..."]);
    assert_eq!(pt.function_args("SWITCH"), ["expressão", "valor1", "resultado1", "[padrão_ou_valor2, resultado2]", "..."]);
    assert_eq!(en.function_args("LAMBDA"), ["[parameter1, ...]", "calculation"]);
    assert_eq!(pt.function_args("LAMBDA"), ["[parâmetro1, ...]", "cálculo"]);
    assert_eq!(en.function_args("LET"), ["name1", "value1", "...", "calculation"]);
    assert_eq!(split_args("a, [b, [c, d]], ..."), ["a", "[b, [c, d]]", "..."]);
    assert_eq!(split_args(""), Vec::<String>::new());
    assert_eq!(split_args("x,, y"), ["x", "y"]);
}

#[test]
fn slugs() {
    assert_eq!(slug("Page Layout"), "page-layout");
    assert_eq!(slug("Find & Select"), "find-select");
    assert_eq!(slug("PivotTable Analyze"), "pivottable-analyze");
    assert_eq!(slug("  Get & Transform Data "), "get-transform-data");
    assert_eq!(slug("Home"), "home");
    assert_eq!(command_key("file.saveAs"), "cmd-file-saveAs-label");
}

#[test]
fn localizer_is_shareable_across_threads() {
    let handles: Vec<_> = (0..4).map(|_| std::thread::spawn(|| Localizer::new("pt-BR").command_label("edit.undo").map(|c| c.into_owned()))).collect();
    for h in handles {
        assert_eq!(h.join().ok().flatten().as_deref(), Some("Desfazer"));
    }
}
