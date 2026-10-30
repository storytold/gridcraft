//! GridCraft's icon set, drawn in code on a 16×16 design grid (original artwork; see
//! ATTRIBUTION.md). Large ribbon icons use the same drawings at 32×32 with accent colours.
//! Accent and paper colours come from the theme tokens, so icons follow light and dark mode;
//! icons placed on the sheet ([`paint_on_sheet`]) keep the light colours, as the sheet does.
//!
//! # Data-model names
//!
//! The Power Pivot window and the Data tab's data-model commands draw the glyphs below. They are
//! chrome vocabulary, so they are named here rather than offered to workbook cells by
//! [`LIBRARY`]; [`from_name`] resolves every name in both tables, case-insensitively.
//!
//! | names | icon |
//! | --- | --- |
//! | `data-model`, `data_model`, `power-pivot`, `power_pivot` | `DataModel` |
//! | `model-table`, `model_table` | `ModelTable` |
//! | `model-column`, `model_column` | `ModelColumn` |
//! | `measure` | `Measure` |
//! | `relationship`, `relationships` | `Relationship` |
//! | `data-view`, `data_view` | `DataView` |
//! | `diagram-view`, `diagram_view` | `DiagramView` |
//! | `calculation-area`, `calculation_area` | `CalculationArea` |
//! | `existing-connections`, `existing_connections`, `connections` | `Connections` |
//! | `refresh` | `Refresh` |
//! | `get-data`, `get_data` | `GetData` |
//! | `data-type`, `data_type` | `DataType` |
//! | `table-properties`, `table_properties` | `TableProperties` |
//! | `calculation-options`, `calculation_options` | `CalculationOptions` |
//! | `mark-as-date-table`, `mark_as_date_table` | `MarkAsDateTable` |
//! | `date-table`, `date_table` | `DateTable` |
//! | `perspectives`, `perspective` | `Perspectives` |
//! | `kpi`, `create-kpi` | `Kpi` |
//! | `summarize-by`, `summarize_by` | `SummarizeBy` |
//! | `hierarchy` | `Hierarchy` |
//! | `hidden-column`, `hidden_column`, `show-hidden` | `HiddenColumn` |
//! | `implicit-measures`, `implicit_measures` | `ImplicitMeasures` |
//! | `column-width`, `column_width` | `ColumnWidth` |
//! | `from-database`, `from_database` | `FromDatabase` |
//! | `data-service`, `data_service` | `DataService` |
//! | `other-sources`, `other_sources` | `OtherSources` |
//! | `table-detail`, `table_detail` | `TableDetail` |
//! | `table-behavior`, `table_behavior` | `TableBehavior` |
//! | `image-url`, `image_url` | `ImageUrl` |

use std::f32::consts::{PI, TAU};

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use crate::theme::{Cat, Tokens};

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
    // Data-model vocabulary (Power Pivot and the Data tab's model commands). The module doc lists
    // the names each one answers to.
    /// A stack of tabular layers: the workbook's data model (Power Pivot).
    DataModel,
    /// A model table: a grid whose first column is the key.
    ModelTable,
    /// One column of a model table, picked out of its neighbours.
    ModelColumn,
    /// A measure, as DAX writes its name: `[Σ]`.
    Measure,
    /// Two tables joined on a column, the join marked by dots.
    Relationship,
    /// The model's rows in a grid, the selected row washed.
    DataView,
    /// Table cards wired together: the model's diagram view.
    DiagramView,
    /// The Calculation Area beneath the Data View grid.
    CalculationArea,
    /// A database cylinder: the source an existing connection points at.
    Connections,
    /// A circular arrow: re-read the model.
    Refresh,
    /// Data arriving into a table from outside the workbook.
    GetData,
    /// A column's type: a letter tile and a number tile.
    DataType,
    /// A table with a properties badge.
    TableProperties,
    /// A calculator with its automatic/manual switch.
    CalculationOptions,
    /// A date table: a calendar grid.
    MarkAsDateTable,
    /// A date table designation: the calendar grid with a check.
    DateTable,
    /// Fanning views of one model: perspectives.
    Perspectives,
    /// A gauge with a needle and target: a KPI.
    Kpi,
    /// A table summed by a Σ row: the default summarization.
    SummarizeBy,
    /// A pyramid of levels: a hierarchy.
    Hierarchy,
    /// A model column struck out: hidden.
    HiddenColumn,
    /// A Σ in a dashed tile: a measure the engine derives.
    ImplicitMeasures,
    /// A column between two width arrows.
    ColumnWidth,
    /// A database cylinder with an import arrow: From Database.
    FromDatabase,
    /// A cloud with an import arrow: From Data Service.
    DataService,
    /// A folder with an import arrow: From Other Sources.
    OtherSources,
    /// A table with a key badge: Table Detail (the row identifier).
    TableDetail,
    /// A table with a gear: Table Behavior.
    TableBehavior,
    /// A picture with a link arrow: Image URL.
    ImageUrl,
    // Ribbon chrome that Excel draws its own way. These replace the glyphs those buttons used to
    // borrow from a neighbour, so each command reads as itself at 16 px.
    /// A 2×2 gallery of small marks: Insert › Icons.
    Icons,
    /// A 2×2 grid of mini charts: the remaining chart types.
    ChartMore,
    /// An arrow cursor: Select Objects.
    Pointer,
    /// A grid with an eye: Watch Window.
    WatchWindow,
    /// A coin and a rising line: the Financial function category.
    FnFinancial,
    /// A fork in a path: the Logical function category.
    FnLogical,
    /// A letter with a quote mark: the Text function category.
    FnText,
    /// A calendar with a clock: Date & Time functions.
    FnDateTime,
    /// A list under a magnifier: Lookup & Reference functions.
    FnLookup,
    /// A π: Math & Trig functions.
    FnMathTrig,
    /// Three dots on a tile: More Functions.
    FnMore,
    /// A row and a column with a curved arrow: Switch Row/Column.
    SwitchRowsColumns,
    /// A table under a line: Select Data.
    SelectData,
    /// A magnifier with a command prompt: Command Palette.
    CommandPalette,
    /// A robot head: Agent Control.
    AgentControl,
    /// A list with a history arrow: Action Journal.
    Journal,
    /// A tray with a down arrow and a new-version dot: Check for Updates.
    Update,
    /// A page with a small bar chart: Workbook Statistics.
    Statistics,
    /// A person inside a ring: Check Accessibility.
    Accessibility,
    /// A locked range with a pencil: Allow Edit Ranges.
    EditRanges,
    /// A flask: Data Analysis.
    Analysis,
    /// A framed percentage: the zoom level Zoom to 100% restores.
    ZoomPercent,
}

