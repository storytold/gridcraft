use super::{Align2, Color32, Icon, Rect, Sense, SheetApp, Stroke, StrokeKind, Style, Tokens, Ui, keytip_table, pos2, theme, vec2};

/// The ribbon card's group-label band: Excel names every group under its buttons, in a band of
/// this height below them.
pub const RIBBON_LABEL_H: f32 = 18.0;

/// One group's name and the rect its label was painted in, in screen coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupLabel {
    /// The group's Excel name (translated).
    pub label: String,
    /// What was painted: `label`, shortened with an ellipsis when the group is narrower.
    pub painted: String,
    pub rect: Rect,
}

/// The group names painted in the card's label band this frame, recorded for the tests.
#[derive(Clone, Debug)]
pub struct RibbonLabels {
    /// The band the names are painted in, below the buttons.
    pub band: Rect,
    pub labels: Vec<GroupLabel>,
}

impl Default for RibbonLabels {
    fn default() -> Self {
        Self { band: Rect::NOTHING, labels: Vec::new() }
    }
}

impl RibbonLabels {
    /// Returns the identifier for the recorded labels.
    fn id() -> egui::Id {
        egui::Id::unique("ribbon_group_labels")
    }

    /// Last frame's labels.
    pub fn get(ctx: &egui::Context) -> RibbonLabels {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }
}

/// A group's span while its tab is drawn: the name of the group that ended at `right` and where
/// it started, both in screen coordinates; `separator` is set when a group separator ends it.
#[derive(Clone, Debug)]
struct GroupSpan {
    label: String,
    left: f32,
    right: f32,
    separator: bool,
}

fn spans_id() -> egui::Id {
    egui::Id::unique("ribbon_group_spans")
}

fn start_id() -> egui::Id {
    egui::Id::unique("ribbon_group_start")
}

/// Starts a tab's groups at the cursor. Called by the ribbon before the active tab draws its
/// groups.
///
/// Group tracking is per frame and single-shot: `begin_groups` resets the spans and the start x,
/// each tab's `sep`/`group_end` calls append one span apiece, and `paint_group_labels` consumes
/// the spans and clears `start_id`. The three calls MUST run in that order inside `show()` on
/// every frame that draws a tab; `sep`/`group_end` MUST NOT be called anywhere else, since an
/// out-of-order call writes `start_id` and leaves stale state until the next `begin_groups`.
pub(super) fn begin_groups(ui: &Ui) {
    let already = ui.ctx().data(|d| d.get_temp::<Vec<GroupSpan>>(spans_id()).is_some());
    debug_assert!(!already, "begin_groups ran twice without paint_group_labels consuming the spans");
    ui.ctx().data_mut(|d| {
        d.insert_temp(spans_id(), Vec::<GroupSpan>::new());
        d.insert_temp(start_id(), ui.cursor().min.x);
    });
}

/// Records that the group named `label` ends at `right` and the next one starts there. Only valid
/// between `begin_groups` and `paint_group_labels` while a tab draws inside `show()` (see the
/// invariant on `begin_groups`).
fn end_group(ui: &Ui, label: &str, right: f32, separator: bool) {
    let left = ui.ctx().data(|d| d.get_temp::<f32>(start_id())).unwrap_or(right);
    let label = crate::i18n::current(ui.ctx()).tr(label).to_owned();
    ui.ctx().data_mut(|d| {
        let Some(mut spans) = d.get_temp::<Vec<GroupSpan>>(spans_id()) else {
            debug_assert!(false, "sep/group_end called outside show(): begin_groups did not start group tracking");
            return;
        };
        spans.push(GroupSpan { label, left, right, separator });
        d.insert_temp(spans_id(), spans);
        d.insert_temp(start_id(), right);
    });
}

/// The separator after a group, named with that group's Excel label. The gap is laid out here;
/// the line itself is painted with the labels, so it can run through the label band (a painter
/// inside the scroll area is clipped to the buttons' height).
///
/// Only valid while a tab draws between `begin_groups` and `paint_group_labels` in `show()`.
pub(crate) fn sep(ui: &mut Ui, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    end_group(ui, label, r.center().x, true);
}

/// Ends the last group of a tab, after which no separator is drawn.
///
/// Only valid while a tab draws between `begin_groups` and `paint_group_labels` in `show()`.
pub(crate) fn group_end(ui: &mut Ui, label: &str) {
    end_group(ui, label, ui.cursor().min.x, false);
}

