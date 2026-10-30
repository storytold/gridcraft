//! Ribbon-style widgets: icon buttons, large buttons, split buttons, colour palettes, and the
//! message box / toast overlays.

use egui::{Align2, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::SheetApp;
pub use crate::code_editor::{Lang, code_editor};
use crate::i18n;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Translates a label or tooltip for the language of the frame being drawn (English if untranslated).
/// Ribbon chrome all flows through these widgets, so translating here localises it in one place.
fn tr<'a>(ui: &Ui, text: &'a str) -> &'a str {
    i18n::current(ui.ctx()).tr(text)
}

/// Keyboard shortcuts shown after a ribbon tooltip's command name: Excel for Mac's on macOS,
/// Excel for Windows' elsewhere (each one GridCraft binds). Only the name is translated.
const TOOLTIP_SHORTCUTS: &[(&str, &str, &str)] = &[
    ("Save", "⌘S", "Ctrl+S"),
    ("Undo", "⌘Z", "Ctrl+Z"),
    ("Redo", "⌘Y", "Ctrl+Y"),
    ("Paste", "⌘V", "Ctrl+V"),
    ("Cut", "⌘X", "Ctrl+X"),
    ("Copy", "⌘C", "Ctrl+C"),
    ("Bold", "⌘B", "Ctrl+B"),
    ("Italic", "⌘I", "Ctrl+I"),
    ("Underline", "⌘U", "Ctrl+U"),
    ("AutoSum", "⌘⇧T", "Alt+="),
    ("Table", "⌘T", "Ctrl+T"),
    ("Insert Link", "⌘K", "Ctrl+K"),
    ("Filter", "⌘⇧F", "Ctrl+Shift+L"),
    ("Flash Fill", "⌘E", "Ctrl+E"),
    // The Control key, written the way each platform names it. macOS's ⌃ is supplied by its
    // system fonts; elsewhere the Windows spelling avoids a glyph no bundled face has.
    ("Show Formulas", "⌃`", "Ctrl+`"),
];

/// The shortcut a ribbon tooltip shows after `tip`, for this platform (`mac`).
pub fn tooltip_shortcut(tip: &str, mac: bool) -> Option<&'static str> {
    TOOLTIP_SHORTCUTS.iter().find(|(name, _, _)| *name == tip).map(|(_, m, other)| if mac { *m } else { *other })
}

/// A ribbon tooltip: the translated command name, then its shortcut for this platform, as Excel
/// shows them ("Bold (Ctrl+B)" on Windows, "Bold (⌘B)" on macOS).
fn tip_text(ui: &Ui, tip: &str) -> String {
    // A name shared with a two-line ribbon label (Czech "Automatický\nsoučet") reads as one line.
    let name = tr(ui, tip).replace('\n', " ");
    let text = match tooltip_shortcut(tip, cfg!(target_os = "macos")) {
        Some(keys) => format!("{name} ({keys})"),
        None => name,
    };
    // A tooltip must not paint a tofu box on a host whose faces lack one of its symbols.
    if text.chars().any(|c| theme::symbol_fallback(c).is_some()) { theme::available_text(ui, &text) } else { text }
}

/// A flat square icon button with hover highlight and tooltip.
pub fn icon_button(ui: &mut Ui, icon: Icon, color: Color32, tip: &str, size: egui::Vec2) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = Tokens::get(ui.ctx());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 6.0, t.pressed);
    } else if let Some(fill) = hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        ui.painter().rect_filled(rect, 6.0, fill);
    }
    icons::paint_ribbon(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, color);
    resp.on_hover_text(tip_text(ui, tip))
}

/// Whether interface animations are on (View › Animations), as the app last published it.
pub fn animations_on(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("gridcraft.animations"))).unwrap_or(true)
}

/// A hover fill that fades in and out over a short time instead of snapping (instant when
/// animations are off). `None` when nothing should be painted.
pub fn hover_fill(ui: &Ui, id: egui::Id, hovered: bool, color: Color32) -> Option<Color32> {
    let a = if animations_on(ui.ctx()) { ui.ctx().animate_bool_with_time(id.with("hover"), hovered, 0.09) } else { f32::from(u8::from(hovered)) };
    (a > 0.0).then(|| color.gamma_multiply(a))
}

/// Height of one row in a dropdown or popup menu.
pub const MENU_ROW_H: f32 = 30.0;
/// The narrowest a menu row is drawn, so short menus still read as a card.
const MENU_ROW_MIN_W: f32 = 200.0;
/// A menu row's corner radius, and the spaces of its icon column.
const MENU_ROW_RADIUS: f32 = 8.0;
const MENU_PAD_X: f32 = 8.0;
const MENU_ICON: f32 = 16.0;
const MENU_ICON_GAP: f32 = 10.0;

/// One row of a dropdown menu, the File view's way: a 16 px icon column (left blank when the
/// command has no icon, so labels stay aligned), the label in the interface face at 12.5 px, and
/// the shortcut right-aligned in dim text. Hover and keyboard focus fill the row with a soft
/// rounded highlight.
pub fn menu_row(ui: &mut Ui, icon: Option<Icon>, label: &str, shortcut: Option<&str>) -> Response {
    menu_row_with(ui, label, shortcut, false, |p, r, ink| {
        if let Some(icon) = icon {
            icons::paint(p, r, icon, ink);
        }
    })
}

