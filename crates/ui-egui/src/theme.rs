//! Design tokens and fonts.
//!
//! Colours are our own, tuned to read like a modern desktop spreadsheet. Fonts come from egui's
//! bundled faces and the operating system at runtime: the UI uses the platform UI font, cells use
//! the best available match for the workbook font (Calibri/Carlito/Aptos/Arial…), and egui's
//! bundled Ubuntu/Hack/egui-icons and monochrome emoji faces are the last resort. A build with
//! `CRAFT_FONTS_DIR` also embeds the permitted OFL craft-fonts faces (barred Adobe/Source/Noto CJK
//! families never enter it, and a web build keeps only its explicit allowlist). Native builds opt
//! into craft-fonts' OFL LXGW Chinese face, so a host without a Han system face still draws Chinese
//! interface text; no font file is stored in this repo.
//!
//! Chrome colours belong in [`Tokens`], each with a light and a dark value, chosen by Display
//! theme. Colours drawn on the sheet follow the sheet, not Display theme: they come from a
//! [`SheetColors`] set, light by default and dark when the dark sheet is on (`docs/dark-sheet.md`),
//! so `Tokens::palette(dark_ui, dark_sheet)` has two axes. The foreground/background pairs are
//! listed in `contrast_tests`, which holds them to WCAG 2 (4.5:1 for text, 3:1 for glyphs) in all
//! four combinations: a new token goes there too.

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use egui::epaint::text::Blob;
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

mod font_fallback;
mod font_setup;

pub use font_fallback::symbol_face_installed;
#[cfg(test)]
use font_fallback::{
    PREFERRED_ARABIC, arabic, cjk_han_japanese, cjk_han_simplified, cjk_hangul, fallback_faces, hebrew, lao, load_faces, preferred_file,
    symbols_more, thai,
};
#[cfg(all(test, unix, not(target_os = "macos")))]
use font_fallback::{covers_arabic, fontconfig_arabic, parse_fc_match};
use font_setup::pick;
pub use font_setup::{
    CELL, CELL_BOLD, CELL_BOLD_ITALIC, CELL_ITALIC, CraftFont, HanOrder, MATH, MATH_BOLD, MATH_ITALIC, MONO, SERIF, UI, UI_BOLD, available_text,
    cell_family, cell_text_width, chinese_font_loaded, craft_font_scripts, craft_fonts_cover, craft_fonts_loaded, craft_fonts_required,
    font_definitions, font_definitions_for_language, font_definitions_with, pinned_font_definitions, symbol_fallback, ui_bold, ui_font,
};
#[cfg(test)]
use font_setup::{candidates, craft_font_faces, font_definitions_impl, try_load};
#[cfg(test)]
pub(crate) use font_setup::{drop_unparseable_fonts, font_parses};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// Display theme: the interface is dark.
    pub dark: bool,
    /// The dark sheet is on: the sheet fields (`grid_bg`, `gridline`, `cell_text`, `sel_*`,
    /// `active_row`, `error_check_green` and every `sheet_*`) hold [`SheetColors::DARK`].
    pub dark_sheet: bool,
    pub window: Color32,
    pub ribbon: Color32,
    pub ribbon_border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub accent: Color32,
    pub accent_dark: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub separator: Color32,
    pub grid_bg: Color32,
    /// Behind the pages in Page Layout view.
    pub desk: Color32,
    pub gridline: Color32,
    pub header_bg: Color32,
    pub header_text: Color32,
    pub header_line: Color32,
    pub header_sel_bg: Color32,
    pub header_sel_text: Color32,
    pub header_all_bg: Color32,
    pub sel_fill: Color32,
    /// The tint over the active cell's row (File ▸ Options ▸ General).
    pub active_row: Color32,
    pub sel_border: Color32,
    pub input_bg: Color32,
    pub input_border: Color32,
    pub tab_bar: Color32,
    pub tab_active: Color32,
    pub status_bar: Color32,
    pub cell_text: Color32,
    pub menu_bg: Color32,
    pub shadow: Color32,
    pub danger: Color32,
    /// Background error checking: the corner triangle on a cell. A sheet colour.
    pub error_check_green: Color32,
    pub warning_fill: Color32,
    pub warning_ink: Color32,
    /// Info-bar (notice strip) fills, by severity.
    pub notice_warn_bg: Color32,
    pub notice_warn_border: Color32,
    pub notice_info_bg: Color32,
    pub notice_info_border: Color32,
    /// Text and glyphs drawn on an `accent` fill (primary buttons, switched-on toggles).
    pub on_accent: Color32,
    /// The category palette, indexed by [`Cat`]: the coloured parts of icons, gallery highlights.
    pub cat: [Color32; 8],
    /// The page colour inside icons (documents, cells, clipboards).
    pub paper: Color32,
    /// Reference colours for formulas edited in the window-coloured formula bar.
    pub ref_colors: [Color32; 8],
    /// Reference colours on the sheet (outlines and the in-cell editor). Sheet colours.
    pub sheet_ref_colors: [Color32; 8],
    /// Code colours in the code editor (and the window-coloured fields it sits in).
    pub syntax: Syntax,
    /// The message box warning disc and its glyph.
    pub warning: Color32,
    pub on_warning: Color32,
    /// Controls drawn on the sheet (filter and validation buttons, object handles). They follow
    /// the sheet, not Display theme.
    pub sheet_control_bg: Color32,
    pub sheet_control_border: Color32,
    pub sheet_control_text: Color32,
    /// A placeholder fill on the sheet (a picture that has not loaded), and a form slider's track.
    pub sheet_track: Color32,
    /// A form slider's thumb.
    pub sheet_thumb: Color32,
    /// A form button or spin arrow under the pointer, and pressed.
    pub sheet_control_hover: Color32,
    pub sheet_control_pressed: Color32,
    /// The filled part of a form slider's track (from the minimum to the thumb).
    pub sheet_fill: Color32,
    /// The ring around the form control that takes the arrow keys.
    pub sheet_focus: Color32,
    /// The macro recording dot (Record Macro, the status bar).
    pub recording: Color32,
    /// A breakpoint dot in the macro step editor's gutter.
    pub breakpoint: Color32,
    /// Macro overlays on the sheet (Check and debugging): the inputs, the cells the macro writes,
    /// the cells it would write over, and the current step's target. Sheet colours.
    pub sheet_macro_input: Color32,
    pub sheet_macro_footprint: Color32,
    pub sheet_macro_conflict: Color32,
    pub sheet_macro_target: Color32,
    /// The PY badge in the formula bar (Python mode) and its text.
    pub badge_py: Color32,
    pub on_badge: Color32,
    /// The PY tag on a Python cell and its text. They follow the sheet, not Display theme.
    pub sheet_py_tag: Color32,
    pub on_sheet_py_tag: Color32,
    /// Trace arrows on the sheet: precedents and dependents, and the links Trace Error follows
    /// between cells showing an error. Sheet colours.
    pub sheet_trace_arrow: Color32,
    pub sheet_trace_error: Color32,
    /// Frozen-pane lines, the fill-handle ring, the fill-drag preview and the move outline. Sheet
    /// colours.
    pub sheet_freeze_line: Color32,
    pub sheet_handle_halo: Color32,
    pub sheet_drag_outline: Color32,
    pub sheet_move_outline: Color32,
    /// Page Break Preview: the shade outside the print area, the page-break lines and the "Page N" watermark.
    pub sheet_page_outside: Color32,
    pub sheet_page_break: Color32,
    pub sheet_page_watermark: Color32,
}

