use super::{Rect, Sense, SheetApp, Stroke, StrokeKind, Style, Tokens, Ui, keytip_table, pos2, theme, vec2};

pub(super) fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

/// Height of the caption under every ribbon group. Fixed, and smaller than the
/// control row, so the words sit on one quiet line and never cover a command.
pub(crate) const GROUP_CAPTION_H: f32 = 16.0;

/// The control row above the caption. Big buttons and the group separator are
/// this tall; a shorter cluster keeps that height so every caption shares a baseline.
const GROUP_CONTROLS_H: f32 = 72.0;

/// Caption size. Big-button labels are 11.5 px.
const GROUP_CAPTION_PT: f32 = 10.0;

/// Translation keys for the caption under each group, in the order the tab draws
/// the groups. English shows the part after `|`. The words name what the group
/// does; they are not copied from another spreadsheet's group titles.
pub(crate) const HOME_CAPTIONS: [&str; 7] = [
    "Ribbon group|Copies",
    "Ribbon group|Type format",
    "Ribbon group|Align",
    "Ribbon group|Number format",
    "Ribbon group|Presets",
    "Ribbon group|Build",
    "Ribbon group|Prepare",
];
pub(crate) const FORMULAS_CAPTIONS: [&str; 4] = ["Ribbon group|Functions", "Ribbon group|Names", "Ribbon group|Check", "Ribbon group|Compute"];
pub(crate) const DATA_CAPTIONS: [&str; 7] = [
    "Ribbon group|Import",
    "Ribbon group|Order",
    "Ribbon group|Clean",
    "Ribbon group|Projection",
    "Ribbon group|Levels",
    "Ribbon group|Examine",
    "Ribbon group|Solve",
];
pub(crate) const VIEW_CAPTIONS: [&str; 5] =
    ["Ribbon group|Look", "Ribbon group|Reveal", "Ribbon group|Scale", "Ribbon group|Workspace", "Ribbon group|Interface"];
pub(crate) const INSERT_CAPTIONS: [&str; 10] = [
    "Ribbon group|Lists",
    "Ribbon group|Artwork",
    "Ribbon group|Graphs",
    "Ribbon group|Slice",
    "Ribbon group|Sparks",
    "Ribbon group|Switches",
    "Ribbon group|Ticks",
    "Ribbon group|Attach",
    "Ribbon group|Wording",
    "Ribbon group|Glyphs",
];
pub(crate) const DRAW_CAPTIONS: [&str; 3] = ["Ribbon group|Grab", "Ribbon group|Strokes", "Ribbon group|Reshape"];
pub(crate) const PAGE_LAYOUT_CAPTIONS: [&str; 3] = ["Ribbon group|Palettes", "Ribbon group|Paper", "Ribbon group|Sheet face"];
pub(crate) const REVIEW_CAPTIONS: [&str; 3] = ["Ribbon group|Corrections", "Ribbon group|Remarks", "Ribbon group|Lock"];
pub(crate) const AUTOMATE_CAPTIONS: [&str; 2] = ["Ribbon group|Scripts", "Ribbon group|App"];
pub(crate) const TABLE_CAPTIONS: [&str; 5] =
    ["Ribbon group|Identity", "Ribbon group|Tidy", "Ribbon group|Slice", "Ribbon group|Traits", "Ribbon group|Swatches"];
pub(crate) const CHART_CAPTIONS: [&str; 1] = ["Ribbon group|Graph"];
pub(crate) const HEADER_FOOTER_CAPTIONS: [&str; 4] = ["Ribbon group|Samples", "Ribbon group|Fields", "Ribbon group|Jump", "Ribbon group|Page rules"];