/// [`menu_row`] with its own drawing in the icon column (a border diagram, a colour sample);
/// `lead` gets the painter, the 16 px slot and the ink colour. `submenu` adds the chevron of an
/// item that opens a further menu.
pub fn menu_row_with(ui: &mut Ui, label: &str, shortcut: Option<&str>, submenu: bool, lead: impl FnOnce(&egui::Painter, Rect, Color32)) -> Response {
    let t = Tokens::get(ui.ctx());
    // Rows touch: the hover fill is the only thing that separates them.
    ui.spacing_mut().item_spacing.y = 0.0;
    let enabled = ui.is_enabled();
    let ink = if enabled { t.text } else { t.text_disabled };
    let label_g = ui.painter().layout_no_wrap(label.to_string(), theme::ui_font(12.5), ink);
    let keys_g =
        shortcut.map(|s| ui.painter().layout_no_wrap(s.to_string(), theme::ui_font(11.5), if enabled { t.text_dim } else { t.text_disabled }));
    let trailing = keys_g.as_ref().map_or(0.0, |g| g.size().x + 28.0).max(if submenu { 26.0 } else { 0.0 });
    let w = (MENU_PAD_X + MENU_ICON + MENU_ICON_GAP + label_g.size().x + trailing + MENU_PAD_X + 4.0).max(MENU_ROW_MIN_W);
    let (rect, resp) = ui.allocate_at_least(vec2(w, MENU_ROW_H), Sense::click());
    let lit = resp.hovered() || resp.has_focus();
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, MENU_ROW_RADIUS, t.pressed);
    } else if enabled && let Some(fill) = hover_fill(ui, resp.id, lit, t.hover) {
        ui.painter().rect_filled(rect, MENU_ROW_RADIUS, fill);
    }
    let slot = Rect::from_center_size(pos2(rect.left() + MENU_PAD_X + MENU_ICON / 2.0, rect.center().y), vec2(MENU_ICON, MENU_ICON));
    lead(ui.painter(), slot, ink);
    let text_x = rect.left() + MENU_PAD_X + MENU_ICON + MENU_ICON_GAP;
    ui.painter().galley(pos2(text_x, rect.center().y - label_g.size().y / 2.0), label_g, ink);
    if let Some(g) = keys_g {
        ui.painter().galley(pos2(rect.right() - MENU_PAD_X - 2.0 - g.size().x, rect.center().y - g.size().y / 2.0), g, t.text_dim);
    }
    if submenu {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - MENU_PAD_X - 5.0, rect.center().y), vec2(11.0, 11.0)),
            Icon::Right,
            t.text_dim,
        );
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    resp
}

/// A menu row that opens a submenu on hover or click (Conditional Formatting › Data Bars).
pub fn menu_submenu<R>(ui: &mut Ui, icon: Option<Icon>, label: &str, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let resp = menu_row_with(ui, label, None, true, |p, r, ink| {
        if let Some(icon) = icon {
            icons::paint(p, r, icon, ink);
        }
    });
    egui::containers::menu::SubMenu::new().show(ui, &resp, content).map(|r| r.inner)
}

/// The space between groups of menu rows: a 7 px gap with a hairline so faint it reads as air.
pub fn menu_gap(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 7.0), Sense::hover());
    let row = ui.min_rect();
    let y = r.center().y.round() + 0.5;
    ui.painter()
        .line_segment([pos2(row.left() + MENU_PAD_X, y), pos2(row.right() - MENU_PAD_X, y)], Stroke::new(1.0, t.ribbon_border.gamma_multiply(0.7)));
}

/// A small dim heading over a group in a menu or gallery (Theme Colors, Light).
pub fn menu_heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(MENU_PAD_X - 2.0);
        ui.label(egui::RichText::new(tr(ui, text)).font(theme::ui_font(11.5)).color(t.text_dim));
    });
    ui.add_space(2.0);
}

/// A palette swatch's size, the space between swatches, and the row's inset from the card edge.
const SWATCH: egui::Vec2 = vec2(18.0, 18.0);
const SWATCH_GAP: f32 = 5.0;
const SWATCH_INSET: f32 = 3.0;

/// One row of palette swatches, inset to line up with the menu rows' icons.
fn swatch_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SWATCH_GAP;
        ui.add_space(SWATCH_INSET);
        add(ui);
        ui.add_space(SWATCH_INSET);
    });
}

/// A rounded colour swatch in a palette, with a ring in the accent colour when hovered or
/// highlighted from the keyboard. `hairline` outlines light colours so they read on the card.
fn swatch(ui: &mut Ui, color: Color32, highlighted: bool, size: egui::Vec2) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 4.0, color);
    // Pale colours on a pale card (and dark ones on a dark card) get an edge so they stay visible.
    let card = t.menu_bg;
    if theme::contrast(color, card) < 1.35 {
        p.rect_stroke(r, 4.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    }
    hover_ring(ui, resp.id, r, 4.0, resp.hovered() || highlighted);
    resp
}

/// The ring around a hovered gallery tile or swatch: a hair of card colour, then the accent,
/// following the tile's `radius`. It fades in and out with the hover (instant without animations).
pub fn hover_ring(ui: &Ui, id: egui::Id, r: Rect, radius: f32, on: bool) {
    let t = Tokens::get(ui.ctx());
    let a = if animations_on(ui.ctx()) { ui.ctx().animate_bool_with_time(id.with("ring"), on, 0.08) } else { f32::from(u8::from(on)) };
    if a > 0.0 {
        let p = ui.painter();
        p.rect_stroke(r.expand(1.0), radius + 1.0, Stroke::new(1.5, t.menu_bg.gamma_multiply(a)), StrokeKind::Outside);
        p.rect_stroke(r.expand(2.5), radius + 2.5, Stroke::new(1.5, t.accent.gamma_multiply(a)), StrokeKind::Outside);
    }
}

/// The sheet card's edge: a hairline with 10 px rounded corners on top (`top`) or at the bottom,
/// the corners outside the curve painted back in the window colour so the content under them
/// (the grid) reads as clipped to the card.
pub fn card_frame(p: &egui::Painter, r: Rect, t: &Tokens, top: bool) {
    const R: f32 = 10.0;
    let corners: [(egui::Pos2, f32, f32); 2] =
        if top { [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0)] } else { [(r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] };
    for (c, sx, sy) in corners {
        // The wedge between the corner and the quarter circle.
        let mut pts = vec![c, pos2(c.x + sx * R, c.y)];
        for k in 0..=8 {
            let a = std::f32::consts::FRAC_PI_2 * k as f32 / 8.0;
            pts.push(pos2(c.x + sx * (R - R * a.sin()), c.y + sy * (R - R * a.cos())));
        }
        pts.push(pos2(c.x, c.y + sy * R));
        p.add(egui::Shape::Path(egui::epaint::PathShape { points: pts, closed: true, fill: t.window, stroke: egui::epaint::PathStroke::NONE }));
    }
    // No outline: the card reads by its fill against the window.
}

/// The fill of a dropdown box in the ribbon: a touch off the card's white, so the box reads as a
/// field without a heavy border.
pub fn field_fill(t: &Tokens) -> Color32 {
    t.input_bg.lerp_to_gamma(t.window, 0.35)
}