/// A group's level when the card has no room for its natural body: one row whose menu holds the
/// whole group, as Excel shrinks a group before letting the tab scroll.
const SECTION_COMPACT: u8 = 0;

/// Points of card width one group plus its separator takes, for the width budget the planner
/// keeps as it walks a tab's groups from left to right.
const SECTION_GAP: f32 = 15.0;

/// Points either side of a group's width breakpoint where its level is kept, so dragging a window
/// across a breakpoint does not make the group flicker between layouts.
const SECTION_HYST: f32 = 12.0;

fn current_sections_id() -> egui::Id {
    egui::Id::unique("ribbon_sections_current")
}

fn last_sections_tab_id() -> egui::Id {
    egui::Id::unique("ribbon_sections_last_tab")
}

/// What the planner remembers about one group of a tab.
#[derive(Clone, Debug, Default)]
struct SectionPlan {
    label: String,
    /// The widest natural body measured for this group, in points.
    natural: f32,
    /// The level the group was last planned at (`0` compact, `1..` its natural variants).
    level: u8,
    /// The breakpoint row it took last frame (an index into the tab's widest-first table), so
    /// hysteresis compares like with like even when two widths share a level.
    slot: u8,
    /// A breakpoint row has been chosen, so `slot` holds history for hysteresis. Breakpoint
    /// groups select their row from the client width and never draw a measuring frame.
    planned: bool,
    /// A frame has drawn the natural body, so its width is known.
    measured: bool,
}

/// The ribbon's per-tab section plan: each group's measured natural width, the level it was last
/// drawn at, and the card width it was planned against.
#[derive(Clone, Debug, Default)]
struct TabSections {
    /// The window's client width (Excel's own measurement basis) which Home's breakpoints use.
    client: f32,
    /// The card's content width: a group without breakpoints compacts once its measured body no
    /// longer fits what is left of it.
    content: f32,
    /// Card width already spent by the groups drawn before the one being planned.
    used: f32,
    /// The widths at which `label` switches level, for the groups that carry Excel's measured
    /// breakpoints (Home).
    breaks: Vec<(String, Vec<(f32, u8)>)>,
    plans: Vec<SectionPlan>,
    /// The natural level every group draws at while a keytip-opened popup is on screen. Keeping
    /// the whole tab in its keytip layout keeps each control's id, and so the popup's anchor,
    /// stable; changing some groups back to compact would shift every later control.
    forced: Option<Vec<(String, u8)>>,
    /// The frame a click or Escape asked to leave the forced layout, handled the frame after so
    /// the click reaches the popup's own widgets.
    forced_unpin_at: Option<u64>,
    /// The forced layout's control took the keytip request (otherwise it goes back next frame).
    forced_used: bool,
}

impl TabSections {
    fn id(tab: &str) -> egui::Id {
        egui::Id::unique(("ribbon_sections", tab))
    }

    /// The level `label` draws at this frame, remembering it for the next one's hysteresis. A group
    /// without breakpoints is natural while its measured body fits what is left of the card, and
    /// compact once it does not; one with breakpoints follows Excel's measured widths.
    fn plan(&mut self, label: &str) -> u8 {
        let i = match self.plans.iter().position(|p| p.label == label) {
            Some(i) => i,
            None => {
                self.plans.push(SectionPlan { label: label.to_owned(), ..Default::default() });
                self.plans.len() - 1
            }
        };
        // A keytip opened a popup: the tab keeps the layout it was drawn in, so the popup's anchor
        // stays put frame to frame.
        if let Some((_, level)) = self.forced.as_ref().and_then(|forced| forced.iter().find(|(name, _)| name == label)) {
            let level = *level;
            self.plans[i].level = level;
            return level;
        }
        let plan = &self.plans[i];
        let breaks = self.breaks.iter().find(|(l, _)| l == label).map(|(_, b)| b.as_slice());
        let mut slot = plan.slot;
        let level = if let Some(b) = breaks {
            // Breakpoint rows need no measurement: an unplanned group starts on the row the
            // client reaches, so the first frame never treats the widest row as hysteresis
            // history (a fresh 810 pt window would otherwise keep Clipboard natural until
            // the next input past the dead band).
            let raw = b.iter().position(|(min, _)| self.client >= *min).map_or(b.len() as u8, |i| i as u8);
            let previous = if plan.planned { plan.slot } else { raw };
            let (level, chosen) = break_level(b, self.client, previous);
            slot = chosen;
            level
        } else if !plan.measured {
            // The first frame draws the group in its natural form, to measure it.
            slot = 0;
            1
        } else {
            let remaining = self.content - self.used;
            let natural = plan.level != SECTION_COMPACT;
            // A dead band either side of the fit keeps a group from flapping as its neighbours
            // change width.
            if natural { u8::from(remaining >= plan.natural - SECTION_HYST) } else { u8::from(remaining >= plan.natural + SECTION_HYST) }
        };
        let plan = &mut self.plans[i];
        plan.level = level;
        plan.slot = slot;
        plan.planned = breaks.is_some();
        level
    }

