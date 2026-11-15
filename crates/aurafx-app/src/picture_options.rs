//! What a picture or an animation looks like, apart from what it shows: its background (as on
//! the screen, transparent, blank, a color or a gradient) and its size and shape. Shared by the
//! Save Screenshot and Animation windows.

use aurafx_pipeline::session::PictureBackground;
use egui::Color32;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Bg {
    /// The view's own background.
    #[default]
    Screen,
    Transparent,
    White,
    Black,
    Color,
    Gradient,
}

/// The shape of the picture: the view's own, any, or a fixed ratio (width : height).
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Shape {
    #[default]
    View,
    Free,
    Ratio(u32, u32),
}

const RATIOS: &[(u32, u32)] = &[(16, 9), (4, 3), (3, 2), (1, 1), (21, 9), (9, 16), (4, 5)];
/// Widths offered as buttons (the height follows the shape).
const WIDTHS: &[u32] = &[1280, 1920, 2560, 3840];

pub struct PictureOptions {
    pub bg: Bg,
    pub color: [f32; 3],
    pub bottom: [f32; 3],
    pub top: [f32; 3],
    pub width: u32,
    pub height: u32,
    pub shape: Shape,
}

impl Default for PictureOptions {
    fn default() -> Self {
        Self { bg: Bg::Screen, color: [1.0, 1.0, 1.0], bottom: [0.93, 0.95, 0.98], top: [0.42, 0.55, 0.74], width: 0, height: 0, shape: Shape::View }
    }
}

impl PictureOptions {
    /// Start from the view's size (when the window opens).
    pub fn reset_size(&mut self, view: (u32, u32)) {
        if self.shape == Shape::View || self.width == 0 {
            (self.width, self.height) = view;
        }
    }

    pub fn transparent(&self) -> bool {
        self.bg == Bg::Transparent
    }

    /// The background to draw instead of the view's own.
    pub fn background(&self) -> Option<PictureBackground> {
        match self.bg {
            Bg::Screen | Bg::Transparent => None,
            Bg::White => Some(PictureBackground::solid([1.0; 3])),
            Bg::Black => Some(PictureBackground::solid([0.0; 3])),
            Bg::Color => Some(PictureBackground::solid(self.color)),
            Bg::Gradient => Some(PictureBackground { bottom: self.bottom, top: self.top }),
        }
    }

    /// The color between the views of a picture of several views (None: the window's).
    pub fn gap_color(&self) -> Option<Color32> {
        self.background().map(|b| Color32::from_rgb((b.bottom[0] * 255.0) as u8, (b.bottom[1] * 255.0) as u8, (b.bottom[2] * 255.0) as u8))
    }

    /// The size to draw at: None when it is the view's own.
    pub fn size(&self, view: (u32, u32)) -> Option<(u32, u32)> {
        ((self.width, self.height) != view && self.width > 0 && self.height > 0).then_some((self.width, self.height))
    }

    fn ratio(&self, view: (u32, u32)) -> Option<f64> {
        match self.shape {
            Shape::View => Some(view.0.max(1) as f64 / view.1.max(1) as f64),
            Shape::Free => None,
            Shape::Ratio(a, b) => Some(a as f64 / b as f64),
        }
    }

    fn height_from_width(&mut self, view: (u32, u32)) {
        if let Some(r) = self.ratio(view) {
            self.height = (self.width as f64 / r).round().max(1.0) as u32;
        }
    }

