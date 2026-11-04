//! Visual theme: dark, slightly warm, with the logo's orange as accent.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(242, 107, 29);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(255, 150, 90);

/// Install the Phosphor icon font as a fallback for both font families.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("phosphor".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../../assets/fonts/Phosphor.ttf"))));
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().push("phosphor".into());
    }
    ctx.set_fonts(fonts);
}

/// Status colors chosen to stay readable on both light and dark backgrounds.
pub const ERROR: Color32 = Color32::from_rgb(225, 72, 72);
pub const WARN: Color32 = Color32::from_rgb(214, 150, 30);
pub const OK: Color32 = Color32::from_rgb(52, 168, 92);
pub const INFO: Color32 = Color32::from_rgb(66, 140, 230);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System];
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark",
            ThemeMode::Light => "Light",
            ThemeMode::System => "Follow system",
        }
    }
}

fn common(v: &mut Visuals) {
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.widgets.open.corner_radius = r;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    v.hyperlink_color = INFO;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
}

pub fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = Color32::from_rgb(24, 26, 31);
    v.window_fill = Color32::from_rgb(30, 32, 38);
    v.extreme_bg_color = Color32::from_rgb(16, 17, 21);
    v.faint_bg_color = Color32::from_rgb(34, 36, 43);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 32, 38);
    v.widgets.inactive.bg_fill = Color32::from_rgb(44, 47, 56);
    v.widgets.hovered.bg_fill = Color32::from_rgb(58, 62, 74);
    v.widgets.active.bg_fill = Color32::from_rgb(70, 74, 88);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 43, 51);
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, ACCENT_SOFT);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.linear_multiply(0.7));
    common(&mut v);
    v
}

pub fn light_visuals() -> Visuals {
    let mut v = Visuals::light();
    v.panel_fill = Color32::from_rgb(246, 247, 249);
    v.window_fill = Color32::from_rgb(252, 252, 253);
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(238, 240, 244);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(246, 247, 249);
    v.widgets.inactive.bg_fill = Color32::from_rgb(228, 231, 237);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(232, 235, 240);
    v.widgets.hovered.bg_fill = Color32::from_rgb(214, 219, 228);
    v.widgets.active.bg_fill = Color32::from_rgb(200, 206, 218);
    v.selection.bg_fill = Color32::from_rgb(255, 214, 186);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(170, 70, 10));
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.linear_multiply(0.8));
    common(&mut v);
    v
}

/// Install both visual sets and select the theme. Call again whenever the mode or scale changes.
pub fn apply(ctx: &egui::Context, mode: ThemeMode, scale: f32) {
    ctx.set_visuals_of(egui::Theme::Dark, dark_visuals());
    ctx.set_visuals_of(egui::Theme::Light, light_visuals());
    ctx.set_theme(match mode {
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::System => egui::ThemePreference::System,
    });
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 22.0;
    });
    ctx.set_pixels_per_point(scale * ctx.native_pixels_per_point().unwrap_or(1.0));
}

/// Text color that contrasts with an RGB background (used for overlays drawn over the 3D view).
pub fn overlay_text(bg: [f32; 3]) -> Color32 {
    let lum = 0.2126 * bg[0] + 0.7152 * bg[1] + 0.0722 * bg[2];
    if lum > 0.5 {
        Color32::from_gray(25)
    } else {
        Color32::from_gray(232)
    }
}
