//! Animation effects: File ▸ Options ▸ Animation effects (and the View tab's Animations box).
//!
//! What moves in the window:
//! - a wheel notch scrolls the sheet eased over a few frames, and Ctrl/⌘+wheel zooms it the same
//!   way (egui's smoothing, which has no switch of its own);
//! - the selection outline glides to a new selection ([`crate::selection_glide`]);
//! - the dashes round a copied range march;
//! - egui's own motion: menus, tooltips and dialogs fading in, scroll areas gliding to a target.
//!
//! [`Animations::Off`] turns all of it off: a notch scrolls and zooms at once, the outline is on
//! its cells, the dashes stand still. [`Animations::System`], the default, follows the system's
//! own setting, which the host reports through [`crate::Services::animations_enabled`]
//! (Windows: Animation effects; macOS: Reduce motion; Linux and the BSDs: the desktop's animations
//! setting; the web: `prefers-reduced-motion`). Where the system says nothing, animations stay on.

use egui::{Event, InputOptions, InputState, MouseWheelUnit, TouchPhase, Vec2, style::ScrollAnimation};
use serde::{Deserialize, Serialize};

/// What the setting does, for its tooltips (an [`crate::i18n`] key).
pub const HINT: &str = "Smooth scrolling and zooming, and a selection outline that glides to the cell you move to";

/// The Animation effects preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Animations {
    /// Follow the system's animation setting.
    #[default]
    System,
    On,
    Off,
}

impl Animations {
    /// In the order the Options page lists them.
    pub const ALL: [Self; 3] = [Self::System, Self::On, Self::Off];

    /// The value saved in `ui.json` (`animationEffects`) and taken by `view.animations {mode}`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        let code = code.trim();
        Self::ALL.into_iter().find(|mode| mode.code().eq_ignore_ascii_case(code))
    }

    /// English label (an [`crate::i18n`] key); translated when drawn.
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "Use system setting",
            Self::On => "Animation effects|On",
            Self::Off => "Animation effects|Off",
        }
    }

    /// Whether animations run, given what the system says (`None`: it doesn't say, so they do).
    pub fn enabled(self, system: Option<bool>) -> bool {
        match self {
            Self::System => system.unwrap_or(true),
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// Deserializes the saved preference leniently. Before the setting had three values it was a
/// checkbox saved as a boolean (under `animations`): `false` was a choice (Off), `true` only its
/// default, which is now Use system setting. Anything else unreadable is the default too, rather
/// than failing the whole [`crate::UiState`].
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Animations, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Code(String),
        Checkbox(bool),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Code(code) => Animations::parse(&code).unwrap_or_default(),
        Raw::Checkbox(false) => Animations::Off,
        Raw::Checkbox(true) | Raw::Other(_) => Animations::default(),
    })
}

/// egui's own animation settings, kept while Animation effects has them switched off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EguiMotion {
    animation_time: f32,
    scroll_animation: ScrollAnimation,
}

impl EguiMotion {
    fn none() -> Self {
        EguiMotion { animation_time: 0.0, scroll_animation: ScrollAnimation::none() }
    }

    fn of(ctx: &egui::Context) -> Self {
        let style = ctx.global_style();
        EguiMotion { animation_time: style.animation_time, scroll_animation: style.scroll_animation }
    }

    fn set(self, ctx: &egui::Context) {
        ctx.all_styles_mut(|style| {
            style.animation_time = self.animation_time;
            style.scroll_animation = self.scroll_animation;
        });
        ctx.request_repaint();
    }
}

/// Switches egui's own motion (fading menus, tooltips and dialogs, gliding scroll areas) off, or
/// back to what it was. Called every frame: `held` remembers the settings to put back, so a
/// context that never animated (the headless harness) is left that way.
pub fn apply(ctx: &egui::Context, on: bool, held: &mut Option<EguiMotion>) {
    if on {
        if let Some(was) = held.take() {
            was.set(ctx);
        }
        return;
    }
    let now = EguiMotion::of(ctx);
    if now != EguiMotion::none() {
        // Kept from the first time only: a later one is these settings coming back by themselves.
        held.get_or_insert(now);
        EguiMotion::none().set(ctx);
    }
}

