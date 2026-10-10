//! GridCraft's icon set, drawn in code on a 16×16 design grid (original artwork; see
//! ATTRIBUTION.md). Large ribbon icons use the same drawings at 32×32 with accent colours.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Paste,
    Cut,
    Copy,
    Brush,
    Bold,
    Italic,
    Underline,
    Strike,
    Borders,
    BorderAll,
    BorderNone,
    BorderOutside,
    BorderThick,
    Fill,
    FontColor,
    AlignTop,
    AlignMiddle,
    AlignBottom,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Orientation,
    IndentDec,
    IndentInc,
    Wrap,
    Merge,
    Currency,
    Percent,
    Comma,
    DecInc,
    DecDec,
    CondFormat,
    FormatTable,
    CellStyles,
    Insert,
    Delete,
    Format,
    Sum,
    FillDown,
    Eraser,
    SortFilter,
    Find,
    Undo,
    Redo,
    Save,
    Home,
    More,
    Chevron,
    ChevronUp,
    Close,
    Check,
    Eye,
    Fx,
    Plus,
    Left,
    Right,
    Normal,
    PageLayout,
    PageBreak,
    Minus,
    Table,
    PivotTable,
    Chart,
    ChartBar,
    ChartLine,
    ChartPie,
    ChartScatter,
    Picture,
    Shapes,
    Link,
    Comment,
    Note,
    TextBox,
    Symbol,
    Sparkline,
    Function,
    Name,
    Trace,
    Calc,
    Filter,
    SortAsc,
    SortDesc,
    Duplicates,
    TextColumns,
    Validation,
    Group,
    Ungroup,
    Subtotal,
    Lock,
    Spell,
    Freeze,
    Zoom,
    Gridlines,
    Theme,
    Margins,
    Orient,
    Paper,
    PrintArea,
    Script,
    Pen,
    Share,
    Search,
    Settings,
    Sheet,
    Book,
    Folder,
}

struct G<'a> {
    p: &'a Painter,
    r: Rect,
    s: f32,
}

impl G<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x * self.s, self.r.top() + y * self.s)
    }
    fn line(&self, pts: &[(f32, f32)], c: Color32, w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(PathShape::line(v, PathStroke::new(w * self.s.clamp(1.0, 2.0), c)));
    }
    fn stroke(&self, pts: &[(f32, f32)], c: Color32) {
        self.line(pts, c, 1.15);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, fill: Option<Color32>, stroke: Option<Color32>) {
        let r = Rect::from_min_size(self.pt(x, y), vec2(w * self.s, h * self.s));
        if let Some(f) = fill {
            self.p.rect_filled(r, 1.0, f);
        }
        if let Some(c) = stroke {
            self.p.rect_stroke(r, 1.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
        }
    }
    fn poly(&self, pts: &[(f32, f32)], fill: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(PathShape::convex_polygon(v, fill, Stroke::NONE));
    }
    fn circle(&self, x: f32, y: f32, r: f32, fill: Option<Color32>, stroke: Option<Color32>) {
        if let Some(f) = fill {
            self.p.circle_filled(self.pt(x, y), r * self.s, f);
        }
        if let Some(c) = stroke {
            self.p.circle_stroke(self.pt(x, y), r * self.s, Stroke::new(1.0, c));
        }
    }
    fn text(&self, x: f32, y: f32, t: &str, size: f32, c: Color32, bold: bool, italic: bool) {
        let fam = if bold {
            crate::theme::CELL_BOLD
        } else if italic {
            crate::theme::CELL_ITALIC
        } else {
            crate::theme::CELL
        };
        let font = egui::FontId::new(size * self.s, egui::FontFamily::Name(fam.into()));
        self.p.text(self.pt(x, y), egui::Align2::CENTER_CENTER, t, font, c);
    }
}

