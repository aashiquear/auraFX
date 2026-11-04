//! Visual theme: dark, slightly warm, with the logo's orange as accent.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(242, 107, 29);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(255, 150, 90);
pub const CYAN: Color32 = Color32::from_rgb(70, 200, 255);

/// Install the Phosphor icon font as a fallback for both font families.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("phosphor".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../../assets/fonts/Phosphor.ttf"))));
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().push("phosphor".into());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, dark: bool, scale: f32) {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    if dark {
        v.panel_fill = Color32::from_rgb(24, 26, 31);
        v.window_fill = Color32::from_rgb(30, 32, 38);
        v.extreme_bg_color = Color32::from_rgb(16, 17, 21);
        v.faint_bg_color = Color32::from_rgb(34, 36, 43);
        v.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 32, 38);
        v.widgets.inactive.bg_fill = Color32::from_rgb(44, 47, 56);
        v.widgets.hovered.bg_fill = Color32::from_rgb(58, 62, 74);
        v.widgets.active.bg_fill = Color32::from_rgb(70, 74, 88);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 43, 51);
    }
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, ACCENT_SOFT);
    v.hyperlink_color = CYAN;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.linear_multiply(0.7));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.widgets.open.corner_radius = r;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    ctx.set_visuals(v);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 22.0;
    });
    ctx.set_pixels_per_point(scale * ctx.native_pixels_per_point().unwrap_or(1.0));
}