/// A wheel event's delta in points, counted the way egui counts it: a line is
/// `line_scroll_speed` points and a page is the window's height.
fn wheel_points(unit: MouseWheelUnit, delta: Vec2, input: &InputState, options: &InputOptions) -> Vec2 {
    let points = match unit {
        MouseWheelUnit::Point => delta,
        MouseWheelUnit::Line => delta * options.line_scroll_speed,
        MouseWheelUnit::Page => delta * input.viewport_rect().height(),
    };
    if points.is_finite() { points } else { Vec2::ZERO }
}

/// This frame's wheel scrolling with nothing held back for later frames. egui eases a wheel notch
/// in over the next frames ([`InputState::smooth_scroll_delta`]); with Animation effects off the
/// app puts this in its place every frame, so the sheet, the sheet tabs and egui's scroll areas
/// move by the whole notch at once. Ctrl/⌘+wheel is left out, as in egui: it zooms.
pub fn unsmoothed_scroll(input: &InputState, options: &InputOptions) -> Vec2 {
    let mut total = Vec2::ZERO;
    for event in &input.events {
        if let Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers } = event
            && !modifiers.matches_any(options.zoom_modifier)
        {
            let points = wheel_points(*unit, *delta, input, options);
            // Shift scrolls sideways and Alt vertically, as egui has it.
            let sideways = modifiers.matches_any(options.horizontal_scroll_modifier);
            let vertical = modifiers.matches_any(options.vertical_scroll_modifier);
            total += match (sideways, vertical) {
                (true, false) => Vec2::new(points.x + points.y, 0.0),
                (false, true) => Vec2::new(0.0, points.x + points.y),
                _ => points,
            };
        }
    }
    if total.is_finite() { total } else { Vec2::ZERO }
}

