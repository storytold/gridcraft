//! The command palette (`commandSearch`): a centred card over a dimmed backdrop. A borderless
//! search field on top, then the matching commands as rows (ribbon icon, label, ribbon path,
//! shortcut keycaps). With no query it lists the commands run recently, then common ones.
//! Arrows (and Page Up/Down, Ctrl+Home/End) move, Enter runs, Esc or a click outside closes.

use egui::{Align2, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{Dialog, SheetApp, json};
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Commands run from the palette, most recent first (kept in egui's memory, never saved).
const RECENT_ID: &str = "gridcraft.palette.recent";
const RECENT_MAX: usize = 5;
/// The card's width and the list's height before it scrolls.
const WIDTH: f32 = 640.0;
const LIST_HEIGHT: f32 = 360.0;
const ROW_H: f32 = 38.0;
const HEADER_H: f32 = 26.0;
const MAX_RESULTS: usize = 50;

/// Shown under "Common" when the query is empty (those that exist and are enabled).
const COMMON: &[&str] = &[
    "formulas.autoSum",
    "insert.table",
    "insert.recommendedCharts",
    "insert.pivotTable",
    "data.sortAscending",
    "data.filter",
    "home.conditionalFormat",
    "view.freezePanes",
    "home.formatCells",
    "edit.find",
    "formulas.insertFunction",
    "data.removeDuplicates",
];

/// The ribbon icon each command shows (its colorized design where the icon has one).
const COMMAND_ICONS: &[(&str, Icon)] = &[
    ("file.new", Icon::NewFile),
    ("file.open", Icon::OpenFolder),
    ("file.save", Icon::Save),
    ("file.saveAs", Icon::Save),
    ("file.print", Icon::Print),
    ("file.printPreview", Icon::Print),
    ("file.exportPdf", Icon::Share),
    ("file.properties", Icon::Info),
    ("file.options", Icon::Settings),
    ("edit.undo", Icon::Undo),
    ("edit.redo", Icon::Redo),
    ("edit.copy", Icon::Copy),
    ("edit.cut", Icon::Cut),
    ("edit.paste", Icon::Paste),
    ("edit.pasteSpecial", Icon::Paste),
    ("edit.clearAll", Icon::Eraser),
    ("edit.clearFormats", Icon::Eraser),
    ("edit.clearContents", Icon::Eraser),
    ("edit.fillDown", Icon::FillDown),
    ("edit.flashFill", Icon::FlashFill),
    ("edit.find", Icon::Find),
    ("edit.replace", Icon::Find),
    ("edit.goTo", Icon::Find),
    ("edit.goToSpecial", Icon::Find),
    ("edit.formatPainter", Icon::Brush),
    ("home.bold", Icon::Bold),
    ("home.italic", Icon::Italic),
    ("home.underline", Icon::Underline),
    ("home.doubleUnderline", Icon::Underline),
    ("home.strikethrough", Icon::Strike),
    ("home.increaseFontSize", Icon::FontGrow),
    ("home.decreaseFontSize", Icon::FontShrink),
    ("home.fontColor", Icon::FontColor),
    ("home.fillColor", Icon::Fill),
    ("home.borders", Icon::Borders),
    ("home.drawBorder", Icon::Borders),
    ("home.formatCells", Icon::Format),
    ("home.alignLeft", Icon::AlignLeft),
    ("home.alignCenter", Icon::AlignCenter),
    ("home.alignRight", Icon::AlignRight),
    ("home.alignTop", Icon::AlignTop),
    ("home.alignMiddle", Icon::AlignMiddle),
    ("home.alignBottom", Icon::AlignBottom),
    ("home.wrapText", Icon::Wrap),
    ("home.increaseIndent", Icon::IndentInc),
    ("home.decreaseIndent", Icon::IndentDec),
    ("home.orientation", Icon::Orientation),
    ("home.mergeCenter", Icon::Merge),
    ("home.mergeAcross", Icon::Merge),
    ("home.mergeCells", Icon::Merge),
    ("home.unmergeCells", Icon::Merge),
    ("home.accounting", Icon::Currency),
    ("home.percent", Icon::Percent),
    ("home.comma", Icon::Comma),
    ("home.increaseDecimal", Icon::DecInc),
    ("home.decreaseDecimal", Icon::DecDec),
    ("home.cellStyle", Icon::CellStyles),
    ("home.newCellStyle", Icon::CellStyles),
    ("home.conditionalFormat", Icon::CondFormat),
    ("home.clearRules", Icon::CondFormat),
    ("home.manageRules", Icon::CondFormat),
    ("home.formatAsTable", Icon::FormatTable),
    ("home.insertRows", Icon::Insert),
    ("home.insertColumns", Icon::Insert),
    ("home.insertCells", Icon::Insert),
    ("home.insertSheet", Icon::Insert),
    ("home.deleteRows", Icon::DeleteCells),
    ("home.deleteColumns", Icon::DeleteCells),
    ("home.deleteCells", Icon::DeleteCells),
    ("home.rowHeight", Icon::Format),
    ("home.columnWidth", Icon::Format),
    ("home.autofitRowHeight", Icon::Format),
    ("home.autofitColumnWidth", Icon::Format),
    ("home.lockCell", Icon::Lock),
    ("insert.table", Icon::Table),
    ("insert.pivotTable", Icon::PivotTable),
    ("insert.recommendedPivotTables", Icon::PivotTable),
    ("table.summarizePivot", Icon::PivotTable),
    ("insert.chart", Icon::Chart),
    ("insert.recommendedCharts", Icon::Chart),
    ("insert.pivotChart", Icon::Chart),
    ("insert.sparkline", Icon::Sparkline),
    ("insert.picture", Icon::Picture),
    ("insert.shape", Icon::Shapes),
    ("insert.icons", Icon::InsertIcons),
    ("insert.textBox", Icon::TextBox),
    ("insert.link", Icon::Link),
    ("insert.symbol", Icon::Symbol),
    ("insert.checkbox", Icon::Checkbox),
    ("insert.slicer", Icon::Slicer),
    ("table.insertSlicer", Icon::Slicer),
    ("table.convertToRange", Icon::ConvertToRange),
    ("table.resize", Icon::ResizeTable),
    ("table.removeDuplicates", Icon::Duplicates),
    ("chart.switchRowColumn", Icon::SwitchRowCol),
    ("chart.selectData", Icon::SelectData),
    ("chart.changeType", Icon::ChangeChartType),
    ("chart.addElement", Icon::ChartElement),
    ("chart.formatSelection", Icon::FormatPane),
    ("data.sortAscending", Icon::SortAsc),
    ("data.sortDescending", Icon::SortDesc),
    ("data.sort", Icon::SortCustom),
    ("data.filter", Icon::Filter),
    ("data.clearFilter", Icon::ClearFilter),
    ("data.reapply", Icon::Reapply),
    ("data.advancedFilter", Icon::AdvancedFilter),
    ("data.removeDuplicates", Icon::Duplicates),
    ("data.textToColumns", Icon::TextColumns),
    ("data.validation", Icon::Validation),
    ("data.group", Icon::Group),
    ("data.ungroup", Icon::Ungroup),
    ("data.subtotal", Icon::Subtotal),
    ("data.showDetail", Icon::ShowDetail),
    ("data.hideDetail", Icon::HideDetail),
    ("data.outlineSettings", Icon::OutlineSettings),
    ("data.flashFill", Icon::FlashFill),
    ("data.getData", Icon::GetData),
    ("data.fromTextCsv", Icon::GetData),
    ("data.scenarioManager", Icon::WhatIf),
    ("data.goalSeek", Icon::WhatIf),
    ("data.dataTable", Icon::WhatIf),
    ("data.forecastSheet", Icon::Forecast),
    ("data.analysisToolPak", Icon::DataAnalysis),
    ("data.solver", Icon::Solver),
    ("formulas.autoSum", Icon::Sum),
    ("formulas.insertFunction", Icon::Function),
    ("formulas.financial", Icon::FnFinancial),
    ("formulas.logical", Icon::FnLogical),
    ("formulas.text", Icon::FnText),
    ("formulas.dateTime", Icon::FnDateTime),
    ("formulas.lookupReference", Icon::FnLookup),
    ("formulas.mathTrig", Icon::FnMathTrig),
    ("formulas.moreFunctions", Icon::FnMore),
    ("formulas.defineName", Icon::DefineName),
    ("formulas.nameManager", Icon::Name),
    ("formulas.useInFormula", Icon::UseInFormula),
    ("formulas.createFromSelection", Icon::CreateFromSelection),
    ("formulas.tracePrecedents", Icon::Trace),
    ("formulas.traceDependents", Icon::TraceDependents),
    ("formulas.removeArrows", Icon::RemoveArrows),
    ("formulas.showFormulas", Icon::ShowFormulas),
    ("formulas.errorChecking", Icon::ErrorCheck),
    ("formulas.evaluateFormula", Icon::EvaluateFormula),
    ("formulas.watchWindow", Icon::WatchWindow),
    ("formulas.calculateNow", Icon::CalcNow),
    ("formulas.calculateSheet", Icon::CalcSheet),
    ("formulas.calculationOptions", Icon::Calc),
    ("review.spelling", Icon::Spell),
    ("review.newComment", Icon::Comment),
    ("review.newNote", Icon::Note),
    ("review.deleteComment", Icon::DeleteComment),
    ("review.previousComment", Icon::PrevComment),
    ("review.nextComment", Icon::NextComment),
    ("review.showComments", Icon::ShowComments),
    ("review.protectSheet", Icon::ProtectSheet),
    ("review.unprotectSheet", Icon::ProtectSheet),
    ("review.protectWorkbook", Icon::ProtectWorkbook),
    ("review.allowEditRanges", Icon::AllowEditRanges),
    ("review.workbookStatistics", Icon::WorkbookStats),
    ("review.checkAccessibility", Icon::Accessibility),
    ("view.freezePanes", Icon::Freeze),
    ("view.freezeTopRow", Icon::Freeze),
    ("view.freezeFirstColumn", Icon::Freeze),
    ("view.unfreezePanes", Icon::Freeze),
    ("view.gridlines", Icon::Gridlines),
    ("view.rightToLeft", Icon::SheetRtl),
    ("view.zoom", Icon::Zoom),
    ("view.zoom100", Icon::Zoom100),
    ("view.zoomToSelection", Icon::ZoomSelection),
    ("view.normal", Icon::Normal),
    ("view.pageLayout", Icon::PageLayout),
    ("view.pageBreakPreview", Icon::PageBreak),
    ("view.newWindow", Icon::NewWindow),
    ("view.arrangeAll", Icon::ArrangeAll),
    ("view.switchWindows", Icon::SwitchWindows),
    ("view.macros", Icon::RunMacro),
    ("macro.record", Icon::Record),
    ("pageLayout.margins", Icon::Margins),
    ("pageLayout.orientation", Icon::Orient),
    ("pageLayout.size", Icon::PaperSize),
    ("pageLayout.printArea", Icon::PrintArea),
    ("pageLayout.printTitles", Icon::PrintTitles),
    ("pageLayout.breaks", Icon::Breaks),
    ("pageLayout.headerFooter", Icon::HeaderFooter),
    ("pageLayout.theme", Icon::Theme),
    ("pageLayout.pageSetup", Icon::PaperSize),
    ("draw.pen", Icon::Pen),
    ("draw.eraser", Icon::DrawEraser),
    ("draw.lasso", Icon::SelectObjects),
    ("draw.inkToShape", Icon::InkToShape),
    ("automate.newScript", Icon::Script),
    ("automate.allScripts", Icon::Script),
    ("automate.recordActions", Icon::Record),
];

/// The icon a command shows in the palette: its ribbon button's, else one for its tab.
pub(crate) fn command_icon(id: &str) -> Icon {
    if let Some((_, icon)) = COMMAND_ICONS.iter().find(|(c, _)| *c == id) {
        return *icon;
    }
    match id.split('.').next().unwrap_or("") {
        "file" => Icon::Book,
        "home" => Icon::Home,
        "insert" => Icon::Insert,
        "formulas" => Icon::Fx,
        "data" => Icon::Table,
        "review" => Icon::Comment,
        "view" => Icon::Normal,
        "pageLayout" => Icon::PageLayout,
        "chart" => Icon::Chart,
        "pivot" => Icon::PivotTable,
        "table" => Icon::Table,
        "draw" => Icon::Pen,
        "macro" | "automate" => Icon::Macro,
        "sheet" => Icon::Sheet,
        "window" => Icon::SwitchWindows,
        _ => Icon::CommandPalette,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Results,
    Recent,
    Common,
}

#[derive(Clone, Debug)]
struct Item {
    id: &'static str,
    label: String,
    path: String,
    shortcut: Option<&'static str>,
    section: Section,
}

/// Remembers `id` as the most recent palette command.
pub(crate) fn remember(ctx: &egui::Context, id: &str) {
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Vec<String>>(egui::Id::unique(RECENT_ID));
        list.retain(|c| c != id);
        list.insert(0, id.to_string());
        list.truncate(RECENT_MAX);
    });
}

/// The palette's recent commands, most recent first.
pub fn recent(ctx: &egui::Context) -> Vec<String> {
    ctx.data(|d| d.get_temp::<Vec<String>>(egui::Id::unique(RECENT_ID))).unwrap_or_default()
}

/// How well `q` (lower case, split into words) matches a command: lower is better; `None` when a
/// word matches nothing. A label that starts with the query beats one with a word starting with
/// each query word, which beats a label that merely contains it; the ribbon path and the id last.
fn score(q: &str, words: &[&str], label: &str, path: &str, id: &str) -> Option<u8> {
    if !words.iter().all(|w| label.contains(w) || path.contains(w) || id.contains(w)) {
        return None;
    }
    let word_start = |w: &str| label.split(|c: char| !c.is_alphanumeric()).any(|lw| lw.starts_with(w));
    Some(if label.starts_with(q) {
        0
    } else if words.iter().all(|w| word_start(w)) {
        1
    } else if words.iter().all(|w| label.contains(w)) {
        2
    } else if words.iter().any(|w| label.contains(w)) {
        3
    } else {
        4
    })
}

/// The rows to show for the dialog's query.
fn items(app: &SheetApp, ctx: &egui::Context, query: &str) -> Vec<Item> {
    let lang = app.ui.language;
    let cmds: Vec<_> = app.session.commands().into_iter().filter(|c| c.enabled).collect();
    let item = |c: &gridcraft_engine::cmd::CommandInfo, section| Item {
        id: c.id,
        label: lang.tr(c.label).to_string(),
        path: c.menu.iter().map(|m| lang.tr(m)).collect::<Vec<_>>().join(" › "),
        shortcut: c.shortcut,
        section,
    };
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        let mut out: Vec<Item> = Vec::new();
        // The palette's own picks first, then commands the journal shows were run recently.
        // File commands are left out: opening the workbook itself is in the journal.
        let journal = app.session.journal.iter().rev().take(200).map(|(id, _)| id.to_string()).filter(|id| !id.starts_with("file."));
        for id in recent(ctx).into_iter().chain(journal) {
            if out.len() >= RECENT_MAX {
                break;
            }
            if out.iter().any(|i| i.id == id) {
                continue;
            }
            if let Some(c) = cmds.iter().find(|c| c.id == id && !c.menu.is_empty()) {
                out.push(item(c, Section::Recent));
            }
        }
        for id in COMMON {
            if out.iter().any(|i| i.id == *id) {
                continue;
            }
            if let Some(c) = cmds.iter().find(|c| c.id == *id) {
                out.push(item(c, Section::Common));
            }
        }
        return out;
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut scored: Vec<(u8, usize, Item)> = cmds
        .iter()
        .enumerate()
        .filter_map(|(n, c)| {
            let it = item(c, Section::Results);
            let label = it.label.to_lowercase();
            // English labels match too, so a typed English name works in every language.
            let en = c.label.to_lowercase();
            let s = score(&q, &words, &label, &it.path.to_lowercase(), &c.id.to_lowercase())
                .into_iter()
                .chain(score(&q, &words, &en, "", &c.id.to_lowercase()))
                .min()?;
            Some((s, n, it))
        })
        .collect();
    scored.sort_by_key(|(s, n, _)| (*s, *n));
    scored.into_iter().take(MAX_RESULTS).map(|(_, _, it)| it).collect()
}

/// A shortcut's keys as keycap labels ("Cmd+Shift+T" → Ctrl, Shift, T off macOS; ⌘ ⇧ T on it).
pub(crate) fn keycaps(shortcut: &str, mac: bool) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let mut rest = shortcut;
    while !rest.is_empty() {
        // A literal "+" key: "Ctrl++" or a trailing "+".
        let (key, tail) = match rest.find('+') {
            Some(0) => ("+", &rest[1..]),
            Some(i) => (&rest[..i], rest.get(i + 1..).unwrap_or("")),
            None => (rest, ""),
        };
        let label = match (key, mac) {
            ("Cmd", true) => "⌘",
            ("Shift", true) => "⇧",
            ("Alt", true) => "⌥",
            ("Ctrl", true) => "⌃",
            ("Cmd", false) => "Ctrl",
            (k, _) => k,
        };
        if keys.last().map(String::as_str) != Some(label) {
            keys.push(label.to_string());
        }
        rest = tail;
    }
    keys
}