/// The eight category colours, in the order of [`Tokens::cat`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cat {
    Blue,
    Green,
    Orange,
    Red,
    Purple,
    Teal,
    Amber,
    Slate,
}

impl Cat {
    pub const ALL: [Cat; 8] = [Cat::Blue, Cat::Green, Cat::Orange, Cat::Red, Cat::Purple, Cat::Teal, Cat::Amber, Cat::Slate];
}

/// A translucent colour from straight (unmultiplied) channels, usable in constants.
const fn tint(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// A colour from a `0xRRGGBB` literal.
const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Code colours for one background; plain text uses the surface's own text colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Syntax {
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub builtin: Color32,
}

const LIGHT_SYNTAX: Syntax =
    Syntax { keyword: rgb(0x0B57B3), string: rgb(0xA3361B), number: rgb(0x157A5A), comment: rgb(0x6B7280), builtin: rgb(0x7B3FB5) };
const DARK_SYNTAX: Syntax =
    Syntax { keyword: rgb(0x6FAEFF), string: rgb(0xF09A7A), number: rgb(0x7FD1A8), comment: rgb(0x9AA3AE), builtin: rgb(0xC69BF2) };

/// Every colour GridCraft itself draws on the sheet, for one sheet: the light sheet (the default,
/// unchanged) or the dark sheet (`docs/dark-sheet.md`). These are not workbook colours (those are
/// mapped per cell by the dark-sheet colour rule, [`ink`]); `contrast_tests` holds each value
/// against the sheet it sits on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetColors {
    /// Cell background where no fill is set.
    pub grid_bg: Color32,
    pub gridline: Color32,
    /// Automatic (unset) font colour.
    pub cell_text: Color32,
    /// Selection outline, fill handle and active cell.
    pub sel_border: Color32,
    pub sel_fill: Color32,
    pub active_row: Color32,
    /// Formula reference outlines and the in-cell editor's references.
    pub ref_colors: [Color32; 8],
    /// Filter, validation and paste-options buttons, object handles, form controls.
    pub control_bg: Color32,
    pub control_border: Color32,
    pub control_text: Color32,
    /// Picture placeholder, form slider track.
    pub track: Color32,
    /// Form slider thumb.
    pub thumb: Color32,
    /// Form button or spin arrow under the pointer, and pressed.
    pub control_hover: Color32,
    pub control_pressed: Color32,
    /// The filled part of a form slider's track.
    pub slider_fill: Color32,
    /// The ring around the form control that takes the arrow keys.
    pub focus: Color32,
    pub macro_input: Color32,
    pub macro_footprint: Color32,
    pub macro_conflict: Color32,
    pub macro_target: Color32,
    /// The PY tag on a Python cell, and its text.
    pub py_tag: Color32,
    pub on_py_tag: Color32,
    pub trace_arrow: Color32,
    pub trace_error: Color32,
    /// The error-checking triangle in a cell's corner.
    pub error_check_green: Color32,
    /// The frozen-pane lines across the cells.
    pub freeze_line: Color32,
    /// The ring round the fill handle: the sheet colour, so the handle stands off the cell edge.
    pub handle_halo: Color32,
    /// The dashed fill-drag preview.
    pub drag_outline: Color32,
    /// The outline where a moved range will land.
    pub move_outline: Color32,
    /// Page Break Preview: shade outside the print area, break lines, "Page N" watermark.
    pub page_outside: Color32,
    pub page_break: Color32,
    pub page_watermark: Color32,
}

impl SheetColors {
    /// The default sheet: white, exactly as it was before the dark sheet existed.
    pub const LIGHT: SheetColors = SheetColors {
        grid_bg: Color32::WHITE,
        gridline: rgb(0xE1E1E1),
        cell_text: Color32::BLACK,
        sel_border: rgb(0x107C41),
        sel_fill: tint(0x10, 0x7C, 0x41, 0x1C),
        active_row: tint(0x10, 0x7C, 0x41, 0x2C),
        ref_colors: [rgb(0x2E6FD8), rgb(0xD03C3C), rgb(0x7A4FC9), rgb(0x23803D), rgb(0xB5531A), rgb(0x16798A), rgb(0xB83B8E), rgb(0x77681A)],
        control_bg: rgb(0xF3F3F3),
        control_border: rgb(0x8A8A8A),
        control_text: rgb(0x1F1F1F),
        track: rgb(0xD6D6D6),
        thumb: rgb(0x6B6B6B),
        control_hover: rgb(0xE4E4E4),
        control_pressed: rgb(0xCFCFCF),
        slider_fill: rgb(0x107C41),
        focus: rgb(0x107C41),
        macro_input: rgb(0x2E6FD8),
        macro_footprint: rgb(0x23803D),
        macro_conflict: rgb(0xC42B1C),
        macro_target: rgb(0x6B3FC0),
        // A deep green, white text on it.
        py_tag: rgb(0x1E6E4E),
        on_py_tag: Color32::WHITE,
        trace_arrow: rgb(0x1F5FC9),
        trace_error: rgb(0xD13438),
        error_check_green: rgb(0x1E8E3E),
        freeze_line: Color32::from_gray(150),
        handle_halo: Color32::WHITE,
        drag_outline: Color32::from_gray(90),
        move_outline: Color32::from_gray(110),
        page_outside: Color32::from_black_alpha(70),
        page_break: rgb(0x2E6FD8),
        page_watermark: Color32::from_black_alpha(45),
    };

    /// The light sheet under a dark interface: [`SheetColors::LIGHT`] with the selection and
    /// active-row tints and the error-check green the dark interface has always used.
    pub const LIGHT_IN_DARK_UI: SheetColors = SheetColors {
        sel_fill: tint(0x10, 0x7C, 0x41, 0x24),
        active_row: tint(0x6F, 0xD3, 0x9A, 0x30),
        error_check_green: rgb(0x4CC26B),
        ..SheetColors::LIGHT
    };

