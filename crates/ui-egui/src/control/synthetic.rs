use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{CellRef, Json, SheetApp};

/// One queued synthetic step, bound to the window it was meant for when the request arrived.
///
/// The window is resolved to a native viewport when the step is offered to a frame, not when it
/// is queued: a request in the same batch as `file.new` names a window whose native window does
/// not exist yet.
pub struct SyntheticBatch {
    /// The engine window that receives the input, or `None` when the request named no window
    /// (the web, tests and embedders, where every window shares the root viewport).
    pub window: Option<u64>,
    pub events: Vec<egui::Event>,
}

impl SyntheticBatch {
    /// One step for `window` (see the field).
    pub fn new(window: Option<u64>, events: Vec<egui::Event>) -> Self {
        Self { window, events }
    }
}

/// Synthetic input for a native window outside the root viewport.
///
/// eframe runs [`crate::SheetApp::raw_input_hook`] for the root viewport only; every other native
/// window is an immediate viewport, whose `RawInput` is built by the backend. `apps/gridcraft`
/// installs this as an `eframe::ViewportHooks::raw_input` callback, which runs for immediate
/// viewports too. The root hook moves a batch queued for another window here, the callback injects
/// it into that window's pass, and the window's `ui` pass clears it once applied.
#[derive(Clone, Default)]
pub struct SyntheticInput {
    state: Arc<Mutex<Option<ChildInput>>>,
}

/// One batch waiting for (or offered to) a native window of its own.
struct ChildInput {
    viewport: egui::ViewportId,
    events: Vec<egui::Event>,
    /// The target viewport's `cumulative_pass_nr` when the events were last offered.
    injected_at_pass: Option<u64>,
}

impl SyntheticInput {
    /// The viewport the waiting batch is bound to, if any.
    pub fn viewport(&self) -> Option<egui::ViewportId> {
        self.lock().as_ref().map(|c| c.viewport)
    }

    /// Whether a batch is waiting for, or was offered to, another window.
    pub fn is_pending(&self) -> bool {
        self.lock().is_some()
    }

    /// Queued events and charged bytes, so the shared synthetic-input budget covers this batch too.
    pub fn usage(&self) -> (usize, usize) {
        self.lock().as_ref().map_or((0, 0), |c| (c.events.len(), c.events.iter().map(event_bytes).fold(0usize, usize::saturating_add)))
    }

    /// Offers `batch` to the window shown in `viewport`, unless the previous one is still in
    /// flight (batches are applied in the order they were queued).
    pub(crate) fn hand_off(&self, batch: SyntheticBatch, viewport: egui::ViewportId) -> bool {
        let mut state = self.lock();
        if state.is_some() {
            return false;
        }
        *state = Some(ChildInput { viewport, events: batch.events, injected_at_pass: None });
        true
    }

    /// Injects the waiting batch into `raw` when this is its window's frame. eframe calls it for
    /// every native viewport before the pass (see the type docs).
    pub fn raw_input(&self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let mut state = self.lock();
        let Some(child) = state.as_mut() else { return };
        if child.viewport != raw.viewport_id {
            return;
        }
        // One offer per pass: a completed pass that did not apply the events (the font gate's
        // early return) has advanced the counter, so the same step is offered again for a real
        // pass; a second call inside the pass must not duplicate it.
        let pass = ctx.cumulative_pass_nr_for(raw.viewport_id);
        if child.injected_at_pass == Some(pass) {
            return;
        }
        raw.events.extend(child.events.iter().cloned());
        child.injected_at_pass = Some(pass);
    }

    /// Clears the batch once a `ui` pass in its own viewport applied it.
    pub(crate) fn applied(&self, viewport: egui::ViewportId, pass: u64) {
        let mut state = self.lock();
        if state.as_ref().is_some_and(|c| c.viewport == viewport && c.injected_at_pass == Some(pass)) {
            *state = None;
        }
    }