/// Every [`Icon`] variant, for the paint and name tests. A new variant belongs here too.
pub const ALL: &[Icon] = &[
    Icon::Paste,
    Icon::Cut,
    Icon::Copy,
    Icon::Brush,
    Icon::Bold,
    Icon::Italic,
    Icon::Underline,
    Icon::Strike,
    Icon::Borders,
    Icon::BorderAll,
    Icon::BorderNone,
    Icon::BorderOutside,
    Icon::BorderThick,
    Icon::Fill,
    Icon::FontColor,
    Icon::AlignTop,
    Icon::AlignMiddle,
    Icon::AlignBottom,
    Icon::AlignLeft,
    Icon::AlignCenter,
    Icon::AlignRight,
    Icon::Orientation,
    Icon::IndentDec,
    Icon::IndentInc,
    Icon::Wrap,
    Icon::Merge,
    Icon::Currency,
    Icon::Percent,
    Icon::Comma,
    Icon::DecInc,
    Icon::DecDec,
    Icon::CondFormat,
    Icon::FormatTable,
    Icon::CellStyles,
    Icon::Insert,
    Icon::Delete,
    Icon::Format,
    Icon::Sum,
    Icon::FillDown,
    Icon::Eraser,
    Icon::SortFilter,
    Icon::Find,
    Icon::Undo,
    Icon::Redo,
    Icon::Save,
    Icon::Home,
    Icon::More,
    Icon::Chevron,
    Icon::ChevronUp,
    Icon::Close,
    Icon::Check,
    Icon::Fx,
    Icon::Plus,
    Icon::Left,
    Icon::Right,
    Icon::Normal,
    Icon::PageLayout,
    Icon::PageBreak,
    Icon::Minus,
    Icon::Table,
    Icon::PivotTable,
    Icon::Chart,
    Icon::ChartBar,
    Icon::ChartLine,
    Icon::ChartPie,
    Icon::ChartScatter,
    Icon::Picture,
    Icon::Shapes,
    Icon::Link,
    Icon::Comment,
    Icon::Note,
    Icon::TextBox,
    Icon::Symbol,
    Icon::Sparkline,
    Icon::Checkbox,
    Icon::Function,
    Icon::Name,
    Icon::Trace,
    Icon::Calc,
    Icon::Filter,
    Icon::SortAsc,
    Icon::SortDesc,
    Icon::Duplicates,
    Icon::TextColumns,
    Icon::Validation,
    Icon::Group,
    Icon::Ungroup,
    Icon::Subtotal,
    Icon::Lock,
    Icon::Spell,
    Icon::Freeze,
    Icon::Zoom,
    Icon::Gridlines,
    Icon::Theme,
    Icon::Margins,
    Icon::Orient,
    Icon::SheetRtl,
    Icon::Paper,
    Icon::PrintArea,
    Icon::Script,
    Icon::Pen,
    Icon::Share,
    Icon::Search,
    Icon::Settings,
    Icon::Appearance,
    Icon::Sheet,
    Icon::SheetList,
    Icon::Book,
    Icon::Folder,
    Icon::Header,
    Icon::Footer,
    Icon::PageNumber,
    Icon::PageCount,
    Icon::Calendar,
    Icon::Clock,
    Icon::FileName,
    Icon::Back,
    Icon::NewFile,
    Icon::OpenFolder,
    Icon::Info,
    Icon::Print,
    Icon::DataModel,
    Icon::ModelTable,
    Icon::ModelColumn,
    Icon::Measure,
    Icon::Relationship,
    Icon::DataView,
    Icon::DiagramView,
    Icon::CalculationArea,
    Icon::Connections,
    Icon::Refresh,
    Icon::GetData,
    Icon::DataType,
    Icon::TableProperties,
    Icon::CalculationOptions,
    Icon::MarkAsDateTable,
    Icon::DateTable,
    Icon::Perspectives,
    Icon::Kpi,
    Icon::SummarizeBy,
    Icon::Hierarchy,
    Icon::HiddenColumn,
    Icon::ImplicitMeasures,
    Icon::ColumnWidth,
    Icon::FromDatabase,
    Icon::DataService,
    Icon::OtherSources,
    Icon::TableDetail,
    Icon::TableBehavior,
    Icon::ImageUrl,
    Icon::Icons,
    Icon::ChartMore,
    Icon::Pointer,
    Icon::WatchWindow,
    Icon::FnFinancial,
    Icon::FnLogical,
    Icon::FnText,
    Icon::FnDateTime,
    Icon::FnLookup,
    Icon::FnMathTrig,
    Icon::FnMore,
    Icon::SwitchRowsColumns,
    Icon::SelectData,
    Icon::CommandPalette,
    Icon::AgentControl,
    Icon::Journal,
    Icon::Update,
    Icon::Statistics,
    Icon::Accessibility,
    Icon::EditRanges,
    Icon::Analysis,
    Icon::ZoomPercent,
];

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
    /// `steps` + 1 points along an ellipse centred at (`cx`, `cy`), from angle `a0` to `a1` radians.
    #[allow(clippy::too_many_arguments)] // Centre, radii, angle range and step count of one arc.
    fn arc(&self, cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32, steps: usize) -> Vec<(f32, f32)> {
        let steps = steps.max(2);
        (0..=steps)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / steps as f32;
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect()
    }
    /// A filled arrowhead with its tip at (`x`, `y`), pointing along the unit vector (`dx`, `dy`).
    fn head(&self, x: f32, y: f32, dx: f32, dy: f32, size: f32, c: Color32) {
        let (nx, ny) = (-dy * size * 0.55, dx * size * 0.55);
        let (bx, by) = (x - dx * size, y - dy * size);
        self.poly(&[(x, y), (bx + nx, by + ny), (bx - nx, by - ny)], c);
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

/// Draws `icon` into `rect` (square-ish), in colour `c`, with the current theme's palette.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    paint_with(p, rect, icon, c, &Palette::of(&Tokens::get(p.ctx())));
}