    /// The dark sheet: WordCraft's paper grey `#333333`, light text, the same hues lifted.
    pub const DARK: SheetColors = SheetColors {
        grid_bg: rgb(0x333333),
        gridline: rgb(0x4B4B4B),
        cell_text: Color32::WHITE,
        sel_border: rgb(0x5CC587),
        sel_fill: tint(0x5C, 0xC5, 0x87, 0x24),
        active_row: tint(0x6F, 0xD3, 0x9A, 0x30),
        ref_colors: [rgb(0x6FA3F5), rgb(0xF27C7C), rgb(0xB394EE), rgb(0x6CCB85), rgb(0xF0A061), rgb(0x5CC9DA), rgb(0xE77FC4), rgb(0xD4C25A)],
        control_bg: rgb(0x3C3C3C),
        control_border: rgb(0x9A9A9A),
        control_text: rgb(0xEEEEEE),
        track: rgb(0x4A4A4A),
        thumb: rgb(0xA9A9A9),
        control_hover: rgb(0x4A4A4A),
        control_pressed: rgb(0x575757),
        slider_fill: rgb(0x5CC587),
        focus: rgb(0x5CC587),
        macro_input: rgb(0x6FA3F5),
        macro_footprint: rgb(0x6CCB85),
        macro_conflict: rgb(0xF1705F),
        macro_target: rgb(0xB394EE),
        py_tag: rgb(0x5CBF94),
        on_py_tag: rgb(0x0B1F17),
        trace_arrow: rgb(0x6FA3F5),
        trace_error: rgb(0xF1707A),
        error_check_green: rgb(0x4CC26B),
        freeze_line: rgb(0x8A8A8A),
        handle_halo: rgb(0x333333),
        drag_outline: rgb(0xC8C8C8),
        move_outline: rgb(0xC8C8C8),
        page_outside: Color32::from_black_alpha(120),
        page_break: rgb(0x6FA3F5),
        page_watermark: Color32::from_rgba_premultiplied(70, 70, 70, 70),
    };

    /// The set for a sheet: dark when the dark sheet is on, whatever the interface; otherwise the
    /// light sheet as each Display theme has always drawn it.
    pub const fn of(dark_ui: bool, dark_sheet: bool) -> SheetColors {
        match (dark_sheet, dark_ui) {
            (true, _) => SheetColors::DARK,
            (false, false) => SheetColors::LIGHT,
            (false, true) => SheetColors::LIGHT_IN_DARK_UI,
        }
    }
}

/// Where the dark-sheet flag lives in the egui context (`Tokens::get` reads it).
fn dark_sheet_key() -> egui::Id {
    egui::Id::unique("gridcraft.dark_sheet")
}

/// Mirrors the dark-sheet preference into the egui context; the update loop calls it every frame,
/// and [`Tokens::get`] reads it, so every painter follows without a new parameter.
pub fn set_dark_sheet(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(dark_sheet_key(), on));
}

/// Whether the dark sheet is on for this context (off until [`set_dark_sheet`] says otherwise).
pub fn dark_sheet(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(dark_sheet_key())).unwrap_or(false)
}

/// Where the dark-charts flag lives in the egui context (`chartview` reads it).
fn dark_charts_key() -> egui::Id {
    egui::Id::unique("gridcraft.dark_charts")
}

/// Mirrors the dark-charts preference into the egui context; the update loop calls it every frame.
pub fn set_dark_charts(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(dark_charts_key(), on));
}

/// Whether charts are drawn dark for this context (off until [`set_dark_charts`] says otherwise).
pub fn dark_charts(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(dark_charts_key())).unwrap_or(false)
}

/// Where the dark-pictures flag lives in the egui context (the picture caches read it).
fn dark_pictures_key() -> egui::Id {
    egui::Id::unique("gridcraft.dark_pictures")
}

/// Mirrors the dark-pictures preference into the egui context; the update loop calls it every frame.
pub fn set_dark_pictures(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(dark_pictures_key(), on));
}

/// Whether pictures are drawn dark for this context (off until [`set_dark_pictures`] says otherwise).
pub fn dark_pictures(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(dark_pictures_key())).unwrap_or(false)
}

/// Applies [`ink`] to every pixel of a decoded picture (the dark-pictures setting).
pub fn ink_image(image: &mut egui::ColorImage) {
    for px in &mut image.pixels {
        *px = ink(*px);
    }
}

impl Tokens {
    /// Light interface, light sheet.
    pub fn light() -> Tokens {
        Tokens::palette(false, false)
    }

    /// Dark interface, light sheet (the dark sheet is a separate setting).
    pub fn dark() -> Tokens {
        Tokens::palette(true, false)
    }

    /// The same tokens with the sheet fields of the other sheet. Print Preview uses
    /// `with_dark_sheet(false)`: it shows what will print.
    pub fn with_dark_sheet(self, dark_sheet: bool) -> Tokens {
        Tokens::palette(self.dark, dark_sheet)
    }

