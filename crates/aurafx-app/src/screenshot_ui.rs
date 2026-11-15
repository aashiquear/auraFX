//! File › Save Screenshot: the active 3D view (size, shape, background) or several views as
//! laid out (3D views and charts, `capture`), then the file.

use crate::app::State;
use crate::picture_options::PictureOptions;
use crate::touch::FitScreen;
use aurafx_pipeline::session::ScreenshotRequest;

#[derive(Default)]
pub struct ScreenshotDialog {
    pub open: bool,
    pub options: PictureOptions,
    no_overlays: bool,
    /// What the picture shows: 0 the active 3D view, 1 every view as laid out, 2 the chart
    /// views, 3 every 3D view, 4 one 3D view by number.
    pub which: u8,
    /// The 3D view of choice 4 (1 = the first).
    pub view_no: u32,
    /// Keep the chart views' controls in a picture of several views.
    controls: bool,
}

impl ScreenshotDialog {
    /// Open with the view's size.
    pub fn show_for(&mut self, view: (u32, u32)) {
        if !self.open {
            self.options.reset_size(view);
            self.view_no = self.view_no.max(1);
        }
        self.open = true;
    }
}

/// The choice of what is shown, shared with the Animation window. Returns the layout to cut out
/// of the window, or None for the active 3D view drawn on its own.
pub fn which_views_ui(ui: &mut egui::Ui, which: &mut u8, view_no: &mut u32, n_views: usize, n_3d: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.radio_value(which, 0, "the active 3D view").on_hover_text("Drawn on its own at the size below");
        ui.radio_value(which, 1, format!("all views ({n_views})")).on_hover_text("Every visible 3D view and chart view, as they are laid out in the window");
        if n_3d > 1 {
            ui.radio_value(which, 3, format!("the 3D views ({n_3d})")).on_hover_text("Every visible 3D view, as laid out, without the charts");
            ui.radio_value(which, 4, "3D view").on_hover_text("One of the 3D views, by its number");
            if *which == 4 {
                *view_no = (*view_no).clamp(1, n_3d as u32);
                ui.add(egui::DragValue::new(view_no).range(1..=n_3d as u32));
            }
        } else if *which == 3 || *which == 4 {
            *which = 0;
        }
        ui.radio_value(which, 2, "chart views");
    });
}

/// The views a choice of `which_views_ui` stands for (None: the active 3D view on its own).
pub fn layout_of(which: u8, view_no: u32) -> Option<crate::capture::Views> {
    match which {
        1 => Some(crate::capture::Views::All),
        2 => Some(crate::capture::Views::Charts),
        3 => Some(crate::capture::Views::Views3d),
        4 => crate::capture::Views::parse(&format!("view {}", view_no.saturating_sub(1))),
        _ => None,
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
    let n_3d = state.views.len() + 1;
    egui::Window::new("Save Screenshot").open(&mut open).collapsible(false).resizable(false).default_width(460.0).show_fit(ctx, |ui| {
        let d = &mut state.screenshot_dialog;
        ui.label("Picture of");
        which_views_ui(ui, &mut d.which, &mut d.view_no, n_views, n_3d);
        ui.separator();
        if d.which != 0 {
            ui.weak("The views are cut out of the window and placed as they are laid out; the picture has the window's pixels (a larger window gives a larger picture).");
            egui::Grid::new("shot several").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Background");
                d.options.background_ui(ui, true, true);
                ui.end_row();
                ui.label("Charts");
                ui.checkbox(&mut d.controls, "keep their controls");
                ui.end_row();
            });
            ui.separator();
            save = ui.button("Save…").clicked();
            return;
        }
        egui::Grid::new("shot").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            d.options.size_rows(ui, view);
            ui.label("Background");
            d.options.background_ui(ui, true, false);
            ui.end_row();
            ui.label("Annotations");
            let mut on = !d.no_overlays;
            if ui.checkbox(&mut on, "color legend, labels, axes").changed() {
                d.no_overlays = !on;
            }
            ui.end_row();
        });
        if d.options.size(view).is_some() {
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
            if let Some(views) = layout_of(d.which, d.view_no) {
                state.capture.request(crate::capture::Request {
                    path,
                    views,
                    transparent: d.options.transparent(),
                    controls: d.controls,
                    done: crate::capture::Done::Toast,
                    background: d.options.gap_color(),
                });
            }
            state.screenshot_dialog.open = false;
        }
        return;
    }
    if save {
        if let Some(path) = crate::dialogs::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .add_filter("JPEG image (no transparency)", &["jpg", "jpeg"])
            .add_filter("BMP image", &["bmp"])
            .add_filter("TIFF image", &["tif", "tiff"])
            .add_filter("SVG vector picture", &["svg"])
            .set_file_name("aurafx.png")
            .save_file()
        {
            let d = &state.screenshot_dialog;
            state.viewport.request_screenshot(ScreenshotRequest {
                size: d.options.size(view),
                transparent: d.options.transparent(),
                overlays: !d.no_overlays,
                background: d.options.background(),
                ..ScreenshotRequest::new(path)
            });
            state.screenshot_dialog.open = false;
        }
    }
}
