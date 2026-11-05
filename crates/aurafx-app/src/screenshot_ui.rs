//! File › Save Screenshot: size, transparent background, then the file.

use crate::app::State;
use aurafx_pipeline::session::ScreenshotRequest;

#[derive(Default)]
pub struct ScreenshotDialog {
    pub open: bool,
    width: u32,
    height: u32,
    keep_aspect: bool,
    transparent: bool,
}

impl ScreenshotDialog {
    /// Open with the view's size.
    pub fn show_for(&mut self, view: (u32, u32)) {
        if !self.open {
            (self.width, self.height) = view;
            self.keep_aspect = true;
        }
        self.open = true;
    }
}

pub fn dialog_ui(state: &mut State, ctx: &egui::Context) {
    if !state.screenshot_dialog.open {
        return;
    }
    let view = state.viewport.size();
    let mut open = true;
    let mut save = false;
    egui::Window::new("Save Screenshot").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let d = &mut state.screenshot_dialog;
        let aspect = view.0.max(1) as f64 / view.1.max(1) as f64;
        egui::Grid::new("shot").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Size");
            ui.horizontal(|ui| {
                let w = ui.add(egui::DragValue::new(&mut d.width).range(16..=16384).suffix(" px"));
                ui.label("×");
                let h = ui.add(egui::DragValue::new(&mut d.height).range(16..=16384).suffix(" px"));
                if d.keep_aspect && w.changed() {
                    d.height = (d.width as f64 / aspect).round().max(1.0) as u32;
                }
                if d.keep_aspect && h.changed() {
                    d.width = (d.height as f64 * aspect).round().max(1.0) as u32;
                }
            });
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                for (label, k) in [("view", 1u32), ("2×", 2), ("4×", 4)] {
                    if ui.small_button(label).on_hover_text("A multiple of the view's size").clicked() {
                        (d.width, d.height) = (view.0 * k, view.1 * k);
                    }
                }
                if ui.small_button("1920×1080").clicked() {
                    (d.width, d.height) = (1920, 1080);
                    d.keep_aspect = false;
                }
                if ui.small_button("3840×2160").clicked() {
                    (d.width, d.height) = (3840, 2160);
                    d.keep_aspect = false;
                }
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut d.keep_aspect, "keep the view's shape");
            ui.end_row();
            ui.label("Background");
            ui.checkbox(&mut d.transparent, "transparent");
            ui.end_row();
        });
        if d.width != view.0 || d.height != view.1 {
            ui.weak("Point sizes and line widths are in pixels, so they look thinner in a larger picture.");
        }
        if state.viewport.settings.lighting.ray_tracing {
            ui.weak("Ray tracing: the picture is traced at this size first.");
        }
        ui.separator();
        save = ui.button("Save…").clicked();
    });
    state.screenshot_dialog.open = open;
    if save {
        if let Some(path) = crate::dialogs::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("aurafx.png").save_file() {
            let d = &state.screenshot_dialog;
            let size = if (d.width, d.height) == view { None } else { Some((d.width, d.height)) };
            state.viewport.request_screenshot(ScreenshotRequest { path, size, transparent: d.transparent, id: 0 });
            state.screenshot_dialog.open = false;
        }
    }
}