    /// Drops the batch when its window is gone (closed or hidden): its input is never typed into
    /// another workbook.
    pub(crate) fn retain_open(&self, open: impl Fn(egui::ViewportId) -> bool) {
        let mut state = self.lock();
        if state.as_ref().is_some_and(|c| !open(c.viewport)) {
            *state = None;
        }
    }

    /// Locks the shared state, recovering from poisoning instead of panicking (the app must never
    /// crash on a control-channel request).
    fn lock(&self) -> MutexGuard<'_, Option<ChildInput>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(super) fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" | "del" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "left" => Some(egui::Key::ArrowLeft),
        "right" => Some(egui::Key::ArrowRight),
        "up" => Some(egui::Key::ArrowUp),
        "down" => Some(egui::Key::ArrowDown),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        "pageup" => Some(egui::Key::PageUp),
        "pagedown" => Some(egui::Key::PageDown),
        "home" => Some(egui::Key::Home),
        "end" => Some(egui::Key::End),
        "f2" => Some(egui::Key::F2),
        "f4" => Some(egui::Key::F4),
        "f9" => Some(egui::Key::F9),
        _ => None,
    })
}

pub(super) fn modifiers(p: &Json) -> egui::Modifiers {
    let b = |n: &str| p.get(n).and_then(Json::as_bool).unwrap_or(false);
    egui::Modifiers {
        alt: b("alt"),
        ctrl: b("ctrl"),
        shift: b("shift"),
        mac_cmd: b("cmd") && cfg!(target_os = "macos"),
        command: b("cmd") || (b("ctrl") && !cfg!(target_os = "macos")),
    }
}

pub(super) fn cell_center(app: &SheetApp, c: CellRef) -> Option<egui::Pos2> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let rect = app.grid.rect?;
    let geo = crate::grid::Geo::new(sh, rect, app.view().scroll);
    Some(geo.screen_cell_rect(sh, c).center())
}

/// Most synthetic events (pointer, key, text and move alike) a control client may keep queued at
/// once. The queue offers one batch per UI pass, and a hidden logic-only frame keeps the same
/// batch unapplied, so every producer must honour this budget.
pub const MAX_QUEUED_SYNTHETIC_EVENTS: usize = 10_000;

/// Most bytes of synthetic input queued at once: the `Event` storage plus every text payload.
pub const MAX_QUEUED_SYNTHETIC_BYTES: usize = 8 * 1024 * 1024;

/// Largest single `ui.text` string accepted (well below the control line limit; one paste-sized
/// string is plenty for a spreadsheet cell and keeps a burst from exhausting memory).
pub const MAX_SYNTHETIC_TEXT_BYTES: usize = 1024 * 1024;

/// Bytes charged to one synthetic event: `size_of::<Event>()` plus the payload of a `Text` event.
fn event_bytes(event: &egui::Event) -> usize {
    std::mem::size_of::<egui::Event>()
        + match event {
            egui::Event::Text(text) => text.len(),
            _ => 0,
        }
}

/// Saturating totals of queued synthetic events and their charged bytes across every pending batch.
fn synthetic_usage(app: &SheetApp) -> (usize, usize) {
    let (mut events, mut bytes) = app
        .synthetic
        .iter()
        .flat_map(|batch| batch.events.iter())
        .fold((0usize, 0usize), |(events, bytes), event| (events.saturating_add(1), bytes.saturating_add(event_bytes(event))));
    let (child_events, child_bytes) = app.child_input.usage();
    events = events.saturating_add(child_events);
    bytes = bytes.saturating_add(child_bytes);
    (events, bytes)
}

