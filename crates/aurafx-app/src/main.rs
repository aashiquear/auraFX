#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animation;
mod app;
mod assistant;
mod charts;
mod console;
mod icons;
mod jobs_panel;
mod palette;
mod panels;
mod pick;
mod settings;
mod theme;
mod viewport;
mod views;
mod widgets;
mod worker;

use std::sync::Arc;

fn load_icon() -> Option<Arc<egui::IconData>> {
    let bytes = include_bytes!("../../../assets/icons/aurafx-256.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some(Arc::new(egui::IconData { rgba: img.into_raw(), width: w, height: h }))
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wgpu=warn,naga=warn,egui_wgpu=warn")),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("auraFX {}\n\nUsage: aurafx [FILES...] [--script SCRIPT.py]\n\n  FILES             data files to open\n  --script FILE     run a Python script after startup (uses the `afx` module)", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut script: Option<std::path::PathBuf> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--script" {
            script = it.next().map(Into::into);
        } else if !a.starts_with('-') {
            files.push(a.into());
        }
    }
    let mut viewport =
        egui::ViewportBuilder::default().with_inner_size([1600.0, 950.0]).with_min_inner_size([900.0, 600.0]).with_title("auraFX").with_app_id("aurafx");
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native("auraFX", options, Box::new(move |cc| Ok(Box::new(app::AuraApp::new(cc, files, script)))))
}