    /// One palette for every combination: the chrome fields take their light or dark value by
    /// Display theme (`dark`), the sheet fields come from [`SheetColors::of`]`(dark, dark_sheet)`.
    pub fn palette(dark: bool, dark_sheet: bool) -> Tokens {
        let sheet = SheetColors::of(dark, dark_sheet);
        Tokens {
            dark,
            dark_sheet,
            window: pick(dark, Color32::from_rgb(0xF3, 0xF3, 0xF3), Color32::from_rgb(0x29, 0x29, 0x29)),
            ribbon: pick(dark, Color32::from_rgb(0xFF, 0xFF, 0xFF), Color32::from_rgb(0x33, 0x33, 0x33)),
            ribbon_border: pick(dark, Color32::from_rgb(0xE3, 0xE3, 0xE3), Color32::from_rgb(0x40, 0x40, 0x40)),
            text: pick(dark, Color32::from_rgb(0x24, 0x24, 0x24), Color32::from_rgb(0xEE, 0xEE, 0xEE)),
            text_dim: pick(dark, Color32::from_rgb(0x61, 0x61, 0x61), Color32::from_rgb(0xB0, 0xB0, 0xB0)),
            text_disabled: pick(dark, Color32::from_rgb(0xB0, 0xB0, 0xB0), Color32::from_rgb(0x70, 0x70, 0x70)),
            accent: pick(dark, Color32::from_rgb(0x10, 0x7C, 0x41), Color32::from_rgb(0x5C, 0xC5, 0x87)),
            accent_dark: pick(dark, Color32::from_rgb(0x0B, 0x5E, 0x31), Color32::from_rgb(0x3F, 0xA8, 0x6C)),
            accent_soft: pick(dark, Color32::from_rgb(0xDF, 0xF1, 0xE6), Color32::from_rgb(0x23, 0x45, 0x31)),
            hover: pick(dark, Color32::from_rgb(0xEB, 0xEB, 0xEB), Color32::from_rgb(0x42, 0x42, 0x42)),
            pressed: pick(dark, Color32::from_rgb(0xDD, 0xDD, 0xDD), Color32::from_rgb(0x4D, 0x4D, 0x4D)),
            separator: pick(dark, Color32::from_rgb(0xE0, 0xE0, 0xE0), Color32::from_rgb(0x48, 0x48, 0x48)),
            grid_bg: sheet.grid_bg,
            desk: pick(dark, Color32::from_rgb(0xDE, 0xDE, 0xDE), Color32::from_rgb(0x3A, 0x3A, 0x3A)),
            gridline: sheet.gridline,
            header_bg: pick(dark, Color32::from_rgb(0xF8, 0xF8, 0xF8), Color32::from_rgb(0x30, 0x30, 0x30)),
            header_text: pick(dark, Color32::from_rgb(0x42, 0x42, 0x42), Color32::from_rgb(0xCC, 0xCC, 0xCC)),
            header_line: pick(dark, Color32::from_rgb(0xD6, 0xD6, 0xD6), Color32::from_rgb(0x45, 0x45, 0x45)),
            header_sel_bg: pick(dark, Color32::from_rgb(0xE1, 0xE1, 0xE1), Color32::from_rgb(0x45, 0x45, 0x45)),
            header_sel_text: pick(dark, Color32::from_rgb(0x0B, 0x5E, 0x31), Color32::from_rgb(0x6F, 0xD3, 0x9A)),
            header_all_bg: pick(dark, Color32::from_rgb(0xC8, 0xE6, 0xD3), Color32::from_rgb(0x2E, 0x55, 0x3E)),
            sel_fill: sheet.sel_fill,
            active_row: sheet.active_row,
            sel_border: sheet.sel_border,
            input_bg: pick(dark, Color32::WHITE, Color32::from_rgb(0x1F, 0x1F, 0x1F)),
            input_border: pick(dark, Color32::from_rgb(0xD1, 0xD1, 0xD1), Color32::from_rgb(0x50, 0x50, 0x50)),
            tab_bar: pick(dark, Color32::from_rgb(0xF3, 0xF3, 0xF3), Color32::from_rgb(0x29, 0x29, 0x29)),
            tab_active: pick(dark, Color32::WHITE, Color32::from_rgb(0x3A, 0x3A, 0x3A)),
            status_bar: pick(dark, Color32::from_rgb(0xF3, 0xF3, 0xF3), Color32::from_rgb(0x29, 0x29, 0x29)),
            cell_text: sheet.cell_text,
            menu_bg: pick(dark, Color32::WHITE, Color32::from_rgb(0x30, 0x30, 0x30)),
            shadow: pick(dark, Color32::from_black_alpha(40), Color32::from_black_alpha(90)),
            danger: pick(dark, Color32::from_rgb(0xC4, 0x2B, 0x1C), Color32::from_rgb(0xFF, 0x95, 0x85)),
            error_check_green: sheet.error_check_green,
            warning_fill: Color32::from_rgb(0xF2, 0xB7, 0x05),
            warning_ink: Color32::BLACK,
            notice_warn_bg: pick(dark, Color32::from_rgb(0xFF, 0xF4, 0xCE), Color32::from_rgb(0x4A, 0x3F, 0x1E)),
            notice_warn_border: pick(dark, Color32::from_rgb(0xF0, 0xC3, 0x6E), Color32::from_rgb(0x7A, 0x67, 0x30)),
            notice_info_bg: pick(dark, Color32::from_rgb(0xEA, 0xF2, 0xFB), Color32::from_rgb(0x24, 0x33, 0x44)),
            notice_info_border: pick(dark, Color32::from_rgb(0xB9, 0xD2, 0xEC), Color32::from_rgb(0x3B, 0x55, 0x72)),
            on_accent: pick(dark, Color32::WHITE, rgb(0x0F2A1A)),
            cat: pick(
                dark,
                [rgb(0x2F6FCF), rgb(0x1E8A4C), rgb(0xC8601A), rgb(0xC8372D), rgb(0x8350C8), rgb(0x13808A), rgb(0x9C6C00), rgb(0x5A6B7D)],
                [rgb(0x6EA8F7), rgb(0x5CC587), rgb(0xF0965A), rgb(0xF07B6E), rgb(0xB394EE), rgb(0x4CC7CF), rgb(0xF2C14E), rgb(0xA9B6C4)],
            ),
            paper: pick(dark, Color32::WHITE, rgb(0x454545)),
            ref_colors: pick(
                dark,
                SheetColors::LIGHT.ref_colors,
                [rgb(0x6FA3F5), rgb(0xF27C7C), rgb(0xB394EE), rgb(0x6CCB85), rgb(0xF0A061), rgb(0x5CC9DA), rgb(0xE77FC4), rgb(0xD4C25A)],
            ),
            sheet_ref_colors: sheet.ref_colors,
            syntax: pick(dark, LIGHT_SYNTAX, DARK_SYNTAX),
            warning: rgb(0xF2C32E),
            on_warning: rgb(0x1F1F1F),
            sheet_control_bg: sheet.control_bg,
            sheet_control_border: sheet.control_border,
            sheet_control_text: sheet.control_text,
            sheet_track: sheet.track,
            sheet_thumb: sheet.thumb,
            sheet_control_hover: sheet.control_hover,
            sheet_control_pressed: sheet.control_pressed,
            sheet_fill: sheet.slider_fill,
            sheet_focus: sheet.focus,
            recording: pick(dark, rgb(0xC42B1C), rgb(0xF1705F)),
            breakpoint: pick(dark, rgb(0xD13438), rgb(0xF1707A)),
            sheet_macro_input: sheet.macro_input,
            sheet_macro_footprint: sheet.macro_footprint,
            sheet_macro_conflict: sheet.macro_conflict,
            sheet_macro_target: sheet.macro_target,
            badge_py: pick(dark, SheetColors::LIGHT.py_tag, rgb(0x5CBF94)),
            on_badge: pick(dark, Color32::WHITE, rgb(0x0B1F17)),
            sheet_py_tag: sheet.py_tag,
            on_sheet_py_tag: sheet.on_py_tag,
            sheet_trace_arrow: sheet.trace_arrow,
            sheet_trace_error: sheet.trace_error,
            sheet_freeze_line: sheet.freeze_line,
            sheet_handle_halo: sheet.handle_halo,
            sheet_drag_outline: sheet.drag_outline,
            sheet_move_outline: sheet.move_outline,
            sheet_page_outside: sheet.page_outside,
            sheet_page_break: sheet.page_break,
            sheet_page_watermark: sheet.page_watermark,
        }
    }

    /// The tokens for this context: its interface theme and its Dark sheet setting.
    pub fn get(ctx: &egui::Context) -> Tokens {
        Tokens::palette(ctx.global_style().visuals.dark_mode, dark_sheet(ctx))
    }

    /// One colour of the category palette.
    pub fn cat(&self, c: Cat) -> Color32 {
        self.cat.get(c as usize).copied().unwrap_or(self.text)
    }

    /// The reference colour for a formula's `index`-th reference, on the bar or on the sheet.
    pub fn ref_color(&self, index: usize, on_sheet: bool) -> Color32 {
        let set = if on_sheet { &self.sheet_ref_colors } else { &self.ref_colors };
        set.get(index % set.len()).copied().unwrap_or(self.text)
    }

    /// A colour authored for the white sheet (a fill, a shape) as painted on this sheet: itself on
    /// the light sheet, through the colour rule ([`ink`]) on the dark sheet.
    pub fn on_sheet(&self, c: Color32) -> Color32 {
        if self.dark_sheet { ink(c) } else { c }
    }

    /// A colour authored over the authored colour `paper` (text on its fill, a border or line on
    /// the sheet) as painted on this sheet: itself on the light sheet, through the colour rule and
    /// the contrast guard ([`ink_guarded`]) on the dark sheet.
    pub fn on_sheet_over(&self, c: Color32, paper: Color32, min_ratio: f32) -> Color32 {
        if self.dark_sheet { ink_guarded(c, paper, min_ratio) } else { c }
    }
}

/// WCAG 2 relative luminance of an opaque colour.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG 2 contrast ratio between two opaque colours (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The grey a white page becomes on the dark sheet (WordCraft's `DARK_PAPER`, `#333333`).
pub const DARK_PAPER: u8 = 0x33;