/// One ribbon cluster. `body` lays out the controls as before (same spacing, no
/// width change). A short caption is painted in a fixed band underneath, centred,
/// in the theme's dim text. A caption wider than the cluster ends in an ellipsis
/// between characters, and the hover text is the full name.
pub(crate) fn group(ui: &mut Ui, caption: &str, body: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let controls = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            body(ui);
        });
        let width = controls.response.rect.width().max(1.0);
        let short = GROUP_CONTROLS_H - controls.response.rect.height();
        if short > 0.5 {
            ui.add_space(short);
        }
        let (band, resp) = ui.allocate_exact_size(vec2(width, GROUP_CAPTION_H), Sense::hover());
        let t = Tokens::get(ui.ctx());
        let font = theme::ui_font(GROUP_CAPTION_PT);
        let shown =
            fit_caption(caption, (width - 4.0).max(0.0), |sample| ui.painter().layout_no_wrap(sample.to_string(), font.clone(), t.text_dim).size().x);
        let galley = ui.painter().layout_no_wrap(shown, font, t.text_dim);
        let pos = pos2(band.center().x - galley.size().x * 0.5, band.center().y - galley.size().y * 0.5);
        ui.painter().galley(pos, galley, t.text_dim);
        let resp = if caption.is_empty() { resp } else { resp.on_hover_text(caption) };
        let enabled = ui.is_enabled();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, enabled, caption));
    });
}

/// Shorten `text` with a trailing ellipsis so `width_of` stays within `max_width`.
/// The cut is between characters, so a narrow group never slices a glyph in half.
pub(crate) fn fit_caption(text: &str, max_width: f32, width_of: impl Fn(&str) -> f32) -> String {
    if text.is_empty() || max_width <= 0.0 {
        return String::new();
    }
    if width_of(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let mut end = text.chars().count();
    while end > 0 {
        end -= 1;
        let mut cut: String = text.chars().take(end).collect();
        cut.push_str(ellipsis);
        if width_of(&cut) <= max_width {
            return cut;
        }
    }
    if width_of(ellipsis) <= max_width { ellipsis.to_string() } else { String::new() }
}

#[cfg(test)]
pub(crate) fn caption_keys() -> impl Iterator<Item = &'static str> {
    HOME_CAPTIONS
        .into_iter()
        .chain(FORMULAS_CAPTIONS)
        .chain(DATA_CAPTIONS)
        .chain(VIEW_CAPTIONS)
        .chain(INSERT_CAPTIONS)
        .chain(DRAW_CAPTIONS)
        .chain(PAGE_LAYOUT_CAPTIONS)
        .chain(REVIEW_CAPTIONS)
        .chain(AUTOMATE_CAPTIONS)
        .chain(TABLE_CAPTIONS)
        .chain(CHART_CAPTIONS)
        .chain(HEADER_FOOTER_CAPTIONS)
}

pub(super) fn active_style(app: &SheetApp) -> Style {
    app.session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            Some(d.wb.styles.get(sh.style_id(d.selection.active)).clone())
        })
        .unwrap_or_default()
}

pub(super) fn theme_colors(app: &SheetApp) -> [u32; 12] {
    app.session.active().map(|d| d.wb.theme.colors).unwrap_or(gridcraft_engine::model::Theme::default().colors)
}

/// Runs a command from a ribbon click (menu-style: no params may open a dialog).
pub(super) fn act(app: &mut SheetApp, id: &str, params: serde_json::Value) {
    app.run_or_alert(id, params);
}

/// Keytips are painted, not focusable widgets: the grid retains keyboard focus.
pub(super) fn keytip(app: &SheetApp, ui: &Ui, sequence: &str, at: egui::Pos2) {
    let Some(rest) = app
        .keytips
        .prefix()
        .filter(|prefix| keytip_table::paints_badges(prefix))
        .and_then(|prefix| sequence.strip_prefix(prefix))
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("ribbon_keytips"))).with_clip_rect(ui.clip_rect());
    let text = painter.layout_no_wrap(rest.into(), theme::ui_bold(10.5), t.text);
    let rect = Rect::from_center_size(at, text.size() + vec2(7.0, 3.0));
    painter.rect_filled(rect, 2.0, t.input_bg);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    painter.galley(rect.min + vec2(3.5, 1.5), text, t.text);
}