/// Paints keycap chips ending at `right` (vertically centred on `cy`); returns their left edge.
fn paint_keycaps(p: &egui::Painter, t: &Tokens, keys: &[String], right: f32, cy: f32) -> f32 {
    let font = theme::ui_font(11.0);
    let mut x = right;
    for k in keys.iter().rev() {
        let g = p.layout_no_wrap(k.clone(), font.clone(), t.text_dim);
        let w = (g.size().x + 10.0).max(20.0);
        let r = Rect::from_min_size(pos2(x - w, cy - 10.0), vec2(w, 20.0));
        p.rect_filled(r, 5.0, t.window);
        p.rect_stroke(r, 5.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
        // The cap's lower lip.
        p.line_segment([pos2(r.left() + 3.0, r.bottom() - 0.5), pos2(r.right() - 3.0, r.bottom() - 0.5)], Stroke::new(1.0, t.separator));
        p.galley(pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0 - 0.5), g, t.text_dim);
        x = r.left() - 4.0;
    }
    x
}

/// An up or down arrow drawn in a keycap-sized box (fonts may lack the arrow glyphs).
fn paint_arrow_cap(p: &egui::Painter, t: &Tokens, r: Rect, up: bool) {
    p.rect_filled(r, 5.0, t.window);
    p.rect_stroke(r, 5.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
    let c = r.center();
    let s = if up { -1.0 } else { 1.0 };
    let stroke = Stroke::new(1.3, t.text_dim);
    p.line_segment([pos2(c.x, c.y - 4.0 * s), pos2(c.x, c.y + 4.0 * s)], stroke);
    p.line_segment([pos2(c.x - 3.0, c.y + 1.0 * s), pos2(c.x, c.y + 4.0 * s)], stroke);
    p.line_segment([pos2(c.x + 3.0, c.y + 1.0 * s), pos2(c.x, c.y + 4.0 * s)], stroke);
}

/// Draws the palette and handles its keys. Returns whether it stays open.
pub(super) fn palette(app: &mut SheetApp, ctx: &egui::Context, d: &mut Dialog) -> bool {
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let mut open = true;
    let mut run: Option<&'static str> = None;

    let (down, up, home, end, pgup, pgdn, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::CTRL, Key::Home),
            i.consume_key(Modifiers::CTRL, Key::End),
            i.consume_key(Modifiers::NONE, Key::PageUp),
            i.consume_key(Modifiers::NONE, Key::PageDown),
            i.key_pressed(Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let moved = down || up || home || end || pgup || pgdn;

    let list = items(app, ctx, &d.search);
    let last = list.len().saturating_sub(1);
    let page = ((LIST_HEIGHT / ROW_H).floor() as usize).saturating_sub(1).max(1);
    if down {
        d.list_index = (d.list_index + 1).min(last);
    }
    if up {
        d.list_index = d.list_index.saturating_sub(1);
    }
    if pgdn {
        d.list_index = (d.list_index + page).min(last);
    }
    if pgup {
        d.list_index = d.list_index.saturating_sub(page);
    }
    if home {
        d.list_index = 0;
    }
    if end {
        d.list_index = last;
    }
    d.list_index = d.list_index.min(last);

    // The backdrop: a soft dim over the window; a click on it closes the palette.
    // It sits above the sheet and the ribbon (Middle) and below the card (Foreground).
    egui::Area::new(egui::Id::unique("commandPalette.backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .fade_in(false)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            let r = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0.0, t.shadow);
            if r.clicked() {
                open = false;
            }
        });

    let width = WIDTH.min(screen.width() - 32.0).max(240.0);
    let top = screen.top() + (screen.height() * 0.16).clamp(48.0, 180.0);
    let card_pos = pos2(screen.center().x - width / 2.0, top);
    egui::Area::new(egui::Id::unique("commandPalette.card")).order(egui::Order::Foreground).fixed_pos(card_pos).show(ctx, |ui| {
        // Above the backdrop, which shares the layer order.
        ui.ctx().move_to_top(ui.layer_id());
        egui::Frame::new()
            .fill(t.menu_bg)
            .stroke(Stroke::new(1.0, t.ribbon_border))
            .corner_radius(12.0)
            .shadow(egui::epaint::Shadow { offset: [0, 12], blur: 40, spread: 0, color: t.shadow })
            .inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 8 })
            .show(ui, |ui| {
                ui.set_width(width - 16.0);
                // The search field: a magnifier and a borderless text box.
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let (ir, _) = ui.allocate_exact_size(vec2(20.0, 40.0), Sense::hover());
                    icons::paint(ui.painter(), Rect::from_center_size(ir.center(), vec2(18.0, 18.0)), Icon::Search, t.text_dim);
                    ui.add_space(4.0);
                    let hint = egui::RichText::new(app.ui.language.tr("Search commands")).color(t.text_disabled);
                    let r = ui
                        .add(
                            egui::TextEdit::singleline(&mut d.search)
                                .frame(egui::Frame::NONE)
                                .font(theme::ui_font(16.0))
                                .text_color(t.text)
                                .hint_text(hint)
                                .desired_width(ui.available_width() - 8.0)
                                .margin(vec2(2.0, 10.0)),
                        )
                        .accessible_name(app.ui.language.tr("Search commands").to_string());
                    if !d.focus_initialized || !r.has_focus() {
                        r.request_focus();
                        d.focus_initialized = true;
                    }
                    if r.changed() {
                        d.list_index = 0;
                    }
                });
                let sep = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range().expand(8.0), sep, Stroke::new(1.0, t.separator));
                ui.add_space(6.0);
                if list.is_empty() {
                    ui.add_space(18.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(app.ui.language.tr("No matching commands")).color(t.text_dim));
                    });
                    ui.add_space(18.0);
                } else {
                    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let mut section = None;
                        for (i, it) in list.iter().enumerate() {
                            if section != Some(it.section) {
                                section = Some(it.section);
                                let title = match it.section {
                                    Section::Recent => Some("Recent"),
                                    Section::Common => Some("Common commands"),
                                    Section::Results => None,
                                };
                                if let Some(title) = title {
                                    let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
                                    ui.painter().text(
                                        pos2(hr.left() + 10.0, hr.bottom() - 7.0),
                                        Align2::LEFT_BOTTOM,
                                        app.ui.language.tr(title),
                                        theme::ui_bold(11.0),
                                        t.text_dim,
                                    );
                                }
                            }
                            let r = row(ui, &t, it, i == d.list_index);
                            if moved && i == d.list_index && !ui.clip_rect().contains_rect(r.rect) {
                                r.scroll_to_me(Some(if down || end || pgdn { egui::Align::Max } else { egui::Align::Min }));
                            }
                            if r.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                                d.list_index = i;
                            }
                            if r.clicked() {
                                d.list_index = i;
                                run = Some(it.id);
                            }
                        }
                    });
                }
                footer(ui, &t, app);
            });
    });

    if enter && let Some(it) = list.get(d.list_index) {
        run = Some(it.id);
    }
    if esc {
        open = false;
    }
    if let Some(id) = run {
        remember(ctx, id);
        app.dialog = None;
        app.run_or_alert(id, json!({}));
        return false;
    }
    open
}

