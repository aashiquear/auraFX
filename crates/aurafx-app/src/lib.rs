//! The auraFX application (desktop binary and mobile entry points share this library).

mod animation;
mod app;
mod assistant;
mod axes_grid;
mod cad;
mod capture;
mod charts;
mod colormap_editor;
mod console;
mod crash;
mod dialogs;
mod goal_stats;
mod icons;
mod jobs_panel;
mod jupyter;
mod lighting_ui;
mod menus;
mod mesh_guide;
mod notebook;
mod overlays;
mod palette;
mod panels;
mod pick;
mod plugin_panels;
mod plugins_ui;
mod ray_tracing;
mod screenshot_ui;
mod server_ui;
mod settings;
mod shortcuts;
mod source_dialog;
mod svg_export;
mod theme;
mod view_menu;
mod viewport;
mod views;
mod welcome;
mod widgets;
mod widgets3d;
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

/// The connection file when the app runs as a Jupyter kernel (`--jupyter-kernel FILE`).
pub static JUPYTER_CONNECTION: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

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
            "auraFX {}\n\nUsage: aurafx [FILES...] [--script SCRIPT.py]\n\n  FILES             data files (or .afx sessions) to open\n  --script FILE     run a Python script after startup (uses the `afx` module)\n  --install-jupyter-kernel   make Jupyter list auraFX as a kernel\n  --jupyter-kernel FILE      run as a Jupyter kernel (Jupyter passes the connection file)\n  --version         print the version",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("auraFX {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--install-jupyter-kernel") {
        match jupyter::install_kernel_spec() {
            Ok(d) => println!("Installed the auraFX kernel for Jupyter in {}\nChoose \"auraFX (Python)\" as the kernel of a notebook.", d.display()),
            Err(e) => {
                eprintln!("could not install the kernel: {e:#}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    crash::install();
    let settings = crate::settings::Settings::load();
    aurafx_filters::init_threads(settings.threads);
    // The GPU for filters, found in the background (it takes a moment).
    aurafx_filters::gpu::set_enabled(settings.gpu_filters);
    std::thread::spawn(|| {
        aurafx_gpu::install();
    });
    let mut files: Vec<PathBuf> = Vec::new();
    let mut script: Option<PathBuf> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--script" {
            script = it.next().map(Into::into);
        } else if a == "--jupyter-kernel" {
            if let Some(f) = it.next() {
                let _ = JUPYTER_CONNECTION.set(f.into());
            }
        } else if !a.starts_with('-') {
            files.push(a.into());
        }
    }
    run(native_options(), files, script)
}
