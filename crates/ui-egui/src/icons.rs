//! GridCraft's icon set, drawn in code on a 16×16 design grid (original artwork; see
//! ATTRIBUTION.md). Ribbon commands draw the colorized design in `icons/colored.rs` (24-unit grid,
//! 20 px on large buttons and 16 px on small ones) with the theme's icon colour roles
//! (`Tokens::icon_inks`); other icons use Lucide or the drawings below.
//! Accent and paper colours come from the theme tokens, so icons follow light and dark mode;
//! icons placed on the sheet ([`paint_on_sheet`]) keep the light colours, as the sheet does.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use crate::theme::{Cat, IconRole, Tokens};

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
    Fx,
    Plus,
    /// Increase Font Size: an A with an up arrow.
    FontGrow,
    /// Decrease Font Size: an A with a down arrow.
    FontShrink,
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
    /// The title bar's Comments pane button: a plain speech bubble, no colour.
    CommentsPane,
    Note,
    TextBox,
    Symbol,
    Sparkline,
    Checkbox,
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
    SheetRtl,
    Paper,
    PrintArea,
    Script,
    Pen,
    Share,
    Search,
    Settings,
    /// Light/dark appearance: a circle, its right half filled.
    Appearance,
    Sheet,
    /// All Sheets navigator next to the sheet tabs: a list of rows with a bullet each.
    SheetList,
    Book,
    Folder,
    /// Header & Footer: a page with its top / bottom band marked.
    Header,
    Footer,
    PageNumber,
    PageCount,
    Calendar,
    Clock,
    FileName,
    /// File › Backstage navigation (line art, drawn in the navigation colour).
    Back,
    NewFile,
    OpenFolder,
    Info,
    Print,
    /// A macro: a script page with a run arrow.
    Macro,
    /// Record Macro: a ring around the recording dot.
    Record,
    /// Use Relative References: a cell and an arrow to a cell offset from it.
    Relative,
    /// A reference box's button: shrink the window to the box (Collapse) or restore it (Expand).
    Collapse,
    Expand,
    /// The macro debugger's Run: a play triangle.
    Play,
    /// Step: an arrow down onto a line.
    Step,
    /// Continue: a bar and a play triangle.
    Continue,
    /// Run to Here: an arrow to a marked line.
    RunTo,
    /// Stop: a square.
    StopSquare,
    // Ribbon commands that only the colorized design draws (`icons/colored.rs`); each used to
    // borrow another command's icon.
    DeleteCells,
    InsertIcons,
    ChartStatistic,
    ChartHierarchy,
    Slicer,
    HeaderFooter,
    SelectObjects,
    PenBlue,
    PenBlack,
    PenRed,
    Highlighter,
    DrawEraser,
    InkToShape,
    PaperSize,
    Breaks,
    PrintTitles,
    FnFinancial,
    FnLogical,
    FnText,
    FnDateTime,
    FnLookup,
    FnMathTrig,
    FnMore,
    DefineName,
    UseInFormula,
    CreateFromSelection,
    TraceDependents,
    RemoveArrows,
    ShowFormulas,
    ErrorCheck,
    EvaluateFormula,
    WatchWindow,
    CalcNow,
    CalcSheet,
    GetData,
    SortCustom,
    ClearFilter,
    Reapply,
    AdvancedFilter,
    FlashFill,
    WhatIf,
    Forecast,
    ShowDetail,
    HideDetail,
    OutlineSettings,
    DataAnalysis,
    Solver,
    WorkbookStats,
    Accessibility,
    DeleteComment,
    PrevComment,
    NextComment,
    ShowComments,
    ProtectSheet,
    ProtectWorkbook,
    AllowEditRanges,
    Zoom100,
    ZoomSelection,
    NewWindow,
    ArrangeAll,
    SwitchWindows,
    RunMacro,
    CommandPalette,
    AgentControl,
    ActionJournal,
    CheckUpdates,
    AboutApp,
    ResizeTable,
    ConvertToRange,
    ChartElement,
    SwitchRowCol,
    SelectData,
    ChangeChartType,
    FormatPane,
    ChartTitle,
    DeleteChart,
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
        self.p.add(PathShape::line(v, PathStroke::new(w * self.s.clamp(1.0, 1.4), c)));
    }
    fn stroke(&self, pts: &[(f32, f32)], c: Color32) {
        self.line(pts, c, 1.3);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, fill: Option<Color32>, stroke: Option<Color32>) {
        let r = Rect::from_min_size(self.pt(x, y), vec2(w * self.s, h * self.s));
        // Softly rounded corners and a stroke that scales with the icon (1.3 px at 16 px).
        let radius = (1.6 * self.s).min(w.min(h) * self.s / 2.0);
        if let Some(f) = fill {
            self.p.rect_filled(r, radius, f);
        }
        if let Some(c) = stroke {
            self.p.rect_stroke(r, radius, Stroke::new(1.3 * self.s.clamp(1.0, 1.4), c), egui::StrokeKind::Inside);
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
            self.p.circle_stroke(self.pt(x, y), r * self.s, Stroke::new(1.3 * self.s.clamp(1.0, 1.4), c));
        }
    }
    fn text(&self, x: f32, y: f32, t: &str, size: f32, c: Color32, style: (bool, bool)) {
        let fam = if style.0 {
            crate::theme::CELL_BOLD
        } else if style.1 {
            crate::theme::CELL_ITALIC
        } else {
            crate::theme::CELL
        };
        let font = egui::FontId::new(size * self.s, egui::FontFamily::Name(fam.into()));
        self.p.text(self.pt(x, y), egui::Align2::CENTER_CENTER, t, font, c);
    }
}

/// The colours an icon draws with besides its main colour: the paper inside documents and cells,
/// and the category palette for its coloured parts.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    paper: Color32,
    cat: [Color32; 8],
    recording: Color32,
}

impl Palette {
    fn of(t: &Tokens) -> Palette {
        Palette { paper: t.paper, cat: t.cat, recording: t.recording }
    }

    /// The chrome's icons: line drawings in the text colour whose one meaningful part takes the
    /// accent (where the older set used blue), so the toolbar reads as one family.
    fn chrome(t: &Tokens) -> Palette {
        let mut cat = t.cat;
        if let Some(b) = cat.get_mut(Cat::Blue as usize) {
            *b = t.accent;
        }
        if let Some(g) = cat.get_mut(Cat::Green as usize) {
            *g = t.accent;
        }
        Palette { paper: t.paper, cat, recording: t.recording }
    }

    fn cat(&self, c: Cat) -> Color32 {
        self.cat.get(c as usize).copied().unwrap_or(Color32::GRAY)
    }

    /// A pale wash of a category colour over the paper (cell fills inside icons).
    fn wash(&self, c: Cat) -> Color32 {
        self.cat(c).gamma_multiply(0.3)
    }
}

/// The code-drawn icon that stands in for a character the current fonts cannot draw (`✕` and
/// `✓` have no bundled egui face). Interface chrome uses this instead of painting a tofu box; a
/// workbook cell still needs a real glyph because its characters are user data.
pub fn fallback_for_symbol(c: char) -> Option<Icon> {
    match c {
        '✕' => Some(Icon::Close),
        '✓' => Some(Icon::Check),
        _ => None,
    }
}

/// Draws `icon` into `rect` (square-ish), in colour `c`, with the current theme's palette. Icons
/// with a colorized ribbon design draw it in full colour when `c` is the text ink; in any other
/// colour they keep their line drawing (menus, panes and notices tint icons with meaning).
pub fn paint(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    let t = Tokens::get(p.ctx());
    if paint_design(p, rect, icon, c, &t, false) {
        return;
    }
    paint_with(p, rect, icon, c, &Palette::chrome(&t));
}

/// Draws a ribbon button's `icon`: its colorized design, in full colour when `c` is the text ink
/// on an enabled button, else in `c` alone (disabled, or tinted by the caller) so the state still
/// reads. Icons without a design fall back to [`paint`].
pub fn paint_ribbon(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    let t = Tokens::get(p.ctx());
    if paint_design(p, rect, icon, c, &t, true) {
        return;
    }
    paint_with(p, rect, icon, c, &Palette::chrome(&t));
}

/// Draws `icon` on the sheet, which stays white in both themes, so with the light palette.
pub fn paint_on_sheet(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    paint_with(p, rect, icon, c, &Palette::of(&Tokens::light()));
}