/// Least contrast the guard keeps for cell text (WCAG 2 AA) and for borders (non-text).
pub const INK_TEXT_MIN: f32 = 4.5;
pub const INK_LINE_MIN: f32 = 3.0;

/// The dark-sheet colour rule (`docs/dark-sheet.md`) for one workbook colour: invert its HSL
/// lightness, keeping hue and saturation, then lift the blacks to [`DARK_PAPER`]. White becomes
/// the sheet grey, black becomes white, pure hues barely change. Alpha is kept.
pub fn ink(c: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let (hi, lo) = (i32::from(r.max(g).max(b)), i32::from(r.min(g).min(b)));
    let d = 255 - hi - lo;
    let p = i32::from(DARK_PAPER);
    let map = |v: u8| {
        let inverted = (i32::from(v) + d).clamp(0, 255);
        u8::try_from(p + inverted * (255 - p) / 255).unwrap_or(u8::MAX)
    };
    Color32::from_rgba_unmultiplied(map(r), map(g), map(b), a)
}

/// [`ink`] for a colour drawn over another workbook colour (text over its cell's fill, a border
/// beside it), with the contrast guard: if the mapped pair reads worse than
/// `min(min_ratio, contrast of the authored pair)`, the ink's HSL lightness moves away from the
/// background, hue kept, until it does (or reaches white or black). `fg` and `bg` are the
/// authored colours; the result goes on `ink(bg)`. A pair the author made illegible (white on
/// white) stays so. Results are cached, so a large sheet pays for each pair once.
pub fn ink_guarded(fg: Color32, bg: Color32, min_ratio: f32) -> Color32 {
    ink_guarded_on(fg, bg, ink(bg), min_ratio)
}

/// [`ink_guarded`] where what is painted behind the ink is not simply `ink(bg)`: a pattern fill is
/// painted as the blend of its two mapped colours, which the non-linear [`ink`] of the authored
/// blend does not match. The target still comes from the authored pair `fg` on `bg`; the mapped ink
/// is held to it against `painted_bg`.
pub fn ink_guarded_on(fg: Color32, bg: Color32, painted_bg: Color32, min_ratio: f32) -> Color32 {
    type Key = ([u8; 4], [u8; 4], [u8; 4], u32);
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<Key, Color32>> = std::cell::RefCell::default();
    }
    /// Distinct pairs kept before the cache starts over (a sheet has a few dozen).
    const CAP: usize = 4096;
    let key = (fg.to_array(), bg.to_array(), painted_bg.to_array(), min_ratio.to_bits());
    let cached = CACHE.try_with(|c| c.try_borrow().ok().and_then(|m| m.get(&key).copied())).ok().flatten();
    if let Some(hit) = cached {
        return hit;
    }
    let out = guard_ink(fg, bg, painted_bg, min_ratio);
    let _ = CACHE.try_with(|c| {
        if let Ok(mut m) = c.try_borrow_mut() {
            if m.len() >= CAP {
                m.clear();
            }
            m.insert(key, out);
        }
    });
    out
}

/// `c` with its alpha dropped: contrast is measured on the straight colour.
fn opaque(c: Color32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgb(r, g, b)
}

/// The uncached guard: `ink(fg)`, its HSL lightness moved away from `painted_bg` until it reads at
/// least `min(min_ratio, contrast of fg on bg)` against it.
fn guard_ink(fg: Color32, bg: Color32, painted_bg: Color32, min_ratio: f32) -> Color32 {
    let target = min_ratio.min(contrast(opaque(fg), opaque(bg)));
    let (fg_ink, bg_ink) = (ink(fg), opaque(painted_bg));
    if contrast(opaque(fg_ink), bg_ink) >= target {
        return fg_ink;
    }
    use gridcraft_engine::model::style::{hsl_to_rgb, rgb_to_hsl};
    let [r, g, b, a] = fg_ink.to_srgba_unmultiplied();
    let (h, s, l0) = rgb_to_hsl([r, g, b]);
    // Lighter on a background that white reads better on, darker otherwise.
    let up = contrast(Color32::WHITE, bg_ink) >= contrast(Color32::BLACK, bg_ink);
    let mut best = fg_ink;
    for step in 1..=50 {
        let l = if up { l0 + f64::from(step) * 0.02 } else { l0 - f64::from(step) * 0.02 }.clamp(0.0, 1.0);
        let [r, g, b] = hsl_to_rgb(h, s, l);
        best = Color32::from_rgba_unmultiplied(r, g, b, a);
        if contrast(Color32::from_rgb(r, g, b), bg_ink) >= target || l <= 0.0 || l >= 1.0 {
            break;
        }
    }
    best
}

/// Installs light and dark token visuals in both theme slots, keeps the 2-3 px widget and 8-10 px window/menu corner radii from 0.36, and sets spacing, text styles and the active theme.
pub fn apply(ctx: &egui::Context, dark: bool) {
    // Install both palettes before System can switch between the style slots.
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        let t = if dark { Tokens::dark() } else { Tokens::light() };
        let mut v = if dark { Visuals::dark() } else { Visuals::light() };
        v.panel_fill = t.window;
        v.window_fill = t.menu_bg;
        v.extreme_bg_color = t.input_bg;
        v.selection.bg_fill = t.accent_soft;
        v.selection.stroke = Stroke::new(1.0, t.accent);
        v.hyperlink_color = t.accent;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.hovered.weak_bg_fill = t.hover;
        v.widgets.active.weak_bg_fill = t.pressed;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.input_border);
        // 0.37 raises the default widget corner radii; GridCraft keeps its 0.36 shape (#8617).
        v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(2);
        v.widgets.inactive.corner_radius = egui::CornerRadius::same(2);
        v.widgets.hovered.corner_radius = egui::CornerRadius::same(3);
        v.widgets.active.corner_radius = egui::CornerRadius::same(2);
        v.widgets.open.corner_radius = egui::CornerRadius::same(2);
        v.window_corner_radius = egui::CornerRadius::same(10);
        v.menu_corner_radius = egui::CornerRadius::same(8);
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow };
        ctx.set_visuals_of(theme, v);
    }
    ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.text_styles.insert(egui::TextStyle::Body, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Button, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Small, ui_font(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, ui_bold(18.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::new(12.0, FontFamily::Name(MONO.into())));
    });
}

