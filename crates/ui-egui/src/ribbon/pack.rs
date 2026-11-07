//! Pack a ribbon tab into the card width.
//!
//! A tab is a row of groups. The row is measured, then the least compact variant that fits is
//! drawn. Anything that still passes the right edge moves, as a whole group, into one overflow
//! menu. Labels are never shortened by clipping: a control is either fully inside the card or in
//! that menu.
//!
//! Home's variants, from widest to narrowest ([`HomeStep`]):
//! 1. Full labels.
//! 2. Wrap Text and Merge & Center become an icon with the command name as the tooltip.
//! 3. Conditional Formatting, Format as Table and Cell Styles stack in one column.
//! 4. The number-format icons wrap under the format dropdown, and the dropdown is only as wide as
//!    its text.
//!
//! Those widths are measured for the current language on the pinned UI font. [`HomeStep::choose`]
//! picks the first variant that fits the card. A layout test locks the order and the widths.
//!
//! On that font the four Home rows measure, in pixels:
//! English 1299, 1205, 1166, 1106; German 1480, 1332, 1248, 1188.
//! The default window insets the card by 28 px on each side, so the budget is the window width
//! minus 56. English therefore draws full labels from about 1360 px, alignment icons from 1266,
//! stacked styles from 1226 and wrapped number icons from 1166. German: 1540, 1392, 1308, 1248.
//! Below that the narrowest row stays and whole groups move into the menu from the right.

use std::cell::Cell;

use egui::{Align, Layout, Rect, Sense, Ui, vec2};

use super::controls::sep;
use super::{Tokens, theme};

/// Horizontal gap between neighbouring widgets inside a group and between a group and a separator.
pub(crate) const ITEM_SPACING_X: f32 = 2.0;

/// Width of the group separator ([`sep`] allocates 13px).
pub(crate) const SEP_WIDTH: f32 = 13.0;

/// Space between two groups: spacing, separator, spacing.
pub(crate) const GROUP_GAP: f32 = ITEM_SPACING_X + SEP_WIDTH + ITEM_SPACING_X;

/// The overflow chevron. Kept inside the card when any group does not fit.
pub(crate) const CHEVRON_WIDTH: f32 = 28.0;

/// Spare pixels so a one-pixel measurement difference overflows a group instead of cutting a word.
pub const PACK_SLACK: f32 = 4.0;

/// How tall a future caption under each group is. Zero until that row exists, so this change
/// reserves the hook without painting the captions.
pub(crate) fn group_caption_height() -> f32 {
    0.0
}

/// Ribbon card height, including the caption row when [`group_caption_height`] is non-zero.
pub(crate) fn card_height() -> f32 {
    88.0 + group_caption_height()
}

/// Top panel height: the tab row, and the card when the ribbon is open.
pub(crate) fn ribbon_panel_height(collapsed: bool) -> f32 {
    if collapsed { 34.0 } else { 34.0 + 92.0 + group_caption_height() }
}

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
}

/// True while a tab is being measured off-screen. Clicks and one-shot keytip invokes must not run.
pub(crate) fn measuring() -> bool {
    MEASURING.with(|c| c.get())
}

struct MeasuringGuard;

impl Drop for MeasuringGuard {
    fn drop(&mut self) {
        MEASURING.with(|c| c.set(false));
    }
}

/// Home's density, least compact first. A later step includes every earlier compaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum HomeStep {
    Full = 0,
    /// Wrap Text and Merge & Center are icons. Their names stay in the tooltip and the translation.
    CompactAlign = 1,
    /// The three style commands stack in one column.
    StackStyles = 2,
    /// Number-format icons sit on two rows under a dropdown as wide as its text.
    WrapNumber = 3,
}

impl HomeStep {
    pub const ALL: [HomeStep; 4] = [HomeStep::Full, HomeStep::CompactAlign, HomeStep::StackStyles, HomeStep::WrapNumber];

    pub(crate) fn compact_align(self) -> bool {
        self >= HomeStep::CompactAlign
    }

    pub(crate) fn stack_styles(self) -> bool {
        self >= HomeStep::StackStyles
    }