/// A ribbon dropdown box (Font, Number Format): its value on the left and a chevron on the right,
/// in a softly rounded field. Returns the click response the dropdown anchors to.
pub fn combo_box(ui: &mut Ui, text: &str, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let border = if resp.hovered() { t.input_border } else { t.ribbon_border };
    ui.painter().rect_filled(rect, 6.0, field_fill(&t));
    ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let font = crate::theme::ui_font(12.0);
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, t.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width - 34.0);
    let galley = ui.painter().layout_job(job);
    let pos = pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(12.0, 12.0)), Icon::Chevron, t.text_dim);
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, text));
    resp
}

/// A button for a character the interface inserts (the Symbol gallery). When the current font set
/// has no glyph for it, the code-drawn icon with the same meaning is painted instead of a tofu
/// box. Only the button's appearance changes: clicking still inserts the exact requested character.
pub fn glyph_button(ui: &mut Ui, ch: char, tip: &str) -> Response {
    let font = theme::ui_font(13.0);
    let has_glyph = ui.ctx().fonts_mut(|f| f.has_glyph(&font, ch));
    if !has_glyph && let Some(icon) = icons::fallback_for_symbol(ch) {
        return icon_button(ui, icon, Tokens::get(ui.ctx()).text, tip, vec2(24.0, 22.0));
    }
    // An unknown future character without a fallback icon still gets a readable label.
    let label = if has_glyph { ch.to_string() } else { format!("U+{:04X}", ch as u32) };
    ui.button(label).on_hover_text(tip_text(ui, tip))
}

/// A ribbon checkbox: a 14 px rounded square that fills with the accent and a white tick when on,
/// with the label in the UI font beside it. Behaves like `Ui::checkbox` (toggles on click, marks the
/// response changed, carries the checked state for assistive technology).
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: impl Into<egui::WidgetText>) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let text_color = if enabled { t.text } else { t.text_disabled };
    let galley = label.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let size = vec2(14.0 + 7.0 + galley.size().x + 4.0, 22.0f32.max(galley.size().y));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let text = galley.text().to_string();
    let checked = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, checked, &text));
    let p = ui.painter();
    if let Some(fill) = hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        p.rect_filled(rect.expand2(vec2(4.0, 0.0)), 6.0, fill);
    }
    let bx = Rect::from_center_size(pos2(rect.left() + 7.0, rect.center().y), vec2(14.0, 14.0));
    let on_t = ui.ctx().animate_bool_with_time(resp.id.with("on"), checked, if animations_on(ui.ctx()) { 0.12 } else { 0.0 });
    if on_t > 0.0 {
        let fill = if enabled { t.accent } else { t.text_disabled };
        p.rect_filled(bx, 4.0, t.input_bg.lerp_to_gamma(fill, on_t));
    } else {
        p.rect_filled(bx, 4.0, t.input_bg);
    }
    if on_t < 1.0 {
        let border = if resp.hovered() && enabled { t.text_dim } else { t.input_border };
        p.rect_stroke(bx, 4.0, Stroke::new(1.0, border.gamma_multiply(1.0 - on_t)), StrokeKind::Inside);
    }
    if on_t > 0.0 {
        // Dark ink on the bright dark-theme accent, white on the deep light-theme one.
        let tick = if t.dark { t.window } else { Color32::WHITE }.gamma_multiply(on_t);
        let pts = [pos2(bx.left() + 3.5, bx.center().y + 0.2), pos2(bx.left() + 6.0, bx.bottom() - 4.0), pos2(bx.right() - 3.3, bx.top() + 4.2)];
        p.line_segment([pts[0], pts[1]], Stroke::new(1.6, tick));
        p.line_segment([pts[1], pts[2]], Stroke::new(1.6, tick));
        p.circle_filled(pts[1], 0.8, tick);
    }
    p.galley(pos2(bx.right() + 7.0, rect.center().y - galley.size().y / 2.0), galley, text_color);
    resp
}

/// A toggle-able small button (e.g. Bold) showing a checked state.
pub fn toggle_button(ui: &mut Ui, icon: Icon, on: bool, tip: &str) -> Response {
    // As tall as the row it sits in: 26 px in a two-row group, 18 in a three-row stack.
    let h = ui.spacing().interact_size.y.clamp(SMALL_H, 26.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, h), Sense::click());
    let t = Tokens::get(ui.ctx());
    if on {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    } else if let Some(fill) = hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    icons::paint_ribbon(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, t.text);
    resp.on_hover_text(tip_text(ui, tip))
}

/// Large ribbon buttons' icon size and the icon's centre below the button top.
const BIG_ICON: f32 = 20.0;
/// Height of a large ribbon button (the streamlined ribbon's 56 px controls row).
const BIG_H: f32 = 56.0;
/// Height of a row in a three-row stack of small buttons.
const SMALL_H: f32 = 18.0;

/// Where a large button's icon centre sits: higher when its label takes two lines.
fn big_icon_y(lines: usize) -> f32 {
    if lines > 1 { 14.0 } else { 19.0 }
}

/// Shorter labels for the ribbon's large buttons, by their full (English) name; the full name
/// stays the tooltip and the accessible name.
const SHORT_LABELS: &[(&str, &str)] = &[
    ("Get Data (Text/CSV)", "Import"),
    ("Remove Duplicates", "Dedupe"),
    ("Data Validation", "Validation"),
    ("Text to Columns", "Split\nText"),
    ("What-If Analysis", "What-If"),
    ("Forecast Sheet", "Forecast"),
    ("Data Analysis", "Analysis"),
    ("Recommended Charts", "Charts"),
    ("Header & Footer", "Header"),
    ("Insert Function", "Function"),
    ("Name Manager", "Names"),
    ("Watch Window", "Watch"),
    ("Calculation Options", "Calculation"),
    ("Lookup & Reference", "Lookup"),
    ("More Functions", "More"),
    ("Workbook Statistics", "Statistics"),
    ("Check Accessibility", "Accessibility"),
    ("New Comment", "Comment"),
    ("Show Comments", "Comments"),
    ("Delete Comment", "Delete"),
    ("Protect Sheet", "Protect"),
    ("Protect Workbook", "Lock\nBook"),
    ("Allow Edit Ranges", "Edit\nRanges"),
    ("Page Break Preview", "Page\nBreaks"),
    ("Zoom to Selection", "Fit"),
    ("New Window", "Window"),
    ("Arrange All", "Arrange"),
    ("Freeze Panes", "Freeze"),
    ("Switch Windows", "Switch"),
    ("Print Titles", "Titles"),
    ("Sheet Right-to-Left", "Right to\nLeft"),
    ("Select Objects", "Select"),
    ("Command Palette", "Commands"),
    ("Agent Control", "Agents"),
    ("Action Journal", "Journal"),
    ("Check for Updates…", "Updates"),
    ("About GridCraft", "About"),
    ("Run Macro", "Run"),
    ("Resize Table", "Resize"),
    ("Convert to Range", "To Range"),
    ("Insert Slicer", "Slicer"),
    ("Add Chart Element", "Elements"),
    ("Switch Row/Column", "Switch"),
    ("Change Chart Type", "Type"),
    ("Edit Chart Title", "Title"),
    ("Delete Chart", "Delete"),
];