/// Draws `icon` into `rect` (square-ish), in colour `c`, with palette `pal`.
fn paint_with(p: &Painter, rect: Rect, icon: Icon, c: Color32, pal: &Palette) {
    if icon == Icon::Fx {
        // A drawn f-hook with a crossbar and a small x (original; 16-unit grid, 1.4 px strokes).
        paint_paths(p, rect, 16.0, &FX_PATHS, 1.4, c);
        return;
    }
    if let Some(name) = lucide_name(icon)
        && paint_lucide(p, rect, name, c)
    {
        return;
    }
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    let g = G { p, r, s: side / 16.0 };
    let light = c.gamma_multiply(0.45);
    let paper = pal.paper;
    let blue = pal.cat(Cat::Blue);
    let green = pal.cat(Cat::Green);
    let red = pal.cat(Cat::Red);
    let orange = pal.cat(Cat::Orange);
    let purple = pal.cat(Cat::Purple);
    let yellow = pal.cat(Cat::Amber);
    match icon {
        Icon::Paste => {
            g.rect(2.5, 2.5, 9.5, 11.5, None, Some(c));
            g.rect(5.0, 1.5, 4.5, 2.5, Some(paper), Some(c));
            g.rect(6.5, 6.5, 7.5, 8.0, Some(pal.wash(Cat::Green)), Some(green));
        }
        Icon::Cut => {
            g.circle(5.0, 12.0, 2.0, None, Some(c));
            g.circle(11.0, 12.0, 2.0, None, Some(c));
            g.stroke(&[(6.3, 10.6), (11.5, 2.0)], c);
            g.stroke(&[(9.7, 10.6), (4.5, 2.0)], c);
        }
        Icon::Copy => {
            g.rect(2.5, 2.5, 7.5, 9.0, Some(paper), Some(c));
            g.rect(6.0, 5.5, 7.5, 9.0, Some(paper), Some(c));
        }
        Icon::Brush => {
            g.rect(2.5, 2.0, 10.0, 4.5, None, Some(c));
            g.stroke(&[(12.5, 4.2), (14.0, 4.2), (14.0, 8.5), (7.0, 8.5), (7.0, 10.0)], c);
            g.rect(5.8, 10.0, 2.6, 4.5, Some(green), None);
        }
        Icon::Bold => g.text(8.0, 8.5, "B", 12.0, c, (true, false)),
        Icon::Italic => g.text(8.0, 8.5, "I", 12.0, c, (false, true)),
        Icon::Underline => {
            g.text(8.0, 7.5, "U", 11.0, c, (false, false));
            g.stroke(&[(4.0, 14.0), (12.0, 14.0)], c);
        }
        Icon::Strike => {
            g.text(8.0, 8.0, "ab", 10.0, c, (false, false));
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
            g.poly(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0)], paper);
            g.line(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0), (3.0, 7.0)], c, 1.1);
            g.circle(13.5, 10.0, 1.2, Some(c), None);
        }
        Icon::FontColor => {
            g.text(8.0, 7.0, "A", 11.0, c, (false, false));
        }
        Icon::AlignTop
        | Icon::AlignMiddle
        | Icon::AlignBottom
        | Icon::AlignLeft
        | Icon::AlignCenter
        | Icon::AlignRight
        | Icon::IndentDec
        | Icon::IndentInc => {
            paint_alignment(&g, icon, c, light, blue);
        }
        Icon::Orientation => {
            g.text(6.0, 9.0, "ab", 8.5, c, (false, false));
            g.stroke(&[(9.0, 13.0), (14.0, 3.0)], blue);
            g.poly(&[(14.5, 2.0), (12.0, 4.0), (14.6, 5.2)], blue);
        }
        Icon::Wrap => {
            g.stroke(&[(2.0, 4.0), (14.0, 4.0)], c);
            g.stroke(&[(2.0, 8.0), (12.0, 8.0)], c);
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(12.0, 8.0), g.pt(15.0, 8.0), g.pt(15.0, 12.0), g.pt(11.0, 12.0)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.1, blue),
            ));
            g.poly(&[(9.0, 12.0), (11.5, 10.5), (11.5, 13.5)], blue);
            g.stroke(&[(2.0, 12.0), (7.0, 12.0)], c);
        }
        Icon::Merge => {
            g.rect(1.5, 3.5, 13.0, 9.0, None, Some(c));
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], blue);
            g.poly(&[(3.0, 8.0), (5.5, 6.5), (5.5, 9.5)], blue);
            g.poly(&[(13.0, 8.0), (10.5, 6.5), (10.5, 9.5)], blue);
        }
        Icon::Currency => g.text(8.0, 8.5, "$", 12.0, c, (false, false)),
        Icon::Percent => g.text(8.0, 8.5, "%", 11.0, c, (false, false)),
        Icon::Comma => g.text(8.0, 7.0, ",", 14.0, c, (true, false)),
        Icon::DecInc => {
            g.text(5.0, 5.0, ".0", 6.5, c, (false, false));
            g.text(10.0, 11.0, ".00", 6.5, c, (false, false));
            g.stroke(&[(2.0, 12.0), (5.0, 12.0)], blue);
            g.poly(&[(1.0, 12.0), (2.8, 10.6), (2.8, 13.4)], blue);
        }
        Icon::DecDec => {
            g.text(6.0, 5.0, ".00", 6.5, c, (false, false));
            g.text(11.0, 11.0, ".0", 6.5, c, (false, false));
            g.stroke(&[(2.0, 12.0), (5.5, 12.0)], blue);
            g.poly(&[(7.0, 12.0), (5.2, 10.6), (5.2, 13.4)], blue);
        }
        Icon::CondFormat => {
            g.rect(1.5, 2.0, 13.0, 12.0, None, Some(c));
            g.rect(4.0, 4.6, 5.5, 1.8, Some(green), None);
            g.rect(4.0, 7.2, 8.0, 1.8, Some(pal.wash(Cat::Green).gamma_multiply(2.2)), None);
            g.rect(4.0, 9.8, 3.5, 1.8, Some(orange), None);
        }
        Icon::FormatTable => {
            g.rect(1.5, 2.5, 13.0, 4.0, Some(green), None);
            g.rect(1.5, 2.5, 13.0, 11.5, None, Some(c));
            g.stroke(&[(1.5, 10.0), (14.5, 10.0)], c);
            g.stroke(&[(6.0, 6.5), (6.0, 14.0)], c);
            g.stroke(&[(10.0, 6.5), (10.0, 14.0)], c);
        }
        Icon::CellStyles => {
            g.rect(1.5, 2.5, 13.0, 5.0, Some(pal.wash(Cat::Green)), Some(green));
            g.rect(1.5, 8.8, 13.0, 5.0, Some(pal.wash(Cat::Red)), Some(red));
        }
        Icon::Insert | Icon::Delete | Icon::Format => {
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(c));
            match icon {
                Icon::Insert => {
                    g.stroke(&[(2.0, 8.0), (14.0, 8.0)], c);
                    g.line(&[(8.0, 4.8), (8.0, 11.2)], green, 1.5);
                }
                Icon::Delete => {
                    g.line(&[(5.8, 5.8), (10.2, 10.2)], red, 1.5);
                    g.line(&[(10.2, 5.8), (5.8, 10.2)], red, 1.5);
                }
                _ => {
                    g.stroke(&[(2.0, 6.0), (14.0, 6.0)], c);
                    g.stroke(&[(6.0, 6.0), (6.0, 14.0)], c);
                }
            }
        }
        Icon::Sum => g.text(8.0, 8.0, "Σ", 13.0, c, (false, false)),
        Icon::FillDown => {
            g.stroke(&[(8.0, 2.5), (8.0, 11.0)], c);
            g.stroke(&[(5.0, 8.2), (8.0, 11.2), (11.0, 8.2)], c);
            g.line(&[(3.0, 13.8), (13.0, 13.8)], green, 1.5);
        }
        Icon::Eraser => {
            g.stroke(&[(9.5, 2.5), (13.5, 6.5), (7.5, 12.5), (4.0, 12.5), (2.5, 11.0), (9.5, 2.5)], c);
            g.stroke(&[(6.0, 6.0), (10.0, 10.0)], c);
            g.stroke(&[(8.0, 13.8), (13.5, 13.8)], c);
        }
        Icon::SortFilter => {
            g.stroke(&[(1.8, 10.0), (4.0, 2.5), (6.2, 10.0)], c);
            g.stroke(&[(2.6, 7.4), (5.4, 7.4)], c);
            g.stroke(&[(8.5, 2.5), (14.5, 2.5), (12.2, 6.0), (12.2, 10.0), (10.8, 11.0), (10.8, 6.0), (8.5, 2.5)], c);
            g.line(&[(4.0, 11.5), (4.0, 15.0)], green, 1.4);
            g.line(&[(2.6, 13.6), (4.0, 15.0), (5.4, 13.6)], green, 1.4);
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
        Icon::SheetList => {
            for y in [4.0, 8.0, 12.0] {
                g.rect(2.5, y - 1.0, 2.0, 2.0, Some(c), None);
                g.stroke(&[(6.5, y), (13.5, y)], c);
            }
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
        Icon::Checkbox => {
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(c));
            g.line(&[(4.5, 8.0), (7.0, 10.5), (11.5, 5.0)], c, 1.4);
        }
        Icon::Fx => g.text(8.0, 8.0, "fx", 10.5, c, (false, true)),
        Icon::Plus => {
            g.line(&[(8.0, 3.0), (8.0, 13.0)], c, 1.3);
            g.line(&[(3.0, 8.0), (13.0, 8.0)], c, 1.3);
        }
        Icon::Minus => g.line(&[(3.0, 8.0), (13.0, 8.0)], c, 1.3),
        // Drawn from Lucide's a-arrow-up / a-arrow-down; this is the text fallback.
        Icon::FontGrow => g.text(8.0, 8.0, "A+", 10.0, c, (false, false)),
        Icon::FontShrink => g.text(8.0, 8.0, "A-", 10.0, c, (false, false)),
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
            g.line(&[(2.5, 8.0), (13.5, 8.0)], blue, 1.6);
        }
        Icon::Table | Icon::PivotTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(paper), Some(c));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(if icon == Icon::Table { blue } else { green }), None);
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(2.0, 9.5), (14.0, 9.5)], light);
            if icon == Icon::PivotTable {
                g.poly(&[(9.0, 7.0), (12.5, 7.0), (12.5, 10.5)], orange);
            }
        }
        Icon::Chart | Icon::ChartBar => {
            g.stroke(&[(2.0, 14.0), (14.0, 14.0)], c);
            g.rect(3.0, 8.0, 2.5, 6.0, Some(blue), None);
            g.rect(6.8, 4.0, 2.5, 10.0, Some(green), None);
            g.rect(10.6, 6.0, 2.5, 8.0, Some(orange), None);
        }
        Icon::ChartLine | Icon::Sparkline => {
            g.stroke(&[(2.0, 14.0), (14.0, 14.0)], light);
            g.line(&[(2.0, 11.0), (5.5, 7.0), (8.5, 9.5), (13.5, 3.5)], blue, 1.5);
        }
        Icon::ChartPie => {
            g.circle(8.0, 8.0, 6.0, Some(blue), None);
            g.poly(&[(8.0, 8.0), (8.0, 2.0), (12.0, 3.4), (14.0, 8.0)], orange);
        }
        Icon::ChartScatter => {
            g.stroke(&[(2.0, 2.0), (2.0, 14.0), (14.0, 14.0)], c);
            for (x, y) in [(5.0, 11.0), (7.0, 8.0), (9.5, 9.5), (11.0, 5.0), (13.0, 4.0)] {
                g.circle(x, y, 1.1, Some(blue), None);
            }
        }
        Icon::Picture => {
            g.rect(2.0, 3.0, 12.0, 10.0, Some(pal.wash(Cat::Blue)), Some(c));
            g.poly(&[(3.0, 12.0), (7.0, 7.0), (10.0, 10.0), (11.5, 8.5), (13.0, 12.0)], green);
            g.circle(11.0, 5.5, 1.2, Some(yellow), None);
        }
        Icon::Shapes => {
            g.circle(5.5, 5.5, 3.5, Some(blue), None);
            g.rect(7.0, 7.0, 7.0, 7.0, Some(orange), None);
        }
        Icon::Link => {
            g.line(&[(6.0, 10.0), (10.0, 6.0)], c, 1.2);
            g.rect(2.0, 8.0, 6.0, 4.0, None, Some(c));
            g.rect(8.0, 4.0, 6.0, 4.0, None, Some(c));
        }
        Icon::Comment | Icon::CommentsPane | Icon::Note => {
            g.poly(&[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (2.0, 11.0)], if icon == Icon::Note { pal.wash(Cat::Amber) } else { paper });
            g.line(&[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (7.0, 11.0), (4.0, 14.0), (4.0, 11.0), (2.0, 11.0), (2.0, 3.0)], c, 1.1);
        }
        Icon::TextBox => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.text(8.0, 8.0, "A", 8.0, c, (false, false));
        }
        Icon::Symbol => g.text(8.0, 8.0, "Ω", 12.0, c, (false, false)),
        Icon::Function => g.text(8.0, 8.0, "fx", 11.0, c, (false, true)),
        Icon::Name => {
            g.rect(1.5, 4.0, 13.0, 8.0, None, Some(c));
            g.text(8.0, 8.0, "ab", 8.0, c, (false, false));
        }
        Icon::Trace => {
            g.circle(4.0, 12.0, 1.6, Some(blue), None);
            g.circle(12.0, 4.0, 1.6, Some(blue), None);
            g.line(&[(5.0, 11.0), (11.0, 5.0)], blue, 1.2);
            g.poly(&[(12.5, 3.5), (9.0, 4.5), (11.5, 7.0)], blue);
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
            g.text(5.0, 4.5, a, 6.5, c, (false, false));
            g.text(5.0, 11.5, b, 6.5, c, (false, false));
            g.stroke(&[(11.0, 2.5), (11.0, 13.0)], blue);
            g.poly(&[(9.0, 11.0), (13.0, 11.0), (11.0, 14.0)], blue);
        }
        Icon::Duplicates => {
            g.rect(2.0, 2.0, 9.0, 4.0, Some(paper), Some(c));
            g.rect(5.0, 7.0, 9.0, 4.0, Some(paper), Some(c));
            g.line(&[(9.0, 12.0), (14.0, 15.0)], red, 1.4);
            g.line(&[(14.0, 12.0), (9.0, 15.0)], red, 1.4);
        }
        Icon::TextColumns => {
            g.rect(1.5, 3.0, 5.5, 10.0, None, Some(c));
            g.rect(9.0, 3.0, 5.5, 10.0, None, Some(c));
            g.poly(&[(7.0, 6.0), (9.0, 8.0), (7.0, 10.0)], blue);
        }
        Icon::Validation => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.line(&[(4.0, 8.0), (6.5, 10.5), (11.5, 5.5)], green, 1.4);
        }
        Icon::Group | Icon::Ungroup | Icon::Subtotal => {
            g.stroke(&[(3.0, 3.0), (3.0, 13.0), (6.0, 13.0)], c);
            for y in [4.0, 7.0, 10.0] {
                g.stroke(&[(7.0, y), (14.0, y)], light);
            }
            match icon {
                Icon::Group => g.text(3.0, 2.5, "+", 6.0, blue, (true, false)),
                Icon::Ungroup => g.text(3.0, 2.5, "−", 6.0, blue, (true, false)),
                _ => g.text(11.0, 13.0, "Σ", 6.0, blue, (false, false)),
            }
        }
        Icon::Lock => {
            g.rect(3.5, 7.0, 9.0, 7.0, Some(yellow), Some(c));
            g.line(&[(5.5, 7.0), (5.5, 4.5), (8.0, 2.5), (10.5, 4.5), (10.5, 7.0)], c, 1.2);
        }
        Icon::Spell => {
            g.text(7.0, 6.0, "abc", 7.0, c, (false, false));
            g.line(&[(4.0, 11.0), (6.5, 13.5), (12.0, 8.5)], green, 1.4);
        }
        Icon::Freeze => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(pal.wash(Cat::Blue)), None);
            g.rect(2.0, 2.5, 3.5, 11.0, Some(pal.wash(Cat::Blue)), None);
            g.line(&[(2.0, 5.5), (14.0, 5.5)], blue, 1.2);
            g.line(&[(5.5, 2.5), (5.5, 13.5)], blue, 1.2);
        }
        Icon::Theme => {
            g.rect(2.0, 2.0, 5.5, 5.5, Some(blue), None);
            g.rect(8.5, 2.0, 5.5, 5.5, Some(orange), None);
            g.rect(2.0, 8.5, 5.5, 5.5, Some(green), None);
            g.rect(8.5, 8.5, 5.5, 5.5, Some(purple), None);
        }
        Icon::Margins => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.rect(5.0, 3.5, 6.0, 9.0, None, Some(blue));
        }
        Icon::Orient => {
            g.rect(2.0, 4.0, 8.0, 10.0, None, Some(c));
            g.rect(6.0, 2.0, 8.0, 6.0, Some(paper), Some(blue));
        }
        Icon::SheetRtl => {
            // A sheet with an arrow running right to left.
            g.rect(2.0, 2.5, 12.0, 8.0, None, Some(c));
            g.stroke(&[(6.0, 2.5), (6.0, 10.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 10.5)], light);
            g.line(&[(13.5, 13.0), (4.5, 13.0)], blue, 1.4);
            g.poly(&[(2.0, 13.0), (5.0, 10.5), (5.0, 15.5)], blue);
        }
        Icon::PrintArea => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(light));
            g.line(&[(4.0, 4.0), (11.0, 4.0), (11.0, 10.0), (4.0, 10.0), (4.0, 4.0)], blue, 1.4);
        }
        Icon::Script => {
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(c));
            g.line(&[(4.5, 6.0), (7.0, 8.0), (4.5, 10.0)], green, 1.3);
            g.stroke(&[(8.0, 10.5), (11.5, 10.5)], c);
        }
        Icon::Pen => {
            g.poly(&[(3.0, 13.0), (4.0, 10.0), (11.0, 3.0), (13.0, 5.0), (6.0, 12.0)], blue);
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
        Icon::Appearance => {
            g.circle(8.0, 8.0, 5.5, None, Some(c));
            let half: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 12.0 - 0.5);
                    (8.0 + 5.5 * a.cos(), 8.0 + 5.5 * a.sin())
                })
                .collect();
            g.poly(&half, c);
        }
        Icon::Book => {
            g.rect(2.0, 2.0, 12.0, 12.0, Some(green), None);
            g.rect(4.0, 4.0, 8.0, 8.0, Some(paper), None);
            g.stroke(&[(8.0, 4.0), (8.0, 12.0)], green);
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], green);
        }
        Icon::Folder => {
            g.poly(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)], yellow);
        }
        Icon::Header | Icon::Footer => {
            g.rect(3.5, 1.5, 9.0, 13.0, None, Some(c));
            let y = if icon == Icon::Header { 2.5 } else { 11.0 };
            g.rect(4.5, y, 7.0, 2.5, Some(blue), None);
            for yy in if icon == Icon::Header { [7.0, 9.5] } else { [4.5, 7.0] } {
                g.stroke(&[(5.5, yy), (10.5, yy)], light);
            }
        }
        Icon::PageNumber | Icon::PageCount => {
            if icon == Icon::PageCount {
                g.rect(5.5, 1.0, 8.5, 11.5, None, Some(light));
            }
            g.rect(2.5, 3.0, 8.5, 12.0, Some(paper), Some(c));
            // A "#" mark.
            g.stroke(&[(5.3, 6.5), (4.8, 12.5)], blue);
            g.stroke(&[(8.3, 6.5), (7.8, 12.5)], blue);
            g.stroke(&[(4.0, 8.4), (9.6, 8.4)], blue);
            g.stroke(&[(3.8, 10.8), (9.4, 10.8)], blue);
        }
        Icon::Calendar => {
            g.rect(2.0, 3.0, 12.0, 11.0, Some(paper), Some(c));
            g.rect(2.0, 3.0, 12.0, 3.0, Some(blue), None);
            for (x, y) in [(4.5, 8.5), (7.5, 8.5), (10.5, 8.5), (4.5, 11.5), (7.5, 11.5)] {
                g.rect(x - 0.6, y - 0.6, 1.4, 1.4, Some(c), None);
            }
        }
        Icon::Clock => {
            let center = g.pt(8.0, 8.0);
            g.p.circle_stroke(center, 6.0 * g.s, Stroke::new(1.15 * g.s.clamp(1.0, 2.0), c));
            g.stroke(&[(8.0, 4.5), (8.0, 8.0), (10.8, 9.6)], blue);
        }
        Icon::FileName => {
            g.poly(&[(3.0, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (3.0, 14.5)], paper);
            g.line(&[(3.0, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (3.0, 14.5), (3.0, 1.5)], c, 1.15);
            g.stroke(&[(10.0, 1.5), (10.0, 4.5), (13.0, 4.5)], c);
            g.line(&[(5.0, 9.0), (11.0, 9.0)], blue, 1.4);
            g.stroke(&[(5.0, 11.5), (9.0, 11.5)], light);
        }
        Icon::Back => {
            g.circle(8.0, 8.0, 6.5, None, Some(c));
            g.line(&[(11.5, 8.0), (4.8, 8.0)], c, 1.2);
            g.line(&[(7.5, 5.3), (4.8, 8.0), (7.5, 10.7)], c, 1.2);
        }
        Icon::NewFile => {
            g.line(&[(9.5, 1.5), (3.5, 1.5), (3.5, 14.5), (12.5, 14.5), (12.5, 4.5), (9.5, 1.5), (9.5, 4.5), (12.5, 4.5)], c, 1.1);
        }
        Icon::OpenFolder => {
            g.line(&[(13.0, 6.5), (13.0, 4.5), (7.5, 4.5), (6.0, 3.0), (1.5, 3.0), (1.5, 13.0)], c, 1.1);
            g.line(&[(1.5, 13.0), (4.0, 7.0), (15.0, 7.0), (12.5, 13.0), (1.5, 13.0)], c, 1.1);
        }
        Icon::Info => {
            g.circle(8.0, 8.0, 6.5, None, Some(c));
            g.line(&[(8.0, 7.0), (8.0, 11.8)], c, 1.3);
            g.circle(8.0, 4.8, 0.8, Some(c), None);
        }
        Icon::Print => {
            g.line(&[(4.5, 5.5), (4.5, 1.5), (11.5, 1.5), (11.5, 5.5)], c, 1.1);
            g.line(&[(4.0, 11.5), (1.5, 11.5), (1.5, 5.5), (14.5, 5.5), (14.5, 11.5), (12.0, 11.5)], c, 1.1);
            g.line(&[(4.5, 9.0), (4.5, 14.5), (11.5, 14.5), (11.5, 9.0), (4.5, 9.0)], c, 1.1);
            g.circle(12.3, 7.6, 0.6, Some(c), None);
        }
        Icon::Macro => {
            g.rect(2.5, 1.5, 9.0, 12.0, Some(paper), Some(c));
            for y in [4.5, 7.0, 9.5] {
                g.stroke(&[(4.5, y), (9.5, y)], light);
            }
            g.poly(&[(9.5, 8.5), (15.0, 11.75), (9.5, 15.0)], green);
        }
        Icon::Record => {
            g.circle(8.0, 8.0, 6.0, None, Some(c));
            g.circle(8.0, 8.0, 3.5, Some(pal.recording), None);
        }
        Icon::Relative => {
            g.rect(1.5, 1.5, 5.5, 5.0, Some(paper), Some(c));
            g.rect(9.0, 9.5, 5.5, 5.0, None, Some(c));
            g.line(&[(5.0, 7.5), (5.0, 12.0), (8.0, 12.0)], c, 1.1);
            g.poly(&[(7.5, 10.2), (9.6, 12.0), (7.5, 13.8)], c);
        }
        Icon::Collapse | Icon::Expand => {
            // A small sheet with a selected block and an arrow pointing up (collapse) or down.
            g.rect(2.0, 6.5, 12.0, 7.5, None, Some(c));
            g.rect(4.0, 9.0, 4.0, 3.0, Some(c), None);
            if icon == Icon::Collapse {
                g.poly(&[(8.0, 1.0), (11.5, 4.5), (4.5, 4.5)], c);
            } else {
                g.poly(&[(8.0, 5.5), (11.5, 2.0), (4.5, 2.0)], c);
            }
        }
        Icon::Play => g.poly(&[(4.0, 2.5), (13.0, 8.0), (4.0, 13.5)], green),
        Icon::Step => {
            g.line(&[(8.0, 1.5), (8.0, 9.0)], c, 1.3);
            g.poly(&[(4.8, 7.0), (11.2, 7.0), (8.0, 10.8)], c);
            g.line(&[(3.0, 13.5), (13.0, 13.5)], c, 1.3);
        }
        Icon::Continue => {
            g.rect(2.5, 2.5, 2.2, 11.0, Some(c), None);
            g.poly(&[(6.5, 2.5), (14.0, 8.0), (6.5, 13.5)], green);
        }
        Icon::RunTo => {
            g.line(&[(2.0, 5.0), (9.0, 5.0)], c, 1.3);
            g.poly(&[(8.0, 2.0), (12.0, 5.0), (8.0, 8.0)], c);
            g.line(&[(2.0, 11.5), (14.0, 11.5)], c, 1.6);
            g.line(&[(13.0, 3.0), (13.0, 9.0)], c, 1.1);
        }
        Icon::StopSquare => g.rect(3.0, 3.0, 10.0, 10.0, Some(pal.recording), None),
        // The design-only icons are drawn by `paint_design`.
        _ => {}
    }
}