/// Draws `icon` on the sheet, which stays white in both themes, so with the light palette.
pub fn paint_on_sheet(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    paint_with(p, rect, icon, c, &Palette::of(&Tokens::light()));
}

/// Draws `icon` into `rect` (square-ish), in colour `c`, with palette `pal`.
#[allow(clippy::cognitive_complexity)] // One arm per icon: a drawing vocabulary, not branching logic.
fn paint_with(p: &Painter, rect: Rect, icon: Icon, c: Color32, pal: &Palette) {
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
            g.rect(3.0, 2.5, 9.0, 12.0, Some(yellow), None);
            g.rect(5.0, 1.5, 5.0, 2.5, Some(pal.cat(Cat::Slate)), None);
            g.rect(6.5, 6.0, 7.5, 9.0, Some(paper), Some(c));
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
            g.rect(2.5, 2.5, 7.5, 9.0, Some(paper), Some(c));
            g.rect(6.0, 5.5, 7.5, 9.0, Some(paper), Some(c));
        }
        Icon::Brush => {
            g.rect(3.0, 2.0, 10.0, 4.0, Some(yellow), Some(c));
            g.stroke(&[(8.0, 6.0), (8.0, 8.5), (5.0, 8.5), (5.0, 10.0)], c);
            g.rect(4.0, 10.0, 2.0, 4.5, Some(c), None);
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
            g.rect(2.0, 2.0, 12.0, 12.0, Some(paper), Some(light));
            g.rect(3.0, 3.5, 4.0, 2.5, Some(red), None);
            g.rect(3.0, 7.0, 7.0, 2.5, Some(yellow), None);
            g.rect(3.0, 10.5, 10.0, 2.5, Some(blue), None);
        }
        Icon::FormatTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(paper), Some(light));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(blue), None);
            g.rect(2.0, 8.0, 12.0, 2.5, Some(pal.wash(Cat::Blue)), None);
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], light);
        }
        Icon::CellStyles => {
            g.rect(2.0, 3.0, 12.0, 4.5, Some(pal.wash(Cat::Green)), Some(light));
            g.rect(2.0, 8.5, 12.0, 4.5, Some(pal.wash(Cat::Red)), Some(light));
            g.stroke(&[(4.0, 5.2), (9.0, 5.2)], green);
            g.stroke(&[(4.0, 10.7), (9.0, 10.7)], red);
        }
        Icon::Insert | Icon::Delete | Icon::Format => {
            g.rect(2.0, 3.0, 12.0, 10.0, Some(paper), Some(light));
            g.stroke(&[(2.0, 6.5), (14.0, 6.5)], light);
            g.stroke(&[(2.0, 10.0), (14.0, 10.0)], light);
            g.stroke(&[(6.0, 3.0), (6.0, 13.0)], light);
            match icon {
                Icon::Insert => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(pal.wash(Cat::Blue)), Some(blue));
                    g.line(&[(1.0, 8.2), (4.5, 8.2)], green, 1.6);
                    g.line(&[(2.75, 6.5), (2.75, 10.0)], green, 1.6);
                }
                Icon::Delete => {
                    g.line(&[(8.0, 6.0), (13.0, 11.0)], red, 1.6);
                    g.line(&[(13.0, 6.0), (8.0, 11.0)], red, 1.6);
                }
                _ => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(pal.wash(Cat::Blue)), None);
                }
            }
        }
        Icon::Sum => g.text(8.0, 8.0, "Σ", 13.0, c, (false, false)),
        Icon::FillDown => {
            g.rect(4.0, 2.0, 8.0, 12.0, Some(paper), Some(light));
            g.stroke(&[(8.0, 4.0), (8.0, 11.0)], blue);
            g.poly(&[(5.5, 9.5), (10.5, 9.5), (8.0, 13.0)], blue);
        }
        Icon::Eraser => {
            g.poly(&[(2.0, 10.0), (8.0, 4.0), (12.0, 8.0), (6.0, 14.0)], red.gamma_multiply(0.55));
            g.poly(&[(8.0, 4.0), (10.5, 1.5), (14.5, 5.5), (12.0, 8.0)], red);
        }
        Icon::SortFilter => {
            g.text(4.5, 4.5, "A", 6.0, c, (false, false));
            g.text(4.5, 11.5, "Z", 6.0, c, (false, false));
            g.stroke(&[(8.0, 3.0), (8.0, 13.0)], blue);
            g.poly(&[(6.5, 11.0), (9.5, 11.0), (8.0, 13.5)], blue);
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
        Icon::Comment | Icon::Note => {
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
            // A gear: eight teeth around a ring with a hub.
            for i in 0..8 {
                let a = TAU / 8.0 * i as f32;
                g.line(&[(8.0 + 3.3 * a.cos(), 8.0 + 3.3 * a.sin()), (8.0 + 5.2 * a.cos(), 8.0 + 5.2 * a.sin())], c, 1.6);
            }
            g.circle(8.0, 8.0, 3.4, None, Some(c));
            g.circle(8.0, 8.0, 1.4, None, Some(c));
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
        Icon::DataModel => {
            // An isometric stack: a data model is tabular layers over one cube.
            g.poly(&[(8.0, 1.5), (14.0, 4.5), (8.0, 7.5), (2.0, 4.5)], blue);
            g.poly(&[(2.0, 4.5), (8.0, 7.5), (8.0, 14.0), (2.0, 11.0)], pal.wash(Cat::Blue));
            g.poly(&[(14.0, 4.5), (8.0, 7.5), (8.0, 14.0), (14.0, 11.0)], paper);
            g.line(&[(8.0, 1.5), (14.0, 4.5), (8.0, 7.5), (2.0, 4.5), (8.0, 1.5)], c, 1.1);
            g.line(&[(2.0, 4.5), (2.0, 11.0), (8.0, 14.0), (14.0, 11.0), (14.0, 4.5)], c, 1.1);
            g.line(&[(8.0, 7.5), (8.0, 14.0)], c, 1.1);
            g.stroke(&[(2.0, 7.75), (8.0, 10.75), (14.0, 7.75)], light);
        }
        Icon::ModelTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(paper), Some(c));
            g.rect(2.0, 2.5, 4.0, 11.0, Some(pal.wash(Cat::Blue)), None);
            g.rect(2.0, 2.5, 12.0, 3.0, Some(blue), None);
            g.stroke(&[(6.0, 5.5), (6.0, 13.5)], light);
            g.stroke(&[(10.0, 5.5), (10.0, 13.5)], light);
            g.circle(4.0, 4.0, 0.9, Some(paper), None);
        }
        Icon::ModelColumn => {
            g.rect(2.5, 3.5, 3.0, 10.0, None, Some(light));
            g.rect(6.5, 2.0, 3.0, 12.0, Some(blue), None);
            g.rect(10.5, 3.5, 3.0, 10.0, None, Some(light));
            for y in [6.5, 9.5] {
                g.stroke(&[(2.5, y), (5.5, y)], light);
                g.stroke(&[(10.5, y), (13.5, y)], light);
            }
        }
        Icon::Measure => {
            // DAX writes a measure's name in brackets.
            g.line(&[(4.5, 2.5), (2.5, 2.5), (2.5, 13.5), (4.5, 13.5)], c, 1.2);
            g.line(&[(11.5, 2.5), (13.5, 2.5), (13.5, 13.5), (11.5, 13.5)], c, 1.2);
            g.text(8.0, 8.0, "Σ", 8.0, blue, (false, false));
        }
        Icon::Relationship => {
            g.rect(1.5, 3.0, 5.0, 8.0, Some(paper), Some(c));
            g.rect(1.5, 3.0, 5.0, 2.0, Some(blue), None);
            g.rect(9.5, 5.0, 5.0, 8.0, Some(paper), Some(c));
            g.rect(9.5, 5.0, 5.0, 2.0, Some(green), None);
            g.line(&[(6.5, 6.5), (9.5, 8.5)], orange, 1.4);
            g.circle(6.5, 6.5, 0.9, Some(orange), None);
            g.circle(9.5, 8.5, 0.9, Some(orange), None);
        }
        Icon::DataView => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(paper), Some(c));
            g.rect(2.0, 2.5, 3.0, 11.0, Some(pal.wash(Cat::Slate)), None);
            g.rect(2.0, 2.5, 12.0, 2.5, Some(blue), None);
            g.stroke(&[(5.0, 5.0), (5.0, 13.5)], light);
            g.stroke(&[(9.0, 5.0), (9.0, 13.5)], light);
            g.rect(5.0, 6.5, 9.0, 2.0, Some(pal.wash(Cat::Blue)), None);
            g.stroke(&[(5.0, 9.0), (14.0, 9.0)], light);
        }
        Icon::DiagramView => {
            for (x, y, band) in [(1.5, 2.5, blue), (9.0, 2.5, green), (4.5, 9.5, purple)] {
                g.rect(x, y, 5.5, 4.5, Some(paper), Some(c));
                g.rect(x, y, 5.5, 1.8, Some(band), None);
            }
            g.line(&[(7.0, 6.0), (9.0, 6.0)], c, 1.1);
            g.line(&[(6.0, 7.0), (6.0, 9.5)], c, 1.1);
        }
        Icon::CalculationArea => {
            g.rect(2.0, 2.0, 12.0, 7.0, None, Some(light));
            for y in [4.5, 6.5] {
                g.stroke(&[(2.0, y), (14.0, y)], light);
            }
            g.rect(2.0, 9.5, 12.0, 4.5, Some(pal.wash(Cat::Blue)), Some(c));
            g.text(8.0, 11.75, "Σ", 5.5, blue, (false, false));
        }
        Icon::Connections => {
            // A database cylinder: the source an existing connection points at.
            g.poly(&[(3.0, 4.0), (3.0, 11.5), (13.0, 11.5), (13.0, 4.0)], pal.wash(Cat::Blue));
            g.poly(&g.arc(8.0, 11.5, 5.0, 1.7, 0.0, PI, 12), pal.wash(Cat::Blue));
            g.line(&[(3.0, 4.0), (3.0, 11.5)], c, 1.1);
            g.line(&[(13.0, 4.0), (13.0, 11.5)], c, 1.1);
            g.line(&g.arc(8.0, 11.5, 5.0, 1.7, 0.0, PI, 12), c, 1.1);
            g.line(&g.arc(8.0, 7.75, 5.0, 1.7, 0.0, PI, 12), light, 1.0);
            let top = g.arc(8.0, 4.0, 5.0, 1.7, 0.0, TAU, 20);
            g.poly(&top, blue);
            g.line(&top, c, 1.1);
        }
        Icon::Refresh => {
            let ring = g.arc(8.0, 8.0, 5.0, 5.0, -PI / 3.0, 4.0 * PI / 3.0, 24);
            g.line(&ring, c, 1.3);
            let a = 4.0 * PI / 3.0;
            g.head(8.0 + 5.0 * a.cos(), 8.0 + 5.0 * a.sin(), -a.sin(), a.cos(), 2.8, c);
        }
        Icon::GetData => {
            g.rect(6.0, 2.5, 8.0, 11.0, Some(paper), Some(c));
            g.rect(6.0, 2.5, 8.0, 2.5, Some(blue), None);
            g.stroke(&[(9.5, 5.0), (9.5, 13.5)], light);
            g.line(&[(1.0, 8.5), (3.5, 8.5)], green, 1.5);
            g.poly(&[(6.0, 8.5), (3.5, 7.0), (3.5, 10.0)], green);
        }
        Icon::DataType => {
            g.rect(2.0, 3.0, 5.5, 10.0, Some(pal.wash(Cat::Amber)), Some(c));
            g.rect(8.5, 3.0, 5.5, 10.0, Some(pal.wash(Cat::Blue)), Some(c));
            g.text(4.75, 8.0, "A", 7.0, c, (false, false));
            g.text(11.25, 8.0, "1", 7.0, c, (false, false));
        }
        Icon::TableProperties => {
            g.rect(2.0, 2.5, 10.0, 10.0, Some(paper), Some(c));
            g.rect(2.0, 2.5, 10.0, 2.5, Some(blue), None);
            g.stroke(&[(5.5, 5.0), (5.5, 12.5)], light);
            g.stroke(&[(2.0, 8.75), (12.0, 8.75)], light);
            g.circle(13.0, 12.5, 2.4, Some(paper), None);
            g.circle(13.0, 12.5, 2.2, None, Some(c));
            g.circle(13.0, 12.5, 0.8, Some(c), None);
        }
        Icon::CalculationOptions => {
            g.rect(2.5, 2.0, 9.0, 12.0, Some(paper), Some(c));
            g.rect(4.0, 3.5, 6.0, 2.5, Some(light), None);
            for (x, y) in [(5.0, 9.0), (8.0, 9.0), (5.0, 11.5), (8.0, 11.5)] {
                g.circle(x, y, 0.8, Some(c), None);
            }
            // The automatic / manual switch.
            g.rect(11.0, 8.5, 4.0, 5.5, Some(paper), Some(c));
            g.line(&[(11.8, 12.0), (14.2, 12.0)], blue, 1.2);
            g.circle(12.6, 12.0, 1.0, Some(blue), None);
        }
        Icon::MarkAsDateTable => {
            g.rect(2.0, 3.0, 12.0, 11.0, Some(paper), Some(c));
            g.rect(2.0, 3.0, 12.0, 3.0, Some(blue), None);
            g.line(&[(5.5, 1.5), (5.5, 4.0)], c, 1.2);
            g.line(&[(10.5, 1.5), (10.5, 4.0)], c, 1.2);
            g.line(&[(4.0, 11.0), (6.5, 13.0), (12.0, 6.5)], green, 1.6);
        }
        Icon::DateTable => {
            g.rect(2.0, 3.0, 12.0, 11.0, Some(paper), Some(c));
            g.rect(2.0, 3.0, 12.0, 3.0, Some(blue), None);
            g.line(&[(5.5, 1.5), (5.5, 4.0)], c, 1.2);
            g.line(&[(10.5, 1.5), (10.5, 4.0)], c, 1.2);
            for (x, y) in [(4.5, 8.0), (7.5, 8.0), (10.5, 8.0), (4.5, 11.5), (7.5, 11.5), (10.5, 11.5)] {
                g.rect(x - 0.7, y - 0.7, 1.4, 1.4, Some(blue), None);
            }
        }
        Icon::Perspectives => {
            g.circle(3.0, 8.0, 1.2, Some(blue), None);
            g.line(&[(4.0, 7.2), (11.5, 3.2)], c, 1.0);
            g.line(&[(4.2, 8.0), (13.0, 8.0)], c, 1.0);
            g.line(&[(4.0, 8.8), (11.5, 12.8)], c, 1.0);
            g.rect(11.5, 2.0, 3.0, 2.4, Some(pal.wash(Cat::Blue)), Some(blue));
            g.rect(12.0, 6.8, 3.0, 2.4, Some(pal.wash(Cat::Green)), Some(green));
            g.rect(11.5, 11.6, 3.0, 2.4, Some(pal.wash(Cat::Purple)), Some(purple));
        }
        Icon::Kpi => {
            g.line(&g.arc(8.0, 10.0, 5.0, 5.0, PI, TAU, 20), c, 1.2);
            g.line(&[(2.5, 10.0), (4.0, 10.0)], light, 1.0);
            g.line(&[(12.0, 10.0), (13.5, 10.0)], light, 1.0);
            g.line(&[(8.0, 10.0), (11.2, 6.8)], blue, 1.4);
            g.circle(8.0, 10.0, 1.1, Some(c), None);
            g.circle(11.2, 6.8, 0.9, Some(orange), None);
        }
        Icon::SummarizeBy => {
            g.rect(1.5, 2.5, 13.0, 6.5, Some(paper), Some(c));
            g.stroke(&[(1.5, 4.75), (14.5, 4.75)], light);
            g.stroke(&[(6.0, 2.5), (6.0, 9.0)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 9.0)], light);
            g.rect(1.5, 10.5, 13.0, 4.0, Some(pal.wash(Cat::Blue)), Some(c));
            g.text(8.0, 12.5, "Σ", 5.5, blue, (false, false));
        }
        Icon::Hierarchy => {
            for (w, y, fill) in [(11.5, 2.0, blue), (8.5, 6.0, pal.wash(Cat::Green)), (5.5, 10.0, pal.wash(Cat::Purple))] {
                g.rect(8.0 - w / 2.0, y, w, 2.6, Some(fill), Some(c));
            }
            g.line(&[(8.0, 4.6), (8.0, 10.0)], light, 1.0);
        }
        Icon::HiddenColumn => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(light));
            g.rect(2.0, 2.5, 12.0, 2.5, None, Some(light));
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], light);
            g.rect(6.0, 6.0, 4.0, 7.5, Some(pal.wash(Cat::Slate)), None);
            g.line(&[(5.0, 12.5), (11.0, 4.5)], red, 1.4);
        }
        Icon::ImplicitMeasures => {
            // A dashed tile: a measure the engine derives, not one the model stores.
            for (a, b) in [(2.5f32, 5.5f32), (6.5, 9.5), (10.5, 13.5)] {
                g.stroke(&[(a, 2.5), (b, 2.5)], light);
                g.stroke(&[(a, 13.5), (b, 13.5)], light);
                g.stroke(&[(2.5, a), (2.5, b)], light);
                g.stroke(&[(13.5, a), (13.5, b)], light);
            }
            g.text(8.0, 8.0, "Σ", 7.5, blue, (false, false));
        }
        Icon::ColumnWidth => {
            g.rect(6.0, 2.5, 4.0, 11.0, None, Some(c));
            g.line(&[(1.0, 8.0), (4.0, 8.0)], blue, 1.2);
            g.poly(&[(1.0, 8.0), (3.2, 6.8), (3.2, 9.2)], blue);
            g.line(&[(12.0, 8.0), (15.0, 8.0)], blue, 1.2);
            g.poly(&[(15.0, 8.0), (12.8, 6.8), (12.8, 9.2)], blue);
        }
        Icon::FromDatabase => {
            let top = g.arc(6.0, 3.5, 4.0, 1.4, 0.0, TAU, 18);
            g.poly(&top, blue);
            g.line(&top, c, 1.1);
            g.line(&[(2.0, 3.5), (2.0, 10.5)], c, 1.1);
            g.line(&[(10.0, 3.5), (10.0, 10.5)], c, 1.1);
            g.line(&g.arc(6.0, 10.5, 4.0, 1.4, 0.0, PI, 12), c, 1.1);
            g.line(&g.arc(6.0, 7.0, 4.0, 1.4, 0.0, PI, 12), light, 1.0);
            g.line(&[(13.0, 2.0), (13.0, 11.0)], green, 1.5);
            g.head(13.0, 12.6, 0.0, 1.0, 2.6, green);
        }
        Icon::DataService => {
            for (x, y, r) in [(4.6, 8.8, 2.2), (7.9, 8.4, 2.6), (11.2, 8.8, 2.2)] {
                g.line(&g.arc(x, y, r, r, PI, TAU, 14), c, 1.1);
            }
            g.line(&[(2.4, 8.8), (13.4, 8.8)], c, 1.1);
            g.line(&[(8.0, 10.2), (8.0, 13.2)], blue, 1.4);
            g.head(8.0, 14.6, 0.0, 1.0, 2.4, blue);
        }
        Icon::OtherSources => {
            g.poly(&[(1.5, 4.5), (5.0, 4.5), (6.2, 5.7), (10.5, 5.7), (10.5, 12.5), (1.5, 12.5)], yellow);
            g.line(&[(1.5, 4.5), (5.0, 4.5), (6.2, 5.7), (10.5, 5.7), (10.5, 12.5), (1.5, 12.5), (1.5, 4.5)], c, 1.0);
            g.line(&[(12.6, 4.0), (12.6, 10.8)], green, 1.5);
            g.head(12.6, 12.4, 0.0, 1.0, 2.6, green);
        }
        Icon::TableDetail => {
            g.rect(1.5, 2.0, 9.0, 9.5, Some(paper), Some(c));
            g.rect(1.5, 2.0, 9.0, 2.2, Some(blue), None);
            g.stroke(&[(4.5, 4.2), (4.5, 11.5)], light);
            g.stroke(&[(1.5, 7.5), (10.5, 7.5)], light);
            // A key: the row identifier Table Detail names.
            g.circle(11.4, 11.0, 1.9, None, Some(c));
            g.line(&[(12.7, 12.3), (14.8, 14.4)], c, 1.3);
            g.line(&[(13.8, 13.4), (14.6, 12.6)], c, 1.1);
        }
        Icon::TableBehavior => {
            g.rect(1.5, 2.0, 8.5, 9.0, Some(paper), Some(c));
            g.rect(1.5, 2.0, 8.5, 2.2, Some(blue), None);
            g.stroke(&[(5.75, 4.2), (5.75, 11.0)], light);
            for i in 0..6 {
                let a = PI / 3.0 * i as f32;
                g.line(&[(12.4 + 2.2 * a.cos(), 11.4 + 2.2 * a.sin()), (12.4 + 3.3 * a.cos(), 11.4 + 3.3 * a.sin())], c, 1.2);
            }
            g.circle(12.4, 11.4, 2.0, None, Some(c));
            g.circle(12.4, 11.4, 0.7, Some(c), None);
        }
        Icon::ImageUrl => {
            g.rect(1.5, 3.0, 10.5, 8.5, Some(pal.wash(Cat::Blue)), Some(c));
            g.poly(&[(2.5, 10.5), (5.5, 6.5), (7.5, 8.5), (8.8, 7.2), (11.0, 10.5)], green);
            g.line(&[(9.5, 14.0), (14.0, 9.5)], c, 1.3);
            g.head(14.8, 8.6, 0.74, -0.67, 2.2, blue);
        }
        Icon::Icons => {
            for (x, y) in [(1.5, 1.5), (8.5, 1.5), (1.5, 8.5), (8.5, 8.5)] {
                g.rect(x, y, 6.0, 6.0, None, Some(light));
            }
            g.circle(4.5, 4.5, 1.6, Some(blue), None);
            g.rect(10.3, 3.3, 2.5, 2.5, Some(orange), None);
            g.poly(&[(4.5, 10.2), (7.0, 13.5), (2.0, 13.5)], green);
            g.poly(&[(11.5, 10.2), (14.0, 11.7), (11.5, 13.2), (9.0, 11.7)], purple);
        }
        Icon::ChartMore => {
            for (x, h) in [(1.5f32, 3.2f32), (3.9, 5.2), (6.3, 4.2)] {
                g.rect(x, 7.0 - h, 1.8, h, Some(blue), None);
            }
            g.line(&[(9.5, 6.6), (10.8, 3.4), (12.2, 5.0), (14.2, 1.6)], green, 1.2);
            g.circle(3.6, 11.6, 2.8, Some(orange), None);
            g.poly(&[(3.6, 11.6), (3.6, 8.8), (6.3, 10.3)], yellow);
            for (x, y) in [(9.5, 13.2), (11.2, 10.8), (12.6, 12.4), (14.0, 9.8)] {
                g.circle(x, y, 0.9, Some(purple), None);
            }
        }
        Icon::Pointer => {
            // An arrow cursor: the solid head, then its tail.
            g.poly(&[(4.0, 2.0), (4.0, 12.4), (11.2, 8.6)], c);
            g.poly(&[(7.4, 9.6), (9.8, 14.4), (11.6, 13.6), (9.2, 8.8)], c);
        }
        Icon::WatchWindow => {
            g.rect(1.5, 2.0, 13.0, 8.0, None, Some(light));
            for x in [5.0, 9.2] {
                g.stroke(&[(x, 2.0), (x, 10.0)], light);
            }
            g.stroke(&[(1.5, 6.0), (14.5, 6.0)], light);
            // The eye that watches the cells.
            g.line(&g.arc(8.0, 12.0, 4.6, 2.6, PI, TAU, 12), c, 1.1);
            g.line(&g.arc(8.0, 12.0, 4.6, 2.6, 0.0, PI, 12), c, 1.1);
            g.circle(8.0, 12.0, 1.4, Some(blue), None);
        }
        Icon::FnFinancial => {
            g.circle(6.2, 8.2, 4.6, Some(pal.wash(Cat::Green)), Some(c));
            g.text(6.2, 8.2, "$", 8.0, c, (true, false));
            g.line(&[(10.5, 13.0), (13.8, 5.2)], green, 1.5);
            g.head(14.3, 3.8, 0.4, -0.92, 2.4, green);
        }
        Icon::FnLogical => {
            g.line(&[(1.5, 8.0), (6.0, 8.0)], c, 1.2);
            g.line(&[(6.0, 8.0), (10.5, 3.5)], c, 1.2);
            g.line(&[(6.0, 8.0), (10.5, 12.5)], c, 1.2);
            g.circle(6.0, 8.0, 1.0, Some(c), None);
            g.head(10.5, 3.5, 0.71, -0.71, 2.4, green);
            g.head(10.5, 12.5, 0.71, 0.71, 2.4, red);
        }
        Icon::FnText => {
            g.text(8.5, 9.5, "A", 9.0, c, (false, false));
            g.line(&[(4.5, 1.5), (3.7, 3.8)], blue, 1.2);
            g.line(&[(6.5, 1.5), (5.7, 3.8)], blue, 1.2);
        }
        Icon::FnDateTime => {
            g.rect(1.5, 2.0, 9.0, 7.5, Some(paper), Some(c));
            g.rect(1.5, 2.0, 9.0, 2.2, Some(blue), None);
            g.line(&[(4.0, 1.0), (4.0, 3.2)], c, 1.1);
            g.line(&[(8.0, 1.0), (8.0, 3.2)], c, 1.1);
            g.stroke(&[(3.0, 5.6), (6.5, 5.6)], light);
            g.stroke(&[(3.0, 7.8), (5.5, 7.8)], light);
            // The clock: the time half of the type.
            g.circle(11.5, 11.5, 3.6, Some(paper), None);
            g.circle(11.5, 11.5, 3.6, None, Some(c));
            g.stroke(&[(11.5, 9.2), (11.5, 11.5), (13.3, 12.4)], blue);
        }
        Icon::FnLookup => {
            for y in [2.2, 5.8, 9.4] {
                g.rect(1.5, y, 1.6, 1.6, Some(light), None);
                g.stroke(&[(4.0, y + 0.8), (8.5, y + 0.8)], light);
            }
            g.circle(10.5, 8.6, 3.4, None, Some(c));
            g.line(&[(13.0, 11.1), (14.8, 12.9)], c, 1.5);
            g.circle(10.5, 8.6, 1.2, Some(blue), None);
        }
        Icon::FnMathTrig => {
            g.text(6.4, 8.6, "π", 13.0, c, (false, false));
            g.poly(&[(10.0, 14.0), (14.5, 14.0), (14.5, 9.5)], blue);
        }
        Icon::FnMore => {
            g.rect(1.5, 3.5, 13.0, 9.0, None, Some(c));
            for x in [5.0, 8.0, 11.0] {
                g.circle(x, 8.0, 1.1, Some(blue), None);
            }
        }
        Icon::SwitchRowsColumns => {
            // A row and a column, with a quarter-turn arrow between them.
            g.rect(1.5, 10.2, 6.5, 2.2, Some(pal.wash(Cat::Blue)), Some(c));
            g.rect(10.2, 1.5, 2.2, 6.5, Some(pal.wash(Cat::Green)), Some(c));
            g.line(&g.arc(10.2, 10.2, 4.2, 4.2, PI, 1.5 * PI, 14), c, 1.4);
            g.head(10.2, 6.0, 0.0, -1.0, 2.6, c);
        }
        Icon::SelectData => {
            // A table with a line drawn through it: the data a series uses.
            g.rect(1.5, 6.0, 8.5, 8.0, Some(paper), Some(c));
            g.rect(1.5, 6.0, 8.5, 2.0, Some(blue), None);
            g.stroke(&[(4.3, 8.0), (4.3, 14.0)], light);
            g.stroke(&[(1.5, 11.0), (10.0, 11.0)], light);
            g.line(&[(9.0, 5.5), (11.5, 2.5), (13.5, 4.0), (15.0, 1.6)], green, 1.3);
            g.circle(11.5, 2.5, 0.8, Some(green), None);
        }
        Icon::CommandPalette => {
            g.circle(7.0, 7.0, 4.4, None, Some(c));
            g.line(&[(10.2, 10.2), (14.2, 14.2)], c, 1.6);
            g.text(7.0, 7.0, ">", 7.0, blue, (true, false));
        }
        Icon::AgentControl => {
            // A robot head: the agent that drives the app.
            g.rect(2.5, 4.5, 11.0, 8.5, Some(paper), Some(c));
            g.line(&[(8.0, 4.5), (8.0, 2.0)], c, 1.2);
            g.circle(8.0, 1.5, 0.9, Some(blue), None);
            g.circle(5.8, 8.0, 1.1, Some(blue), None);
            g.circle(10.2, 8.0, 1.1, Some(blue), None);
            g.stroke(&[(6.0, 11.0), (10.0, 11.0)], light);
            g.rect(1.0, 6.5, 1.6, 3.0, Some(pal.wash(Cat::Slate)), Some(c));
            g.rect(13.4, 6.5, 1.6, 3.0, Some(pal.wash(Cat::Slate)), Some(c));
        }
        Icon::Journal => {
            for y in [3.0, 6.0, 9.0] {
                g.circle(2.5, y, 0.8, Some(light), None);
                g.stroke(&[(4.5, y), (10.0, y)], light);
            }
            // A clock: the session's history.
            g.circle(11.8, 11.8, 3.2, Some(paper), None);
            g.circle(11.8, 11.8, 3.2, None, Some(c));
            g.stroke(&[(11.8, 9.8), (11.8, 11.8), (13.3, 12.6)], blue);
        }
        Icon::Update => {
            g.line(&[(2.5, 10.0), (2.5, 13.5), (12.5, 13.5), (12.5, 10.0)], c, 1.2);
            g.line(&[(7.5, 2.0), (7.5, 10.5)], c, 1.4);
            g.head(7.5, 11.8, 0.0, 1.0, 2.6, blue);
            g.circle(13.0, 4.0, 2.0, Some(green), None);
            g.circle(13.0, 4.0, 0.8, Some(paper), None);
        }
        Icon::Statistics => {
            g.rect(1.5, 1.5, 13.0, 13.0, Some(paper), Some(c));
            for y in [4.5, 6.5] {
                g.stroke(&[(4.0, y), (12.0, y)], light);
            }
            g.rect(3.5, 8.5, 2.2, 4.5, Some(blue), None);
            g.rect(6.7, 6.5, 2.2, 6.5, Some(green), None);
            g.rect(9.9, 9.5, 2.2, 3.5, Some(orange), None);
        }
        Icon::Accessibility => {
            g.circle(8.0, 8.0, 6.6, None, Some(light));
            g.circle(8.0, 4.6, 1.7, Some(c), None);
            g.line(&[(4.2, 8.0), (11.8, 8.0)], c, 1.5);
            g.line(&[(8.0, 8.0), (8.0, 10.2)], c, 1.5);
            g.line(&[(8.0, 10.2), (6.0, 13.6)], c, 1.5);
            g.line(&[(8.0, 10.2), (10.0, 13.6)], c, 1.5);
        }
        Icon::EditRanges => {
            g.rect(1.5, 2.5, 9.0, 9.0, None, Some(light));
            g.rect(4.0, 5.0, 3.0, 3.0, Some(pal.wash(Cat::Blue)), Some(blue));
            g.stroke(&[(6.0, 2.5), (6.0, 11.5)], light);
            g.stroke(&[(1.5, 6.5), (10.5, 6.5)], light);
            g.poly(&[(8.5, 13.5), (9.2, 11.0), (13.5, 6.7), (15.0, 8.2), (10.7, 12.5)], blue);
            g.stroke(&[(8.5, 13.5), (9.2, 11.0)], c);
            g.line(&[(12.8, 7.4), (14.3, 8.9)], c, 1.0);
        }
        Icon::Analysis => {
            // A flask: the statistical analyses run on a sample.
            g.line(&[(6.0, 1.5), (10.0, 1.5)], c, 1.2);
            g.line(&[(6.6, 1.5), (6.6, 6.0), (2.5, 13.2), (13.5, 13.2), (9.4, 6.0), (9.4, 1.5)], c, 1.15);
            g.poly(&[(4.1, 9.6), (11.9, 9.6), (13.5, 13.2), (2.5, 13.2)], pal.wash(Cat::Green));
            g.circle(7.0, 11.6, 0.9, Some(green), None);
            g.circle(9.5, 12.2, 0.8, Some(green), None);
        }
        Icon::ZoomPercent => {
            // A framed %: the zoom level the button restores.
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(light));
            g.text(8.0, 8.5, "%", 10.0, c, (false, false));
        }
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