/// The label a large button shows: its short form in English, the translation otherwise.
fn shown_label(ui: &Ui, label: &str) -> String {
    let full = label.replace('\n', " ");
    if crate::i18n::current(ui.ctx()) == crate::i18n::Language::En
        && let Some((_, short)) = SHORT_LABELS.iter().find(|(f, _)| *f == full)
    {
        return (*short).to_string();
    }
    tr(ui, label).to_string()
}

/// Large ribbon button: 20 px icon over a one- or two-line label.
pub fn big_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let full = tr(ui, label).replace('\n', " ");
    let shown = shown_label(ui, label);
    let label = shown.as_str();
    let (lines, w) = big_label(ui, label, dropdown);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, BIG_H), Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 5.0, t.pressed);
    } else if let Some(fill) = hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        ui.painter().rect_filled(rect, 5.0, fill);
    }
    icons::paint_ribbon(
        ui.painter(),
        Rect::from_center_size(pos2(rect.center().x, rect.top() + big_icon_y(lines.len())), vec2(BIG_ICON, BIG_ICON)),
        icon,
        t.text,
    );
    paint_big_label(ui, rect, &lines, dropdown);
    let name = full;
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
    resp.on_hover_text(tip_text(ui, tip))
}

/// Large split button (Formulas › AutoSum): the icon runs the default action, the label and its
/// arrow open the menu. Returns the (main, arrow) responses; their accessible names are the label
/// and "<label> options".
pub fn big_split_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str) -> (Response, Response) {
    let t = Tokens::get(ui.ctx());
    let full = tr(ui, label).replace('\n', " ");
    let shown = shown_label(ui, label);
    let label = shown.as_str();
    let (lines, w) = big_label(ui, label, true);
    let (rect, _) = ui.allocate_exact_size(vec2(w, BIG_H), Sense::hover());
    let split = rect.top() + big_icon_y(lines.len()) + 13.0;
    let main_rect = Rect::from_min_max(rect.min, pos2(rect.right(), split));
    let arrow_rect = Rect::from_min_max(pos2(rect.left(), split), rect.max);
    let main = ui.interact(main_rect, ui.scope_id().with((label, "big-split-main")), Sense::click());
    let arrow = ui.interact(arrow_rect, ui.scope_id().with((label, "big-split-arrow")), Sense::click());
    if main.hovered() || arrow.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.hover);
        ui.painter().line_segment([pos2(rect.left() + 4.0, split), pos2(rect.right() - 4.0, split)], Stroke::new(1.0, t.separator));
    }
    for (part, r) in [(&main, main_rect), (&arrow, arrow_rect)] {
        if part.is_pointer_button_down_on() {
            ui.painter().rect_filled(r, 5.0, t.pressed);
        }
    }
    icons::paint_ribbon(
        ui.painter(),
        Rect::from_center_size(pos2(rect.center().x, rect.top() + big_icon_y(lines.len())), vec2(BIG_ICON, BIG_ICON)),
        icon,
        t.text,
    );
    paint_big_label(ui, rect, &lines, true);
    let name = full;
    let enabled = ui.is_enabled();
    main.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
    arrow.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, format!("{name} options")));
    let tip = tip_text(ui, tip);
    (main.on_hover_text(tip.clone()), arrow.on_hover_text(tip))
}

/// Size of the dropdown arrow beside a big button's last label line.
const BIG_ARROW: f32 = 10.0;

/// A big button's label lines, laid out, and the width the button needs for them.
fn big_label(ui: &Ui, label: &str, dropdown: bool) -> (Vec<std::sync::Arc<egui::Galley>>, f32) {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(11.5);
    // One line, unless it would be long (then the button stays narrow with two), or a dropdown's
    // arrow follows a second line.
    let joined = label.replace('\n', " ");
    let joined_w = ui.painter().layout_no_wrap(joined.clone(), font.clone(), t.text).size().x;
    let label = if dropdown || joined_w > 60.0 { label } else { joined.as_str() };
    let lines: Vec<_> = label.split('\n').map(|line| ui.painter().layout_no_wrap(line.to_string(), font.clone(), t.text)).collect();
    // No caret: a button with a menu shows it when clicked.
    let _ = (dropdown, BIG_ARROW);
    let arrow_space = 0.0;
    let text_w = lines.iter().enumerate().map(|(i, line)| line.size().x + if i + 1 == lines.len() { arrow_space } else { 0.0 }).fold(0.0, f32::max);
    (lines, (text_w + 12.0).max(48.0))
}

/// Paints a big button's label lines in `rect`, with the dropdown arrow beside the last one.
fn paint_big_label(ui: &Ui, rect: Rect, lines: &[std::sync::Arc<egui::Galley>], dropdown: bool) {
    let t = Tokens::get(ui.ctx());
    // No caret: a button with a menu shows it when clicked.
    let _ = (dropdown, BIG_ARROW);
    let arrow_space = 0.0;
    for (i, line) in lines.iter().enumerate() {
        let has_arrow = false;
        let line_w = line.size().x + if has_arrow { arrow_space } else { 0.0 };
        let left = rect.center().x - line_w / 2.0;
        let y = if lines.len() > 1 { rect.top() + 33.0 + i as f32 * 14.0 } else { rect.top() + 43.0 };
        ui.painter().galley(pos2(left, y - line.size().y / 2.0), line.clone(), t.text);
        if has_arrow {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(left + line_w - BIG_ARROW / 2.0, y + 1.0), vec2(BIG_ARROW, BIG_ARROW)),
                Icon::Chevron,
                t.text_dim,
            );
        }
    }
}