    /// The natural level of every group planned so far: the form a keytip frame draws the tab in
    /// when the request's group cannot be named. A compact group takes its narrowest natural
    /// form, the one its menu holds.
    fn natural_layout(&self) -> Vec<(String, u8)> {
        self.plans.iter().map(|plan| (plan.label.clone(), self.natural_level(&plan.label))).collect()
    }

    /// The natural form a group takes when a keytip draws it: its planned natural level, or the
    /// narrowest natural form when it is compact.
    fn natural_level(&self, label: &str) -> u8 {
        self.plans
            .iter()
            .find(|plan| plan.label == label)
            .map(|plan| plan.level)
            .filter(|level| *level != SECTION_COMPACT)
            .or_else(|| self.breaks.iter().find(|(name, _)| name == label).and_then(|(_, breaks)| breaks.last().map(|(_, level)| *level)))
            .unwrap_or(1)
    }

    /// Notes the width `label` took this frame and remembers the natural body's own width.
    /// True when this recording can change the next frame's plan: the group's first measure,
    /// which moves it off the first-frame natural layout.
    fn record(&mut self, label: &str, level: u8, width: f32) -> bool {
        self.used += width + SECTION_GAP;
        if level == SECTION_COMPACT {
            return false;
        }
        if let Some(plan) = self.plans.iter_mut().find(|p| p.label == label) {
            let first = !plan.measured;
            plan.natural = plan.natural.max(width);
            plan.measured = true;
            return first;
        }
        false
    }
}

/// The level at breakpoint row `slot`; the row past the table is the compact one.
fn level_at(breaks: &[(f32, u8)], slot: u8) -> u8 {
    breaks.get(slot as usize).map_or(SECTION_COMPACT, |(_, level)| *level)
}

/// The level a group with breakpoints draws at, and the row it took (an index into the
/// widest-first table): the first row whose width the client reaches. Leaving a row takes the
/// client [`SECTION_HYST`] points past that row's boundary, so a window dragged across a
/// breakpoint does not make the group flicker between layouts.
fn break_level(breaks: &[(f32, u8)], client: f32, previous: u8) -> (u8, u8) {
    if breaks.is_empty() {
        return (1, 0);
    }
    let previous = previous.min(breaks.len() as u8);
    let raw = breaks.iter().position(|(min, _)| client >= *min).map_or(breaks.len() as u8, |i| i as u8);
    if raw == previous {
        return (level_at(breaks, raw), raw);
    }
    if raw < previous {
        // A wider row: the card must clearly reach its width. Step through each crossed row
        // from the one past `previous`, so a wide jump lands on the widest row it reaches
        // instead of testing only the destination's boundary (Font widened 819→1280 pt
        // would otherwise sit 12 pt short of 1280+HYST and stay compact forever).
        let mut slot = previous;
        while slot > raw && client >= breaks[(slot - 1) as usize].0 + SECTION_HYST {
            slot -= 1;
        }
        return (level_at(breaks, slot), slot);
    }
    let (boundary, level) = if let Some((min, _)) = breaks.get(previous as usize) {
        // A narrower row: the card must clearly leave the row it is on. The compact slot past
        // the table has no boundary of its own, so leaving it re-enters the narrowest row.
        (*min, level_at(breaks, raw))
    } else {
        (breaks[breaks.len() - 1].0, level_at(breaks, raw))
    };
    if client < boundary - SECTION_HYST { (level, raw) } else { (level_at(breaks, previous), previous) }
}

