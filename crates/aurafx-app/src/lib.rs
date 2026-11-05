//! The auraFX application (desktop binary and mobile entry points share this library).

mod animation;
mod app;
mod assistant;
mod axes_grid;
mod charts;
mod colormap_editor;
mod console;
mod dialogs;
mod icons;
mod jobs_panel;
mod lighting_ui;
mod palette;
mod panels;
mod pick;
mod plugins_ui;
mod ray_tracing;
mod server_ui;
mod settings;
mod theme;
mod viewport;
mod views;
mod widgets;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;

pub use eframe;

fn load_icon() -> Option<Arc<egui::IconData>> {
    let bytes = include_bytes!("../../../assets/icons/aurafx-256.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some(Arc::new(egui::IconData { rgba: img.into_raw(), width: w, height: h }))
}

pub fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wgpu=warn,naga=warn,egui_wgpu=warn")),
        )
        .try_init();
}

/// Default window/renderer options.
pub fn native_options() -> eframe::NativeOptions {
    let mut viewport =
        egui::ViewportBuilder::default().with_inner_size([1600.0, 950.0]).with_min_inner_size([900.0, 600.0]).with_title("auraFX").with_app_id("aurafx");
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() }
}

/// Run the app with the given files to open and optional startup script.
pub fn run(options: eframe::NativeOptions, files: Vec<PathBuf>, script: Option<PathBuf>) -> eframe::Result<()> {
    eframe::run_native("auraFX", options, Box::new(move |cc| Ok(Box::new(app::AuraApp::new(cc, files, script)))))
}

/// Desktop entry point: parses command-line arguments.
pub fn main_desktop() -> eframe::Result<()> {
    init_logging();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "auraFX {}\n\nUsage: aurafx [FILES...] [--script SCRIPT.py]\n\n  FILES             data files (or .afx sessions) to open\n  --script FILE     run a Python script after startup (uses the `afx` module)\n  --version         print the version",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("auraFX {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    aurafx_filters::init_threads(crate::settings::Settings::load().threads);
    let mut files: Vec<PathBuf> = Vec::new();
    let mut script: Option<PathBuf> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--script" {
            script = it.next().map(Into::into);
        } else if !a.starts_with('-') {
            files.push(a.into());
        }
    }
    run(native_options(), files, script)
}