/// Paint the badge for a control that runs command `id`, when the armed prefix matches.
pub(super) fn control_keytip(app: &SheetApp, ui: &Ui, response: &egui::Response, id: &str) {
    if let Some(sequence) = keytip_table::control_badge(app.keytips.prefix().unwrap_or_default(), id) {
        keytip(app, ui, sequence, response.rect.center_bottom());
    }
}

/// Paint the single badge for a menu anchor whose popup items run `targets`.
pub(super) fn anchor_keytip(app: &SheetApp, ui: &Ui, response: &egui::Response, targets: &[&str]) {
    if let Some(sequence) = keytip_table::anchor_badge(app.keytips.prefix().unwrap_or_default(), targets) {
        keytip(app, ui, sequence, response.rect.center_bottom());
    }
}

/// The `on` value for a ribbon checkbox command. A click has already flipped the checkbox bool, so
/// it is sent as is; a key tip leaves the bool untouched, so the command gets the opposite.
pub(super) fn checkbox_on(clicked: bool, shown: bool) -> bool {
    shown == clicked
}

/// Consume a one-shot keytip request for control `id` (set by `Action::Control`).
pub(super) fn invoke(app: &mut SheetApp, id: &str) -> bool {
    if app.ui.keytip_invoke == Some(id) {
        app.ui.keytip_invoke = None;
        true
    } else {
        false
    }
}

/// Force-open command for a popup: either a keytip invoke (open it) or a click (toggle).
pub(super) fn popup_command(forced: bool, clicked: bool) -> Option<egui::SetOpenCommand> {
    if forced { Some(egui::SetOpenCommand::Bool(true)) } else { clicked.then_some(egui::SetOpenCommand::Toggle) }
}

/// Per-control state of an [`editable_combo`]: the filter text and highlighted row.
#[derive(Clone, Default)]
struct ComboState {
    text: String,
    sel: i32,
    was_open: bool,
}

/// What the user did with an [`editable_combo`].
pub(crate) enum ComboResult {
    None,
    /// A row was applied: clicked or Enter on the highlighted row.
    Picked(String),
}

/// Excel-style dropdown: a filter field (type to narrow the list) plus arrow/Enter selection.
/// `items` are `(value, display)`; the value is what comes back. When `forced` (keytip invoke)
/// or clicked, the popup opens, focuses the filter field, and selects its text.
pub(crate) fn editable_combo(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
    forced: bool,
) -> ComboResult {
    combo_popup(ui, anchor, salt, current_value, items, ComboPopupOptions { forced, filter: true, under: None })
}

/// The dropdown of [`editable_combo`] without the filter field: the whole list, picked by click or
/// arrows and Enter. It drops directly below `under` and is as wide as it, like Excel's Font Size
/// list under its box.
pub(super) fn list_combo(
    ui: &mut Ui,
    anchor: &egui::Response,
    under: Rect,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
) -> ComboResult {
    combo_popup(ui, anchor, salt, current_value, items, ComboPopupOptions { forced: false, filter: false, under: Some(under) })
}

/// What the header of a combo popup hands to its list: the rows to show and the highlighted one.
struct ComboHead {
    filtered: Vec<usize>,
    sel: i32,
    down: bool,
    up: bool,
    enter: bool,
}

struct ComboPopupOptions {
    forced: bool,
    filter: bool,
    under: Option<Rect>,
}

