//! Plugins window: Python plugins that add filters, sources and readers.

use crate::app::State;
use crate::touch::FitScreen;
use egui::RichText;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct PluginsWindow {
    pub open: bool,
    last_sync: f64,
    started: bool,
}

/// Open a file or folder with the system's default application.
pub fn open_in_os(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let mut c = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut c = std::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut c = std::process::Command::new("xdg-open");
    c.arg(path).spawn().map(|_| ())
}

impl State {
    fn plugin_search_paths(&self) -> Vec<PathBuf> {
        #[cfg(feature = "python")]
        {
            aurafx_python::plugins::search_paths(&self.settings.plugin_paths)
        }
        #[cfg(not(feature = "python"))]
        {
            self.settings.plugin_paths.clone()
        }
    }

    /// Ask the Python worker to pick up new or changed plugins.
    pub fn sync_plugins(&mut self, force: bool) {
        let paths = self.plugin_search_paths();
        // Rust plugins (shared libraries) load once; a new build needs a restart.
        let msgs = aurafx_filters::rust_plugins::load_all(&paths, &self.settings.disabled_plugins);
        if !msgs.is_empty() {
            let ids: Vec<String> = aurafx_filters::registry().iter().filter(|f| f.ext.is_some()).map(|f| f.id.to_string()).collect();
            let mut s = self.session.lock();
            s.refresh_filter_defs(&ids);
            for m in &msgs {
                let lvl = if m.contains("failed") { aurafx_pipeline::LogLevel::Error } else { aurafx_pipeline::LogLevel::Info };
                s.log(lvl, m.clone());
            }
            drop(s);
            self.python.plugin_messages.lock().extend(msgs);
        }
        self.python.sync_plugins(paths, self.settings.disabled_plugins.clone(), force);
    }

    /// Every couple of seconds: reload edited plugins, show their messages.
    pub fn plugins_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if !self.plugins.started || now - self.plugins.last_sync > 2.0 {
            self.plugins.started = true;
            self.plugins.last_sync = now;
            self.sync_plugins(false);
            // The panels of the plugins (after the sync above, on the same Python thread).
            let tx = self.python.sender();
            self.plugin_panels.refresh_titles(&tx);
        }
        let msgs: Vec<String> = std::mem::take(&mut *self.python.plugin_messages.lock());
        for m in msgs {
            let err = m.contains("failed");
            self.toast(m, err);
        }
    }
}

#[cfg(feature = "python")]
fn new_plugin_file() -> std::io::Result<PathBuf> {
    let dir = aurafx_python::plugins::user_plugin_dir().ok_or_else(|| std::io::Error::other("no config folder"))?;
    std::fs::create_dir_all(&dir)?;
    let mut k = 1;
    let path = loop {
        let p = dir.join(if k == 1 { "my_plugin.py".to_string() } else { format!("my_plugin_{k}.py") });
        if !p.exists() {
            break p;
        }
        k += 1;
    };
    std::fs::write(&path, aurafx_python::plugins::TEMPLATE)?;
    Ok(path)
}

pub fn plugins_ui(st: &mut State, ctx: &egui::Context) {
    if !st.plugins.open {
        return;
    }
    let mut open = true;
    egui::Window::new(format!("{} Plugins", crate::icons::PLUS)).open(&mut open).resizable(true).default_width(560.0).show_fit(ctx, |ui| {
        #[cfg(not(feature = "python"))]
        {
            ui.label("Python is not included in this build, so plugins are unavailable.");
        }
        #[cfg(feature = "python")]
        body(st, ui);
    });
    st.plugins.open = open;
}