/// One result row: icon, label, ribbon path (dim) and shortcut keycaps on the right.
fn row(ui: &mut egui::Ui, t: &Tokens, it: &Item, selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let p = ui.painter();
    let inner = r.shrink2(vec2(2.0, 1.0));
    if selected {
        p.rect_filled(inner, 8.0, t.accent_soft);
    } else if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        p.rect_filled(inner, 8.0, fill);
    }
    let cy = r.center().y;
    icons::paint_ribbon(p, Rect::from_center_size(pos2(r.left() + 22.0, cy), vec2(18.0, 18.0)), command_icon(it.id), t.text);
    let right = match it.shortcut {
        Some(s) => paint_keycaps(p, t, &keycaps(s, cfg!(target_os = "macos")), r.right() - 10.0, cy) - 8.0,
        None => r.right() - 10.0,
    };
    let x = r.left() + 42.0;
    let label = p.layout_no_wrap(it.label.clone(), theme::ui_font(13.5), t.text);
    let lw = label.size().x.min((right - x).max(0.0));
    let label_pos = Pos2::new(x, cy - label.size().y / 2.0);
    p.with_clip_rect(Rect::from_min_max(pos2(x, r.top()), pos2(right, r.bottom()))).galley(label_pos, label, t.text);
    if !it.path.is_empty() {
        let px = x + lw + 10.0;
        if px < right - 24.0 {
            let mut job = egui::text::LayoutJob::simple_singleline(it.path.clone(), theme::ui_font(12.0), t.text_dim);
            job.wrap = egui::text::TextWrapping::truncate_at_width(right - px);
            let g = p.layout_job(job);
            p.galley(pos2(px, cy - g.size().y / 2.0 + 0.5), g, t.text_dim);
        }
    }
    let label = format!("{} {}", it.label, it.path);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
    resp
}