/// Icons offered by Insert › Icons, by name.
pub const LIBRARY: &[(&str, Icon)] = &[
    ("check", Icon::Check),
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

fn paint_alignment(g: &G<'_>, icon: Icon, c: Color32, light: Color32, blue: Color32) {
    if matches!(icon, Icon::AlignTop | Icon::AlignMiddle | Icon::AlignBottom) {
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
    } else {
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
            g.poly(&[(2.0, 6.0), (5.0, 8.0), (2.0, 10.0)], blue);
        }
        if icon == Icon::IndentDec {
            g.poly(&[(5.0, 6.0), (2.0, 8.0), (5.0, 10.0)], blue);
        }
    }
}

mod lucide;

/// The Lucide drawing used for `icon`, when Lucide has one (the spreadsheet-specific icons —
/// borders, cell styles, number formats — stay drawn in code above).
fn lucide_name(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Paste => "clipboard-paste",
        Icon::Cut => "scissors",
        Icon::Copy => "copy",
        Icon::Brush => "paintbrush",
        Icon::Bold => "bold",
        Icon::Italic => "italic",
        Icon::Underline => "underline",
        Icon::Strike => "strikethrough",
        Icon::Fill => "paint-bucket",
        Icon::FontColor => "baseline",
        Icon::Borders => "square-dashed-bottom",
        Icon::BorderAll => "grid-2x2",
        Icon::BorderNone => "square-dashed",
        Icon::BorderOutside => "square",
        Icon::Merge => "table-cells-merge",
        Icon::Orientation => "rotate-ccw-square",
        Icon::Currency => "dollar-sign",
        Icon::AlignLeft => "align-left",
        Icon::AlignCenter => "align-center",
        Icon::AlignRight => "align-right",
        Icon::IndentDec => "indent-decrease",
        Icon::IndentInc => "indent-increase",
        Icon::Wrap => "wrap-text",
        Icon::Percent => "percent",
        Icon::Sum => "sigma",
        Icon::FillDown => "arrow-down-to-line",
        Icon::Eraser => "eraser",
        Icon::Find | Icon::Search => "search",
        Icon::Zoom => "zoom-in",
        Icon::Undo => "undo-2",
        Icon::Redo => "redo-2",
        Icon::Save => "save",
        Icon::Home => "house",
        Icon::More => "ellipsis",
        Icon::Chevron => "chevron-down",
        Icon::ChevronUp => "chevron-up",
        Icon::Close => "x",
        Icon::Check => "check",
        Icon::Plus => "plus",
        Icon::FontGrow => "a-arrow-up",
        Icon::FontShrink => "a-arrow-down",
        Icon::Minus => "minus",
        Icon::Left => "chevron-left",
        Icon::Right => "chevron-right",
        Icon::Table => "table",
        Icon::PivotTable => "table-2",
        Icon::Chart => "chart-column",
        Icon::ChartBar => "chart-bar",
        Icon::ChartLine => "chart-line",
        Icon::ChartPie => "chart-pie",
        Icon::ChartScatter => "chart-scatter",
        Icon::Picture => "image",
        Icon::Shapes => "shapes",
        Icon::Link => "link",
        Icon::Comment | Icon::CommentsPane => "message-square",
        Icon::Note => "sticky-note",
        Icon::TextBox => "text-cursor-input",
        Icon::Symbol => "omega",
        Icon::Sparkline => "activity",
        Icon::Checkbox => "square-check",
        Icon::Function => "square-function",
        Icon::Name => "tag",
        Icon::Calc => "calculator",
        Icon::Filter => "filter",
        Icon::SortAsc => "arrow-down-a-z",
        Icon::SortDesc => "arrow-down-z-a",
        Icon::Duplicates => "copy-x",
        Icon::TextColumns => "columns-3",
        Icon::Validation => "shield-check",
        Icon::Group => "group",
        Icon::Ungroup => "ungroup",
        Icon::Lock => "lock",
        Icon::Spell => "spell-check",
        Icon::Freeze => "snowflake",
        Icon::Gridlines => "grid-3x3",
        Icon::Theme => "palette",
        Icon::Paper => "file",
        Icon::Script => "square-terminal",
        Icon::Pen => "pen",
        Icon::Share => "share",
        Icon::Settings => "settings",
        Icon::Appearance => "contrast",
        Icon::Sheet => "sheet",
        Icon::SheetList => "list",
        Icon::Book => "book-open",
        Icon::Folder => "folder",
        Icon::Header => "panel-top",
        Icon::Footer => "panel-bottom",
        Icon::PageNumber => "hash",
        Icon::Calendar => "calendar",
        Icon::Clock => "clock",
        Icon::Back => "arrow-left",
        Icon::NewFile => "file-plus",
        Icon::OpenFolder => "folder-open",
        Icon::Info => "info",
        Icon::Print => "printer",
        Icon::Record => "circle-dot",
        Icon::Collapse => "minimize-2",
        Icon::Expand => "maximize-2",
        Icon::Play => "play",
        Icon::StopSquare => "square",
        _ => return None,
    })
}

