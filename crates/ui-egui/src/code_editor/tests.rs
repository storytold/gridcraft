use super::*;
use egui::Modifiers;

fn spans(text: &str, lang: Lang) -> Vec<(&str, Token)> {
    highlight(text, lang).into_iter().map(|(r, k)| (text.get(r).unwrap_or("?"), k)).collect()
}

#[test]
fn python_tokens() {
    let s = spans("def f(x):\n    return len(x) + 1.5  # done\nxl(\"A1:B2\")", Lang::Python);
    assert_eq!(
        s,
        vec![
            ("def", Token::Keyword),
            ("return", Token::Keyword),
            ("len", Token::Builtin),
            ("1.5", Token::Number),
            ("# done", Token::Comment),
            ("xl", Token::Builtin),
            ("\"A1:B2\"", Token::String),
        ]
    );
    assert_eq!(spans("s = '''a\n'b'\nc'''", Lang::Python), vec![("'''a\n'b'\nc'''", Token::String)]);
    assert_eq!(spans("x = 'it\\'s'", Lang::Python), vec![("'it\\'s'", Token::String)]);
    assert_eq!(spans("hello", Lang::Plain), vec![]);
}

#[test]
fn latex_tokens() {
    let s = spans("\\frac{a^2}{3} % half\n\\{", Lang::Latex);
    assert_eq!(
        s,
        vec![
            ("\\frac", Token::Keyword),
            ("^", Token::Builtin),
            ("2", Token::Number),
            ("3", Token::Number),
            ("% half", Token::Comment),
            ("\\{", Token::Keyword),
        ]
    );
}

#[test]
fn hostile_input_never_panics_and_spans_stay_on_char_boundaries() {
    let inputs = [
        "",
        "'",
        "\"",
        "'''",
        "\"\"\"x",
        "#",
        "\\",
        "%",
        "1",
        ".",
        "é'ü\\",
        "θ = 'π' # ∑",
        "\u{1F600}'\u{1F600}",
        "'\\",
        "\\\u{1F600}",
        "\u{200B}\n\t'''\n",
        "0x_é",
        "a.b.c",
        "____",
    ];
    for text in inputs {
        for lang in [Lang::Python, Lang::Latex, Lang::Plain] {
            let mut last = 0;
            for (r, _) in highlight(text, lang) {
                assert!(r.start >= last && r.start <= r.end && r.end <= text.len(), "{text:?} {lang:?} {r:?}");
                assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end), "{text:?} {lang:?} {r:?}");
                last = r.end;
            }
            let job = layout_job(text, lang, &FontId::default(), egui::Color32::BLACK, &Tokens::light().syntax, f32::INFINITY);
            assert_eq!(job.text, text, "every character is laid out once");
        }
    }
}

#[test]
fn enter_keeps_indentation_and_indents_after_a_colon() {
    let mut s = "    x = 1".to_string();
    let e = newline(&mut s, (9, 9), Lang::Python);
    assert_eq!(s, "    x = 1\n    ");
    assert_eq!(e.caret, 14);
    let mut s = "if x:".to_string();
    let e = newline(&mut s, (5, 5), Lang::Python);
    assert_eq!(s, "if x:\n    ");
    assert_eq!(e.caret, 10);
    // Plain text keeps the indentation but adds none after a colon.
    let mut s = "  a:".to_string();
    newline(&mut s, (4, 4), Lang::Plain);
    assert_eq!(s, "  a:\n  ");
    // A selection is replaced; out-of-range carets are clamped.
    let mut s = "abcdef".to_string();
    let e = newline(&mut s, (4, 2), Lang::Plain);
    assert_eq!((s.as_str(), e.caret), ("ab\nef", 3));
    let mut s = "é".to_string();
    let e = newline(&mut s, (99, 99), Lang::Plain);
    assert_eq!((s.as_str(), e.caret), ("é\n", 2));
}