/// Model colour → egui colour.
pub fn color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    /// Body text needs 4.5:1 (WCAG AA); glyphs, icons and other graphics need 3:1.
    const TEXT: f32 = 4.5;
    const GLYPH: f32 = 3.0;

    /// Every foreground/background pair the chrome draws: ribbon, popups, dialogs, panes, formula
    /// bar, status bar, plus the colours that sit on the sheet (light or dark).
    fn pairs(t: &Tokens) -> Vec<(String, Color32, Color32, f32)> {
        let mut v: Vec<(String, Color32, Color32, f32)> = Vec::new();
        let mut add = |name: &str, fg: Color32, bg: Color32, min: f32| v.push((name.to_string(), fg, bg, min));
        let surfaces = [
            ("window", t.window),
            ("ribbon", t.ribbon),
            ("menu", t.menu_bg),
            ("input", t.input_bg),
            ("status bar", t.status_bar),
            ("tab bar", t.tab_bar),
            ("active tab", t.tab_active),
            ("hover", t.hover),
        ];
        for (name, bg) in surfaces {
            add(&format!("text on {name}"), t.text, bg, TEXT);
            add(&format!("dim text on {name}"), t.text_dim, bg, TEXT);
            add(&format!("accent glyph on {name}"), t.accent, bg, GLYPH);
            add(&format!("danger on {name}"), t.danger, bg, TEXT);
        }
        add("text on pressed", t.text, t.pressed, TEXT);
        add("accent text (File, contextual tabs) on window", t.accent, t.window, TEXT);
        for (name, bg) in [("ribbon", t.ribbon), ("menu", t.menu_bg), ("hover", t.hover), ("pressed", t.pressed)] {
            for c in Cat::ALL {
                add(&format!("{c:?} icon colour on {name}"), t.cat(c), bg, GLYPH);
            }
        }
        add("icon stroke on paper", t.text, t.paper, GLYPH);
        add("on_accent on accent", t.on_accent, t.accent, TEXT);
        add("warning glyph", t.on_warning, t.warning, TEXT);
        add("notice text (warning)", t.text, t.notice_warn_bg, TEXT);
        add("notice text (info)", t.text, t.notice_info_bg, TEXT);
        add("header text", t.header_text, t.header_bg, TEXT);
        add("selected header text", t.header_sel_text, t.header_sel_bg, TEXT);
        for (i, c) in t.ref_colors.iter().enumerate() {
            add(&format!("formula bar reference {i}"), *c, t.input_bg, TEXT);
        }
        let s = t.syntax;
        for (name, c) in [("keyword", s.keyword), ("string", s.string), ("number", s.number), ("comment", s.comment), ("builtin", s.builtin)] {
            add(&format!("code {name} on input"), c, t.input_bg, TEXT);
        }
        // On the sheet.
        add("cell text", t.cell_text, t.grid_bg, TEXT);
        for (i, c) in t.sheet_ref_colors.iter().enumerate() {
            add(&format!("sheet reference {i}"), *c, t.grid_bg, TEXT);
        }
        add("selection border on sheet", t.sel_border, t.grid_bg, GLYPH);
        if t.dark_sheet {
            // The light sheet keeps the values it had before the dark sheet existed; these two are
            // held to WCAG on the dark sheet only.
            add("error-check triangle on the dark sheet", t.error_check_green, t.grid_bg, GLYPH);
            add("frozen-pane line on the dark sheet", t.sheet_freeze_line, t.grid_bg, GLYPH);
        }
        // The tints are translucent: judge the ink under them (cell text over the blended tint).
        for (name, tint) in [("selection tint", t.sel_fill), ("active-row tint", t.active_row)] {
            add(&format!("cell text under {name}"), t.cell_text, over(tint, t.grid_bg), TEXT);
            add(&format!("sheet reference 0 outline under {name}"), t.sheet_ref_colors[0], over(tint, t.grid_bg), GLYPH);
        }
        add("sheet control text", t.sheet_control_text, t.sheet_control_bg, TEXT);
        add("sheet control border on sheet", t.sheet_control_border, t.grid_bg, GLYPH);
        add("slider thumb on track", t.sheet_thumb, t.sheet_track, GLYPH);
        add("sheet control text on hover", t.sheet_control_text, t.sheet_control_hover, TEXT);
        add("sheet control text when pressed", t.sheet_control_text, t.sheet_control_pressed, TEXT);
        add("slider fill on sheet", t.sheet_fill, t.grid_bg, GLYPH);
        add("slider fill on track", t.sheet_fill, t.sheet_track, GLYPH);
        add("control focus ring on sheet", t.sheet_focus, t.grid_bg, GLYPH);
        add("recording dot on ribbon", t.recording, t.ribbon, GLYPH);
        add("recording dot on status bar", t.recording, t.status_bar, GLYPH);
        for (name, c) in [
            ("macro input", t.sheet_macro_input),
            ("macro footprint", t.sheet_macro_footprint),
            ("macro conflict", t.sheet_macro_conflict),
            ("macro target", t.sheet_macro_target),
        ] {
            add(&format!("{name} outline on sheet"), c, t.grid_bg, GLYPH);
        }
        add("trace arrow on sheet", t.sheet_trace_arrow, t.grid_bg, GLYPH);
        add("trace error arrow on sheet", t.sheet_trace_error, t.grid_bg, GLYPH);
        add("fill-drag outline on sheet", t.sheet_drag_outline, t.grid_bg, GLYPH);
        add("move outline on sheet", t.sheet_move_outline, t.grid_bg, GLYPH);
        add("page-break line on sheet", t.sheet_page_break, t.grid_bg, GLYPH);
        add("fill-handle fill on its halo", t.sel_border, t.sheet_handle_halo, GLYPH);
        // The macro step editor: breakpoint dots in the gutter, error markers, the current step.
        add("breakpoint on window", t.breakpoint, t.window, GLYPH);
        add("breakpoint on current step", t.breakpoint, t.accent_soft, GLYPH);
        add("text on current step", t.text, t.accent_soft, TEXT);
        add("current step arrow", t.accent, t.accent_soft, GLYPH);
        add("error marker on window", t.danger, t.window, GLYPH);
        add("PY badge text", t.on_badge, t.badge_py, TEXT);
        add("PY badge on window", t.badge_py, t.window, GLYPH);
        add("PY badge on input", t.badge_py, t.input_bg, GLYPH);
        add("PY tag text", t.on_sheet_py_tag, t.sheet_py_tag, TEXT);
        add("PY tag on sheet", t.sheet_py_tag, t.grid_bg, GLYPH);
        v
    }

    /// A translucent colour over an opaque one, as the painter blends it.
    fn over(top: Color32, under: Color32) -> Color32 {
        let a = f32::from(top.a()) / 255.0;
        let mix = |t: u8, u: u8| (f32::from(t) * a + f32::from(u) * (1.0 - a)).round() as u8;
        // `Color32` stores premultiplied alpha; `to_srgba_unmultiplied` gives the straight colour.
        let [r, g, b, _] = top.to_srgba_unmultiplied();
        Color32::from_rgb(mix(r, under.r()), mix(g, under.g()), mix(b, under.b()))
    }

    /// Display theme × sheet: the interface and the sheet are chosen independently.
    fn combinations() -> [(&'static str, Tokens); 4] {
        [
            ("light UI, light sheet", Tokens::palette(false, false)),
            ("dark UI, light sheet", Tokens::palette(true, false)),
            ("light UI, dark sheet", Tokens::palette(false, true)),
            ("dark UI, dark sheet", Tokens::palette(true, true)),
        ]
    }

    /// Every text and background token pair meets WCAG 2 contrast in each theme on both sheets.
    #[test]
    fn token_pairs_meet_wcag_contrast_in_every_theme_and_sheet() {
        let mut failures = Vec::new();
        for (label, t) in combinations() {
            for (name, fg, bg, min) in pairs(&t) {
                let ratio = contrast(fg, bg);
                if ratio < min {
                    failures.push(format!("{label}: {name} is {ratio:.2}:1, needs {min}:1"));
                }
            }
        }
        assert!(failures.is_empty(), "contrast failures:\n{}", failures.join("\n"));
    }

    #[test]
    fn contrast_matches_wcag_reference_values() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
        // #767676 on white is the classic 4.54:1 grey.
        assert!((contrast(rgb(0x767676), Color32::WHITE) - 4.54).abs() < 0.01);
    }

    /// Sheet tokens change with the Dark sheet setting only; the interface theme leaves them alone.
    #[test]
    fn sheet_tokens_follow_the_sheet_not_the_theme() {
        let sheet_fields = |t: &Tokens| {
            (
                (t.grid_bg, t.gridline, t.cell_text, t.sel_border, t.sel_fill, t.active_row, t.error_check_green),
                t.sheet_ref_colors,
                (t.sheet_control_bg, t.sheet_control_border, t.sheet_control_text, t.sheet_track),
                (t.sheet_thumb, t.sheet_control_hover, t.sheet_control_pressed, t.sheet_fill, t.sheet_focus),
                (t.sheet_macro_input, t.sheet_macro_footprint, t.sheet_macro_conflict, t.sheet_macro_target),
                (t.sheet_py_tag, t.on_sheet_py_tag, t.sheet_trace_arrow, t.sheet_trace_error),
                (t.sheet_freeze_line, t.sheet_handle_halo, t.sheet_drag_outline, t.sheet_move_outline),
                (t.sheet_page_outside, t.sheet_page_break, t.sheet_page_watermark),
            )
        };
        // Display theme never changes the dark sheet.
        assert_eq!(sheet_fields(&Tokens::palette(false, true)), sheet_fields(&Tokens::palette(true, true)));
        // On the light sheet it changes only what it always changed: two tints and the error-check green.
        let (l, ld) = (Tokens::palette(false, false), Tokens::palette(true, false));
        let others = |t: &Tokens| {
            let mut f = sheet_fields(t);
            f.0 = (f.0.0, f.0.1, f.0.2, f.0.3, Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::TRANSPARENT);
            f
        };
        assert_eq!(others(&l), others(&ld));
        // The dark sheet never changes the interface.
        let d = Tokens::palette(false, true);
        assert_ne!(sheet_fields(&l), sheet_fields(&d));
        assert_eq!(
            (l.window, l.ribbon, l.text, l.header_bg, l.header_text, l.tab_bar, l.accent, l.ref_colors, l.paper),
            (d.window, d.ribbon, d.text, d.header_bg, d.header_text, d.tab_bar, d.accent, d.ref_colors, d.paper)
        );
        for c in Cat::ALL {
            assert_ne!(Tokens::light().cat(c), Tokens::dark().cat(c), "{c:?} has a dark-mode value of its own");
        }
    }

    /// With the setting off, every sheet token keeps the colour it had before the dark sheet existed.
    #[test]
    fn the_light_sheet_keeps_the_colours_it_had_before_the_dark_sheet() {
        // With the dark sheet off nothing on the sheet changes, under either Display theme: the
        // values the tokens had, and those the painters used to hard-code.
        for dark_ui in [false, true] {
            let t = Tokens::palette(dark_ui, false);
            assert!(!t.dark_sheet);
            assert_eq!((t.grid_bg, t.gridline, t.cell_text, t.sel_border), (Color32::WHITE, rgb(0xE1E1E1), Color32::BLACK, rgb(0x107C41)));
            let (sel_fill, active_row, error_check) = if dark_ui {
                (Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x24), Color32::from_rgba_unmultiplied(0x6F, 0xD3, 0x9A, 0x30), rgb(0x4CC26B))
            } else {
                (Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x1C), Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x2C), rgb(0x1E8E3E))
            };
            assert_eq!((t.sel_fill, t.active_row, t.error_check_green), (sel_fill, active_row, error_check));
            assert_eq!(
                t.sheet_ref_colors,
                [rgb(0x2E6FD8), rgb(0xD03C3C), rgb(0x7A4FC9), rgb(0x23803D), rgb(0xB5531A), rgb(0x16798A), rgb(0xB83B8E), rgb(0x77681A)]
            );
            assert_eq!(
                (t.sheet_control_bg, t.sheet_control_border, t.sheet_control_text, t.sheet_track),
                (rgb(0xF3F3F3), rgb(0x8A8A8A), rgb(0x1F1F1F), rgb(0xD6D6D6))
            );
            assert_eq!(
                (t.sheet_thumb, t.sheet_control_hover, t.sheet_control_pressed, t.sheet_fill, t.sheet_focus),
                (rgb(0x6B6B6B), rgb(0xE4E4E4), rgb(0xCFCFCF), rgb(0x107C41), rgb(0x107C41))
            );
            assert_eq!(
                (t.sheet_macro_input, t.sheet_macro_footprint, t.sheet_macro_conflict, t.sheet_macro_target),
                (rgb(0x2E6FD8), rgb(0x23803D), rgb(0xC42B1C), rgb(0x6B3FC0))
            );
            assert_eq!(
                (t.sheet_py_tag, t.on_sheet_py_tag, t.sheet_trace_arrow, t.sheet_trace_error),
                (rgb(0x1E6E4E), Color32::WHITE, rgb(0x1F5FC9), rgb(0xD13438))
            );
            assert_eq!(
                (t.sheet_freeze_line, t.sheet_handle_halo, t.sheet_drag_outline, t.sheet_move_outline),
                (Color32::from_gray(150), Color32::WHITE, Color32::from_gray(90), Color32::from_gray(110))
            );
            assert_eq!(
                (t.sheet_page_outside, t.sheet_page_break, t.sheet_page_watermark),
                (Color32::from_black_alpha(70), rgb(0x2E6FD8), Color32::from_black_alpha(45))
            );
            // Workbook colours are painted as authored.
            assert_eq!(t.on_sheet(rgb(0xDDEBF7)), rgb(0xDDEBF7));
            assert_eq!(t.on_sheet_over(rgb(0x0000FF), Color32::WHITE, INK_TEXT_MIN), rgb(0x0000FF));
        }
    }

    /// The dark sheet tokens are the values of the design note (docs/dark-sheet.md).
    #[test]
    fn the_dark_sheet_is_the_design_note() {
        let d = Tokens::palette(false, true);
        assert_eq!((d.grid_bg, d.gridline, d.cell_text, d.sel_border), (rgb(0x333333), rgb(0x4B4B4B), Color32::WHITE, rgb(0x5CC587)));
        assert!(d.dark_sheet && !d.dark);
        let l = Tokens::light();
        assert_eq!(l.with_dark_sheet(true), d);
        assert_eq!(d.with_dark_sheet(false), l);
        assert_eq!(d.on_sheet(Color32::WHITE), rgb(0x333333));
        assert_eq!(d.on_sheet_over(Color32::BLACK, Color32::WHITE, INK_TEXT_MIN), Color32::WHITE);
    }

    /// The dark gridline contrasts with its sheet at least as much as the light one does.
    #[test]
    fn dark_sheet_gridline_is_at_least_as_visible_as_the_light_one() {
        let light = contrast(SheetColors::LIGHT.gridline, SheetColors::LIGHT.grid_bg);
        let dark = contrast(SheetColors::DARK.gridline, SheetColors::DARK.grid_bg);
        assert!(light > 1.3 && dark >= light, "light {light:.2}:1, dark {dark:.2}:1");
    }

    #[test]
    fn ink_image_maps_every_pixel_and_keeps_alpha() {
        let mut image = egui::ColorImage::new([2, 1], vec![Color32::WHITE, Color32::from_rgba_unmultiplied(0, 0, 0, 128)]);
        ink_image(&mut image);
        assert_eq!(image.pixels[0], Color32::from_rgb(DARK_PAPER, DARK_PAPER, DARK_PAPER));
        assert_eq!(image.pixels[1].to_srgba_unmultiplied()[3], 128);
        assert_eq!(image.pixels[1], ink(Color32::from_rgba_unmultiplied(0, 0, 0, 128)));
    }

    /// Dark pictures is off in a fresh context and on only after it is set.
    #[test]
    fn dark_pictures_flag_is_off_until_set() {
        let ctx = egui::Context::default();
        assert!(!dark_pictures(&ctx));
        set_dark_pictures(&ctx, true);
        assert!(dark_pictures(&ctx));
        assert!(!dark_charts(&ctx) && !dark_sheet(&ctx), "the three settings are separate");
    }

    #[test]
    fn tokens_get_reads_the_dark_sheet_flag_from_the_context() {
        let ctx = egui::Context::default();
        assert!(!dark_sheet(&ctx), "off until set");
        assert_eq!(Tokens::get(&ctx).grid_bg, Color32::WHITE);
        set_dark_sheet(&ctx, true);
        let t = Tokens::get(&ctx);
        assert!(t.dark_sheet);
        assert_eq!(t.grid_bg, SheetColors::DARK.grid_bg);
        set_dark_sheet(&ctx, false);
        assert_eq!(Tokens::get(&ctx).grid_bg, Color32::WHITE);
    }

    /// `ink` inverts HSL lightness, keeps the hue and lifts black to the sheet grey #333333.
    #[test]
    fn ink_inverts_lightness_and_lifts_to_the_sheet_grey() {
        // The design note's examples.
        assert_eq!(ink(Color32::WHITE), rgb(0x333333));
        assert_eq!(ink(Color32::BLACK), Color32::WHITE);
        assert_eq!(ink(rgb(0x1F4E79)), rgb(0x9EC3E6), "dark blue heading → light blue");
        assert_eq!(ink(rgb(0xDDEBF7)), rgb(0x39444E), "pale fill → deep fill");
        assert_eq!(ink(rgb(0x0563C1)), rgb(0x64AFFB), "hyperlink blue");
        assert_eq!(ink(rgb(0xFF0000)), rgb(0xFF3333), "pure hues keep their lightness");
        // The dark sheet's own background, gridline and automatic text are the rule applied to the
        // light sheet's.
        for (light, dark) in [
            (SheetColors::LIGHT.grid_bg, SheetColors::DARK.grid_bg),
            (SheetColors::LIGHT.gridline, SheetColors::DARK.gridline),
            (SheetColors::LIGHT.cell_text, SheetColors::DARK.cell_text),
        ] {
            assert_eq!(ink(light), dark);
        }
        assert_eq!(ink(Color32::from_rgba_unmultiplied(0, 0, 0, 128)).a(), 128, "alpha kept");
    }

    /// The contrast guard makes mapped text legible but never asks for more contrast than authored.
    #[test]
    fn the_guard_restores_legibility_without_beating_the_author() {
        let white = Color32::WHITE;
        // Blue text on the sheet: 8.6:1 on white, 1.8:1 once mapped; lifted to 4.5:1, still blue.
        let blue = ink_guarded(rgb(0x0000FF), white, INK_TEXT_MIN);
        assert!(contrast(blue, ink(white)) >= 4.5, "{blue:?}");
        assert!(blue.b() > blue.r() && blue.b() > blue.g(), "hue kept: {blue:?}");
        // Automatic text on a yellow fill: white on yellow would be 1.1:1, so it goes dark.
        let yellow = rgb(0xFFFF00);
        let on_yellow = ink_guarded(Color32::BLACK, yellow, INK_TEXT_MIN);
        assert!(contrast(on_yellow, ink(yellow)) >= 4.5, "{on_yellow:?}");
        // [Red]: 4.0:1 on white, so 4.0:1 is enough on the dark sheet.
        let red = ink_guarded(rgb(0xFF0000), white, INK_TEXT_MIN);
        assert!(contrast(red, ink(white)) >= contrast(rgb(0xFF0000), white) - 0.01, "{red:?}");
        // Pairs that already read stay exactly ink().
        for c in [Color32::BLACK, rgb(0x1F4E79), rgb(0x0563C1)] {
            assert_eq!(ink_guarded(c, white, INK_TEXT_MIN), ink(c));
        }
        // Text hidden by its author (white on white) stays hidden.
        assert_eq!(ink_guarded(white, white, INK_TEXT_MIN), ink(white));
        // Borders are held to 3:1, and the cache answers the same the second time.
        let line = ink_guarded(rgb(0x0000FF), white, INK_LINE_MIN);
        assert!(contrast(line, ink(white)) >= 3.0);
        assert_eq!(ink_guarded(rgb(0x0000FF), white, INK_LINE_MIN), line);
    }

    /// Text authored on white is guarded against the dark grey actually painted, not white.
    #[test]
    fn ink_authored_on_white_is_held_against_the_dark_surface_actually_painted() {
        // Chart text, border-preview lines and cell text on an unfilled cell are authored on white
        // and painted on ink(white), the dark sheet's grey: the guard measures them there.
        let d = Tokens::palette(false, true);
        assert_eq!(ink(Color32::WHITE), d.grid_bg);
        for c in [rgb(0x0000FF), rgb(0x1F4E79), rgb(0xFF0000), rgb(0x00B050), rgb(0x7F7F7F), Color32::BLACK] {
            for min in [INK_TEXT_MIN, INK_LINE_MIN] {
                let target = min.min(contrast(c, Color32::WHITE));
                let got = contrast(d.on_sheet_over(c, Color32::WHITE, min), d.grid_bg);
                assert!(got >= target - 0.05, "{c:?}: {got:.2} on the dark sheet, needs {target:.2}");
            }
        }
    }

    /// Over a grid of colour pairs, guarded text contrasts at least as authored or at the minimum.
    #[test]
    fn every_guarded_pair_reads_at_least_as_well_as_authored_or_at_the_minimum() {
        let colours: Vec<Color32> = (0..=255u8)
            .step_by(51)
            .flat_map(|r| (0..=255u8).step_by(51).flat_map(move |g| (0..=255u8).step_by(85).map(move |b| Color32::from_rgb(r, g, b))))
            .collect();
        for &fg in &colours {
            for &bg in &colours {
                let target = INK_TEXT_MIN.min(contrast(fg, bg));
                let got = contrast(ink_guarded(fg, bg, INK_TEXT_MIN), ink(bg));
                // White or black is as far as the guard can go.
                let best = contrast(Color32::WHITE, ink(bg)).max(contrast(Color32::BLACK, ink(bg)));
                assert!(got >= target.min(best) - 0.05, "{fg:?} on {bg:?}: {got:.2} < {target:.2}");
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