/// One flattened stroke of a Lucide icon on its 24×24 grid, and whether it closes.
type Polyline = (Vec<(f32, f32)>, bool);

/// The Lucide icons, parsed and flattened once.
fn lucide_shapes(name: &str) -> Option<&'static [Polyline]> {
    static CACHE: std::sync::OnceLock<std::collections::HashMap<&'static str, Vec<Polyline>>> = std::sync::OnceLock::new();
    let map =
        CACHE.get_or_init(|| lucide::ICONS.iter().map(|(n, paths)| (*n, paths.iter().flat_map(|d| flatten_path(d)).map(dedupe).collect())).collect());
    map.get(name).map(Vec::as_slice)
}

/// Drops repeated points (a closed path that ends on its start, curve steps that do not move):
/// a zero-length segment makes egui's mitred join spike far outside the icon.
fn dedupe((pts, closed): Polyline) -> Polyline {
    let mut v: Vec<(f32, f32)> = Vec::with_capacity(pts.len());
    for p in pts {
        if v.last().is_none_or(|q: &(f32, f32)| (q.0 - p.0).abs() + (q.1 - p.1).abs() > 0.05) {
            v.push(p);
        }
    }
    if closed && v.len() > 2 && v.first().zip(v.last()).is_some_and(|(a, b)| (a.0 - b.0).abs() + (a.1 - b.1).abs() <= 0.05) {
        v.pop();
    }
    (v, closed)
}