    pub(crate) fn wrap_number(self) -> bool {
        self >= HomeStep::WrapNumber
    }

    /// The least compact step whose measured content width fits `budget`.
    ///
    /// `content` is one width per [`HomeStep`], in enum order, from a live measurement. When none
    /// fit, the narrowest step is returned and [`groups_that_fit`] moves whole groups into the menu.
    pub fn choose(content: [f32; 4], budget: f32) -> HomeStep {
        for step in Self::ALL {
            if content[step as usize] + PACK_SLACK <= budget {
                return step;
            }
        }
        HomeStep::WrapNumber
    }
}

/// Width of a row of groups, including the separator gap between them.
pub(crate) fn row_width(widths: &[f32]) -> f32 {
    if widths.is_empty() {
        return 0.0;
    }
    let sum: f32 = widths.iter().copied().sum();
    sum + (widths.len() - 1) as f32 * GROUP_GAP
}

/// How many leading groups fit in `budget`, leaving room for the chevron when any group is left out.
/// A group is never split. Zero means the card shows only the chevron.
pub(crate) fn groups_that_fit(widths: &[f32], budget: f32) -> usize {
    if row_width(widths) + PACK_SLACK <= budget {
        return widths.len();
    }
    let usable = budget - CHEVRON_WIDTH - ITEM_SPACING_X - PACK_SLACK;
    if usable <= 0.0 {
        return 0;
    }
    let mut used = 0.0;
    let mut n = 0;
    for (i, w) in widths.iter().copied().enumerate() {
        let add = w + if i == 0 { 0.0 } else { GROUP_GAP };
        if used + add <= usable {
            used += add;
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// What the last frame placed. Tests read it; the chevron and the card edges come from the same pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RibbonLayout {
    /// [`HomeStep`] for Home. For a tab with a label-compact step, 0 is full and 1 is compact.
    pub step: u8,
    /// Measured content width of each density step. Unused entries stay 0.
    pub content: [f32; 4],
    /// Width the groups had to fit into (the card's inner width).
    pub budget: f32,
    /// Leading groups drawn in the card. The rest are in the overflow menu.
    pub fit: usize,
    pub groups: usize,
    pub placed_right: f32,
    pub ribbon_left: f32,
    pub ribbon_right: f32,
    pub ribbon_top: f32,
    pub ribbon_bottom: f32,
}

impl RibbonLayout {
    fn id() -> egui::Id {
        egui::Id::unique("ribbon_layout")
    }

    pub fn get(ctx: &egui::Context) -> RibbonLayout {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }
}

enum Mode {
    Measure,
    Place,
    Overflow,
}

/// One pass over a tab's groups. [`Groups::group`] runs `body` only when this pass draws it.
pub(crate) struct Groups<'a> {
    ui: &'a mut Ui,
    mode: Mode,
    index: usize,
    fit: usize,
    widths: &'a mut Vec<f32>,
    pass: u64,
}

impl Groups<'_> {
    pub(crate) fn group(&mut self, body: impl FnOnce(&mut Ui)) {
        let index = self.index;
        self.index += 1;
        match self.mode {
            Mode::Measure => {
                let w = measure_group(self.ui, self.pass, index, body);
                self.widths.push(w);
            }
            Mode::Place => {
                if index >= self.fit {
                    return;
                }
                if index > 0 {
                    sep(self.ui);
                }
                place_group(self.ui, index, body);
            }
            Mode::Overflow => {
                if index < self.fit {
                    return;
                }
                if index > self.fit {
                    self.ui.separator();
                }
                place_group(self.ui, index, body);
            }
        }
    }
}

