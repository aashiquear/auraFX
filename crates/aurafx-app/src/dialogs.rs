//! File dialogs: native (rfd) on desktop, an in-app browser where native dialogs are not
//! available (Android, iOS, or builds without the `native-dialogs` feature).

use std::path::{Path, PathBuf};

/// True when native OS file dialogs are available in this build.
pub const NATIVE: bool = cfg!(feature = "native-dialogs");

/// Same builder shape as `rfd::FileDialog` for the calls auraFX uses.
#[derive(Default)]
pub struct FileDialog {
    #[cfg(feature = "native-dialogs")]
    inner: Option<rfd::FileDialog>,
    file_name: Option<String>,
}

impl FileDialog {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "native-dialogs")]
            inner: Some(rfd::FileDialog::new()),
            file_name: None,
        }
    }
    #[allow(unused_mut)]
    pub fn add_filter(mut self, name: impl Into<String>, exts: &[impl ToString]) -> Self {
        #[cfg(feature = "native-dialogs")]
        {
            let e: Vec<String> = exts.iter().map(|x| x.to_string()).collect();
            self.inner = self.inner.map(|d| d.add_filter(name.into(), &e));
        }
        #[cfg(not(feature = "native-dialogs"))]
        let _ = (name.into(), exts.len());
        self
    }
    #[allow(unused_mut)]
    pub fn set_directory(mut self, p: impl AsRef<Path>) -> Self {
        #[cfg(feature = "native-dialogs")]
        {
            self.inner = self.inner.map(|d| d.set_directory(p));
        }
        #[cfg(not(feature = "native-dialogs"))]
        let _ = p.as_ref();
        self
    }
    pub fn set_file_name(mut self, n: impl Into<String>) -> Self {
        let n = n.into();
        #[cfg(feature = "native-dialogs")]
        {
            self.inner = self.inner.map(|d| d.set_file_name(n.clone()));
        }
        self.file_name = Some(n);
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        #[cfg(feature = "native-dialogs")]
        return self.inner.and_then(|d| d.pick_file());
        #[cfg(not(feature = "native-dialogs"))]
        None
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        #[cfg(feature = "native-dialogs")]
        return self.inner.and_then(|d| d.pick_files());
        #[cfg(not(feature = "native-dialogs"))]
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        #[cfg(feature = "native-dialogs")]
        return self.inner.and_then(|d| d.pick_folder());
        #[cfg(not(feature = "native-dialogs"))]
        None
    }
    /// Without native dialogs, files are saved into the app's documents folder.
    pub fn save_file(self) -> Option<PathBuf> {
        #[cfg(feature = "native-dialogs")]
        return self.inner.and_then(|d| d.save_file());
        #[cfg(not(feature = "native-dialogs"))]
        {
            let dir = documents_dir();
            let _ = std::fs::create_dir_all(&dir);
            Some(dir.join(self.file_name.unwrap_or_else(|| "aurafx-output".into())))
        }
    }
}

/// Where files are read from / written to when there is no native dialog.
pub fn documents_dir() -> PathBuf {
    // Mobile entry points set this to the app's private storage.
    if let Some(d) = std::env::var_os("AURAFX_DATA_DIR") {
        return PathBuf::from(d).join("Documents");
    }
    if let Some(d) = directories::UserDirs::new().and_then(|u| u.document_dir().map(|d| d.join("auraFX"))) {
        return d;
    }
    directories::ProjectDirs::from("org", "aurafx", "auraFX").map(|d| d.data_dir().to_path_buf()).unwrap_or_else(|| std::env::temp_dir().join("auraFX"))
}

/// In-app file browser for opening data files.
pub struct FileBrowser {
    pub open: bool,
    pub dir: PathBuf,
    pub filter: String,
}

impl Default for FileBrowser {
    fn default() -> Self {
        Self { open: false, dir: documents_dir(), filter: String::new() }
    }
}

/// Draw the browser; returns a file the user chose to open.
pub fn file_browser_ui(fb: &mut FileBrowser, ctx: &egui::Context) -> Option<PathBuf> {
    if !fb.open {
        return None;
    }
    let mut chosen = None;
    let mut open = true;
    egui::Window::new("Open file").open(&mut open).default_size([520.0, 480.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button(crate::icons::CARET_LEFT).on_hover_text("Parent folder").clicked() {
                if let Some(p) = fb.dir.parent() {
                    fb.dir = p.to_path_buf();
                }
            }
            let mut d = fb.dir.display().to_string();
            if ui.add(egui::TextEdit::singleline(&mut d).desired_width(300.0)).lost_focus() {
                fb.dir = PathBuf::from(d);
            }
            ui.add(egui::TextEdit::singleline(&mut fb.filter).hint_text("filter").desired_width(90.0));
        });
        let exts = aurafx_io::all_extensions();
        let mut entries: Vec<(String, bool)> = std::fs::read_dir(&fb.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().to_string(), e.path().is_dir()))
            .filter(|(n, _)| !n.starts_with('.'))
            .filter(|(n, _)| fb.filter.is_empty() || n.to_lowercase().contains(&fb.filter.to_lowercase()))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, is_dir) in entries {
                let readable = name.ends_with(".afx") || exts.iter().any(|e| name.to_lowercase().ends_with(&format!(".{e}")));
                let label = if is_dir { format!("{} {name}", crate::icons::FOLDER_OPEN) } else { name.clone() };
                let text = if is_dir || readable { egui::RichText::new(label) } else { egui::RichText::new(label).weak() };
                if ui.selectable_label(false, text).clicked() {
                    if is_dir {
                        fb.dir = fb.dir.join(&name);
                    } else if readable {
                        chosen = Some(fb.dir.join(&name));
                    }
                }
            }
        });
    });
    if !open || chosen.is_some() {
        fb.open = false;
    }
    chosen
}