/// Small ribbon button: 16px icon with optional label to the right.
pub fn small_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(12.0);
    let label = tr(ui, label);
    let tw = if label.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x + 6.0 };
    let w = 24.0 + tw;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, SMALL_H), Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
    } else if let Some(fill) = hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    icons::paint_ribbon(ui.painter(), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0)), icon, t.text);
    if !label.is_empty() {
        ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, label, font, t.text);
    }
    // No caret: the menu opens on click.
    let _ = dropdown;
    // The accessible name is the label, or the tooltip for an icon-only button.
    let name = if label.is_empty() { tr(ui, tip) } else { label };
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    resp.on_hover_text(tip_text(ui, tip))
}

/// Small labelled toggle button (Use Relative References): drawn pressed while `on`. Its
/// accessible role is a checkbox, selected while `on`.
pub fn small_toggle_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(12.0);
    let label = tr(ui, label);
    let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x + 6.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0 + tw, SMALL_H), Sense::click());
    if on || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    } else if let Some(fill) = hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    let color = if ui.is_enabled() { t.text } else { t.text_disabled };
    icons::paint_ribbon(ui.painter(), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0)), icon, color);
    ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, label, font, color);
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label));
    resp.on_hover_text(tip_text(ui, tip))
}

/// A split button: main part runs the default action, the arrow opens a menu.
/// Returns (main clicked, arrow response).
pub fn split_button(ui: &mut Ui, icon: Icon, accent: Option<Color32>, tip: &str) -> (bool, Response) {
    let t = Tokens::get(ui.ctx());
    // The arrow half is narrow: its caret shows only under the pointer.
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 24.0), Sense::hover());
    let main = Rect::from_min_size(rect.min, vec2(25.0, 24.0));
    let arrow = Rect::from_min_max(pos2(main.right(), rect.top()), rect.max);
    let m = ui.interact(main, ui.scope_id().with((tip, "main")), Sense::click());
    let a = ui.interact(arrow, ui.scope_id().with((tip, "arrow")), Sense::click());
    if m.hovered() || a.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
        ui.painter().line_segment([pos2(main.right(), rect.top() + 3.0), pos2(main.right(), rect.bottom() - 3.0)], Stroke::new(1.0, t.separator));
    }
    let icon_rect = Rect::from_center_size(main.center(), vec2(16.0, 16.0));
    icons::paint_ribbon(ui.painter(), icon_rect, icon, t.text);
    if let Some(c) = accent {
        // The current colour over the icon's own colour bar (24-unit grid: x 2.5..21.5, y 19..22.5).
        let s = icon_rect.width() / 24.0;
        let bar = Rect::from_min_max(icon_rect.min + vec2(2.5 * s, 19.0 * s), icon_rect.min + vec2(21.5 * s, 22.5 * s));
        ui.painter().rect_filled(bar, s, c);
    }
    // The arrow half shows its caret only under the pointer.
    if m.hovered() || a.hovered() {
        icons::paint(ui.painter(), Rect::from_center_size(arrow.center(), vec2(10.0, 10.0)), Icon::Chevron, t.text_dim);
    }
    (m.on_hover_text(tip_text(ui, tip)).clicked(), a)
}

/// A small ribbon split button with a label (Error Checking): the icon and label run the default
/// action, the arrow at the right opens a menu. Returns (main response, arrow response).
pub fn small_split_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str) -> (Response, Response) {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(12.0);
    let label = tr(ui, label);
    let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x + 6.0;
    let (rect, _) = ui.allocate_exact_size(vec2(24.0 + tw + 11.0, SMALL_H), Sense::hover());
    let main = Rect::from_min_size(rect.min, vec2(24.0 + tw, rect.height()));
    let arrow = Rect::from_min_max(pos2(main.right(), rect.top()), rect.max);
    let m = ui.interact(main, ui.scope_id().with((tip, "main")), Sense::click());
    let a = ui.interact(arrow, ui.scope_id().with((tip, "arrow")), Sense::click());
    let name = label.replace('\n', " ");
    let enabled = ui.is_enabled();
    m.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
    a.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, format!("{name} options")));
    if m.hovered() || a.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
        for (part, r) in [(&m, main), (&a, arrow)] {
            if part.is_pointer_button_down_on() {
                ui.painter().rect_filled(r, 4.0, t.pressed);
            }
        }
        ui.painter().line_segment([pos2(main.right(), rect.top() + 3.0), pos2(main.right(), rect.bottom() - 3.0)], Stroke::new(1.0, t.separator));
    }
    icons::paint_ribbon(ui.painter(), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0)), icon, t.text);
    ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, label, font, t.text);
    if m.hovered() || a.hovered() {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(arrow.center().x, rect.center().y + 1.0), vec2(10.0, 10.0)),
            Icon::Chevron,
            t.text_dim,
        );
    }
    let tip = tip_text(ui, tip);
    (m.on_hover_text(tip.clone()), a.on_hover_text(tip))
}

/// A one-line text field for a value stored in the workbook, which callers reload every frame. The text being typed is
/// kept in egui memory until the field loses focus, then returned to commit (`None` when Escape cancels the edit).
pub fn committed_text(ui: &mut Ui, id: egui::Id, current: &str, width: Option<f32>) -> Option<String> {
    let width = width.unwrap_or(ui.spacing().text_edit_width);
    let draft = id.with("draft");
    let mut text = ui.data_mut(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(width));
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(draft));
        return (!ui.input(|i| i.key_pressed(egui::Key::Escape))).then_some(text);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(draft));
    }
    None
}

/// How many custom colours the palette's Recent Colors row remembers.
pub const RECENT_COLORS: usize = 10;

/// `#RRGGBB` for a colour.
pub fn hex_of(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// A colour typed as `#RRGGBB`, `RRGGBB`, `#RGB` or `RGB` (any case, spaces around it ignored).
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    let digits: Vec<u32> = t.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
    let byte = |hi: u32, lo: u32| u8::try_from(hi * 16 + lo).ok();
    match digits[..] {
        [r, g, b] => Some([byte(r, r)?, byte(g, g)?, byte(b, b)?]),
        [r1, r2, g1, g2, b1, b2] => Some([byte(r1, r2)?, byte(g1, g2)?, byte(b1, b2)?]),
        _ => None,
    }
}

