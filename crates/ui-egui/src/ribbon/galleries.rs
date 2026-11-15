use super::controls::act;
use super::{Align2, Cat, Color32, Rect, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, json, pos2, theme, vec2};

/// A gallery of table styles drawn as mini tables.
pub(super) fn table_gallery(app: &mut SheetApp, ui: &mut Ui, cmd: &str) {
    ui.set_width(430.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    for (fam, n) in [("Light", 21u32), ("Medium", 28), ("Dark", 11)] {
        ui.label(egui::RichText::new(fam).strong());
        egui::Grid::new(("tg", fam)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for i in 1..=n {
                let name = format!("TableStyle{fam}{i}");
                let (r, resp) = ui.allocate_exact_size(vec2(54.0, 38.0), Sense::click());
                paint_table_swatch(ui, r, &wb, &name);
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Tokens::get(ui.ctx()).cat(Cat::Orange)), StrokeKind::Outside);
                }
                if resp.on_hover_text(&name).clicked() {
                    act(app, cmd, json!({"style": name}));
                    ui.close();
                }
                if i % 7 == 0 {
                    ui.end_row();
                }
            }
        });
    }
}

/// Paints a small preview of a table style: the authored colours, mapped and guarded for the
/// dark sheet like the cells they preview.
pub(super) fn paint_table_swatch(ui: &Ui, r: Rect, wb: &gridcraft_engine::model::Workbook, style: &str) {
    let lk = gridcraft_engine::tables::look(style);
    let t = Tokens::get(ui.ctx());
    // Authored colours; painted through `t.on_sheet` (fills) and `t.on_sheet_over` (text, lines).
    let res = |c: Option<gridcraft_engine::model::Color>| c.and_then(|c| c.resolve(&wb.theme)).map(theme::color32);
    let p = ui.painter();
    p.rect_filled(r, 0.0, t.grid_bg);
    let rows = 5;
    let rh = r.height() / rows as f32;
    for i in 0..rows {
        let rr = Rect::from_min_size(pos2(r.left(), r.top() + i as f32 * rh), vec2(r.width(), rh));
        let fill = if i == 0 {
            res(lk.header_fill)
        } else if i % 2 == 1 {
            res(lk.band_fill)
        } else {
            res(lk.body_fill)
        };
        if let Some(f) = fill {
            p.rect_filled(rr, 0.0, t.on_sheet(f));
        }
        if let Some(l) = res(lk.line) {
            p.line_segment(
                [rr.left_bottom(), rr.right_bottom()],
                Stroke::new(1.0, t.on_sheet_over(l, fill.unwrap_or(Color32::WHITE), theme::INK_LINE_MIN)),
            );
        }
        for k in 0..3 {
            let x = rr.left() + 4.0 + k as f32 * (r.width() - 8.0) / 3.0;
            let col = if i == 0 {
                let c = res(Some(lk.header_font)).unwrap_or(Color32::BLACK);
                t.on_sheet_over(c, fill.unwrap_or(Color32::WHITE), theme::INK_TEXT_MIN)
            } else {
                t.sheet_control_border
            };
            p.line_segment([pos2(x, rr.center().y), pos2(x + 10.0, rr.center().y)], Stroke::new(1.0, col));
        }
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
}

/// The Cell Styles gallery: each built-in style as a button showing its fill, font and border,
/// drawn as it will look on the current sheet; a click applies it to the selection.
pub(super) fn cell_style_gallery(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(520.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let groups: &[(&str, &[&str])] = &[
        ("Good, Bad and Neutral", &["Normal", "Bad", "Good", "Neutral"]),
        ("Data and Model", &["Calculation", "Check Cell", "Explanatory Text", "Input", "Linked Cell", "Note", "Output", "Warning Text"]),
        ("Titles and Headings", &["Heading 1", "Heading 2", "Heading 3", "Heading 4", "Title", "Total"]),
        (
            "Themed Cell Styles",
            &[
                "20% - Accent1",
                "20% - Accent2",
                "20% - Accent3",
                "20% - Accent4",
                "20% - Accent5",
                "20% - Accent6",
                "40% - Accent1",
                "40% - Accent2",
                "40% - Accent3",
                "40% - Accent4",
                "40% - Accent5",
                "40% - Accent6",
                "60% - Accent1",
                "60% - Accent2",
                "60% - Accent3",
                "60% - Accent4",
                "60% - Accent5",
                "60% - Accent6",
                "Accent1",
                "Accent2",
                "Accent3",
                "Accent4",
                "Accent5",
                "Accent6",
            ],
        ),
        ("Number Format", &["Comma", "Comma [0]", "Currency", "Currency [0]", "Percent"]),
    ];
    for (g, names) in groups {
        ui.label(egui::RichText::new(*g).strong());
        egui::Grid::new(("csg", *g)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (i, n) in names.iter().enumerate() {
                let st = gridcraft_engine::cmd::format::builtin_cell_style(n, &wb.theme).unwrap_or_default();
                let (r, resp) = ui.allocate_exact_size(vec2(82.0, 24.0), Sense::click());
                // The authored fill (white where there is none) is what the text and border were
                // chosen against; the painted fill is mapped on the dark sheet.
                let authored =
                    st.fill.fg.resolve(&wb.theme).filter(|_| st.fill.pattern != gridcraft_engine::model::PatternType::None).map(theme::color32);
                let fill = authored.map_or(t.grid_bg, |c| t.on_sheet(c));
                let paper = authored.unwrap_or(Color32::WHITE);
                let ink = |c: Option<Color32>, min| t.on_sheet_over(c.unwrap_or(Color32::BLACK), paper, min);
                ui.painter().rect_filled(r, 0.0, fill);
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
                if !st.borders.bottom.is_none() {
                    let c = ink(st.borders.bottom.color.resolve(&wb.theme).map(theme::color32), theme::INK_LINE_MIN);
                    ui.painter().line_segment(
                        [r.left_bottom() + vec2(0.0, -1.5), r.right_bottom() + vec2(0.0, -1.5)],
                        Stroke::new(st.borders.bottom.style.width(), c),
                    );
                }
                let col = ink(st.font.color.resolve(&wb.theme).map(theme::color32), theme::INK_TEXT_MIN);
                let fam = theme::cell_family(&st.font.name, st.font.bold, st.font.italic);
                ui.painter().text(
                    r.left_center() + vec2(4.0, 0.0),
                    Align2::LEFT_CENTER,
                    *n,
                    egui::FontId::new((st.font.size).clamp(10.0, 15.0), fam),
                    col,
                );
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Tokens::get(ui.ctx()).cat(Cat::Orange)), StrokeKind::Outside);
                }
                if resp.clicked() {
                    act(app, "home.cellStyle", json!({"name": n}));
                    ui.close();
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
    }
}