/// Arrow and Enter keys (taken out of the input so a filter field never sees them), the filter
/// field when `text` is given, and the rows its text matches.
fn combo_head(ui: &mut Ui, text: Option<(&mut String, egui::Id)>, open_now: bool, sel: i32, items: &[(&str, String)]) -> ComboHead {
    let (mut down, mut up, mut enter) = (false, false, false);
    ui.input(|i| {
        down = i.key_pressed(egui::Key::ArrowDown);
        up = i.key_pressed(egui::Key::ArrowUp);
        enter = i.key_pressed(egui::Key::Enter);
    });
    if down || up || enter {
        ui.ctx().input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::ArrowDown | egui::Key::ArrowUp | egui::Key::Enter, pressed: true, .. }));
        });
    }
    let mut lower = String::new();
    if let Some((text, text_id)) = text {
        let te = ui.add(egui::TextEdit::singleline(text).id(text_id).desired_width(220.0).hint_text("Type to filter…"));
        if open_now && !te.has_focus() {
            ui.ctx().memory_mut(|m| m.request_focus(text_id));
            // Select all once focus lands so typing replaces the filter.
            if let Some(mut s) = egui::text_edit::TextEditState::load(ui.ctx(), text_id) {
                let len = text.chars().count();
                s.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(len))));
                s.store(ui.ctx(), text_id);
            }
        }
        ui.separator();
        lower = text.to_lowercase();
    }
    let filtered: Vec<usize> = items.iter().enumerate().filter(|(_, (_, d))| d.to_lowercase().contains(&lower)).map(|(i, _)| i).collect();
    let mut sel = sel;
    if !filtered.is_empty() {
        let n = filtered.len() as i32;
        sel = sel.clamp(0, n - 1);
        if down {
            sel = (sel + 1).rem_euclid(n);
        }
        if up {
            sel = (sel - 1).rem_euclid(n);
        }
    }
    ComboHead { filtered, sel, down, up, enter }
}

/// The rows of a combo popup. Returns the picked value and the highlighted row.
fn combo_rows(ui: &mut Ui, head: ComboHead, items: &[(&str, String)], row_w: f32) -> (Option<String>, i32) {
    let ComboHead { filtered, mut sel, down, up, enter } = head;
    let mut picked = None;
    if enter && !filtered.is_empty() {
        let idx = filtered[sel.clamp(0, filtered.len() as i32 - 1) as usize];
        picked = Some(items[idx].0.to_string());
    }
    for (pos, i) in filtered.iter().enumerate() {
        let (value, disp) = &items[*i];
        let row = ui.add(egui::Button::selectable(pos as i32 == sel, egui::RichText::new(disp)).min_size(vec2(row_w, 20.0)));
        if (down || up) && pos as i32 == sel && !ui.clip_rect().contains_rect(row.rect) {
            row.scroll_to_me(Some(if down { egui::Align::Max } else { egui::Align::Min }));
        }
        if row.clicked() {
            picked = Some(value.to_string());
        }
        if row.hovered() {
            sel = pos as i32;
        }
    }
    if picked.is_some() {
        ui.close();
    }
    (picked, sel)
}