/// The hint strip under the list: the keys that drive the palette.
fn footer(ui: &mut egui::Ui, t: &Tokens, app: &SheetApp) {
    ui.add_space(6.0);
    let top = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range().expand(8.0), top, Stroke::new(1.0, t.separator));
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter();
    let cy = r.center().y + 3.0;
    let font = theme::ui_font(11.5);
    let tr = |s: &'static str| app.ui.language.tr(s).to_string();
    let mut x = r.left() + 8.0;
    for up in [true, false] {
        paint_arrow_cap(p, t, Rect::from_min_size(pos2(x, cy - 10.0), vec2(20.0, 20.0)), up);
        x += 24.0;
    }
    let g = p.layout_no_wrap(tr("to move"), font.clone(), t.text_dim);
    let gw = g.size().x;
    p.galley(pos2(x + 2.0, cy - g.size().y / 2.0), g, t.text_dim);
    x += gw + 18.0;
    for (key, what) in [("Enter", "to run"), ("Esc", "to close")] {
        let k = p.layout_no_wrap(tr(key), font.clone(), t.text_dim);
        let w = k.size().x + 12.0;
        let kr = Rect::from_min_size(pos2(x, cy - 10.0), vec2(w, 20.0));
        p.rect_filled(kr, 5.0, t.window);
        p.rect_stroke(kr, 5.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
        p.galley(pos2(kr.center().x - k.size().x / 2.0, kr.center().y - k.size().y / 2.0 - 0.5), k, t.text_dim);
        x = kr.right() + 6.0;
        let g = p.layout_no_wrap(tr(what), font.clone(), t.text_dim);
        let gw = g.size().x;
        p.galley(pos2(x, cy - g.size().y / 2.0), g, t.text_dim);
        x += gw + 18.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycaps_split_modifiers_and_literal_plus() {
        assert_eq!(keycaps("Cmd+Shift+T", false), ["Ctrl", "Shift", "T"]);
        assert_eq!(keycaps("Cmd+Shift+T", true), ["⌘", "⇧", "T"]);
        assert_eq!(keycaps("Ctrl++", false), ["Ctrl", "+"]);
        assert_eq!(keycaps("Ctrl+Cmd+V", false), ["Ctrl", "V"]);
        assert_eq!(keycaps("F9", false), ["F9"]);
        assert_eq!(keycaps("Cmd+-", false), ["Ctrl", "-"]);
    }

    #[test]
    fn every_mapped_command_exists() {
        let ids: Vec<&str> = gridcraft_engine::command_specs().iter().map(|c| c.id).collect();
        for (id, _) in COMMAND_ICONS {
            assert!(ids.contains(id), "palette icon for unknown command {id}");
        }
        for id in COMMON {
            assert!(ids.contains(id), "unknown common command {id}");
        }
    }

    #[test]
    fn a_label_prefix_ranks_before_a_path_match() {
        let q = "sort";
        let w = [q];
        let prefix = score(q, &w, "sort a to z", "data › sort & filter", "data.sortascending");
        let path = score(q, &w, "filter", "data › sort & filter", "data.filter");
        assert!(prefix < path, "{prefix:?} vs {path:?}");
        assert_eq!(score("zzz", &["zzz"], "bold", "home", "home.bold"), None);
    }
}
