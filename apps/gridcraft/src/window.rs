//! Main-window geometry stored alongside the existing desktop preferences.

use egui::{Pos2, Vec2, ViewportBuilder, ViewportInfo};
use serde_json::Value as Json;

#[derive(Clone, Copy, Default)]
pub struct WindowState {
    size: Option<Vec2>,
    position: Option<Pos2>,
    maximized: bool,
}

impl WindowState {
    pub fn from_prefs(prefs: &Json) -> Self {
        let saved = prefs.get("window").unwrap_or(&Json::Null);
        let pair = |key: &str| -> Option<[f32; 2]> {
            let [x, y] = saved.get(key)?.as_array()?.as_slice() else { return None };
            let pair = [x.as_f64()? as f32, y.as_f64()? as f32];
            pair.iter().all(|v| v.is_finite()).then_some(pair)
        };
        Self {
            size: pair("size").filter(|s| s.iter().all(|v| *v > 0.0 && *v <= 16384.0)).map(Vec2::from),
            position: pair("position").map(Pos2::from),
            maximized: saved.get("maximized").and_then(Json::as_bool).unwrap_or(false),
        }
    }

    pub fn update(&mut self, info: &ViewportInfo, zoom_factor: f32) {
        if info.minimized == Some(true) || info.fullscreen == Some(true) || !zoom_factor.is_finite() || zoom_factor <= 0.0 {
            return;
        }
        self.maximized = info.maximized.unwrap_or(self.maximized);
        if self.maximized {
            return;
        }
        if let Some(rect) = info.inner_rect {
            // ViewportInfo is in scaled UI points; the startup builder takes native window points.
            let size = rect.size() * zoom_factor;
            if size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0 && size.x <= 16384.0 && size.y <= 16384.0 {
                self.size = Some(size);
            }
        }
        if let Some(rect) = info.outer_rect {
            let position = rect.min * zoom_factor;
            if position.x.is_finite() && position.y.is_finite() {
                self.position = Some(position);
            }
        }
    }

    pub fn apply(&self, mut viewport: ViewportBuilder) -> ViewportBuilder {
        if let Some(size) = self.size {
            viewport = viewport.with_inner_size(size);
        }
        if let Some(position) = self.position {
            viewport = viewport.with_position(position);
        }
        viewport.with_maximized(self.maximized)
    }

    pub fn store(&self, prefs: &mut Json) {
        if let Some(prefs) = prefs.as_object_mut() {
            prefs.insert(
                "window".into(),
                serde_json::json!({
                    "size": self.size.map(|s| [s.x, s.y]),
                    "position": self.position.map(|p| [p.x, p.y]),
                    "maximized": self.maximized,
                }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use egui::{Rect, pos2, vec2};
    use serde_json::json;

    fn normal() -> ViewportInfo {
        ViewportInfo {
            inner_rect: Some(Rect::from_min_size(pos2(-1200.0, 180.0), vec2(1100.0, 700.0))),
            outer_rect: Some(Rect::from_min_size(pos2(-1208.0, 150.0), vec2(1116.0, 738.0))),
            maximized: Some(false),
            ..Default::default()
        }
    }

    #[test]
    fn geometry_survives_the_preferences_round_trip() {
        let mut state = WindowState::default();
        state.update(&normal(), 1.0);
        let mut prefs = serde_json::to_value(gridcraft_engine::Prefs::default()).unwrap();
        state.store(&mut prefs);
        let saved = serde_json::to_vec(&prefs).unwrap();
        let loaded: Json = serde_json::from_slice(&saved).unwrap();
        assert!(serde_json::from_value::<gridcraft_engine::Prefs>(loaded.clone()).is_ok());
        let viewport = WindowState::from_prefs(&loaded).apply(ViewportBuilder::default());
        assert_eq!(viewport.inner_size, Some(vec2(1100.0, 700.0)));
        assert_eq!(viewport.position, Some(pos2(-1208.0, 150.0)));
    }

    #[test]
    fn maximized_and_minimized_windows_keep_the_normal_bounds() {
        let mut state = WindowState::default();
        state.update(&normal(), 1.0);
        state.update(
            &ViewportInfo { inner_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0))), maximized: Some(true), ..Default::default() },
            1.0,
        );
        state.update(&ViewportInfo { inner_rect: Some(Rect::ZERO), minimized: Some(true), maximized: Some(false), ..Default::default() }, 1.0);
        let mut prefs = json!({});
        state.store(&mut prefs);
        let viewport = WindowState::from_prefs(&prefs).apply(ViewportBuilder::default());
        assert_eq!(viewport.inner_size, Some(vec2(1100.0, 700.0)));
        assert_eq!(viewport.position, Some(pos2(-1208.0, 150.0)));
        assert_eq!(viewport.maximized, Some(true));
    }

    #[test]
    fn malformed_geometry_keeps_the_default_window() {
        let prefs = json!({"window": {"size": [-1, 0], "position": [1e99, 0], "maximized": "yes"}});
        let viewport = WindowState::from_prefs(&prefs).apply(ViewportBuilder::default().with_inner_size([1440.0, 900.0]));
        assert_eq!(viewport.inner_size, Some(vec2(1440.0, 900.0)));
        assert_eq!(viewport.position, None);
    }

    #[test]
    fn interface_scale_does_not_shrink_saved_native_bounds() {
        let mut info = normal();
        info.inner_rect = info.inner_rect.map(|r| Rect::from_min_max(r.min * 0.5, r.max * 0.5));
        info.outer_rect = info.outer_rect.map(|r| Rect::from_min_max(r.min * 0.5, r.max * 0.5));
        let mut state = WindowState::default();
        state.update(&info, 2.0);
        let mut prefs = json!({});
        state.store(&mut prefs);
        let viewport = WindowState::from_prefs(&prefs).apply(ViewportBuilder::default());
        assert_eq!(viewport.inner_size, Some(vec2(1100.0, 700.0)));
        assert_eq!(viewport.position, Some(pos2(-1208.0, 150.0)));
    }
}