/// Refuses a request that would push the queued synthetic input past either limit; the queue is
/// left unchanged.
fn synthetic_budget(app: &SheetApp, extra_events: usize, extra_bytes: usize) -> Result<(), String> {
    let (events, bytes) = synthetic_usage(app);
    if events.saturating_add(extra_events) > MAX_QUEUED_SYNTHETIC_EVENTS {
        return Err(format!("too many synthetic input events (limit {MAX_QUEUED_SYNTHETIC_EVENTS} queued at once)"));
    }
    if bytes.saturating_add(extra_bytes) > MAX_QUEUED_SYNTHETIC_BYTES {
        return Err(format!("synthetic input exceeds the {MAX_QUEUED_SYNTHETIC_BYTES}-byte queue limit"));
    }
    Ok(())
}

/// Checks the synthetic-event budget, then queues one pointer move followed by `count` press/release pairs (`count` is at least one).
pub(super) fn push_click(
    app: &mut SheetApp,
    window: Option<u64>,
    pos: egui::Pos2,
    button: egui::PointerButton,
    count: u32,
    m: egui::Modifiers,
) -> Result<(), String> {
    let count = count.max(1) as usize;
    let events = count.saturating_mul(2).saturating_add(1);
    synthetic_budget(app, events, events.saturating_mul(std::mem::size_of::<egui::Event>()))?;
    app.synthetic.push_back(SyntheticBatch { window, events: vec![egui::Event::PointerMoved(pos)] });
    let mut batch = Vec::with_capacity(count * 2);
    for _ in 0..count {
        batch.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
        batch.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
    }
    app.synthetic.push_back(SyntheticBatch { window, events: batch });
    Ok(())
}

/// Checks the synthetic-event budget, then queues a move, a press, `steps` interpolated moves and a release.
pub(super) fn push_drag(
    app: &mut SheetApp,
    window: Option<u64>,
    a: egui::Pos2,
    b: egui::Pos2,
    steps: usize,
    m: egui::Modifiers,
) -> Result<(), String> {
    let steps = steps.max(1);
    let events = steps.saturating_add(3);
    synthetic_budget(app, events, events.saturating_mul(std::mem::size_of::<egui::Event>()))?;
    let mut push = |events: Vec<egui::Event>| app.synthetic.push_back(SyntheticBatch { window, events });
    push(vec![egui::Event::PointerMoved(a)]);
    push(vec![egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        push(vec![egui::Event::PointerMoved(a + (b - a) * t)]);
    }
    push(vec![egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: m }]);
    Ok(())
}

/// Checks the synthetic-event budget and queues one key press/release pair.
pub(super) fn push_key(app: &mut SheetApp, window: Option<u64>, key: egui::Key, m: egui::Modifiers) -> Result<(), String> {
    let events = 2;
    synthetic_budget(app, events, events * std::mem::size_of::<egui::Event>())?;
    app.synthetic.push_back(SyntheticBatch {
        window,
        events: vec![
            egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m },
            egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m },
        ],
    });
    Ok(())
}

/// Rejects text longer than `MAX_SYNTHETIC_TEXT_BYTES`, then queues one `Text` event under the shared queue budget.
pub(super) fn push_text(app: &mut SheetApp, window: Option<u64>, text: String) -> Result<(), String> {
    if text.len() > MAX_SYNTHETIC_TEXT_BYTES {
        return Err(format!("text input is {} bytes, over the {MAX_SYNTHETIC_TEXT_BYTES}-byte limit for one event", text.len()));
    }
    let event = egui::Event::Text(text);
    synthetic_budget(app, 1, event_bytes(&event))?;
    app.synthetic.push_back(SyntheticBatch { window, events: vec![event] });
    Ok(())
}

/// Queues one pointer-move event under the shared synthetic queue budget.
pub(super) fn push_move(app: &mut SheetApp, window: Option<u64>, pos: egui::Pos2) -> Result<(), String> {
    let event = egui::Event::PointerMoved(pos);
    synthetic_budget(app, 1, event_bytes(&event))?;
    app.synthetic.push_back(SyntheticBatch { window, events: vec![event] });
    Ok(())
}