/// Puts `hex` first in the recent colours, once, keeping at most [`RECENT_COLORS`].
pub fn remember_color(recent: &mut Vec<String>, hex: &str) {
    let hex = hex.to_ascii_uppercase();
    recent.retain(|h| *h != hex);
    recent.insert(0, hex);
    recent.truncate(RECENT_COLORS);
}

/// Returns the stable id for the recent-colors cache.
fn recent_colors_id() -> egui::Id {
    egui::Id::unique("gridcraft.recent_colors")
}

/// The More Colors… picker's state while it is open in a palette.
#[derive(Clone)]
struct MoreColors {
    hsva: egui::ecolor::Hsva,
    hex: String,
}

impl MoreColors {
    fn new(rgb: [u8; 3]) -> Self {
        Self { hsva: egui::ecolor::Hsva::from_srgb(rgb), hex: hex_of(rgb) }
    }
}

/// Our colour palette (theme row + tints + standard colours, then Recent Colors and More Colors…),
/// returns a picked hex colour, `Some("none")` for No Fill/Automatic. Arrow keys move a highlight
/// through the swatches and Enter applies; the key events are stripped so the grid behind the popup
/// does not also react (not while More Colors… is open, whose boxes take the keys).
pub fn color_palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    palette(ui, theme_colors, none_label, true)
}

/// [`color_palette`] without More Colors…, for a menu that closes on any click inside it (a sub-menu
/// of a context menu), where the picker would vanish as soon as it is used. Recent Colors stays.
pub fn color_palette_fixed(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str) -> Option<String> {
    palette(ui, theme_colors, none_label, false)
}

