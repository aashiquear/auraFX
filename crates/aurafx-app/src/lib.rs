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
#[cfg(target_os = "ios")]
mod ios_safe_area;
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
mod picture_options;
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
pub mod touch;
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

/// On Linux the taskbar and the window switcher take an application's icon from its desktop
/// entry (found by the window's application id, `aurafx`), not from the window. A copy of the
/// program run on its own (a build from source, an unpacked archive, an AppImage that was not
/// integrated) has no entry and shows a generic icon, so write one for this user when none
/// exists anywhere. `AURAFX_NO_DESKTOP_ENTRY=1` turns this off; an installed entry is never touched.
#[cfg(target_os = "linux")]
fn ensure_desktop_entry() {
    if std::env::var_os("AURAFX_NO_DESKTOP_ENTRY").is_some() {
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".local/share"));
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let own = data_home.join("applications/aurafx.desktop");
    // An entry this function wrote earlier is kept current (the program may have moved).
    let ours = std::fs::read_to_string(&own).map(|s| s.contains("X-auraFX-SelfRegistered=true")).unwrap_or(false);
    let exists = own.exists() || dirs.split(':').any(|d| !d.is_empty() && std::path::Path::new(d).join("applications/aurafx.desktop").exists());
    if exists && !ours {
        return;
    }
    let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok());
    let Some(exe) = exe else { return };
    let icon = data_home.join("icons/hicolor/256x256/apps/aurafx.png");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=auraFX\nGenericName=Scientific Visualization\nComment=Visualize, analyze and run simulations with AI and Python\nExec=\"{}\" %F\nIcon={}\nTerminal=false\nCategories=Science;Education;Graphics;Physics;DataVisualization;\nStartupWMClass=aurafx\nX-auraFX-SelfRegistered=true\n",
        exe.display(),
        icon.display()
    );
    if std::fs::read_to_string(&own).is_ok_and(|s| s == entry) && icon.exists() {
        return;
    }
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(icon.parent().unwrap())?;
        std::fs::write(&icon, include_bytes!("../../../assets/icons/aurafx-256.png"))?;
        std::fs::create_dir_all(own.parent().unwrap())?;
        std::fs::write(&own, entry)
    };
    match write() {
        Ok(()) => tracing::info!("wrote {} so the taskbar shows the auraFX icon", own.display()),
        Err(e) => tracing::warn!("no desktop entry written ({e}); the taskbar may show a generic icon"),
    }
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
    // `AURAFX_WINDOW=390x844`: a window of that size (points), as small as a phone's screen when
    // asked. With `AURAFX_INTERFACE` this shows the tablet and phone interfaces on a desktop machine.
    if let Some((w, h)) = window_size_from_env() {
        viewport = viewport.with_inner_size([w, h]).with_min_inner_size([w.min(320.0), h.min(320.0)]);
    }
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() }
}

/// The size `AURAFX_WINDOW=WIDTHxHEIGHT` asks for, in points.
fn window_size_from_env() -> Option<(f32, f32)> {
    parse_window_size(&std::env::var("AURAFX_WINDOW").ok()?)
}

fn parse_window_size(text: &str) -> Option<(f32, f32)> {
    let (w, h) = text.trim().split_once(['x', 'X'])?;
    let (w, h): (f32, f32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    ((200.0..=10000.0).contains(&w) && (200.0..=10000.0).contains(&h)).then_some((w, h))
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_size_text() {
        assert_eq!(super::parse_window_size("390x844"), Some((390.0, 844.0)));
        assert_eq!(super::parse_window_size(" 1180 X 820 "), Some((1180.0, 820.0)));
        assert_eq!(super::parse_window_size("390"), None);
        assert_eq!(super::parse_window_size("10x10"), None);
    }
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
    #[cfg(target_os = "ios")]
    ios_safe_area::install();
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
    #[cfg(target_os = "linux")]
    if !args.iter().any(|a| a == "--jupyter-kernel" || a == "--install-jupyter-kernel") {
        ensure_desktop_entry();
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