#[test]
/// Shift+Tab removes four leading spaces, while a plain Tab still inserts four spaces at the caret.
fn shift_tab_in_the_editor_dedents_rather_than_indents() {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
        |ui, text: &mut String| {
            if ui.ctx().cumulative_pass_nr() == 0 {
                ui.ctx().set_fonts(theme::pinned_font_definitions());
                return;
            }
            let id = egui::Id::unique("code");
            ui.memory_mut(|m| m.request_focus(id));
            code_editor(ui, id, text, Lang::Python, 5);
        },
        "        x".to_string(),
    );
    h.run_steps(2);
    let tab = |modifiers| egui::Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers };
    h.input_mut().events.push(tab(Modifiers::SHIFT));
    h.run_steps(1);
    assert_eq!(h.state(), "    x");
    // A plain Tab still inserts spaces at the caret (the end).
    h.input_mut().events.push(tab(Modifiers::NONE));
    h.run_steps(1);
    assert_eq!(h.state(), "    x    ");
}

/// Gives the context the deterministic font set, including the editor's monospace family.
fn install_mono(ctx: &egui::Context) {
    ctx.set_fonts(theme::pinned_font_definitions());
}

/// Builds a 400x300 code-editor harness with pinned mono fonts, focus, two passes and an accessibility check.
fn editor_harness(text: &str) -> egui_kittest::Harness<'static, String> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
        |ui, text: &mut String| {
            if ui.ctx().cumulative_pass_nr() == 0 {
                install_mono(ui.ctx());
                return;
            }
            let id = egui::Id::unique("code");
            ui.memory_mut(|m| m.request_focus(id));
            code_editor(ui, id, text, Lang::Python, 5);
        },
        text.to_string(),
    );
    h.run_steps(2);
    h.check_accessibility();
    h
}

fn key(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

#[test]
fn batched_input_applies_in_order_and_keeps_every_key() {
    // Events gather between repaints (typing, key repeat). The text typed before an Enter
    // comes first, and two Enters make two lines.
    let mut h = editor_harness("");
    h.input_mut().events.extend([egui::Event::Text("if x:".into()), key(Key::Enter, Modifiers::NONE), key(Key::Enter, Modifiers::NONE)]);
    h.run_steps(1);
    assert_eq!(h.state(), "if x:\n    \n    ");
    // Backspace and Tab in the same frame as the typing around them.
    h.input_mut().events.extend([
        egui::Event::Text("y = 1".into()),
        key(Key::Backspace, Modifiers::NONE),
        egui::Event::Text("2".into()),
        key(Key::Tab, Modifiers::NONE),
    ]);
    h.run_steps(1);
    assert_eq!(h.state(), "if x:\n    \n    y = 2    ");
    // An event the field acts on (an arrow) keeps the order too: the text typed before it
    // is applied first, the field moves the caret, and the Enter after it waits for the
    // next pass, where the editor applies it at the moved caret, with the indentation.
    h.input_mut().events.extend([egui::Event::Text("a".into()), key(Key::ArrowLeft, Modifiers::NONE), key(Key::Enter, Modifiers::NONE)]);
    h.run_steps(1);
    assert_eq!(h.state(), "if x:\n    \n    y = 2    a", "the Enter waits for the arrow");
    h.run_steps(1);
    assert_eq!(h.state(), "if x:\n    \n    y = 2    \n    a");
}

#[test]
fn a_selection_ending_at_a_line_start_leaves_that_line_alone() {
    // Selecting "a\n" in "a\nb" ends at the start of the second line, which is not selected.
    let mut s = "a\nb".to_string();
    indent(&mut s, (0, 2), false);
    assert_eq!(s, "    a\nb");
    let mut s = "    a\n    b".to_string();
    indent(&mut s, (0, 6), true);
    assert_eq!(s, "a\n    b");
    // One character into the second line, both lines are touched.
    let mut s = "a\nb".to_string();
    indent(&mut s, (0, 3), false);
    assert_eq!(s, "    a\n    b");
}

#[test]
fn typed_text_and_deletions_replace_the_selection() {
    let mut s = "abcd".to_string();
    assert_eq!(insert(&mut s, (3, 1), "X"), Edit { anchor: 2, caret: 2 });
    assert_eq!(s, "aXd");
    assert_eq!(delete(&mut s, (2, 2), true), Edit { anchor: 1, caret: 1 });
    assert_eq!(s, "ad");
    assert_eq!(delete(&mut s, (1, 1), false), Edit { anchor: 1, caret: 1 });
    assert_eq!(s, "a");
    assert_eq!(delete(&mut s, (0, 0), true), Edit { anchor: 0, caret: 0 });
    assert_eq!(delete(&mut s, (9, 9), false), Edit { anchor: 1, caret: 1 });
    assert_eq!(s, "a");
    let mut s = "éa".to_string();
    delete(&mut s, (2, 0), true);
    assert_eq!(s, "");
    // The queue keeps what the editor does not handle, and everything after it.
    let mut events =
        vec![egui::Event::Text("x".into()), egui::Event::Text("\n".into()), key(Key::ArrowLeft, Modifiers::NONE), key(Key::Enter, Modifiers::NONE)];
    let (mut s, mut sel) = (String::new(), (0, 0));
    let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
    assert!(out.edited);
    assert_eq!((s.as_str(), sel), ("x", (1, 1)));
    assert_eq!(events.len(), 2, "the lone newline and the arrow stay for the field: {events:?}");
    assert_eq!(out.held, vec![key(Key::Enter, Modifiers::NONE)], "the Enter after the arrow waits for the next pass");
    let mut events = vec![egui::Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }];
    let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
    assert!(!out.edited && out.held.is_empty(), "a key release is nothing to apply");
    assert_eq!(events.len(), 1);
}