fn combo_popup(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
    options: ComboPopupOptions,
) -> ComboResult {
    let ComboPopupOptions { forced, filter, under } = options;
    let popup_id = egui::Popup::default_response_id(anchor);
    let state_id = anchor.id.with(salt).with("__combo_state");
    let mut st = ui.data_mut(|d| d.get_temp::<ComboState>(state_id)).unwrap_or_default();
    if forced {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    let open_now = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if open_now && !st.was_open {
        st.text = String::new();
        st.sel = items.iter().position(|(v, _)| *v == current_value).map(|i| i as i32).unwrap_or(0);
        // The reset lands on the frame AFTER the popup opens, which usually has no input of
        // its own — without a repaint request the stale filtered list stays on screen.
        ui.ctx().request_repaint();
    }
    st.was_open = open_now;
    // Below `under` and as wide as it, or below the anchor itself.
    let mut field = anchor.clone();
    if let Some(r) = under {
        field.rect = r;
    }
    let row_w = under.map_or(220.0, |r| r.width() - 12.0);
    // Nine rows as today, plus the filter field and its separator, plus the popup frame.
    let button_h = ui.spacing().interact_size.y.max(20.0);
    let spacing = ui.spacing().item_spacing.y;
    let max_rows = 9.0_f32;
    let mut height = max_rows * button_h + (max_rows - 1.0) * spacing + egui::Frame::popup(ui.style()).total_margin().sum().y;
    if filter {
        height += ui.spacing().interact_size.y + 4.0 * spacing + 1.0;
    }
    let menu = craft_ui::combo::Menu::new(ui, &field).height(height);
    let text_id = anchor.id.with(salt).with("__combo_text");
    let sel0 = st.sel;
    let shown = if filter {
        menu.show_ui_with_header(
            |ui| combo_head(ui, Some((&mut st.text, text_id)), open_now, sel0, items),
            |ui, head| combo_rows(ui, head, items, row_w),
        )
    } else {
        menu.show_ui(|ui| {
            let head = combo_head(ui, None, open_now, sel0, items);
            combo_rows(ui, head, items, row_w)
        })
    };
    let mut result = ComboResult::None;
    if let Some((picked, sel)) = shown {
        st.sel = sel;
        if let Some(value) = picked {
            result = ComboResult::Picked(value);
        }
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    result
}

pub(super) fn menu_items(app: &mut SheetApp, ui: &mut Ui, items: &[(&str, &str, serde_json::Value)]) {
    for (label, id, p) in items {
        if *label == "-" {
            ui.separator();
            continue;
        }
        let lang = app.ui.language;
        let response = ui.add(egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(220.0, 22.0)));
        if let Some(sequence) = keytip_table::item_letter(app.keytips.prefix().unwrap_or_default(), id) {
            keytip(app, ui, sequence, pos2(response.rect.right() - 10.0, response.rect.center().y));
        }
        if response.clicked() {
            if let Some(d) = id.strip_prefix("dialog:") {
                app.open_dialog(d, p.clone());
            } else if *id == "edit.paste" {
                app.paste_from_clipboard();
            } else {
                act(app, id, p.clone());
            }
        }
    }
}

#[cfg(test)]
mod checkbox_on_tests {
    use super::*;

    #[test]
    fn click_sends_the_new_checkbox_value_and_key_tip_the_opposite() {
        // Click: the checkbox bool already holds the new choice.
        assert!(!checkbox_on(true, false));
        assert!(checkbox_on(true, true));
        // Key tip: the bool still holds the current setting, so toggle it.
        assert!(checkbox_on(false, false));
        assert!(!checkbox_on(false, true));
    }
}

#[cfg(test)]
mod caption_tests {
    use super::{caption_keys, fit_caption};
    use crate::i18n::Language;

    #[test]
    fn a_narrow_caption_ends_in_an_ellipsis() {
        let width = |s: &str| s.chars().count() as f32;
        assert_eq!(fit_caption("Zahlenformat", 100.0, width), "Zahlenformat");
        assert_eq!(fit_caption("Zahlenformat", 6.0, width), "Zahle…");
        assert_eq!(fit_caption("Zahlenformat", 1.0, width), "…");
        assert_eq!(fit_caption("Zahlenformat", 0.0, width), "");
        let cut = fit_caption("Schriftformat", 8.0, width);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= 8);
    }

    #[test]
    fn every_group_caption_has_its_own_short_name() {
        fn english(key: &str) -> &str {
            match key.rsplit_once('|') {
                Some((_, label)) => label,
                None => key,
            }
        }
        assert_eq!(Language::De.tr("Ribbon group|Type format"), "Schriftformat");
        assert_eq!(Language::De.tr("Ribbon group|Number format"), "Zahlenformat");
        for (key, _) in GERMAN {
            assert!(caption_keys().any(|candidate| candidate == *key), "{key} is not a ribbon group");
        }
        for key in caption_keys() {
            let de = GERMAN.iter().find_map(|(candidate, value)| (*candidate == key).then_some(*value)).unwrap_or("");
            assert!(!de.is_empty(), "missing German caption for {key}");
            assert_eq!(Language::De.tr(key), de, "{key}");
            for language in Language::ALL {
                let shown = language.tr(key);
                assert!(!shown.is_empty(), "{language:?} {key}");
                assert!(!shown.contains('\n'), "{language:?} {key} => {shown}");
                assert!(!shown.contains('|'), "{language:?} {key} => {shown}");
                if language == Language::En {
                    assert_eq!(shown, english(key));
                } else {
                    assert_ne!(shown, english(key), "{language:?} falls back to English for {key}");
                }
            }
        }
    }

    /// German caption for every group key, in `caption_keys` order.
    const GERMAN: &[(&str, &str)] = &[
        ("Ribbon group|Copies", "Ablage"),
        ("Ribbon group|Type format", "Schriftformat"),
        ("Ribbon group|Align", "Ausrichten"),
        ("Ribbon group|Number format", "Zahlenformat"),
        ("Ribbon group|Presets", "Vorlagen"),
        ("Ribbon group|Build", "Aufbau"),
        ("Ribbon group|Prepare", "Aufbereiten"),
        ("Ribbon group|Functions", "Funktionen"),
        ("Ribbon group|Names", "Namen"),
        ("Ribbon group|Check", "Prüfen"),
        ("Ribbon group|Compute", "Rechnen"),
        ("Ribbon group|Import", "Holen"),
        ("Ribbon group|Order", "Ordnen"),
        ("Ribbon group|Clean", "Bereinigen"),
        ("Ribbon group|Projection", "Ausblick"),
        ("Ribbon group|Levels", "Ebenen"),
        ("Ribbon group|Examine", "Auswerten"),
        ("Ribbon group|Solve", "Lösen"),
        ("Ribbon group|Look", "Blick"),
        ("Ribbon group|Reveal", "Einblenden"),
        ("Ribbon group|Scale", "Maßstab"),
        ("Ribbon group|Workspace", "Umgebung"),
        ("Ribbon group|Interface", "Oberfläche"),
        ("Ribbon group|Lists", "Listen"),
        ("Ribbon group|Artwork", "Grafik"),
        ("Ribbon group|Graphs", "Schaubilder"),
        ("Ribbon group|Slice", "Ausschnitt"),
        ("Ribbon group|Sparks", "Minikurven"),
        ("Ribbon group|Switches", "Schalter"),
        ("Ribbon group|Ticks", "Häkchen"),
        ("Ribbon group|Attach", "Beifügen"),
        ("Ribbon group|Wording", "Beschriften"),
        ("Ribbon group|Glyphs", "Zeichen"),
        ("Ribbon group|Grab", "Greifen"),
        ("Ribbon group|Strokes", "Striche"),
        ("Ribbon group|Reshape", "Umwandeln"),
        ("Ribbon group|Palettes", "Farbwelt"),
        ("Ribbon group|Paper", "Papier"),
        ("Ribbon group|Sheet face", "Blattlage"),
        ("Ribbon group|Corrections", "Korrektur"),
        ("Ribbon group|Remarks", "Anmerkungen"),
        ("Ribbon group|Lock", "Sperren"),
        ("Ribbon group|Scripts", "Abläufe"),
        ("Ribbon group|App", "Programm"),
        ("Ribbon group|Identity", "Kennung"),
        ("Ribbon group|Tidy", "Säubern"),
        ("Ribbon group|Slice", "Ausschnitt"),
        ("Ribbon group|Traits", "Merkmale"),
        ("Ribbon group|Swatches", "Muster"),
        ("Ribbon group|Graph", "Schaubild"),
        ("Ribbon group|Samples", "Vorgaben"),
        ("Ribbon group|Fields", "Felder"),
        ("Ribbon group|Jump", "Wechseln"),
        ("Ribbon group|Page rules", "Seitenregeln"),
    ];
}