const FX_PATHS: [&str; 3] =
    ["M3.5 13.25c1.4 0 2.05-.75 2.2-2.1l.75-6.3C6.6 3.5 7.3 2.75 8.75 2.75", "M4.25 6.75h3.75", "M9.75 8.5l3.5 4.25M13.25 8.5l-3.5 4.25"];

/// Draws SVG path data drawn on a `grid`-unit square into `rect`, with round caps and joins; the
/// stroke is `width` at 16 px and scales with the icon.
fn paint_paths(p: &Painter, rect: Rect, grid: f32, paths: &[&str], width: f32, c: Color32) {
    let side = rect.width().min(rect.height());
    let s = side / grid;
    let origin = rect.center() - vec2(side / 2.0, side / 2.0);
    let w = (width * side / 16.0).clamp(1.0, 2.2);
    for d in paths {
        for (pts, closed) in flatten_path(d) {
            let v: Vec<Pos2> = pts.iter().map(|(x, y)| origin + vec2(x * s, y * s)).collect();
            if v.len() < 2 {
                continue;
            }
            let stroke = PathStroke::new(w, c);
            p.add(if closed { PathShape::closed_line(v.clone(), stroke) } else { PathShape::line(v.clone(), stroke) });
            for q in &v {
                p.circle_filled(*q, w / 2.0, c);
            }
        }
    }
}

/// Draws a Lucide icon in `rect`, its 2 px strokes scaled to the icon size (1.33 px at 16 px).
fn paint_lucide(p: &Painter, rect: Rect, name: &str, c: Color32) -> bool {
    let Some(shapes) = lucide_shapes(name) else { return false };
    let side = rect.width().min(rect.height());
    let s = side / 24.0;
    let origin = rect.center() - vec2(side / 2.0, side / 2.0);
    let width = (2.0 * s).clamp(1.0, 1.7);
    for (pts, closed) in shapes {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| origin + vec2(x * s, y * s)).collect();
        if v.len() < 2 {
            if let Some(dot) = v.first() {
                p.circle_filled(*dot, width / 2.0, c);
            }
            continue;
        }
        let stroke = PathStroke::new(width, c);
        p.add(if *closed { PathShape::closed_line(v.clone(), stroke) } else { PathShape::line(v.clone(), stroke) });
        // Round line caps and joins, as Lucide draws them.
        for q in v.iter().skip(usize::from(*closed)).step_by(1) {
            p.circle_filled(*q, width / 2.0, c);
        }
    }
    true
}

