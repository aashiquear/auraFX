//! File › Save Screenshot: the active 3D view (size, transparent background) or every open
//! view as laid out (3D views and charts, `capture`), then the file.

use crate::app::State;
use aurafx_pipeline::session::ScreenshotRequest;

#[derive(Default)]
pub struct ScreenshotDialog {
    pub open: bool,
    width: u32,
    height: u32,
    keep_aspect: bool,
    transparent: bool,
    no_overlays: bool,
    /// What the picture shows: 0 the active 3D view, 1 every view as laid out, 2 the chart views.
    pub which: u8,
    /// Keep the chart views' controls in a picture of several views.
    controls: bool,
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
    let n_views = state.capture.pane_rects.iter().filter(|(p, _)| crate::capture::is_view(*p)).count();
    egui::Window::new("Save Screenshot").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let d = &mut state.screenshot_dialog;
        let aspect = view.0.max(1) as f64 / view.1.max(1) as f64;
        ui.horizontal(|ui| {
            ui.label("Picture of");
            ui.radio_value(&mut d.which, 0, "the active 3D view");
            ui.radio_value(&mut d.which, 1, format!("all views ({n_views})")).on_hover_text("Every visible 3D view and chart view, as they are laid out in the window");
            ui.radio_value(&mut d.which, 2, "chart views");
        });
        if d.which != 0 {
            ui.weak("The views are cut out of the window and placed as they are laid out; the picture has the window's pixels (a larger window gives a larger picture).");
            ui.checkbox(&mut d.transparent, "transparent background between the views");
            ui.checkbox(&mut d.controls, "keep the chart views' controls");
            ui.separator();
            save = ui.button("Save…").clicked();
            return;
        }
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
            ui.label("Annotations");
            let mut on = !d.no_overlays;
            if ui.checkbox(&mut on, "color legend, labels, axes").changed() {
                d.no_overlays = !on;
            }
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
    if save && state.screenshot_dialog.which != 0 {
        if let Some(path) = crate::dialogs::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("aurafx-views.png").save_file() {
            let d = &state.screenshot_dialog;
            let views = if d.which == 1 { crate::capture::Views::All } else { crate::capture::Views::Charts };
            state.capture.request(crate::capture::Request { path, views, transparent: d.transparent, controls: d.controls, done: crate::capture::Done::Toast });
            state.screenshot_dialog.open = false;
        }
        return;
    }
    if save {
        if let Some(path) = crate::dialogs::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .add_filter("SVG vector picture", &["svg"])
            .set_file_name("aurafx.png")
            .save_file()
        {
            let d = &state.screenshot_dialog;
            let size = if (d.width, d.height) == view { None } else { Some((d.width, d.height)) };
            state.viewport.request_screenshot(ScreenshotRequest { path, size, transparent: d.transparent, id: 0, overlays: !d.no_overlays });
            state.screenshot_dialog.open = false;
        }
    }
}