/// Shared body of [`color_palette`] and [`color_palette_fixed`]: a 7×10 palette (six tinted theme rows
/// above ten standard colours), then Recent Colors; the separator and More Colors… button appear only
/// when `more_colors` is true; an existing picker state renders while open. Arrow keys move a highlight
/// through all 70 swatches (rows clamp, columns
/// wrap), Enter picks it, and those key events are stripped so the grid behind does not react; while More
/// Colors… is open its HSVA/hex widgets take the keys instead. The None label returns `Some("none")`;
/// otherwise the result is the picked hex or `None`. Highlight, picker state and recent colours live in
/// egui temp storage, and More Colors… OK falls back to the picker colour for an invalid typed hex and
/// records the result as the newest recent colour (capped at `RECENT_COLORS`).
fn palette(ui: &mut Ui, theme_colors: &[u32; 12], none_label: &str, more_colors: bool) -> Option<String> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    const ROWS: usize = 7; // 6 tint rows + 1 standard row
    const COLS: usize = 10;
    let pal_id = ui.scope_id().with("__color_palette");
    let mut cur: Option<(usize, usize)> = ui.data_mut(|d| d.get_temp(pal_id)).unwrap_or(None);
    let more_id = pal_id.with("more");
    let mut more: Option<MoreColors> = ui.data_mut(|d| d.get_temp(more_id)).unwrap_or(None);
    let (mut dl, mut dr, mut du, mut dd, mut ent) = (false, false, false, false, false);
    if more.is_none() {
        ui.input(|i| {
            dl = i.key_pressed(egui::Key::ArrowLeft);
            dr = i.key_pressed(egui::Key::ArrowRight);
            du = i.key_pressed(egui::Key::ArrowUp);
            dd = i.key_pressed(egui::Key::ArrowDown);
            ent = i.key_pressed(egui::Key::Enter);
        });
    }
    if dl || dr || du || dd {
        cur = match cur {
            None => Some((
                0,
                if dr {
                    1
                } else if dl {
                    COLS - 1
                } else {
                    0
                },
            )),
            Some((r, c)) => {
                let r2 = if du {
                    r.saturating_sub(1)
                } else if dd {
                    (r + 1).min(ROWS - 1)
                } else {
                    r
                };
                let c2 = if dl {
                    (c + COLS - 1) % COLS
                } else if dr {
                    (c + 1) % COLS
                } else {
                    c
                };
                Some((r2, c2))
            }
        };
    }
    if dl || dr || du || dd || ent {
        ui.ctx().input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Enter,
                        pressed: true,
                        ..
                    }
                )
            });
        });
    }
    let theme_hex = |base: u32, tint: f64| -> String {
        let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
        let c = gridcraft_engine::model::style::apply_tint(rgb, tint);
        let (r0, r1, r2) = (c[0], c[1], c[2]);
        format!("#{r0:02X}{r1:02X}{r2:02X}")
    };
    let std_hex = ["#C00000", "#FF0000", "#FFC000", "#FFFF00", "#92D050", "#00B050", "#00B0F0", "#0070C0", "#002060", "#7030A0"];
    if ent && let Some((r, c)) = cur {
        picked = Some(if r < ROWS - 1 {
            let base = theme_colors.get(c).copied().unwrap_or(0);
            theme_hex(base, [0.0f64, 0.8, 0.6, 0.4, -0.25, -0.5][r])
        } else {
            std_hex[c].to_string()
        });
        ui.data_mut(|d| d.remove_temp::<Option<(usize, usize)>>(pal_id));
    }
    let danger = t.danger;
    let none_lead = move |p: &egui::Painter, r: Rect, ink: Color32| {
        // An empty swatch struck through: no colour.
        let sw = r.shrink(1.0);
        p.rect_stroke(sw, 3.0, Stroke::new(1.0, ink.gamma_multiply(0.6)), StrokeKind::Inside);
        p.line_segment([sw.left_bottom() + vec2(2.5, -2.5), sw.right_top() + vec2(-2.5, 2.5)], Stroke::new(1.2, danger));
    };
    if menu_row_with(ui, tr(ui, none_label), None, false, none_lead).clicked() {
        picked = Some("none".to_string());
    }
    menu_heading(ui, "Theme Colors");
    let order = [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let tints = [0.0f64, 0.8, 0.6, 0.4, -0.25, -0.5];
    for (row, tint) in tints.iter().enumerate() {
        // The theme colours themselves, then their tints and shades a little apart below.
        ui.add_space(match row {
            0 => 0.0,
            1 => SWATCH_GAP + 3.0,
            _ => SWATCH_GAP,
        });
        swatch_row(ui, |ui| {
            for (col, &i) in order.iter().enumerate() {
                let base = theme_colors.get(i).copied().unwrap_or(0);
                let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
                let c = gridcraft_engine::model::style::apply_tint(rgb, *tint);
                if swatch(ui, Color32::from_rgb(c[0], c[1], c[2]), cur == Some((row, col)), SWATCH).clicked() {
                    picked = Some(theme_hex(base, *tint));
                }
            }
        });
    }
    menu_heading(ui, "Standard Colors");
    swatch_row(ui, |ui| {
        for (col, hex) in std_hex.iter().enumerate() {
            let c = gridcraft_engine::model::Color::from_hex(hex)
                .and_then(|c| c.resolve(&Default::default()))
                .map(theme::color32)
                .unwrap_or(Color32::BLACK);
            if swatch(ui, c, cur == Some((ROWS - 1, col)), SWATCH).clicked() {
                picked = Some(hex.to_string());
            }
        }
    });
    // Recent Colors: the custom colours picked below, newest first.
    let mut recent: Vec<String> = ui.ctx().data(|d| d.get_temp(recent_colors_id())).unwrap_or_default();
    if !recent.is_empty() {
        menu_heading(ui, "Recent Colors");
        swatch_row(ui, |ui| {
            for hex in &recent {
                let c = parse_hex(hex).map_or(Color32::BLACK, |c| Color32::from_rgb(c[0], c[1], c[2]));
                if swatch(ui, c, false, SWATCH).on_hover_text(hex).clicked() {
                    picked = Some(hex.clone());
                }
            }
        });
    }
    ui.add_space(6.0);
    if more_colors {
        menu_gap(ui);
    }
    if more_colors && menu_row(ui, Some(Icon::Theme), tr(ui, "More Colors…"), None).clicked() {
        let start = recent.first().and_then(|h| parse_hex(h)).unwrap_or([0x44, 0x72, 0xC4]);
        more = Some(MoreColors::new(start));
    }
    if let Some(m) = more.as_mut() {
        let mut done = false;
        // The saturation / value area with its hue bar and egui's red-green-blue boxes, then the hex
        // box with the new colour beside it.
        if egui::color_picker::color_picker_hsva_2d(ui, &mut m.hsva, egui::color_picker::Alpha::Opaque) {
            m.hex = hex_of(m.hsva.to_srgb());
        }
        ui.horizontal(|ui| {
            let label = ui.label("Hex");
            let r = ui.add(egui::TextEdit::singleline(&mut m.hex).desired_width(72.0).char_limit(7)).labelled_by(label.id);
            if r.changed()
                && let Some(c) = parse_hex(&m.hex)
            {
                m.hsva = egui::ecolor::Hsva::from_srgb(c);
            }
            let c = m.hsva.to_srgb();
            let (sw, _) = ui.allocate_exact_size(vec2(36.0, 18.0), Sense::hover());
            ui.painter().rect_filled(sw, 1.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.painter().rect_stroke(sw, 1.0, Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
            ui.label(egui::RichText::new("New").small());
        });
        ui.horizontal(|ui| {
            if ui.button("  OK  ").clicked() {
                // A hex still being typed that is not a colour falls back to the picker's.
                let c = parse_hex(&m.hex).unwrap_or_else(|| m.hsva.to_srgb());
                let hex = hex_of(c);
                remember_color(&mut recent, &hex);
                picked = Some(hex);
                done = true;
            }
            if ui.button("Cancel").clicked() {
                done = true;
            }
        });
        if done {
            more = None;
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(recent_colors_id(), recent));
    ui.data_mut(|d| d.insert_temp(more_id, more));
    ui.data_mut(|d| d.insert_temp(pal_id, cur));
    picked
}

/// The card every dialog is drawn in: the menu colour, 14 px corners, a hairline and a soft, wide
/// shadow, with 20 px of padding (the title row and the buttons sit inside it).
pub fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    let t = Tokens::get(ctx);
    egui::Frame::new()
        .fill(t.menu_bg)
        .corner_radius(14)
        .stroke(Stroke::new(1.0, t.ribbon_border))
        .shadow(egui::epaint::Shadow { offset: [0, 12], blur: 40, spread: 0, color: t.shadow })
        .inner_margin(egui::Margin { left: 22, right: 22, top: 18, bottom: 18 })
}

/// A dialog's title row: the title in bold on the left and a quiet close icon on the right, with
/// no band behind either. Returns true when the close icon was clicked.
pub fn dialog_header(ui: &mut Ui, title: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut closed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).font(theme::ui_bold(15.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            closed = icon_button(ui, Icon::Close, t.text_dim, "Close", vec2(24.0, 24.0)).clicked();
        });
    });
    ui.add_space(10.0);
    closed
}

/// A dialog button: 30 px tall with 8 px corners. The primary one is the accent colour; the
/// others are a soft field that lifts on hover, with no outline.
pub fn dialog_button(ui: &mut Ui, label: &str, primary: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(13.0);
    let ink = if primary { t.on_accent } else { t.text };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(vec2((galley.size().x + 32.0).max(72.0), 30.0), Sense::click());
    let rest = if primary { t.accent } else { t.hover };
    let fill =
        if resp.is_pointer_button_down_on() || resp.hovered() || resp.has_focus() { if primary { t.accent_dark } else { t.pressed } } else { rest };
    ui.painter().rect_filled(rect, 8.0, fill);
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), 10.0, Stroke::new(1.5, t.accent), StrokeKind::Outside);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    resp
}

/// Draws the modal warning/error message; OK, Enter or Esc clears it and restores editor focus when an edit exists.
pub fn message_box(app: &mut SheetApp, ctx: &egui::Context) {
    let Some((title, msg)) = app.message.clone() else { return };
    let title = app.ui.language.tr(&title).to_owned();
    let msg = app.ui.language.tr_error(&msg);
    let mut close = false;
    egui::Modal::new(egui::Id::unique("message_box")).frame(dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(360.0);
        let t = Tokens::get(ui.ctx());
        ui.horizontal_top(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 14.0, t.warning);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "!", theme::ui_bold(17.0), t.on_warning);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(title).font(theme::ui_bold(15.0)).color(t.text));
                ui.add_space(4.0);
                ui.add(egui::Label::new(egui::RichText::new(msg).color(t.text_dim)).wrap());
            });
        });
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if dialog_button(ui, "OK", true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if close {
        app.message = None;
        // Back to the entry the message was about (a validation alert, a formula error), so it can
        // be fixed from the keyboard without clicking the cell again.
        if let Some(ed) = &mut app.editor {
            ed.request_focus = true;
        }
    }
}