    /// The Background row. `transparent`: offer it (pictures; not videos). `several`: a picture
    /// of several views, where the choice colors the space between the views.
    pub fn background_ui(&mut self, ui: &mut egui::Ui, transparent: bool, several: bool) {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                let screen = if several { "as in the window" } else { "as on screen" };
                ui.radio_value(&mut self.bg, Bg::Screen, screen).on_hover_text("The background the view has now");
                if transparent {
                    ui.radio_value(&mut self.bg, Bg::Transparent, "transparent").on_hover_text("PNG and TIFF keep it; JPEG and BMP have no transparency");
                } else if self.bg == Bg::Transparent {
                    self.bg = Bg::Screen;
                }
                ui.radio_value(&mut self.bg, Bg::White, "white").on_hover_text("A blank white background, for print and slides");
                ui.radio_value(&mut self.bg, Bg::Black, "black");
                ui.radio_value(&mut self.bg, Bg::Color, "color");
                if !several {
                    ui.radio_value(&mut self.bg, Bg::Gradient, "gradient").on_hover_text("From one color at the bottom to another at the top");
                } else if self.bg == Bg::Gradient {
                    self.bg = Bg::Color;
                }
            });
            match self.bg {
                Bg::Color => {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_rgb(&mut self.color);
                        ui.weak("click to choose");
                    });
                }
                Bg::Gradient => {
                    ui.horizontal(|ui| {
                        ui.label("bottom");
                        ui.color_edit_button_rgb(&mut self.bottom);
                        ui.label("top");
                        ui.color_edit_button_rgb(&mut self.top);
                        if crate::touch::small_button(ui, "swap").clicked() {
                            std::mem::swap(&mut self.bottom, &mut self.top);
                        }
                    });
                }
                _ => {}
            }
            if several && !matches!(self.bg, Bg::Screen) {
                ui.weak("Fills the space between the views; each view keeps its own background.");
            }
        });
    }

    /// The Size and Shape rows of a two-column grid.
    pub fn size_rows(&mut self, ui: &mut egui::Ui, view: (u32, u32)) {
        ui.label("Shape");
        ui.horizontal(|ui| {
            let text = match self.shape {
                Shape::View => "the view's".to_string(),
                Shape::Free => "any".to_string(),
                Shape::Ratio(a, b) => format!("{a} : {b}"),
            };
            let before = self.shape;
            egui::ComboBox::from_id_salt("picture shape").selected_text(text).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.shape, Shape::View, "the view's").on_hover_text("The picture shows what the view shows");
                for &(a, b) in RATIOS {
                    let about = match (a, b) {
                        (16, 9) => "  (video, slides)",
                        (4, 3) => "  (older slides)",
                        (1, 1) => "  (square)",
                        (9, 16) => "  (upright video)",
                        (21, 9) => "  (wide)",
                        _ => "",
                    };
                    ui.selectable_value(&mut self.shape, Shape::Ratio(a, b), format!("{a} : {b}{about}"));
                }
                ui.selectable_value(&mut self.shape, Shape::Free, "any").on_hover_text("Width and height are set separately");
            });
            if self.shape != before {
                self.height_from_width(view);
            }
            if self.shape != Shape::View {
                ui.weak("wider or taller than the view: more or less is seen at the sides");
            }
        });
        ui.end_row();
        ui.label("Size");
        ui.horizontal(|ui| {
            let w = ui.add(egui::DragValue::new(&mut self.width).range(16..=16384).suffix(" px"));
            ui.label("×");
            let h = ui.add(egui::DragValue::new(&mut self.height).range(16..=16384).suffix(" px"));
            if w.changed() {
                self.height_from_width(view);
            }
            if h.changed() {
                if let Some(r) = self.ratio(view) {
                    self.width = (self.height as f64 * r).round().max(1.0) as u32;
                }
            }
        });
        ui.end_row();
        ui.label("");
        ui.horizontal_wrapped(|ui| {
            for (label, k) in [("view", 1u32), ("2×", 2), ("4×", 4)] {
                if crate::touch::small_button(ui, label).on_hover_text("A multiple of the view's size").clicked() {
                    self.width = view.0 * k;
                    if self.shape == Shape::Free {
                        self.height = view.1 * k;
                    }
                    self.height_from_width(view);
                }
            }
            for &w in WIDTHS {
                if crate::touch::small_button(ui, format!("{w} wide")).on_hover_text("1920 wide at 16 : 9 is 1080p, 3840 is 4K").clicked() {
                    self.width = w;
                    self.height_from_width(view);
                }
            }
        });
        ui.end_row();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_follows_the_shape() {
        let mut o = PictureOptions::default();
        o.reset_size((800, 600));
        assert_eq!(o.size((800, 600)), None, "the view's own size needs no second drawing");
        o.shape = Shape::Ratio(16, 9);
        o.width = 1920;
        o.height_from_width((800, 600));
        assert_eq!(o.size((800, 600)), Some((1920, 1080)));
        o.shape = Shape::View;
        o.width = 1600;
        o.height_from_width((800, 600));
        assert_eq!((o.width, o.height), (1600, 1200));
    }

    #[test]
    fn backgrounds() {
        let mut o = PictureOptions::default();
        assert_eq!(o.background(), None);
        o.bg = Bg::White;
        assert_eq!(o.background(), Some(PictureBackground::solid([1.0; 3])));
        assert_eq!(o.gap_color(), Some(Color32::WHITE));
        o.bg = Bg::Gradient;
        assert!(o.background().unwrap().is_gradient());
        o.bg = Bg::Transparent;
        assert!(o.transparent() && o.background().is_none());
    }
}
