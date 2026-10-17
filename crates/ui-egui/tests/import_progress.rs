//! Import progress keeps the normal workspace background and identifies the current sheet.
use egui_kittest::kittest::Queryable;
use gridcraft_engine::{
    Session,
    core::work::{WorkProgress, WorkStage},
};
use gridcraft_ui_egui::{ImportTask, Services, SheetApp};
use std::{cell::RefCell, rc::Rc};

#[test]
fn loading_background_and_sheet_label_follow_the_current_theme() {
    for dark in [false, true] {
        let senders = Rc::new(RefCell::new(Vec::new()));
        let keep = senders.clone();
        let services = Services {
            start_import: Some(Box::new(move |_, _, _| {
                let (task, sender) = ImportTask::channel();
                *task.progress.lock().unwrap() =
                    WorkProgress { stage: WorkStage::Worksheet, completed: 25, total: 100, part: 2, parts: 3, sheet_name: Some("Premium".into()) };
                keep.borrow_mut().push(sender);
                task
            })),
            ..Default::default()
        };
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, services);
        app.ui.dark = dark;
        let capture = std::env::var_os("GRIDCRAFT_IMPORT_SCREENSHOTS");
        let builder = egui_kittest::Harness::builder().with_size(egui::vec2(800.0, 600.0)).with_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let builder = if capture.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_ui_state(
            move |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        h.state_mut().open_path("synthetic.xlsx");
        h.run_steps(4);
        assert!(h.state().is_importing());
        h.get_by_label("Sheet 2 / 3: Premium");
        let expected = egui::Context::default();
        expected.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let fill = gridcraft_ui_egui::theme::Tokens::get(&expected).grid_bg;
        assert!(
            h.output()
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape,egui::Shape::Rect(rect) if rect.rect.contains(egui::pos2(20.0,20.0)) && rect.fill==fill))
        );
        if let Some(dir) = capture {
            let image = h.render().unwrap();
            assert_eq!(image.get_pixel(20, 20).0, [fill.r(), fill.g(), fill.b(), 255]);
            image.save(std::path::PathBuf::from(dir).join(if dark { "import-dark.png" } else { "import-light.png" })).unwrap();
        }
    }
}