#[cfg(feature = "python")]
fn body(st: &mut State, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Python plugins add filters, sources and file readers with @afx.filter, @afx.source and @afx.reader. \
             Files in the plugin folder load automatically and reload when you save them.",
        )
        .weak(),
    );
    let dir = aurafx_python::plugins::user_plugin_dir();
    ui.horizontal_wrapped(|ui| {
        if ui.button(format!("{} New plugin…", crate::icons::PLUS)).on_hover_text("Create a plugin from a template and open it in your editor").clicked() {
            match new_plugin_file() {
                Ok(p) => {
                    let _ = open_in_os(&p);
                    st.toast(format!("Created {}", p.display()), false);
                    st.sync_plugins(false);
                }
                Err(e) => st.toast(format!("Could not create a plugin: {e}"), true),
            }
        }
        if let Some(d) = &dir {
            if ui.button(format!("{} Open plugin folder", crate::icons::FOLDER_OPEN)).on_hover_text(d.display().to_string()).clicked() {
                let _ = std::fs::create_dir_all(d);
                if let Err(e) = open_in_os(d) {
                    st.toast(format!("Could not open {}: {e}", d.display()), true);
                }
            }
        }
        if ui.button("Add file or folder…").on_hover_text("Load plugins from another place too").clicked() {
            if let Some(p) = crate::dialogs::FileDialog::new().add_filter("Plugin (Python or Rust)", &["py", std::env::consts::DLL_EXTENSION]).pick_file() {
                if !st.settings.plugin_paths.contains(&p) {
                    st.settings.plugin_paths.push(p);
                    st.settings.save();
                }
                st.sync_plugins(false);
            }
        }
        if ui.button(format!("{} Reload all", crate::icons::ARROW_CLOCKWISE)).clicked() {
            st.sync_plugins(true);
        }
    });
    if let Some(d) = &dir {
        ui.label(RichText::new(format!("Plugin folder: {}", d.display())).small().weak());
    }
    ui.separator();
    let list = aurafx_python::plugins::list();
    if list.is_empty() {
        ui.label(RichText::new("No plugins yet. \"New plugin…\" writes a working example you can edit.").weak());
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for p in list {
            let name = p.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui.horizontal(|ui| {
                let mut on = !st.settings.disabled_plugins.contains(&p.path);
                if ui.checkbox(&mut on, RichText::new(&name).strong()).on_hover_text(p.path.display().to_string()).changed() {
                    if on {
                        st.settings.disabled_plugins.retain(|d| d != &p.path);
                    } else {
                        st.settings.disabled_plugins.push(p.path.clone());
                    }
                    st.settings.save();
                    st.sync_plugins(false);
                }
                if let Some(e) = &p.error {
                    ui.colored_label(crate::theme::ERROR, format!("{} error", crate::icons::WARNING)).on_hover_text(e);
                } else if p.enabled {
                    let mut parts = Vec::new();
                    if !p.filters.is_empty() {
                        parts.push(format!("{} filter{}", p.filters.len(), if p.filters.len() == 1 { "" } else { "s" }));
                    }
                    if !p.readers.is_empty() {
                        parts.push(format!("{} reader{}", p.readers.len(), if p.readers.len() == 1 { "" } else { "s" }));
                    }
                    ui.label(RichText::new(if parts.is_empty() { "nothing registered".into() } else { parts.join(", ") }).weak());
                } else {
                    ui.label(RichText::new("off").weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if st.settings.plugin_paths.contains(&p.path)
                        && crate::touch::small_button(ui, crate::icons::TRASH).on_hover_text("Stop loading this file").clicked()
                    {
                        st.settings.plugin_paths.retain(|x| x != &p.path);
                        st.settings.save();
                        st.sync_plugins(false);
                    }
                    if crate::touch::small_button(ui, format!("{} Edit", crate::icons::PENCIL_SIMPLE))
                        .on_hover_text("Open in your editor; saving reloads it")
                        .clicked()
                    {
                        if let Err(e) = open_in_os(&p.path) {
                            st.toast(format!("Could not open {}: {e}", p.path.display()), true);
                        }
                    }
                });
            });
            if let Some(e) = &p.error {
                egui::CollapsingHeader::new(RichText::new(e.lines().last().unwrap_or("")).color(crate::theme::ERROR).small())
                    .id_salt(("plugin_err", p.path.clone()))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(e).monospace().small()).wrap());
                    });
            } else if p.enabled && (!p.filters.is_empty() || !p.readers.is_empty()) {
                let labels: Vec<String> = p
                    .filters
                    .iter()
                    .filter_map(|id| aurafx_filters::find(id))
                    .map(|f| format!("{} ({})", f.label, f.category))
                    .chain(p.readers.iter().map(|r| format!("{r} reader")))
                    .collect();
                ui.label(RichText::new(labels.join(" · ")).small().weak());
            }
            ui.add_space(4.0);
        }
    });
    // Rust plugins (shared libraries built with the aurafx-plugin crate).
    let libs = aurafx_filters::rust_plugins::loaded();
    if !libs.is_empty() {
        ui.separator();
        ui.label(RichText::new("Rust plugins").strong());
        for l in libs {
            let key = l.display().to_string();
            let labels: Vec<String> = aurafx_filters::registry()
                .iter()
                .filter(|f| f.ext.as_ref().is_some_and(|e| e.plugin == key))
                .map(|f| format!("{} ({})", f.label, f.category))
                .collect();
            ui.horizontal(|ui| {
                ui.label(RichText::new(l.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).strong()).on_hover_text(&key);
                ui.label(RichText::new(labels.join(" · ")).small().weak());
            });
        }
        ui.label(RichText::new("A new build of a Rust plugin loads at the next start.").small().weak());
    }
    ui.separator();
    ui.label(
        RichText::new(format!(
            "In Python: afx.load_plugin(path), afx.plugins(). Rust plugins: shared libraries (.{}) built with the aurafx-plugin crate, in the plugin folder.",
            std::env::consts::DLL_EXTENSION
        ))
        .small()
        .weak(),
    );
}
