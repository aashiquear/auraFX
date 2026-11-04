//! Persistent settings (TOML in the platform config directory).

use aurafx_ai::AiConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ai: AiConfig,
    pub recent_files: Vec<PathBuf>,
    pub background: [f32; 3],
    pub dark_theme: bool,
    pub ui_scale: f32,
    pub auto_run_ai_code: bool,
    pub show_orientation_axes: bool,
    pub default_colormap: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ai: AiConfig::default(),
            recent_files: Vec::new(),
            background: [0.12, 0.13, 0.16],
            dark_theme: true,
            ui_scale: 1.0,
            auto_run_ai_code: false,
            show_orientation_axes: true,
            default_colormap: "Viridis".into(),
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("org", "aurafx", "auraFX").map(|d| d.config_dir().join("settings.toml"))
    }

    pub fn load() -> Self {
        let mut s = Self::load_file();
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