/// This frame's zoom factor from Ctrl/⌘+wheel and pinches (1.0: none), each wheel notch applied
/// whole. [`InputState::zoom_delta`] eases a notch's zoom in over a few frames; with Animation
/// effects off the grid asks this instead. Fingers on a touch screen are measured, never eased.
pub fn unsmoothed_zoom(input: &InputState, options: &InputOptions) -> f32 {
    if let Some(touch) = input.multi_touch() {
        return if touch.zoom_delta.is_finite() && touch.zoom_delta > 0.0 { touch.zoom_delta } else { 1.0 };
    }
    let mut zoom = 1.0_f32;
    for event in &input.events {
        match event {
            Event::Zoom(factor) => zoom *= factor,
            Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers } if modifiers.matches_any(options.zoom_modifier) => {
                let points = wheel_points(*unit, *delta, input, options);
                zoom *= (options.scroll_zoom_speed * (points.x + points.y)).exp();
            }
            _ => {}
        }
    }
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Modifiers, vec2};

    fn wheel(unit: MouseWheelUnit, delta: Vec2, modifiers: Modifiers) -> Event {
        Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers }
    }

    /// What the two readers make of one frame's events. The events are handed over as they
    /// are: egui's own smoothing, which a debug build stops at a NaN, isn't run on them.
    fn read(events: Vec<Event>) -> (Vec2, f32) {
        let mut input = InputState::default();
        input.events = events;
        let options = InputOptions::default();
        (unsmoothed_scroll(&input, &options), unsmoothed_zoom(&input, &options))
    }

    #[test]
    fn the_setting_reads_writes_and_follows_the_system() {
        assert_eq!(Animations::default(), Animations::System);
        for mode in Animations::ALL {
            assert_eq!(Animations::parse(mode.code()), Some(mode));
            assert_eq!(serde_json::to_value(mode).ok(), Some(serde_json::json!(mode.code())));
        }
        assert_eq!(Animations::parse(" OFF "), Some(Animations::Off));
        assert_eq!(Animations::parse("reduced"), None);
        assert_eq!(Animations::parse(""), None);
        // Use system setting follows the system, and animates when the system says nothing.
        assert!(Animations::System.enabled(None) && Animations::System.enabled(Some(true)));
        assert!(!Animations::System.enabled(Some(false)));
        // On and Off don't ask.
        assert!(Animations::On.enabled(Some(false)) && Animations::On.enabled(None));
        assert!(!Animations::Off.enabled(Some(true)) && !Animations::Off.enabled(None));
    }

    #[test]
    fn a_notch_scrolls_whole_and_keeps_eguis_axes() {
        let line = InputOptions::default().line_scroll_speed;
        assert_eq!(read(vec![]), (Vec2::ZERO, 1.0));
        // One notch down, then two more in the same frame: all of it, now.
        assert_eq!(read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::NONE)]).0, vec2(0.0, -line));
        let three =
            vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::NONE), wheel(MouseWheelUnit::Line, vec2(0.0, -2.0), Modifiers::NONE)];
        assert_eq!(read(three).0, vec2(0.0, -3.0 * line));
        // Points come as they are, a page is the window's height.
        assert_eq!(read(vec![wheel(MouseWheelUnit::Point, vec2(3.0, -7.5), Modifiers::NONE)]).0, vec2(3.0, -7.5));
        let page = InputState::default().viewport_rect().height();
        assert_eq!(read(vec![wheel(MouseWheelUnit::Page, vec2(0.0, 1.0), Modifiers::NONE)]).0, vec2(0.0, page));
        // Shift turns the wheel sideways.
        assert_eq!(read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::SHIFT)]).0, vec2(-line, 0.0));
        // A gesture's start and end carry no scrolling.
        let ended = Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, 50.0), phase: TouchPhase::End, modifiers: Modifiers::NONE };
        assert_eq!(read(vec![ended]).0, Vec2::ZERO);
    }

    #[test]
    fn ctrl_wheel_zooms_whole_and_never_scrolls() {
        let options = InputOptions::default();
        let notch = (options.scroll_zoom_speed * options.line_scroll_speed).exp();
        let (scroll, zoom) = read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, 1.0), Modifiers::COMMAND)]);
        assert_eq!(scroll, Vec2::ZERO);
        assert!((zoom - notch).abs() < 1e-6 && zoom > 1.1, "{zoom}");
        let (_, out) = read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::COMMAND)]);
        assert!((out * notch - 1.0).abs() < 1e-5, "a notch back undoes it: {out}");
        // A pinch the system reports as one follows the fingers.
        assert_eq!(read(vec![Event::Zoom(1.25)]).1, 1.25);
        // Wheel and pinch without Ctrl: one scrolls, the other zooms.
        let (scroll, zoom) = read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, 1.0), Modifiers::NONE), Event::Zoom(0.5)]);
        assert!(scroll.y > 0.0 && zoom == 0.5);
    }

    #[test]
    fn hostile_wheel_numbers_scroll_and_zoom_nothing() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN] {
            for unit in [MouseWheelUnit::Point, MouseWheelUnit::Line, MouseWheelUnit::Page] {
                for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND] {
                    let (scroll, zoom) = read(vec![wheel(unit, vec2(bad, bad), modifiers), wheel(unit, vec2(0.0, bad), modifiers)]);
                    assert!(scroll.is_finite(), "{bad} {unit:?}: {scroll:?}");
                    assert!(zoom.is_finite() && zoom > 0.0, "{bad} {unit:?}: {zoom}");
                }
            }
            let zoom = read(vec![Event::Zoom(bad), Event::Zoom(0.0), Event::Zoom(-2.0)]).1;
            assert_eq!(zoom, 1.0, "{bad}");
        }
    }

    #[test]
    fn eguis_own_motion_goes_off_and_comes_back_as_it_was() {
        let ctx = egui::Context::default();
        let before = EguiMotion::of(&ctx);
        assert!(before.animation_time > 0.0);
        let mut held = None;
        // On from the start: nothing to do.
        apply(&ctx, true, &mut held);
        assert_eq!((EguiMotion::of(&ctx), held), (before, None));
        // Off: both styles (light and dark) stand still, every frame.
        apply(&ctx, false, &mut held);
        apply(&ctx, false, &mut held);
        assert_eq!(EguiMotion::of(&ctx), EguiMotion::none());
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            assert_eq!(ctx.style_of(theme).animation_time, 0.0);
        }
        // Something sets a style's animations again while they are off: off they stay.
        ctx.all_styles_mut(|style| style.animation_time = 0.5);
        apply(&ctx, false, &mut held);
        assert_eq!(EguiMotion::of(&ctx), EguiMotion::none());
        // On again: as they were before the first Off.
        apply(&ctx, true, &mut held);
        assert_eq!((EguiMotion::of(&ctx), held), (before, None));
        // A context that never animated (the headless harness) is left alone both ways.
        EguiMotion::none().set(&ctx);
        apply(&ctx, false, &mut held);
        assert_eq!(held, None);
        apply(&ctx, true, &mut held);
        assert_eq!(EguiMotion::of(&ctx), EguiMotion::none());
    }
}