#[test]
fn select_all_then_tab_in_one_pass_indents_every_line() {
    // Cmd+A is the field's, Tab the editor's: the Tab waits for the pass after the
    // selection and indents the selected lines, rather than reaching the field, which would
    // replace the selection with a tab character.
    let mut h = editor_harness("a\nb");
    h.input_mut().events.extend([key(Key::A, Modifiers::COMMAND), key(Key::Tab, Modifiers::NONE)]);
    h.run_steps(1);
    assert_eq!(h.state(), "a\nb", "the Tab waits for the selection");
    h.run_steps(1);
    assert_eq!(h.state(), "    a\n    b");
    h.input_mut().events.extend([key(Key::A, Modifiers::COMMAND), key(Key::Tab, Modifiers::SHIFT)]);
    h.run_steps(2);
    assert_eq!(h.state(), "a\nb");
}

#[test]
fn a_keystroke_sequence_as_the_platform_sends_it_keeps_the_editors_enter() {
    // egui-winit sends a Key event before each Text event. Those presses are nobody's to
    // read, so they do not hand the Enter to the field, which knows no indentation.
    let mut h = editor_harness("");
    let typed = |k, c: &str, m| [key(k, m), egui::Event::Text(c.into())];
    let mut events = Vec::new();
    events.extend(typed(Key::I, "i", Modifiers::NONE));
    events.extend(typed(Key::F, "f", Modifiers::NONE));
    events.extend(typed(Key::Space, " ", Modifiers::NONE));
    events.extend(typed(Key::X, "x", Modifiers::NONE));
    events.extend(typed(Key::Colon, ":", Modifiers::SHIFT));
    events.push(key(Key::Enter, Modifiers::NONE));
    h.input_mut().events.extend(events);
    h.run_steps(1);
    assert_eq!(h.state(), "if x:\n    ");
    // A character typed with Option (macOS) comes as a key under that modifier and its text;
    // the field reads neither the key nor Option, so the Enter after it is still the editor's.
    h.input_mut().events.extend([key(Key::E, Modifiers::ALT), egui::Event::Text("\u{e9}".into()), key(Key::Enter, Modifiers::NONE)]);
    h.run_steps(1);
    assert_eq!(h.state(), "if x:\n    \u{e9}\n    ");
}