/// Runs `f` on the plan of the tab the ribbon is drawing.
fn with_tab_sections(ui: &Ui, f: impl FnOnce(&mut TabSections)) {
    let ctx = ui.ctx();
    let Some(id) = ctx.data(|d| d.get_temp::<egui::Id>(current_sections_id())) else { return };
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<TabSections>(id)));
}

/// The ribbon's adaptive group layout: each tab remembers how wide its groups are naturally and
/// picks, left to right, whether they draw natural or as one compact row.
pub struct RibbonSections;

impl RibbonSections {
    /// Starts a frame's pass over `tab`'s groups: records the window's client width (for the
    /// measured breakpoints) and the card's content width, and resets the width spent so far, so
    /// each group plans from what is left to its right.
    pub(super) fn begin(ui: &Ui, tab: &str, content: f32, client: f32, keytip_group: Option<&str>) {
        let id = TabSections::id(tab);
        let frame = ui.ctx().cumulative_frame_nr();
        // A tab opened afresh starts compact again: a layout held open by a keytip on an earlier
        // visit has lost the popup that held it open.
        let previous = ui.ctx().data(|d| d.get_temp::<String>(last_sections_tab_id()));
        ui.ctx().data_mut(|d| d.insert_temp(last_sections_tab_id(), tab.to_owned()));
        let switched = previous.as_deref() != Some(tab);
        // A click dismisses whatever a keytip opened, and Escape does the same; the tab settles
        // back the frame after, once the click has reached the popup's own widgets. egui
        // registers a click on release, so a press inside an open popup must not end the layout
        // before the release arrives, or the popup's anchor would move out from under it.
        let any_popup = egui::Popup::is_any_open(ui.ctx());
        // Read focus before `data_mut`: `ctx.memory` must not run inside that closure.
        let size_typing = ui.ctx().memory(|m| m.has_focus(super::home::font_size_field()));
        let dismiss = ui.input(|i| i.key_pressed(egui::Key::Escape) || (i.pointer.any_pressed() && !any_popup));
        ui.ctx().data_mut(|d| {
            d.insert_temp(current_sections_id(), id);
            let sections = d.get_temp_mut_or_default::<TabSections>(id);
            sections.content = content;
            sections.client = client;
            sections.used = 0.0;
            if let Some(group) = keytip_group {
                // Draw the target group in a natural form (and keep it until its popup closes):
                // a control's id, and so its popup's anchor, must not move between frames. When
                // the request cannot name the group, the whole tab takes its natural forms.
                sections.forced = if sections.plans.iter().any(|plan| plan.label == group) {
                    Some(vec![(group.to_owned(), sections.natural_level(group))])
                } else {
                    Some(sections.natural_layout())
                };
                sections.forced_used = false;
                sections.forced_unpin_at = None;
            } else if sections.forced.is_some() {
                // The popup the keytip opened has closed (Enter applied the size, a click picked
                // a row, or Escape dismissed it): settle back even with no pointer press, or the
                // group would stay expanded at narrow widths after a keyboard-only edit. A
                // focused size field still counts as the interaction running: the fontSize
                // keytip opens no popup, and the layout must stay until the field loses focus.
                let popup_gone = sections.forced_used && !any_popup && !size_typing;
                let stale = switched || popup_gone || sections.forced_unpin_at.is_some_and(|at| frame > at);
                if stale {
                    sections.forced = None;
                    sections.forced_unpin_at = None;
                    sections.forced_used = false;
                } else if dismiss {
                    sections.forced_unpin_at.get_or_insert(frame);
                }
            }
        });
    }

    /// Registers the card widths (client points) at which `label` takes each level, as Home's
    /// groups do with Excel's measured breakpoints.
    pub(super) fn breaks(ui: &Ui, label: &str, levels: &[(f32, u8)]) {
        with_tab_sections(ui, |sections| {
            if let Some(slot) = sections.breaks.iter_mut().find(|(l, _)| l == label) {
                slot.1 = levels.to_vec();
            } else {
                sections.breaks.push((label.to_owned(), levels.to_vec()));
            }
        });
    }

    /// The level a group's body draws at this frame (`0` compact, `1..` natural variants).
    pub(super) fn level(ui: &Ui, label: &str) -> u8 {
        let mut level = 1;
        with_tab_sections(ui, |sections| {
            level = sections.plans.iter().find(|p| p.label == label).map_or(1, |p| p.level);
        });
        level
    }