/// A tiny SVG path-data parser (M L H V C S Q T A Z, absolute and relative) that flattens curves
/// and arcs into polylines. Malformed data ends the path early instead of failing.
fn flatten_path(d: &str) -> Vec<Polyline> {
    let mut toks: Vec<Tok> = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, toks: &mut Vec<Tok>| {
        if !num.is_empty() {
            if let Ok(v) = num.parse::<f32>() {
                toks.push(Tok::Num(v));
            }
            num.clear();
        }
    };
    let mut prev = ' ';
    for ch in d.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' if ch != 'e' && ch != 'E' => {
                flush(&mut num, &mut toks);
                toks.push(Tok::Cmd(ch));
            }
            '-' if !(prev == 'e' || prev == 'E') => {
                flush(&mut num, &mut toks);
                num.push(ch);
            }
            '.' if num.contains('.') && !num.contains('e') => {
                flush(&mut num, &mut toks);
                num.push(ch);
            }
            ',' | ' ' | '\n' | '\t' => flush(&mut num, &mut toks),
            _ => num.push(ch),
        }
        prev = ch;
    }
    flush(&mut num, &mut toks);

    let mut out: Vec<Polyline> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let (mut x, mut y, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut last_ctrl: Option<(f32, f32)> = None;
    let mut cmd = 'M';
    let mut i = 0;
    let nums = |i: &mut usize, n: usize| -> Option<Vec<f32>> {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            match toks.get(*i) {
                Some(Tok::Num(f)) => v.push(*f),
                _ => return None,
            }
            *i += 1;
        }
        Some(v)
    };
    loop {
        match toks.get(i) {
            None => break,
            Some(Tok::Cmd(c)) => {
                cmd = *c;
                i += 1;
                if cmd == 'Z' || cmd == 'z' {
                    if cur.len() > 1 {
                        out.push((std::mem::take(&mut cur), true));
                    } else {
                        cur.clear();
                    }
                    x = sx;
                    y = sy;
                    last_ctrl = None;
                    continue;
                }
            }
            Some(Tok::Num(_)) => {}
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let Some(v) = nums(&mut i, 2) else { break };
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), false));
                }
                cur.clear();
                x = ox + v[0];
                y = oy + v[1];
                sx = x;
                sy = y;
                cur.push((x, y));
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let Some(v) = nums(&mut i, 2) else { break };
                x = ox + v[0];
                y = oy + v[1];
                cur.push((x, y));
                last_ctrl = None;
            }
            'H' => {
                let Some(v) = nums(&mut i, 1) else { break };
                x = if rel { x + v[0] } else { v[0] };
                cur.push((x, y));
                last_ctrl = None;
            }
            'V' => {
                let Some(v) = nums(&mut i, 1) else { break };
                y = if rel { y + v[0] } else { v[0] };
                cur.push((x, y));
                last_ctrl = None;
            }
            'C' | 'S' => {
                let smooth = cmd.eq_ignore_ascii_case(&'S');
                let Some(v) = nums(&mut i, if smooth { 4 } else { 6 }) else { break };
                let (c1, c2, end) = if smooth {
                    let c1 = last_ctrl.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy));
                    (c1, (ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]))
                } else {
                    ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]), (ox + v[4], oy + v[5]))
                };
                for k in 1..=12 {
                    let t = k as f32 / 12.0;
                    let u = 1.0 - t;
                    let px = u * u * u * x + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * end.0;
                    let py = u * u * u * y + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * end.1;
                    cur.push((px, py));
                }
                last_ctrl = Some(c2);
                (x, y) = end;
            }
            'Q' | 'T' => {
                let smooth = cmd.eq_ignore_ascii_case(&'T');
                let Some(v) = nums(&mut i, if smooth { 2 } else { 4 }) else { break };
                let (c, end) = if smooth {
                    (last_ctrl.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy)), (ox + v[0], oy + v[1]))
                } else {
                    ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]))
                };
                for k in 1..=10 {
                    let t = k as f32 / 10.0;
                    let u = 1.0 - t;
                    cur.push((u * u * x + 2.0 * u * t * c.0 + t * t * end.0, u * u * y + 2.0 * u * t * c.1 + t * t * end.1));
                }
                last_ctrl = Some(c);
                (x, y) = end;
            }
            'A' => {
                let Some(v) = nums(&mut i, 7) else { break };
                let end = (ox + v[5], oy + v[6]);
                arc_points(&mut cur, (x, y), (v[0], v[1]), v[2], v[3] != 0.0, v[4] != 0.0, end);
                (x, y) = end;
                last_ctrl = None;
            }
            _ => break,
        }
    }
    if !cur.is_empty() {
        out.push((cur, false));
    }
    out
}

#[derive(Clone, Copy)]
enum Tok {
    Cmd(char),
    Num(f32),
}

/// Appends the points of an SVG elliptical arc (endpoint form) after `from`.
fn arc_points(out: &mut Vec<(f32, f32)>, from: (f32, f32), radii: (f32, f32), rot_deg: f32, large: bool, sweep: bool, to: (f32, f32)) {
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    if rx < 1e-4 || ry < 1e-4 || (from.0 - to.0).abs() + (from.1 - to.1).abs() < 1e-5 {
        out.push(to);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let k = lambda.sqrt();
        rx *= k;
        ry *= k;
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cos * cx1 - sin * cy1 + (from.0 + to.0) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from.1 + to.1) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let th1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dth = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dth > 0.0 {
        dth -= std::f32::consts::TAU;
    } else if sweep && dth < 0.0 {
        dth += std::f32::consts::TAU;
    }
    let steps = ((dth.abs() / std::f32::consts::TAU) * 40.0).ceil().clamp(2.0, 40.0) as usize;
    for k in 1..=steps {
        let t = th1 + dth * k as f32 / steps as f32;
        let (st, ct) = t.sin_cos();
        out.push((cx + cos * rx * ct - sin * ry * st, cy + sin * rx * ct + cos * ry * st));
    }
}

mod colored;