#[test]
fn tab_and_enter_under_modifiers_never_reach_the_field() {
    // The field would make a tab character of Ctrl+Tab (in place of the selection) and a
    // newline of Shift+Enter; Cmd+Enter is nobody's here and stays for a shortcut.
    let mut events = vec![key(Key::Tab, Modifiers::CTRL), key(Key::Enter, Modifiers::SHIFT), key(Key::Enter, Modifiers::COMMAND)];
    let (mut s, mut sel) = ("if x:".to_string(), (5, 5));
    let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
    assert_eq!(s, "if x:\n    ");
    assert_eq!(events, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(out.held.is_empty());
}

#[test]
fn events_after_a_held_key_wait_with_it_and_pointer_events_do_not() {
    let mut events = vec![
        key(Key::X, Modifiers::NONE),
        egui::Event::Text("x".into()),
        key(Key::A, Modifiers::COMMAND),
        key(Key::Tab, Modifiers::NONE),
        egui::Event::PointerMoved(egui::Pos2::ZERO),
        key(Key::ArrowLeft, Modifiers::NONE),
        egui::Event::Text("y".into()),
    ];
    let (mut s, mut sel) = (String::new(), (0, 0));
    let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
    assert_eq!((s.as_str(), sel), ("x", (1, 1)), "the typing before the select-all is applied");
    assert_eq!(events, vec![key(Key::X, Modifiers::NONE), key(Key::A, Modifiers::COMMAND), egui::Event::PointerMoved(egui::Pos2::ZERO)]);
    assert_eq!(out.held, vec![key(Key::Tab, Modifiers::NONE), key(Key::ArrowLeft, Modifiers::NONE), egui::Event::Text("y".into())]);
}

#[test]
/// Events held by the previous pass are applied once in the next pass; older held events are dropped and cleared.
fn held_back_events_land_in_the_next_pass_only() {
    let mut h = editor_harness("");
    let id = egui::Id::unique("code");
    let held = |pass, c: &str| Held { pass, events: vec![egui::Event::Text(c.into())] };
    // What the pass just completed held back comes in with the next one.
    let last = h.ctx.cumulative_pass_nr() - 1;
    h.ctx.data_mut(|d| d.insert_temp(held_id(id), held(last, "a")));
    h.run_steps(1);
    assert_eq!(h.state(), "a");
    // Anything older is dropped rather than typed out of context, and cleared.
    h.ctx.data_mut(|d| d.insert_temp(held_id(id), held(last, "b")));
    h.run_steps(1);
    assert_eq!(h.state(), "a");
    assert!(h.ctx.data(|d| d.get_temp::<Held>(held_id(id))).is_none());
}

#[test]
/// Held keyboard input does not land in the next focused text field once the code editor has lost focus.
fn held_back_events_go_nowhere_once_the_editor_lost_the_focus() {
    // Typed into the editor, they must not land in whatever has the focus next.
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
        |ui, s: &mut (String, String, bool)| {
            if ui.ctx().cumulative_pass_nr() == 0 {
                install_mono(ui.ctx());
                return;
            }
            let (code, other) = (egui::Id::unique("code"), egui::Id::unique("other"));
            ui.memory_mut(|m| m.request_focus(if s.2 { code } else { other }));
            code_editor(ui, code, &mut s.0, Lang::Python, 5);
            ui.add(TextEdit::singleline(&mut s.1).id(other)).accessible_name("Other field");
        },
        (String::new(), String::new(), true),
    );
    h.run_steps(2);
    h.input_mut().events.extend([key(Key::ArrowLeft, Modifiers::NONE), egui::Event::Text("x".into())]);
    h.run_steps(1);
    assert_eq!(h.state().0, "", "the x waits for the arrow");
    h.state_mut().2 = false;
    h.run_steps(2);
    assert_eq!((h.state().0.as_str(), h.state().1.as_str()), ("", ""));
}

#[test]
/// A discard-triggered repeat pass receives held keys with the selection made by the first pass.
fn a_repeat_pass_of_the_frame_counts_as_the_next_pass() {
    // When a widget asks egui to discard and repeat the pass, the repeat pass gets the
    // held-back keys, with the selection the field made in the first pass.
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
        |ui, s: &mut (String, bool)| {
            if ui.ctx().cumulative_pass_nr() == 0 {
                install_mono(ui.ctx());
                return;
            }
            let id = egui::Id::unique("code");
            ui.memory_mut(|m| m.request_focus(id));
            code_editor(ui, id, &mut s.0, Lang::Python, 5);
            if std::mem::take(&mut s.1) {
                ui.ctx().request_discard("the test repeats this pass");
            }
        },
        ("a\nb".to_string(), false),
    );
    h.run_steps(2);
    h.state_mut().1 = true;
    h.input_mut().events.extend([key(Key::A, Modifiers::COMMAND), key(Key::Tab, Modifiers::NONE)]);
    h.run_steps(1);
    assert_eq!(h.state().0, "    a\n    b");
}