    /// Draws one group: `body` in place while the group is natural, or as a single compact row
    /// whose menu holds the whole natural body. The following `sep`/`group_end` still names the
    /// group, so its label sits in the band either way.
    fn draw(tab: &str, ui: &mut Ui, app: &mut SheetApp, label: &str, icon: Icon, mut body: impl FnMut(&mut SheetApp, &mut Ui)) {
        // A keytip request must reach its control: `begin` already put the whole tab in its
        // natural layout for that frame, so the control that owns the request is drawn (each body
        // still draws exactly once). The layout is held until the popup closes so its anchor, and
        // so this body's control ids, stay put.
        let pending = app.ui.keytip_invoke;
        let level = Self::plan(ui, label);
        if level == SECTION_COMPACT {
            // Excel's compact group: its icon and name with a chevron, the whole group in its menu.
            let text = crate::i18n::current(ui.ctx()).tr(label).replace('\n', " ");
            let button = crate::widgets::small_button(ui, icon, &text, &text, true);
            let width = button.rect.width();
            Self::record(ui, label, level, width);
            // The menu is an area, not a popup: it holds buttons that open popups of their own,
            // and a popup nested in a popup would close its parent. Clicks inside reach those
            // buttons; a click elsewhere (or Escape) closes it.
            // Stable across resizes: the open flag and the Area derive from the tab and the
            // group's label, not the allocation-order button id, so a resize that moves the
            // button keeps the menu open instead of reading another group's flag.
            let menu_id = egui::Id::unique(("gridcraft.compact_group", tab, label));
            let open_id = menu_id.with("open");
            let mut open = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
            if button.clicked() {
                open = !open;
            }
            if open {
                let area = egui::Area::new(menu_id.with("body"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(button.rect.left_bottom() + vec2(0.0, 4.0))
                    .constrain(true);
                let shown = area.show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        // A body wider than the viewport (Function Library on a narrow card)
                        // still fits: the menu scrolls instead of running off screen.
                        let room = ui.ctx().content_rect().width() - 16.0;
                        egui::ScrollArea::horizontal().max_width(room.max(0.0)).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                                body(app, ui);
                            });
                        })
                    })
                });
                let rect = shown.response.rect;
                // A click that opened one of the body's menus keeps the group; any other click
                // outside the group closes it, including the one that picked a menu item.
                // Closing the menu also releases a prefix-forced layout (HB's Borders menu):
                // the popup it held open is done, like a consumed keytip request.
                let any_click = ui.input(|i| i.pointer.any_click());
                let menu_open = egui::Popup::is_any_open(ui.ctx());
                let pos = ui.input(|i| i.pointer.interact_pos());
                let pressed = any_click && !menu_open;
                let outside = pos.is_some_and(|p| !rect.contains(p) && !button.rect.contains(p));
                let done = pressed && outside || ui.input(|i| i.key_pressed(egui::Key::Escape));
                if done {
                    open = false;
                    Self::release_forced(ui);
                }
            }
            ui.ctx().data_mut(|d| d.insert_temp(open_id, open));
        } else {
            let left = ui.cursor().min.x;
            body(app, ui);
            let width = ui.cursor().min.x - left;
            Self::record(ui, label, level, width);
            // The forced layout's control took the request and opened its popup: keep the layout.
            if pending.is_some() && app.ui.keytip_invoke.is_none() {
                Self::mark_forced_used(ui);
            }
        }
    }

    /// Records that the forced layout's control took the keytip request, so the layout stays while
    /// the popup it opened is on screen.
    fn mark_forced_used(ui: &Ui) {
        with_tab_sections(ui, |sections| {
            if sections.forced.is_some() {
                sections.forced_used = true;
            }
        });
    }

    /// Releases a prefix-forced layout whose popup is done: a Continue prefix sets no invoke,
    /// so nothing marks it used, and without this the group would stay expanded until some
    /// other dismissal. The next frame settles back, after the closing click reaches its target.
    fn release_forced(ui: &Ui) {
        // Read the frame before `with_tab_sections`: its `data_mut` holds the context write
        // lock and `cumulative_frame_nr` takes the read lock, which deadlocks the UI thread
        // (debug builds panic after the lock timeout).
        let frame = ui.ctx().cumulative_frame_nr();
        with_tab_sections(ui, |sections| {
            if sections.forced.is_some() && !sections.forced_used {
                sections.forced_unpin_at.get_or_insert(frame);
            }
        });
    }

    fn plan(ui: &Ui, label: &str) -> u8 {
        let mut level = 1;
        with_tab_sections(ui, |sections| level = sections.plan(label));
        level
    }

    fn record(ui: &Ui, label: &str, level: u8, width: f32) {
        let mut first = false;
        with_tab_sections(ui, |sections| first = sections.record(label, level, width));
        // The first measure moves the group off its first-frame natural layout; ask for the
        // frame that applies the compact plan instead of waiting for the next input event.
        if first {
            ui.ctx().request_repaint();
        }
    }

    /// The level each group of `tab` was last drawn at, in draw order (`0` compact, `1..` natural
    /// variants). Empty for a tab that has not drawn.
    pub fn levels(ctx: &egui::Context, tab: &str) -> Vec<(String, u8)> {
        ctx.data(|d| d.get_temp::<TabSections>(TabSections::id(tab)))
            .map(|sections| sections.plans.into_iter().map(|p| (p.label, p.level)).collect())
            .unwrap_or_default()
    }
}