/// Accent palette for icons (our own).
pub const GREEN: Color32 = Color32::from_rgb(0x10, 0x7C, 0x41);
pub const BLUE: Color32 = Color32::from_rgb(0x2B, 0x7C, 0xD3);
pub const RED: Color32 = Color32::from_rgb(0xD1, 0x3B, 0x2F);
pub const YELLOW: Color32 = Color32::from_rgb(0xF2, 0xC3, 0x2E);
pub const ORANGE: Color32 = Color32::from_rgb(0xE8, 0x7D, 0x2B);
pub const PURPLE: Color32 = Color32::from_rgb(0x88, 0x55, 0xC8);

/// Draws `icon` into `rect` (square-ish), in colour `c`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    let g = G { p, r, s: side / 16.0 };
    let light = c.gamma_multiply(0.45);
    match icon {
        Icon::Paste => {
            g.rect(3.0, 2.5, 9.0, 12.0, Some(Color32::from_rgb(0xC8, 0x9B, 0x5C)), None);
            g.rect(5.0, 1.5, 5.0, 2.5, Some(Color32::from_rgb(0x8A, 0x8A, 0x8A)), None);
            g.rect(6.5, 6.0, 7.5, 9.0, Some(Color32::WHITE), Some(c));
            for y in [8.5, 10.5, 12.5] {
                g.stroke(&[(8.0, y), (12.5, y)], light);
            }
        }
        Icon::Cut => {
            g.circle(5.0, 12.0, 2.0, None, Some(c));
            g.circle(11.0, 12.0, 2.0, None, Some(c));
            g.stroke(&[(6.3, 10.6), (11.5, 2.0)], c);
            g.stroke(&[(9.7, 10.6), (4.5, 2.0)], c);
        }
        Icon::Copy => {
            g.rect(2.5, 2.5, 7.5, 9.0, Some(Color32::WHITE), Some(c));
            g.rect(6.0, 5.5, 7.5, 9.0, Some(Color32::WHITE), Some(c));
        }
        Icon::Brush => {
            g.rect(3.0, 2.0, 10.0, 4.0, Some(YELLOW), Some(c));
            g.stroke(&[(8.0, 6.0), (8.0, 8.5), (5.0, 8.5), (5.0, 10.0)], c);
            g.rect(4.0, 10.0, 2.0, 4.5, Some(c), None);
        }
        Icon::Bold => g.text(8.0, 8.5, "B", 12.0, c, true, false),
        Icon::Italic => g.text(8.0, 8.5, "I", 12.0, c, false, true),
        Icon::Underline => {
            g.text(8.0, 7.5, "U", 11.0, c, false, false);
            g.stroke(&[(4.0, 14.0), (12.0, 14.0)], c);
        }
        Icon::Strike => {
            g.text(8.0, 8.0, "ab", 10.0, c, false, false);
            g.stroke(&[(2.5, 8.5), (13.5, 8.5)], c);
        }
        Icon::Borders | Icon::BorderAll => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(light));
            g.stroke(&[(8.0, 2.5), (8.0, 13.5)], light);
            g.stroke(&[(2.5, 8.0), (13.5, 8.0)], light);
            if icon == Icon::Borders {
                g.line(&[(2.0, 13.5), (14.0, 13.5)], c, 1.6);
            } else {
                g.rect(2.5, 2.5, 11.0, 11.0, None, Some(c));
                g.stroke(&[(8.0, 2.5), (8.0, 13.5)], c);
                g.stroke(&[(2.5, 8.0), (13.5, 8.0)], c);
            }
        }
        Icon::BorderNone => {
            for i in 0..6 {
                let t = 2.5 + i as f32 * 2.2;
                g.circle(t, 2.5, 0.4, Some(light), None);
                g.circle(t, 13.5, 0.4, Some(light), None);
                g.circle(2.5, t, 0.4, Some(light), None);
                g.circle(13.5, t, 0.4, Some(light), None);
            }
        }
        Icon::BorderOutside | Icon::BorderThick => {
            g.stroke(&[(8.0, 2.5), (8.0, 13.5)], light);
            g.stroke(&[(2.5, 8.0), (13.5, 8.0)], light);
            g.line(&[(2.5, 2.5), (13.5, 2.5), (13.5, 13.5), (2.5, 13.5), (2.5, 2.5)], c, if icon == Icon::BorderThick { 2.2 } else { 1.2 });
        }
        Icon::Fill => {
            g.poly(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0)], Color32::WHITE);
            g.line(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0), (3.0, 7.0)], c, 1.1);
            g.circle(13.5, 10.0, 1.2, Some(c), None);
        }
        Icon::FontColor => {
            g.text(8.0, 7.0, "A", 11.0, c, false, false);
        }
        Icon::AlignTop | Icon::AlignMiddle | Icon::AlignBottom => {
            let ys: [f32; 3] = match icon {
                Icon::AlignTop => [3.0, 5.5, 8.0],
                Icon::AlignMiddle => [5.5, 8.0, 10.5],
                _ => [8.0, 10.5, 13.0],
            };
            let edge = match icon {
                Icon::AlignTop => 1.5,
                Icon::AlignMiddle => -1.0,
                _ => 14.5,
            };
            if edge > 0.0 {
                g.stroke(&[(2.0, edge), (14.0, edge)], light);
            }
            for (i, y) in ys.iter().enumerate() {
                let w = if i == 1 { 8.0 } else { 10.0 };
                g.stroke(&[(8.0 - w / 2.0, *y), (8.0 + w / 2.0, *y)], c);
            }
        }
        Icon::AlignLeft | Icon::AlignCenter | Icon::AlignRight | Icon::IndentDec | Icon::IndentInc => {
            for (i, y) in [3.5f32, 6.5, 9.5, 12.5].iter().enumerate() {
                let w = if i % 2 == 0 { 12.0 } else { 8.0 };
                let (x0, x1) = match icon {
                    Icon::AlignLeft => (2.0, 2.0 + w),
                    Icon::AlignRight => (14.0 - w, 14.0),
                    Icon::AlignCenter => (8.0 - w / 2.0, 8.0 + w / 2.0),
                    _ => (if i == 0 || i == 3 { 2.0 } else { 7.0 }, 14.0),
                };
                g.stroke(&[(x0, *y), (x1, *y)], c);
            }
            if icon == Icon::IndentInc {
                g.poly(&[(2.0, 6.0), (5.0, 8.0), (2.0, 10.0)], BLUE);
            }
            if icon == Icon::IndentDec {
                g.poly(&[(5.0, 6.0), (2.0, 8.0), (5.0, 10.0)], BLUE);
            }
        }
        Icon::Orientation => {
            g.text(6.0, 9.0, "ab", 8.5, c, false, false);
            g.stroke(&[(9.0, 13.0), (14.0, 3.0)], BLUE);
            g.poly(&[(14.5, 2.0), (12.0, 4.0), (14.6, 5.2)], BLUE);
        }
        Icon::Wrap => {
            g.stroke(&[(2.0, 4.0), (14.0, 4.0)], c);
            g.stroke(&[(2.0, 8.0), (12.0, 8.0)], c);
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(12.0, 8.0), g.pt(15.0, 8.0), g.pt(15.0, 12.0), g.pt(11.0, 12.0)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.1, BLUE),
            ));
            g.poly(&[(9.0, 12.0), (11.5, 10.5), (11.5, 13.5)], BLUE);
            g.stroke(&[(2.0, 12.0), (7.0, 12.0)], c);
        }
        Icon::Merge => {
            g.rect(1.5, 3.5, 13.0, 9.0, None, Some(c));
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], BLUE);
            g.poly(&[(3.0, 8.0), (5.5, 6.5), (5.5, 9.5)], BLUE);
            g.poly(&[(13.0, 8.0), (10.5, 6.5), (10.5, 9.5)], BLUE);
        }
        Icon::Currency => g.text(8.0, 8.5, "$", 12.0, c, false, false),
        Icon::Percent => g.text(8.0, 8.5, "%", 11.0, c, false, false),
        Icon::Comma => g.text(8.0, 7.0, ",", 14.0, c, true, false),
        Icon::DecInc => {
            g.text(5.0, 5.0, ".0", 6.5, c, false, false);
            g.text(10.0, 11.0, ".00", 6.5, c, false, false);
            g.stroke(&[(2.0, 12.0), (5.0, 12.0)], BLUE);
            g.poly(&[(1.0, 12.0), (2.8, 10.6), (2.8, 13.4)], BLUE);
        }
        Icon::DecDec => {
            g.text(6.0, 5.0, ".00", 6.5, c, false, false);
            g.text(11.0, 11.0, ".0", 6.5, c, false, false);
            g.stroke(&[(2.0, 12.0), (5.5, 12.0)], BLUE);
            g.poly(&[(7.0, 12.0), (5.2, 10.6), (5.2, 13.4)], BLUE);
        }
        Icon::CondFormat => {
            g.rect(2.0, 2.0, 12.0, 12.0, Some(Color32::WHITE), Some(light));
            g.rect(3.0, 3.5, 4.0, 2.5, Some(RED), None);
            g.rect(3.0, 7.0, 7.0, 2.5, Some(YELLOW), None);
            g.rect(3.0, 10.5, 10.0, 2.5, Some(BLUE), None);
        }
        Icon::FormatTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(Color32::WHITE), Some(light));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(BLUE), None);
            g.rect(2.0, 8.0, 12.0, 2.5, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], light);
        }
        Icon::CellStyles => {
            g.rect(2.0, 3.0, 12.0, 4.5, Some(Color32::from_rgb(0xC6, 0xEF, 0xCE)), Some(light));
            g.rect(2.0, 8.5, 12.0, 4.5, Some(Color32::from_rgb(0xFF, 0xC7, 0xCE)), Some(light));
            g.stroke(&[(4.0, 5.2), (9.0, 5.2)], GREEN);
            g.stroke(&[(4.0, 10.7), (9.0, 10.7)], RED);
        }
        Icon::Insert | Icon::Delete | Icon::Format => {
            g.rect(2.0, 3.0, 12.0, 10.0, Some(Color32::WHITE), Some(light));
            g.stroke(&[(2.0, 6.5), (14.0, 6.5)], light);
            g.stroke(&[(2.0, 10.0), (14.0, 10.0)], light);
            g.stroke(&[(6.0, 3.0), (6.0, 13.0)], light);
            match icon {
                Icon::Insert => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), Some(BLUE));
                    g.line(&[(1.0, 8.2), (4.5, 8.2)], GREEN, 1.6);
                    g.line(&[(2.75, 6.5), (2.75, 10.0)], GREEN, 1.6);
                }
                Icon::Delete => {
                    g.line(&[(8.0, 6.0), (13.0, 11.0)], RED, 1.6);
                    g.line(&[(13.0, 6.0), (8.0, 11.0)], RED, 1.6);
                }
                _ => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
                }
            }
        }
        Icon::Sum => g.text(8.0, 8.0, "Σ", 13.0, c, false, false),
        Icon::FillDown => {
            g.rect(4.0, 2.0, 8.0, 12.0, Some(Color32::WHITE), Some(light));
            g.stroke(&[(8.0, 4.0), (8.0, 11.0)], BLUE);
            g.poly(&[(5.5, 9.5), (10.5, 9.5), (8.0, 13.0)], BLUE);
        }
        Icon::Eraser => {
            g.poly(&[(2.0, 10.0), (8.0, 4.0), (12.0, 8.0), (6.0, 14.0)], Color32::from_rgb(0xE9, 0x8A, 0xB8));
            g.poly(&[(8.0, 4.0), (10.5, 1.5), (14.5, 5.5), (12.0, 8.0)], Color32::from_rgb(0xB6, 0x4E, 0x86));
        }
        Icon::SortFilter => {
            g.text(4.5, 4.5, "A", 6.0, c, false, false);
            g.text(4.5, 11.5, "Z", 6.0, c, false, false);
            g.stroke(&[(8.0, 3.0), (8.0, 13.0)], BLUE);
            g.poly(&[(6.5, 11.0), (9.5, 11.0), (8.0, 13.5)], BLUE);
            g.poly(&[(10.0, 3.0), (15.0, 3.0), (13.0, 7.5), (13.0, 12.0), (12.0, 11.0), (12.0, 7.5)], light);
        }
        Icon::Find | Icon::Search | Icon::Zoom => {
            g.circle(6.5, 6.5, 4.0, None, Some(c));
            g.line(&[(9.5, 9.5), (14.0, 14.0)], c, 1.6);
        }
        Icon::Undo | Icon::Redo => {
            let flip = icon == Icon::Redo;
            let x = |v: f32| if flip { 16.0 - v } else { v };
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(x(4.0), 7.0), g.pt(x(9.0), 3.0), g.pt(x(15.0), 6.0), g.pt(x(12.0), 12.5)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.2, c),
            ));
            g.poly(&[(x(2.5), 7.5), (x(6.5), 4.5), (x(6.5), 9.5)], c);
        }
        Icon::Save => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(c));
            g.rect(5.0, 2.5, 6.0, 4.0, None, Some(c));
            g.rect(4.5, 9.0, 7.0, 4.5, None, Some(c));
        }
        Icon::Home => {
            g.line(&[(2.0, 8.0), (8.0, 2.5), (14.0, 8.0)], c, 1.2);
            g.line(&[(4.0, 6.5), (4.0, 13.5), (12.0, 13.5), (12.0, 6.5)], c, 1.2);
        }
        Icon::More => {
            for x in [4.0, 8.0, 12.0] {
                g.circle(x, 8.0, 1.0, Some(c), None);
            }
        }
        Icon::Chevron => g.line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)], c, 1.2),
        Icon::ChevronUp => g.line(&[(4.5, 10.0), (8.0, 6.5), (11.5, 10.0)], c, 1.2),
        Icon::Left => g.line(&[(10.0, 4.0), (6.0, 8.0), (10.0, 12.0)], c, 1.2),
        Icon::Right => g.line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)], c, 1.2),
        Icon::Close => {
            g.line(&[(4.0, 4.0), (12.0, 12.0)], c, 1.3);
            g.line(&[(12.0, 4.0), (4.0, 12.0)], c, 1.3);
        }
        Icon::Check => g.line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)], c, 1.3),
        Icon::Eye => {
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(2.0, 8.0), g.pt(5.0, 3.5), g.pt(11.0, 3.5), g.pt(14.0, 8.0)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.2, c),
            ));
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(14.0, 8.0), g.pt(11.0, 12.5), g.pt(5.0, 12.5), g.pt(2.0, 8.0)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.2, c),
            ));
            g.circle(8.0, 8.0, 2.0, Some(c), None);
        }
        Icon::Fx => g.text(8.0, 8.0, "fx", 10.5, c, false, true),
        Icon::Plus => {
            g.line(&[(8.0, 3.0), (8.0, 13.0)], c, 1.3);
            g.line(&[(3.0, 8.0), (13.0, 8.0)], c, 1.3);
        }
        Icon::Minus => g.line(&[(3.0, 8.0), (13.0, 8.0)], c, 1.3),
        Icon::Normal | Icon::Gridlines | Icon::Sheet => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], c);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], c);
            g.stroke(&[(2.0, 6.2), (14.0, 6.2)], c);
            g.stroke(&[(2.0, 9.8), (14.0, 9.8)], c);
        }
        Icon::PageLayout | Icon::Paper => {
            g.rect(3.5, 1.5, 9.0, 13.0, None, Some(c));
            for y in [5.0, 7.5, 10.0] {
                g.stroke(&[(5.5, y), (10.5, y)], light);
            }
        }
        Icon::PageBreak => {
            g.rect(2.5, 2.0, 11.0, 12.0, None, Some(c));
            g.line(&[(2.5, 8.0), (13.5, 8.0)], BLUE, 1.6);
        }
        Icon::Table | Icon::PivotTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(Color32::WHITE), Some(c));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(if icon == Icon::Table { BLUE } else { GREEN }), None);
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(2.0, 9.5), (14.0, 9.5)], light);
            if icon == Icon::PivotTable {
                g.poly(&[(9.0, 7.0), (12.5, 7.0), (12.5, 10.5)], ORANGE);
            }
        }
        Icon::Chart | Icon::ChartBar => {
            g.stroke(&[(2.0, 14.0), (14.0, 14.0)], c);
            g.rect(3.0, 8.0, 2.5, 6.0, Some(BLUE), None);
            g.rect(6.8, 4.0, 2.5, 10.0, Some(GREEN), None);
            g.rect(10.6, 6.0, 2.5, 8.0, Some(ORANGE), None);
        }
        Icon::ChartLine | Icon::Sparkline => {
            g.stroke(&[(2.0, 14.0), (14.0, 14.0)], light);
            g.line(&[(2.0, 11.0), (5.5, 7.0), (8.5, 9.5), (13.5, 3.5)], BLUE, 1.5);
        }
        Icon::ChartPie => {
            g.circle(8.0, 8.0, 6.0, Some(BLUE), None);
            g.poly(&[(8.0, 8.0), (8.0, 2.0), (12.0, 3.4), (14.0, 8.0)], ORANGE);
        }
        Icon::ChartScatter => {
            g.stroke(&[(2.0, 2.0), (2.0, 14.0), (14.0, 14.0)], c);
            for (x, y) in [(5.0, 11.0), (7.0, 8.0), (9.5, 9.5), (11.0, 5.0), (13.0, 4.0)] {
                g.circle(x, y, 1.1, Some(BLUE), None);
            }
        }
        Icon::Picture => {
            g.rect(2.0, 3.0, 12.0, 10.0, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), Some(c));
            g.poly(&[(3.0, 12.0), (7.0, 7.0), (10.0, 10.0), (11.5, 8.5), (13.0, 12.0)], GREEN);
            g.circle(11.0, 5.5, 1.2, Some(YELLOW), None);
        }
        Icon::Shapes => {
            g.circle(5.5, 5.5, 3.5, Some(BLUE), None);
            g.rect(7.0, 7.0, 7.0, 7.0, Some(ORANGE), None);
        }
        Icon::Link => {
            g.line(&[(6.0, 10.0), (10.0, 6.0)], c, 1.2);
            g.rect(2.0, 8.0, 6.0, 4.0, None, Some(c));
            g.rect(8.0, 4.0, 6.0, 4.0, None, Some(c));
        }
        Icon::Comment | Icon::Note => {
            g.poly(
                &[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (2.0, 11.0)],
                if icon == Icon::Note { Color32::from_rgb(0xFF, 0xF2, 0xA8) } else { Color32::WHITE },
            );
            g.line(&[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (7.0, 11.0), (4.0, 14.0), (4.0, 11.0), (2.0, 11.0), (2.0, 3.0)], c, 1.1);
        }
        Icon::TextBox => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.text(8.0, 8.0, "A", 8.0, c, false, false);
        }
        Icon::Symbol => g.text(8.0, 8.0, "Ω", 12.0, c, false, false),
        Icon::Function => g.text(8.0, 8.0, "fx", 11.0, c, false, true),
        Icon::Name => {
            g.rect(1.5, 4.0, 13.0, 8.0, None, Some(c));
            g.text(8.0, 8.0, "ab", 8.0, c, false, false);
        }
        Icon::Trace => {
            g.circle(4.0, 12.0, 1.6, Some(BLUE), None);
            g.circle(12.0, 4.0, 1.6, Some(BLUE), None);
            g.line(&[(5.0, 11.0), (11.0, 5.0)], BLUE, 1.2);
            g.poly(&[(12.5, 3.5), (9.0, 4.5), (11.5, 7.0)], BLUE);
        }
        Icon::Calc => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.rect(4.5, 3.0, 7.0, 3.0, Some(light), None);
            for (x, y) in [(5.5, 8.5), (8.0, 8.5), (10.5, 8.5), (5.5, 11.5), (8.0, 11.5), (10.5, 11.5)] {
                g.circle(x, y, 0.8, Some(c), None);
            }
        }
        Icon::Filter => g.poly(&[(2.0, 3.0), (14.0, 3.0), (9.5, 8.5), (9.5, 13.5), (6.5, 12.0), (6.5, 8.5)], c),
        Icon::SortAsc | Icon::SortDesc => {
            let (a, b) = if icon == Icon::SortAsc { ("A", "Z") } else { ("Z", "A") };
            g.text(5.0, 4.5, a, 6.5, c, false, false);
            g.text(5.0, 11.5, b, 6.5, c, false, false);
            g.stroke(&[(11.0, 2.5), (11.0, 13.0)], BLUE);
            g.poly(&[(9.0, 11.0), (13.0, 11.0), (11.0, 14.0)], BLUE);
        }
        Icon::Duplicates => {
            g.rect(2.0, 2.0, 9.0, 4.0, Some(Color32::WHITE), Some(c));
            g.rect(5.0, 7.0, 9.0, 4.0, Some(Color32::WHITE), Some(c));
            g.line(&[(9.0, 12.0), (14.0, 15.0)], RED, 1.4);
            g.line(&[(14.0, 12.0), (9.0, 15.0)], RED, 1.4);
        }
        Icon::TextColumns => {
            g.rect(1.5, 3.0, 5.5, 10.0, None, Some(c));
            g.rect(9.0, 3.0, 5.5, 10.0, None, Some(c));
            g.poly(&[(7.0, 6.0), (9.0, 8.0), (7.0, 10.0)], BLUE);
        }
        Icon::Validation => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.line(&[(4.0, 8.0), (6.5, 10.5), (11.5, 5.5)], GREEN, 1.4);
        }
        Icon::Group | Icon::Ungroup | Icon::Subtotal => {
            g.stroke(&[(3.0, 3.0), (3.0, 13.0), (6.0, 13.0)], c);
            for y in [4.0, 7.0, 10.0] {
                g.stroke(&[(7.0, y), (14.0, y)], light);
            }
            match icon {
                Icon::Group => g.text(3.0, 2.5, "+", 6.0, BLUE, true, false),
                Icon::Ungroup => g.text(3.0, 2.5, "−", 6.0, BLUE, true, false),
                _ => g.text(11.0, 13.0, "Σ", 6.0, BLUE, false, false),
            }
        }
        Icon::Lock => {
            g.rect(3.5, 7.0, 9.0, 7.0, Some(YELLOW), Some(c));
            g.line(&[(5.5, 7.0), (5.5, 4.5), (8.0, 2.5), (10.5, 4.5), (10.5, 7.0)], c, 1.2);
        }
        Icon::Spell => {
            g.text(7.0, 6.0, "abc", 7.0, c, false, false);
            g.line(&[(4.0, 11.0), (6.5, 13.5), (12.0, 8.5)], GREEN, 1.4);
        }
        Icon::Freeze => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
            g.rect(2.0, 2.5, 3.5, 11.0, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
            g.line(&[(2.0, 5.5), (14.0, 5.5)], BLUE, 1.2);
            g.line(&[(5.5, 2.5), (5.5, 13.5)], BLUE, 1.2);
        }
        Icon::Theme => {
            g.rect(2.0, 2.0, 5.5, 5.5, Some(BLUE), None);
            g.rect(8.5, 2.0, 5.5, 5.5, Some(ORANGE), None);
            g.rect(2.0, 8.5, 5.5, 5.5, Some(GREEN), None);
            g.rect(8.5, 8.5, 5.5, 5.5, Some(PURPLE), None);
        }
        Icon::Margins => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.rect(5.0, 3.5, 6.0, 9.0, None, Some(BLUE));
        }
        Icon::Orient => {
            g.rect(2.0, 4.0, 8.0, 10.0, None, Some(c));
            g.rect(6.0, 2.0, 8.0, 6.0, Some(Color32::WHITE), Some(BLUE));
        }
        Icon::PrintArea => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(light));
            g.line(&[(4.0, 4.0), (11.0, 4.0), (11.0, 10.0), (4.0, 10.0), (4.0, 4.0)], BLUE, 1.4);
        }
        Icon::Script => {
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(c));
            g.line(&[(4.5, 6.0), (7.0, 8.0), (4.5, 10.0)], GREEN, 1.3);
            g.stroke(&[(8.0, 10.5), (11.5, 10.5)], c);
        }
        Icon::Pen => {
            g.poly(&[(3.0, 13.0), (4.0, 10.0), (11.0, 3.0), (13.0, 5.0), (6.0, 12.0)], BLUE);
            g.stroke(&[(3.0, 13.0), (4.0, 10.0)], c);
        }
        Icon::Share => {
            g.line(&[(4.0, 7.0), (4.0, 13.0), (12.0, 13.0), (12.0, 7.0)], c, 1.2);
            g.line(&[(8.0, 10.0), (8.0, 2.5)], c, 1.2);
            g.line(&[(5.5, 5.0), (8.0, 2.5), (10.5, 5.0)], c, 1.2);
        }
        Icon::Settings => {
            g.circle(8.0, 8.0, 5.0, None, Some(c));
            g.circle(8.0, 8.0, 2.0, Some(c), None);
        }
        Icon::Book => {
            g.rect(2.0, 2.0, 12.0, 12.0, Some(GREEN), None);
            g.rect(4.0, 4.0, 8.0, 8.0, Some(Color32::WHITE), None);
            g.stroke(&[(8.0, 4.0), (8.0, 12.0)], GREEN);
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], GREEN);
        }
        Icon::Folder => {
            g.poly(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)], YELLOW);
        }
    }
}