/// The colorized design drawn for `icon` (a key of `colored::ICONS`), when it has one. `large`
/// picks the large-button variant where the design draws one (AutoSum).
fn design_key(icon: Icon, large: bool) -> Option<&'static str> {
    Some(match icon {
        // Home
        Icon::Paste => "paste",
        Icon::Cut => "cut",
        Icon::Copy => "copy",
        Icon::Brush => "painter",
        Icon::FontGrow => "grow",
        Icon::FontShrink => "shrink",
        Icon::Bold => "bold",
        Icon::Italic => "italic",
        Icon::Underline => "underline",
        Icon::Strike => "strike",
        Icon::Borders => "borders",
        Icon::Fill => "fillColor",
        Icon::FontColor => "fontColor",
        Icon::AlignTop => "alignTop",
        Icon::AlignMiddle => "alignMiddle",
        Icon::AlignBottom => "alignBottom",
        Icon::Orientation => "orientation",
        Icon::Wrap => "wrap",
        Icon::AlignLeft => "alignLeft",
        Icon::AlignCenter => "alignCenter",
        Icon::AlignRight => "alignRight",
        Icon::IndentDec => "indentDec",
        Icon::IndentInc => "indentInc",
        Icon::Merge => "merge",
        Icon::Currency => "accounting",
        Icon::Percent => "percent",
        Icon::Comma => "comma",
        Icon::DecInc => "decInc",
        Icon::DecDec => "decDec",
        Icon::CondFormat => "condFormat",
        Icon::FormatTable => "formatTable",
        Icon::CellStyles => "cellStyles",
        Icon::Insert => "insertCells",
        Icon::DeleteCells => "deleteCells",
        Icon::Format => "formatCells",
        Icon::Sum if large => "autosumL",
        Icon::Sum => "autosum",
        Icon::FillDown => "fill",
        Icon::Eraser => "clear",
        Icon::SortFilter => "sortFilter",
        Icon::Find => "find",
        // Insert
        Icon::PivotTable => "pivot",
        Icon::Table => "table",
        Icon::Picture => "pictures",
        Icon::Shapes => "shapes",
        Icon::InsertIcons => "icons",
        Icon::Chart => "recCharts",
        Icon::ChartBar => "colBar",
        Icon::ChartLine => "lineArea",
        Icon::ChartPie => "pie",
        Icon::ChartScatter => "scatter",
        Icon::ChartStatistic => "statistic",
        Icon::ChartHierarchy => "hierarchy",
        Icon::Slicer => "slicer",
        Icon::Sparkline => "sparklines",
        Icon::Checkbox => "checkbox",
        Icon::Link => "link",
        Icon::Comment => "comment",
        Icon::TextBox => "textBox",
        Icon::HeaderFooter => "headerFooter",
        Icon::Symbol => "symbol",
        // Draw
        Icon::SelectObjects => "select",
        Icon::PenBlue => "penBlue",
        Icon::PenBlack => "penBlack",
        Icon::PenRed => "penRed",
        Icon::Highlighter => "highlighter",
        Icon::DrawEraser => "eraser",
        Icon::InkToShape => "inkToShape",
        // Page Layout
        Icon::Theme => "themes",
        Icon::Margins => "margins",
        Icon::Orient => "orient",
        Icon::PaperSize => "size",
        Icon::PrintArea => "printArea",
        Icon::Breaks => "breaks",
        Icon::PrintTitles => "printTitles",
        Icon::SheetRtl => "rtl",
        // Formulas
        Icon::Function => "insertFunction",
        Icon::FnFinancial => "financial",
        Icon::FnLogical => "logical",
        Icon::FnText => "text",
        Icon::FnDateTime => "dateTime",
        Icon::FnLookup => "lookup",
        Icon::FnMathTrig => "mathTrig",
        Icon::FnMore => "moreFunctions",
        Icon::Name => "nameManager",
        Icon::DefineName => "defineName",
        Icon::UseInFormula => "useInFormula",
        Icon::CreateFromSelection => "createFromSel",
        Icon::Trace => "tracePrec",
        Icon::TraceDependents => "traceDep",
        Icon::RemoveArrows => "removeArrows",
        Icon::ShowFormulas => "showFormulas",
        Icon::ErrorCheck => "errorCheck",
        Icon::EvaluateFormula => "evaluate",
        Icon::WatchWindow => "watchWindow",
        Icon::Calc => "calcOptions",
        Icon::CalcNow => "calcNow",
        Icon::CalcSheet => "calcSheet",
        // Data
        Icon::GetData => "getData",
        Icon::SortAsc => "sortAZ",
        Icon::SortDesc => "sortZA",
        Icon::SortCustom => "sort",
        Icon::Filter => "filter",
        Icon::ClearFilter => "clearFilter",
        Icon::Reapply => "reapply",
        Icon::AdvancedFilter => "advanced",
        Icon::TextColumns => "textToColumns",
        Icon::FlashFill => "flashFill",
        Icon::Duplicates => "removeDup",
        Icon::Validation => "validation",
        Icon::WhatIf => "whatIf",
        Icon::Forecast => "forecast",
        Icon::Group => "group",
        Icon::Ungroup => "ungroup",
        Icon::Subtotal => "subtotal",
        Icon::ShowDetail => "showDetail",
        Icon::HideDetail => "hideDetail",
        Icon::OutlineSettings => "outlineSettings",
        Icon::DataAnalysis => "dataAnalysis",
        Icon::Solver => "solver",
        // Review
        Icon::Spell => "spelling",
        Icon::WorkbookStats => "wbStats",
        Icon::Accessibility => "accessibility",
        Icon::DeleteComment => "deleteComment",
        Icon::PrevComment => "previous",
        Icon::NextComment => "next",
        Icon::ShowComments => "showComments",
        Icon::Note => "notes",
        Icon::ProtectSheet => "protectSheet",
        Icon::ProtectWorkbook => "protectWorkbook",
        Icon::AllowEditRanges => "allowEdit",
        // View
        Icon::Normal => "normal",
        Icon::PageBreak => "pageBreakPreview",
        Icon::PageLayout => "pageLayoutView",
        Icon::Zoom => "zoom",
        Icon::Zoom100 => "zoom100",
        Icon::ZoomSelection => "zoomSel",
        Icon::NewWindow => "newWindow",
        Icon::ArrangeAll => "arrangeAll",
        Icon::Freeze => "freeze",
        Icon::SwitchWindows => "switchWindows",
        // Automate
        Icon::Script => "macros",
        Icon::Record => "record",
        Icon::Relative => "relative",
        Icon::Macro => "newMacro",
        Icon::RunMacro => "run",
        Icon::CommandPalette => "cmdPalette",
        Icon::AgentControl => "agentControl",
        Icon::ActionJournal => "journal",
        Icon::CheckUpdates => "updates",
        Icon::AboutApp => "about",
        // Table and chart tools
        Icon::ResizeTable => "resizeTable",
        Icon::ConvertToRange => "convertToRange",
        Icon::ChartElement => "addChartElement",
        Icon::SwitchRowCol => "switchRowCol",
        Icon::SelectData => "selectData",
        Icon::ChangeChartType => "changeChartType",
        Icon::FormatPane => "formatPane",
        Icon::ChartTitle => "editChartTitle",
        Icon::DeleteChart => "deleteChart",
        _ => return None,
    })
}

/// Whether `icon` exists only as a colorized design (no line drawing to fall back to).
fn design_only(icon: Icon) -> bool {
    // The design-only variants close the enum, from `DeleteCells` on.
    icon as u32 >= Icon::DeleteCells as u32
}

/// Whether `c` is the text ink of the current theme or of one of the built-in themes.
/// The opacity the ribbon's tab-switch fade draws at this frame (1 when not fading). A
/// colorized icon below it belongs to a disabled control; at it, the icon keeps its colours.
pub fn set_ribbon_fade(ctx: &egui::Context, a: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("icons.ribbon_fade"), a));
}

fn is_ink(c: Color32, t: &Tokens) -> bool {
    c == t.text || c == Tokens::light().text || c == Tokens::dark().text || c == Tokens::dark_green().text
}

/// Draws `icon`'s colorized design when it has one and the call wants it; false otherwise.
fn paint_design(p: &Painter, rect: Rect, icon: Icon, c: Color32, t: &Tokens, ribbon: bool) -> bool {
    let large = rect.width().min(rect.height()) >= 18.0;
    let Some(key) = design_key(icon, large) else { return false };
    // Disabled controls draw at a lowered opacity; a fading ribbon tab lowers it for every
    // control, so only opacity below the fade's counts as disabled.
    let fade = p.ctx().data(|d| d.get_temp::<f32>(egui::Id::unique("icons.ribbon_fade"))).unwrap_or(1.0);
    let full = is_ink(c, t) && p.opacity() >= fade - 0.001;
    if !full && !ribbon && !design_only(icon) {
        return false;
    }
    let Some(els) = design_shapes(key) else { return false };
    let inks = t.icon_inks();
    let color = |role: IconRole| -> Color32 {
        if full || role == IconRole::Paper {
            inks.get(role)
        } else if role.is_soft() {
            c.gamma_multiply(0.25)
        } else if role == IconRole::Dim {
            c.gamma_multiply(0.6)
        } else {
            c
        }
    };
    paint_colored(p, rect, els, large, &color);
    true
}

/// One element of a colorized icon with its path flattened (24-unit grid).
struct DesignEl {
    fill: Option<IconRole>,
    stroke: Option<IconRole>,
    width: Option<f32>,
    dash: Option<(f32, f32)>,
    paths: Vec<Polyline>,
}

/// The colorized designs, parsed and flattened once.
fn design_shapes(key: &str) -> Option<&'static [DesignEl]> {
    static CACHE: std::sync::OnceLock<std::collections::HashMap<&'static str, Vec<DesignEl>>> = std::sync::OnceLock::new();
    let map = CACHE.get_or_init(|| {
        colored::ICONS
            .iter()
            .map(|(k, els)| {
                let v = els
                    .iter()
                    .map(|e| DesignEl {
                        fill: e.fill,
                        stroke: e.stroke,
                        width: e.width,
                        dash: e.dash,
                        paths: flatten_path(e.d).into_iter().map(dedupe).collect(),
                    })
                    .collect();
                (*k, v)
            })
            .collect()
    });
    map.get(key).map(Vec::as_slice)
}

/// Paints a colorized design into `rect`: fills then strokes per element, in element order, with
/// round caps and joins. Strokes are 1.8 grid units on large (20 px) icons and 2 on small ones; a
/// width override applies as is on large icons and 0.2 wider on small ones.
fn paint_colored(p: &Painter, rect: Rect, els: &[DesignEl], large: bool, color: &dyn Fn(IconRole) -> Color32) {
    let side = rect.width().min(rect.height());
    let s = side / 24.0;
    let origin = rect.center() - vec2(side / 2.0, side / 2.0);
    let feather = 1.0 / p.ctx().pixels_per_point().max(0.1);
    let map = |pts: &[(f32, f32)]| -> Vec<Pos2> { pts.iter().map(|(x, y)| origin + vec2(x * s, y * s)).collect() };
    for el in els {
        if let Some(role) = el.fill {
            let c = color(role);
            for (pts, _) in &el.paths {
                fill_polygon(p, map(pts), c, feather);
            }
        }
        if let Some(role) = el.stroke {
            let c = color(role);
            let units = match (el.width, large) {
                (Some(w), true) => w,
                (Some(w), false) => w + 0.2,
                (None, true) => 1.8,
                (None, false) => 2.0,
            };
            let width = units * s;
            for (pts, closed) in &el.paths {
                let v = map(pts);
                match el.dash {
                    Some((on, off)) => stroke_dashed(p, &v, *closed, on * s, off * s, width, c),
                    None => stroke_round(p, v, *closed, width, c),
                }
            }
        }
    }
}

