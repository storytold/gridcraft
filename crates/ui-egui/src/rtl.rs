use egui::{Layout, Ui};

pub trait UiRtlExt {
    fn rtl_container<R>(
        &mut self,
        is_rtl: bool,
        content_w: f32,
        margin_right: f32,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> R;

    /// Returns `top_down(Align::Max)` for RTL languages and `top_down(Align::Min)` for LTR.
    fn menu_layout(&self, is_rtl: bool) -> Layout;
}

impl UiRtlExt for Ui {
    fn rtl_container<R>(
        &mut self,
        is_rtl: bool,
        content_w: f32,
        margin_right: f32,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let full_width = self.available_width();
        let target_w = content_w.min(full_width);

        let left_pad = if is_rtl && full_width > (target_w + margin_right) {
            full_width - target_w - margin_right
        } else if !is_rtl {
            44.0
        } else {
            0.0
        };

        self.horizontal(|ui| {
            if left_pad > 0.0 {
                ui.add_space(left_pad);
            }
            ui.vertical(|ui| {
                ui.set_width(target_w);
                add_contents(ui)
            })
            .inner
        })
        .inner
    }

    fn menu_layout(&self, is_rtl: bool) -> Layout {
        if is_rtl {
            Layout::top_down(egui::Align::Max)
        } else {
            Layout::top_down(egui::Align::Min)
        }
    }
}