/// Icons offered by Insert › Icons, by name.
pub const LIBRARY: &[(&str, Icon)] = &[
    ("check", Icon::Check),
    ("eye", Icon::Eye),
    ("close", Icon::Close),
    ("plus", Icon::Plus),
    ("minus", Icon::Minus),
    ("chart", Icon::Chart),
    ("line chart", Icon::ChartLine),
    ("pie chart", Icon::ChartPie),
    ("scatter", Icon::ChartScatter),
    ("table", Icon::Table),
    ("sum", Icon::Sum),
    ("filter", Icon::Filter),
    ("lock", Icon::Lock),
    ("comment", Icon::Comment),
    ("note", Icon::Note),
    ("picture", Icon::Picture),
    ("book", Icon::Book),
    ("folder", Icon::Folder),
    ("search", Icon::Search),
    ("calculator", Icon::Calc),
    ("link", Icon::Link),
    ("pen", Icon::Pen),
    ("share", Icon::Share),
    ("settings", Icon::Settings),
    ("home", Icon::Home),
    ("save", Icon::Save),
    ("theme", Icon::Theme),
    ("shapes", Icon::Shapes),
    ("text", Icon::TextBox),
    ("trace", Icon::Trace),
    ("validation", Icon::Validation),
];

pub fn from_name(name: &str) -> Option<Icon> {
    LIBRARY.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, i)| *i)
}
