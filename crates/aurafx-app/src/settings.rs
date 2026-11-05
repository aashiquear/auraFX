//! Persistent settings (TOML in the platform config directory).

use aurafx_ai::AiConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ai: AiConfig,
    pub recent_files: Vec<PathBuf>,
    /// 3D view background in dark mode.
    pub background: [f32; 3],
    /// 3D view background in light mode.
    pub background_light: [f32; 3],
    pub theme: crate::theme::ThemeMode,
    /// Legacy flag from older settings files.
    pub dark_theme: bool,
    pub ui_scale: f32,
    pub auto_run_ai_code: bool,
    pub ai_agent_mode: bool,
    pub show_orientation_axes: bool,
    pub default_colormap: String,
    pub hosts: Vec<aurafx_jobs::Host>,
    /// Color maps the user saved or imported.
    pub user_colormaps: Vec<aurafx_pipeline::colormap::ColorMap>,
    /// Plugin files and folders added in the Plugins window (besides the user plugin folder).
    pub plugin_paths: Vec<PathBuf>,
    /// Plugin files the user switched off.
    pub disabled_plugins: Vec<PathBuf>,
    /// Threads for filters (0 = all cores). Takes effect at the next start.
    pub threads: usize,
    /// Lights, shadows and screen effects of the 3D views.
    pub lighting: aurafx_pipeline::Lighting,
    /// Show the custom lights as handles in the 3D views (drag to aim them).
    pub show_light_widgets: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ai: AiConfig::default(),
            recent_files: Vec::new(),
            background: [0.12, 0.13, 0.16],
            background_light: [0.93, 0.94, 0.96],
            theme: crate::theme::ThemeMode::Dark,
            dark_theme: true,
            ui_scale: 1.0,
            auto_run_ai_code: false,
            ai_agent_mode: false,
            show_orientation_axes: true,
            default_colormap: "Viridis".into(),
            hosts: Vec::new(),
            user_colormaps: Vec::new(),
            plugin_paths: Vec::new(),
            disabled_plugins: Vec::new(),
            threads: 0,
            lighting: Default::default(),
            show_light_widgets: true,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        if let Some(d) = std::env::var_os("AURAFX_DATA_DIR") {
            return Some(PathBuf::from(d).join("settings.toml"));
        }
        directories::ProjectDirs::from("org", "aurafx", "auraFX").map(|d| d.config_dir().join("settings.toml"))
    }

    pub fn load() -> Self {
        let mut s = Self::load_file();
        // Migrate the old boolean setting.
        if !s.dark_theme && s.theme == crate::theme::ThemeMode::Dark {
            s.theme = crate::theme::ThemeMode::Light;
        }
        if let Ok(h) = std::env::var("OLLAMA_HOST") {
            if !h.is_empty() && s.ai.ollama_url.contains("localhost:11434") {
                s.ai.ollama_url = if h.starts_with("http") { h } else { format!("http://{h}") };
            }
        }
        s
    }

    fn load_file() -> Self {
        let Some(p) = Self::path() else { return Self::default() };
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!("bad settings file {}: {e}", p.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// 3D background for the theme currently shown.
    pub fn background_for(&self, dark: bool) -> [f32; 3] {
        if dark {
            self.background
        } else {
            self.background_light
        }
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match toml::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&p, s) {
                    tracing::warn!("cannot save settings: {e}");
                }
            }
            Err(e) => tracing::warn!("cannot serialize settings: {e}"),
        }
    }

    pub fn push_recent(&mut self, p: PathBuf) {
        self.recent_files.retain(|x| x != &p);
        self.recent_files.insert(0, p);
        self.recent_files.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lighting_round_trips_through_toml() {
        let mut s = Settings::default();
        s.lighting.lights = aurafx_pipeline::Light::kit();
        s.lighting.lights[1].camera_relative = false;
        s.lighting.ambient = Some(0.1);
        s.lighting.shadows = true;
        s.lighting.tone_mapping = aurafx_pipeline::ToneMapping::Aces;
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.lighting, s.lighting);
        // Older files without a [lighting] table get the defaults.
        let old: Settings = toml::from_str("ui_scale = 1.0\n").unwrap();
        assert_eq!(old.lighting, aurafx_pipeline::Lighting::default());
    }
}