/// Shows a translated toast for 2.5 seconds and clears it after the deadline.
pub fn toast(app: &mut SheetApp, ctx: &egui::Context) {
    let Some((msg, at)) = app.toast.clone() else { return };
    if crate::now_ms() - at > 2500.0 {
        app.toast = None;
        return;
    }
    let msg = app.ui.language.tr_error(&msg);
    let Some(rect) = app.grid.rect else { return };
    egui::Area::new(egui::Id::unique("toast")).fixed_pos(pos2(rect.center().x - 160.0, rect.bottom() - 48.0)).order(egui::Order::Tooltip).show(
        ctx,
        |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(320.0);
                ui.label(msg);
            });
        },
    );
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
}

/// The info bar under the ribbon (Excel's message-bar slot). Shows an open-time notice with an
/// optional action button and a dismiss ✕. Returns nothing; dismissing clears `app.notice`.
pub fn notice_bar(app: &mut SheetApp, ui: &mut Ui) {
    let Some(notice) = app.notice.clone() else { return };
    let t = Tokens::get(ui.ctx());
    let (bg, border) = match notice.kind {
        crate::NoticeKind::Warning => (t.notice_warn_bg, t.notice_warn_border),
        crate::NoticeKind::Info => (t.notice_info_bg, t.notice_info_border),
    };
    let mut dismiss = false;
    let mut action: Option<(String, String)> = None;
    egui::Panel::top("notice_bar").show_separator_line(false).frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::symmetric(12, 0))).show(
        ui,
        |ui| {
            // Bottom border as a 1px strip spanning the bar.
            let bar = ui.max_rect();
            ui.painter().rect_stroke(bar, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
            ui.horizontal_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(
                    ui.painter(),
                    r,
                    match notice.kind {
                        crate::NoticeKind::Warning => Icon::Lock,
                        crate::NoticeKind::Info => Icon::Note,
                    },
                    border,
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&notice.text).font(theme::ui_font(12.5)).color(t.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, t.text_dim, "Dismiss", vec2(20.0, 20.0)).clicked() {
                        dismiss = true;
                    }
                    if let Some((label, dialog)) = &notice.action
                        && ui.button(egui::RichText::new(label).font(theme::ui_font(12.5))).clicked()
                    {
                        action = Some((label.clone(), dialog.clone()));
                    }
                });
            });
        },
    );
    if let Some((_, dialog)) = action {
        app.open_dialog(&dialog, serde_json::json!({}));
    }
    if dismiss {
        app.notice = None;
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn hex_is_read_in_the_forms_people_type() {
        assert_eq!(parse_hex("#1A2b3C"), Some([0x1A, 0x2B, 0x3C]));
        assert_eq!(parse_hex(" 1a2b3c "), Some([0x1A, 0x2B, 0x3C]));
        assert_eq!(parse_hex("#f80"), Some([0xFF, 0x88, 0x00]));
        for bad in ["", "#", "#12", "#1234", "#12345", "#1234567", "#12345G", "##123456", "é12345"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert_eq!(hex_of([0, 10, 255]), "#000AFF");
    }

    #[test]
    fn recent_colours_keep_the_newest_first_once_each_and_at_most_ten() {
        let mut r = Vec::new();
        for i in 0..12u8 {
            remember_color(&mut r, &hex_of([i, 0, 0]));
        }
        assert_eq!(r.len(), RECENT_COLORS);
        assert_eq!(r[0], "#0B0000");
        remember_color(&mut r, "#050000");
        remember_color(&mut r, "#0b0000");
        assert_eq!(&r[..2], ["#0B0000", "#050000"]);
        assert_eq!(r.iter().filter(|h| *h == "#050000").count(), 1);
        assert_eq!(r.len(), RECENT_COLORS);
    }
}

#[cfg(test)]
mod glyph_button_tests {
    use super::*;

    #[test]
    /// The `✓` gallery button paints vector paths instead of the missing character.
    fn a_symbol_with_no_bundled_glyph_paints_a_code_icon() {
        // ✓ has no bundled egui face. The gallery button must paint Icon::Check (vector paths),
        // not the missing character, without weakening the font or accessibility policy.
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::default());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = glyph_button(ui, '✓', "Insert check mark");
            assert!(response.rect.width() > 0.0);
        });
        out.textures_delta.clear();
        assert!(!out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains('✓'))));
        assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Path(_))), "the code-drawn check must paint vector paths");
    }
}

#[cfg(test)]
mod tooltip_shortcut_tests {
    use super::tooltip_shortcut;

    #[test]
    /// Windows/Linux tooltips use Ctrl/Alt shortcuts and macOS uses Command glyphs; a command without a shortcut reports none.
    fn tooltips_show_each_platforms_own_shortcut() {
        // #1191: Windows and Linux show Excel for Windows' shortcuts, never the Mac ⌘ ones.
        assert_eq!(tooltip_shortcut("Bold", false), Some("Ctrl+B"));
        assert_eq!(tooltip_shortcut("AutoSum", false), Some("Alt+="));
        assert_eq!(tooltip_shortcut("Filter", false), Some("Ctrl+Shift+L"));
        assert_eq!(tooltip_shortcut("Bold", true), Some("⌘B"));
        assert_eq!(tooltip_shortcut("Filter", true), Some("⌘⇧F"));
        // Search commands has no shortcut of its own (Ctrl+Shift+Space selects all).
        assert_eq!(tooltip_shortcut("Search commands", false), None);
        for (name, mac, other) in super::TOOLTIP_SHORTCUTS {
            assert!(!other.contains(['⌘', '⇧', '⌥']), "{name}: {other}");
            assert!(mac.starts_with('⌘') || mac.starts_with('⌃'), "{name}: {mac}");
        }
    }
}