/// Draws one group of a tab: its natural body in place when the planner has room for it, or one
/// compact row whose menu holds the body. The `sep`/`group_end` that follows still names the
/// group, so its label stays in the band either way.
pub(crate) fn section(tab: &str, ui: &mut Ui, app: &mut SheetApp, label: &str, icon: Icon, body: impl FnMut(&mut SheetApp, &mut Ui)) {
    RibbonSections::draw(tab, ui, app, label, icon, body);
}

/// Paints the separators (from the buttons into the label band) and each group's name centred in
/// the band, clipped to the card's content viewport so a scrolled group is cut at the card's edge
/// as Excel's is. Returns what was painted.
pub(super) fn paint_group_labels(ui: &Ui, content: Rect, band: Rect, viewport: Rect) -> RibbonLabels {
    let t = Tokens::get(ui.ctx());
    let spans = ui.ctx().data_mut(|d| d.remove_temp::<Vec<GroupSpan>>(spans_id())).unwrap_or_default();
    ui.ctx().data_mut(|d| d.remove_temp::<f32>(start_id()));
    let line_top = content.top() + 6.0;
    let line_bottom = band.bottom() - 2.0;
    let lines = ui.painter().with_clip_rect(Rect::from_min_max(pos2(viewport.left(), line_top), pos2(viewport.right(), line_bottom)));
    let clip = Rect::from_min_max(pos2(viewport.left(), band.top()), pos2(viewport.right(), band.bottom()));
    let painter = ui.painter().with_clip_rect(clip);
    let font = theme::ui_font(11.0);
    let labels: Vec<GroupLabel> = spans
        .into_iter()
        .map(|s| {
            if s.separator {
                lines.line_segment([pos2(s.right, line_top), pos2(s.right, line_bottom)], Stroke::new(1.0, t.separator));
            }
            let painted = fit_label(&painter, &s.label, &font, (s.right - s.left - 8.0).max(0.0));
            let rect = Rect::from_min_max(pos2(s.left, band.top()), pos2(s.right, band.bottom()));
            painter.text(rect.center(), Align2::CENTER_CENTER, painted.clone(), font.clone(), t.text_dim);
            GroupLabel { label: s.label, painted, rect }
        })
        .collect();
    let snapshot = RibbonLabels { band, labels };
    ui.ctx().data_mut(|d| d.insert_temp(RibbonLabels::id(), snapshot.clone()));
    snapshot
}

/// `label`, shortened with an ellipsis until it fits `max_w` (or empty when nothing fits), as
/// Excel shortens a group name wider than its group.
fn fit_label(painter: &egui::Painter, label: &str, font: &egui::FontId, max_w: f32) -> String {
    let width = |text: &str| painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x;
    if width(label) <= max_w {
        return label.to_owned();
    }
    let mut text = label.to_owned();
    while !text.is_empty() && width(&format!("{text}…")) > max_w {
        text.pop();
    }
    if text.is_empty() { String::new() } else { format!("{text}…") }
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