/// Strokes a polyline with round caps and joins.
fn stroke_round(p: &Painter, v: Vec<Pos2>, closed: bool, width: f32, c: Color32) {
    let r = width / 2.0;
    match v.len() {
        0 => return,
        1 => {
            if let Some(q) = v.first() {
                p.circle_filled(*q, r, c);
            }
            return;
        }
        _ => {}
    }
    // Round joins where the path turns, round caps at the ends of an open path.
    let n = v.len();
    let mut dots: Vec<Pos2> = Vec::new();
    for (i, q) in v.iter().enumerate() {
        let prev = if i > 0 {
            v.get(i - 1)
        } else if closed {
            v.last()
        } else {
            None
        };
        let next = if i + 1 < n {
            v.get(i + 1)
        } else if closed {
            v.first()
        } else {
            None
        };
        let corner = match (prev, next) {
            (Some(a), Some(b)) => (*q - *a).normalized().dot((*b - *q).normalized()) < 0.97,
            _ => true,
        };
        if corner {
            dots.push(*q);
        }
    }
    let stroke = PathStroke::new(width, c);
    p.add(if closed { PathShape::closed_line(v, stroke) } else { PathShape::line(v, stroke) });
    for q in dots {
        p.circle_filled(q, r, c);
    }
}

/// Strokes a polyline with a dash pattern of `on`/`off` lengths; dashes have round caps, and a
/// near-zero dash is a round dot.
fn stroke_dashed(p: &Painter, v: &[Pos2], closed: bool, on: f32, off: f32, width: f32, c: Color32) {
    if on + off <= 0.01 {
        return;
    }
    let mut pts: Vec<Pos2> = v.to_vec();
    if closed && let Some(first) = v.first() {
        pts.push(*first);
    }
    // Walk the path, switching between dash and gap at the pattern's lengths.
    let mut drawing = true;
    let mut left = on;
    let mut dash: Vec<Pos2> = pts.first().copied().into_iter().collect();
    for seg in pts.windows(2) {
        let (Some(a), Some(b)) = (seg.first(), seg.get(1)) else { continue };
        let len = a.distance(*b);
        let mut at = 0.0;
        while len - at > left {
            at += left;
            let q = *a + (*b - *a) * (at / len.max(1e-6));
            if drawing {
                dash.push(q);
                stroke_round(p, std::mem::take(&mut dash), false, width, c);
                left = off;
            } else {
                dash = vec![q];
                left = on;
            }
            drawing = !drawing;
        }
        left -= len - at;
        if drawing {
            dash.push(*b);
        }
    }
    if drawing && dash.len() > 1 {
        stroke_round(p, dash, false, width, c);
    }
}

/// Fills a closed polygon (the path closes implicitly, as SVG fills do): convex ones as egui convex
/// shapes, concave ones ear-clipped into an anti-aliased mesh.
fn fill_polygon(p: &Painter, mut v: Vec<Pos2>, c: Color32, feather: f32) {
    if v.len() > 2 && v.first().zip(v.last()).is_some_and(|(a, b)| a.distance(*b) < 0.01) {
        v.pop();
    }
    if v.len() < 3 {
        return;
    }
    // Clockwise on screen (y down), the winding egui feathers outward.
    if signed_area(&v) < 0.0 {
        v.reverse();
    }
    if is_convex(&v) {
        p.add(PathShape::convex_polygon(v, c, Stroke::NONE));
        return;
    }
    let tris = ear_clip(&v);
    let n = v.len();
    let mut mesh = egui::Mesh::default();
    // Inner ring in the colour, outer ring transparent, half a feather either side of the edge.
    let normals: Vec<egui::Vec2> = (0..n)
        .map(|i| {
            let (Some(a), Some(q), Some(b)) = (v.get((i + n - 1) % n), v.get(i), v.get((i + 1) % n)) else { return egui::Vec2::ZERO };
            let n1 = (*q - *a).normalized().rot90() * -1.0;
            let n2 = (*b - *q).normalized().rot90() * -1.0;
            let m = (n1 + n2).normalized();
            let k = m.dot(n1).max(0.3);
            m / k
        })
        .collect();
    for (q, nm) in v.iter().zip(&normals) {
        mesh.colored_vertex(*q - *nm * feather / 2.0, c);
    }
    for (q, nm) in v.iter().zip(&normals) {
        mesh.colored_vertex(*q + *nm * feather / 2.0, Color32::TRANSPARENT);
    }
    for (a, b, d) in tris {
        mesh.add_triangle(a, b, d);
    }
    let n = n as u32;
    for i in 0..n {
        let j = (i + 1) % n;
        mesh.add_triangle(i, j, n + i);
        mesh.add_triangle(j, n + j, n + i);
    }
    p.add(egui::Shape::mesh(mesh));
}

/// Twice the signed area (positive when clockwise on screen, y pointing down).
fn signed_area(v: &[Pos2]) -> f32 {
    let n = v.len();
    (0..n).filter_map(|i| Some((v.get(i)?, v.get((i + 1) % n)?))).map(|(a, b)| a.x * b.y - b.x * a.y).sum()
}

fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Whether a clockwise polygon is convex (never turns the other way, within a small tolerance).
fn is_convex(v: &[Pos2]) -> bool {
    let n = v.len();
    (0..n).all(|i| match (v.get(i), v.get((i + 1) % n), v.get((i + 2) % n)) {
        (Some(a), Some(b), Some(c)) => cross(*a, *b, *c) >= -1e-3,
        _ => true,
    })
}

/// Ear-clipping triangulation of a simple clockwise polygon, as index triples. Falls back to a fan
/// for what it cannot clip (degenerate input), so it always ends.
fn ear_clip(v: &[Pos2]) -> Vec<(u32, u32, u32)> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    let mut out = Vec::with_capacity(v.len().saturating_sub(2));
    let at = |i: usize| v.get(i).copied().unwrap_or(Pos2::ZERO);
    let mut guard = v.len() * v.len() + 8;
    while idx.len() > 3 && guard > 0 {
        guard -= 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (Some(&ia), Some(&ib), Some(&ic)) = (idx.get((k + m - 1) % m), idx.get(k), idx.get((k + 1) % m)) else { continue };
            let (a, b, c) = (at(ia), at(ib), at(ic));
            if cross(a, b, c) <= 1e-6 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let q = at(j);
                cross(a, b, q) >= 0.0 && cross(b, c, q) >= 0.0 && cross(c, a, q) >= 0.0
            });
            if inside {
                continue;
            }
            out.push((ia as u32, ib as u32, ic as u32));
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    // The last triangle, or a fan over whatever could not be clipped.
    if let Some(&first) = idx.first() {
        for w in idx.windows(2).skip(1) {
            if let (Some(&b), Some(&c)) = (w.first(), w.get(1)) {
                out.push((first as u32, b as u32, c as u32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colorized_design_parses_and_every_mapping_names_one() {
        for (key, _) in colored::ICONS {
            let els = design_shapes(key).unwrap_or_default();
            assert!(!els.is_empty() && els.iter().all(|e| !e.paths.is_empty()), "{key} has no shapes");
        }
        // Every key `design_key` returns is a design.
        let src = include_str!("icons.rs");
        let body = src.split("fn design_key(").nth(1).and_then(|s| s.split("\nfn design_only").next()).unwrap_or_default();
        let keys: Vec<&str> = body.split("=> \"").skip(1).filter_map(|s| s.split('"').next()).collect();
        assert!(keys.len() > 150);
        for k in keys {
            assert!(design_shapes(k).is_some(), "no design named {k}");
        }
    }

    #[test]
    fn ear_clipping_covers_a_concave_polygon() {
        // An L shape, clockwise on screen: six corners, four triangles, area 3.
        let v = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(2.0, 1.0), pos2(2.0, 2.0), pos2(0.0, 2.0)];
        assert!(signed_area(&v) > 0.0 && !is_convex(&v));
        let tris = ear_clip(&v);
        assert_eq!(tris.len(), 4);
        let at = |i: u32| v.get(i as usize).copied().unwrap_or(Pos2::ZERO);
        let area: f32 = tris.iter().map(|&(a, b, c)| cross(at(a), at(b), at(c)) / 2.0).sum();
        assert!((area - 3.0).abs() < 1e-4, "{area}");
    }

    #[test]
    fn design_only_icons_close_the_enum() {
        assert!(design_only(Icon::DeleteCells) && design_only(Icon::DeleteChart));
        assert!(!design_only(Icon::StopSquare) && !design_only(Icon::Paste));
    }
}