/// Names for the data-model glyphs, which are chrome vocabulary rather than artwork offered to
/// cells by Insert › Icons. Every name is listed in the module doc; kebab-case is canonical and
/// the snake_case spelling is accepted beside it.
pub const MODEL_ICONS: &[(&str, Icon)] = &[
    ("data-model", Icon::DataModel),
    ("data_model", Icon::DataModel),
    ("power-pivot", Icon::DataModel),
    ("power_pivot", Icon::DataModel),
    ("model-table", Icon::ModelTable),
    ("model_table", Icon::ModelTable),
    ("model-column", Icon::ModelColumn),
    ("model_column", Icon::ModelColumn),
    ("measure", Icon::Measure),
    ("relationship", Icon::Relationship),
    ("relationships", Icon::Relationship),
    ("data-view", Icon::DataView),
    ("data_view", Icon::DataView),
    ("diagram-view", Icon::DiagramView),
    ("diagram_view", Icon::DiagramView),
    ("calculation-area", Icon::CalculationArea),
    ("calculation_area", Icon::CalculationArea),
    ("existing-connections", Icon::Connections),
    ("existing_connections", Icon::Connections),
    ("connections", Icon::Connections),
    ("refresh", Icon::Refresh),
    ("get-data", Icon::GetData),
    ("get_data", Icon::GetData),
    ("data-type", Icon::DataType),
    ("data_type", Icon::DataType),
    ("table-properties", Icon::TableProperties),
    ("table_properties", Icon::TableProperties),
    ("calculation-options", Icon::CalculationOptions),
    ("calculation_options", Icon::CalculationOptions),
    ("mark-as-date-table", Icon::MarkAsDateTable),
    ("mark_as_date_table", Icon::MarkAsDateTable),
    ("date-table", Icon::DateTable),
    ("date_table", Icon::DateTable),
    ("perspectives", Icon::Perspectives),
    ("perspective", Icon::Perspectives),
    ("kpi", Icon::Kpi),
    ("create-kpi", Icon::Kpi),
    ("summarize-by", Icon::SummarizeBy),
    ("summarize_by", Icon::SummarizeBy),
    ("hierarchy", Icon::Hierarchy),
    ("hidden-column", Icon::HiddenColumn),
    ("hidden_column", Icon::HiddenColumn),
    ("show-hidden", Icon::HiddenColumn),
    ("implicit-measures", Icon::ImplicitMeasures),
    ("implicit_measures", Icon::ImplicitMeasures),
    ("column-width", Icon::ColumnWidth),
    ("column_width", Icon::ColumnWidth),
    ("from-database", Icon::FromDatabase),
    ("from_database", Icon::FromDatabase),
    ("data-service", Icon::DataService),
    ("data_service", Icon::DataService),
    ("other-sources", Icon::OtherSources),
    ("other_sources", Icon::OtherSources),
    ("table-detail", Icon::TableDetail),
    ("table_detail", Icon::TableDetail),
    ("table-behavior", Icon::TableBehavior),
    ("table_behavior", Icon::TableBehavior),
    ("image-url", Icon::ImageUrl),
    ("image_url", Icon::ImageUrl),
];

pub fn from_name(name: &str) -> Option<Icon> {
    LIBRARY.iter().chain(MODEL_ICONS).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, i)| *i)
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