/// A tab whose groups stay at full density. Groups that do not fit move into the chevron.
pub(crate) fn pack_tab(ui: &mut Ui, budget: f32, mut build: impl FnMut(&mut Groups<'_>)) {
    let widths = measure_groups(ui, 0, &mut build);
    let mut content = [0.0; 4];
    content[0] = row_width(&widths);
    place_groups(ui, budget, &widths, 0, content, &mut build);
}

/// Two densities: full labels, then the same groups with long text buttons reduced to icons.
/// The icon step is used only when the full row does not fit. Group overflow comes after that.
pub(crate) fn pack_density(ui: &mut Ui, budget: f32, mut build: impl FnMut(bool, &mut Groups<'_>)) {
    let full = measure_groups(ui, 0, &mut |g| build(false, g));
    let compact = measure_groups(ui, 1, &mut |g| build(true, g));
    let full_w = row_width(&full);
    let compact_w = row_width(&compact);
    let use_compact = full_w + PACK_SLACK > budget;
    let widths = if use_compact { compact } else { full };
    let mut content = [0.0; 4];
    content[0] = full_w;
    content[1] = compact_w;
    place_groups(ui, budget, &widths, u8::from(use_compact), content, &mut |g| build(use_compact, g));
}

/// Home's four measured densities. The least compact one that fits is placed.
pub(crate) fn pack_home(ui: &mut Ui, budget: f32, mut build: impl FnMut(HomeStep, &mut Groups<'_>)) {
    let mut content = [0.0; 4];
    let mut widths_of = Vec::new();
    for (i, step) in HomeStep::ALL.into_iter().enumerate() {
        let widths = measure_groups(ui, i as u64, &mut |g| build(step, g));
        content[i] = row_width(&widths);
        widths_of.push(widths);
    }
    let chosen = HomeStep::choose(content, budget);
    let widths = widths_of.get(chosen as usize).map(Vec::as_slice).unwrap_or(&[]);
    place_groups(ui, budget, widths, chosen as u8, content, &mut |g| build(chosen, g));
}

fn measure_groups(ui: &mut Ui, pass: u64, build: &mut impl FnMut(&mut Groups<'_>)) -> Vec<f32> {
    MEASURING.with(|c| c.set(true));
    let _guard = MeasuringGuard;
    let mut widths = Vec::new();
    let mut groups = Groups { ui, mode: Mode::Measure, index: 0, fit: 0, widths: &mut widths, pass };
    build(&mut groups);
    widths
}

fn place_groups(ui: &mut Ui, budget: f32, widths: &[f32], step: u8, content: [f32; 4], build: &mut impl FnMut(&mut Groups<'_>)) {
    let fit = groups_that_fit(widths, budget);
    let card = ui.max_rect();
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("ribbon-pack-row")
            .max_rect(Rect::from_min_size(card.left_top(), vec2(8000.0, card.height())))
            .layout(Layout::left_to_right(Align::Min)),
    );
    row.set_clip_rect(card);
    row.spacing_mut().item_spacing = vec2(ITEM_SPACING_X, 2.0);
    {
        let mut unused = Vec::new();
        let mut groups = Groups { ui: &mut row, mode: Mode::Place, index: 0, fit, widths: &mut unused, pass: 0 };
        build(&mut groups);
    }
    if fit < widths.len() {
        let resp = overflow_button(&mut row);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(48.0);
            let mut unused = Vec::new();
            let mut groups = Groups { ui, mode: Mode::Overflow, index: 0, fit, widths: &mut unused, pass: 0 };
            build(&mut groups);
        });
    }
    let used = row.min_rect();
    // A menu opened under a button can stretch `min_rect` below the card. The groups themselves
    // stay in the card; the height the rest of the window sees is the part inside it.
    let shown = used.intersect(card);
    ui.allocate_rect(shown, Sense::hover());
    let layout = RibbonLayout {
        step,
        content,
        budget,
        fit,
        groups: widths.len(),
        placed_right: used.right(),
        ribbon_left: card.left(),
        ribbon_right: card.right(),
        ribbon_top: card.top(),
        ribbon_bottom: card.bottom(),
    };
    let overflow = super::RibbonOverflow {
        content: row_width(widths),
        visible: card.width(),
        offset: 0.0,
        content_height: shown.height(),
        visible_height: card.height(),
    };
    ui.ctx().data_mut(|d| {
        d.insert_temp(RibbonLayout::id(), layout);
        d.insert_temp(super::RibbonOverflow::id(), overflow);
    });
}

/// One group, laid out left to right at its natural width. The same layout is used to measure.
///
/// The child is given a wide rectangle and labels are not wrapped. A zero-width slot would make a
/// checkbox or a caption break one letter per line, which is the clipping this packer exists to
/// avoid. The parent only keeps the rectangle the group actually used.
fn place_group(ui: &mut Ui, index: usize, body: impl FnOnce(&mut Ui)) {
    let used = layout_group(ui, ui.cursor().min, 100, index, false, body);
    ui.allocate_rect(used, Sense::hover());
}

fn measure_group(ui: &mut Ui, pass: u64, index: usize, body: impl FnOnce(&mut Ui)) -> f32 {
    layout_group(ui, ui.max_rect().left_top(), pass, index, true, body).width()
}

fn layout_group(ui: &mut Ui, origin: egui::Pos2, pass: u64, index: usize, invisible: bool, body: impl FnOnce(&mut Ui)) -> Rect {
    let mut builder = egui::UiBuilder::new()
        .id_salt(("ribbon-group", pass, index))
        .max_rect(Rect::from_min_size(origin, vec2(8000.0, 400.0)))
        .layout(Layout::left_to_right(Align::Min));
    if invisible {
        builder = builder.invisible();
    }
    let mut child = ui.new_child(builder);
    if invisible {
        child.set_clip_rect(Rect::NOTHING);
    }
    child.spacing_mut().item_spacing = vec2(ITEM_SPACING_X, 2.0);
    child.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    body(&mut child);
    child.min_rect()
}

fn overflow_button(ui: &mut Ui) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(CHEVRON_WIDTH, 72.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if resp.hovered() {
        Some(t.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "»", theme::ui_font(18.0), t.text);
    let name = crate::i18n::current(ui.ctx()).tr("More ribbon commands");
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    resp.on_hover_text(name.to_string())
}

/// The one-line form of a two-line ribbon label, in the language of this frame.
/// A hyphen that exists only to break the line ("Zell-\nformate") is dropped with the break.
pub(crate) fn flat_label(ui: &Ui, wrapped: &str) -> String {
    crate::i18n::current(ui.ctx()).tr(wrapped).replace("-\n", "").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_steps_prefer_the_least_compact_that_fits() {
        let content = [1000.0, 800.0, 640.0, 500.0];
        assert_eq!(HomeStep::choose(content, 1000.0 + PACK_SLACK), HomeStep::Full);
        assert_eq!(HomeStep::choose(content, 1000.0), HomeStep::CompactAlign);
        assert_eq!(HomeStep::choose(content, 800.0 + PACK_SLACK), HomeStep::CompactAlign);
        assert_eq!(HomeStep::choose(content, 640.0 + PACK_SLACK), HomeStep::StackStyles);
        assert_eq!(HomeStep::choose(content, 500.0 + PACK_SLACK), HomeStep::WrapNumber);
        assert_eq!(HomeStep::choose(content, 100.0), HomeStep::WrapNumber);
        assert!(
            HomeStep::Full < HomeStep::CompactAlign && HomeStep::CompactAlign < HomeStep::StackStyles && HomeStep::StackStyles < HomeStep::WrapNumber
        );
    }

    #[test]
    fn groups_drop_from_the_right_and_keep_the_chevron() {
        let widths = [100.0, 100.0, 100.0];
        let all = row_width(&widths);
        assert_eq!(groups_that_fit(&widths, all + PACK_SLACK), 3);
        assert!(groups_that_fit(&widths, all) < 3);
        let two = 100.0 + GROUP_GAP + 100.0;
        let budget_for_two = two + CHEVRON_WIDTH + ITEM_SPACING_X + PACK_SLACK;
        assert_eq!(groups_that_fit(&widths, budget_for_two), 2);
        assert_eq!(groups_that_fit(&[500.0], 40.0), 0);
    }

    #[test]
    fn an_empty_row_fits() {
        assert_eq!(row_width(&[]), 0.0);
        assert_eq!(groups_that_fit(&[], 0.0), 0);
    }

    #[test]
    fn the_caption_row_is_not_reserved_yet() {
        assert_eq!(group_caption_height(), 0.0);
        assert_eq!(card_height(), 88.0);
        assert_eq!(ribbon_panel_height(false), 126.0);
        assert_eq!(ribbon_panel_height(true), 34.0);
    }
}
