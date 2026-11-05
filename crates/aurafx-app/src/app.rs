//! Application state, layout and top-level chrome (menu bar, toolbar, status bar).

use crate::assistant::Assistant;
use crate::console::Console;
use crate::palette::Palette;
use crate::settings::Settings;
use crate::viewport::Viewport;
use crate::worker::{spawn_pipeline_worker, PythonWorker};
use aurafx_filters::Association;
use aurafx_pipeline::session::CameraRequest;
use aurafx_pipeline::{ColorBy, NodeId, Representation, Session, SharedSession};
use egui::{Color32, RichText};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Pane {
    Pipeline,
    Properties,
    Viewport,
    Python,
    Assistant,
    Log,
    Spreadsheet,
    Information,
    Jobs,
    Charts,
    /// An additional 3D view.
    View(u32),
}

impl Pane {
    pub const ALL: [Pane; 10] = [
        Pane::Pipeline,
        Pane::Properties,
        Pane::Viewport,
        Pane::Python,
        Pane::Assistant,
        Pane::Log,
        Pane::Spreadsheet,
        Pane::Information,
        Pane::Jobs,
        Pane::Charts,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Pane::Pipeline => "Pipeline",
            Pane::Properties => "Properties",
            Pane::Viewport => "3D View",
            Pane::Python => "Python",
            Pane::Assistant => "Assistant",
            Pane::Log => "Log",
            Pane::Spreadsheet => "Spreadsheet",
            Pane::Information => "Information",
            Pane::Jobs => "Simulations",
            Pane::Charts => "Charts",
            Pane::View(_) => "3D View",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Pane::Pipeline => crate::icons::TREE_STRUCTURE,
            Pane::Properties => crate::icons::SLIDERS,
            Pane::Viewport => crate::icons::CUBE,
            Pane::Python => crate::icons::TERMINAL_WINDOW,
            Pane::Assistant => crate::icons::SPARKLE,
            Pane::Log => crate::icons::LIST,
            Pane::Spreadsheet => crate::icons::TABLE,
            Pane::Information => crate::icons::INFO,
            Pane::Jobs => crate::icons::ATOM,
            Pane::Charts => crate::icons::WAVE_SINE,
            Pane::View(_) => crate::icons::CUBE,
        }
    }
}

/// Everything except the dock tree (the tree borrows this mutably while drawing).
pub struct State {
    pub session: SharedSession,
    pub settings: Settings,
    pub python: PythonWorker,
    pub console: Console,
    pub assistant: Assistant,
    pub viewport: Viewport,
    pub palette: Palette,
    pub status: String,
    pub show_settings: bool,
    pub show_about: bool,
    pub show_shortcuts: bool,
    pub rename_target: Option<(NodeId, String)>,
    pub playing: bool,
    pub play_fps: f32,
    last_play_step: f64,
    pub pending_open: Vec<PathBuf>,
    pub spreadsheet_assoc: Association,
    pub spreadsheet_page: usize,
    pub spreadsheet_only_selected: bool,
    /// Nodes selected together in the Pipeline browser (Ctrl+click), in click order. Empty
    /// when only the active node is selected. Multi-input filters take these as inputs.
    pub pipeline_selection: Vec<aurafx_pipeline::NodeId>,
    pub toasts: Vec<(String, f64, bool)>,
    pub jobs_panel: crate::jobs_panel::JobsPanel,
    pub charts: crate::charts::ChartsState,
    pub anim_dialog: crate::animation::AnimDialog,
    pub cmap_editor: crate::colormap_editor::ColormapEditor,
    pub plugins: crate::plugins_ui::PluginsWindow,
    pub server: crate::server_ui::ServerWindow,
    pub anim_job: Option<crate::animation::AnimJob>,
    pub anim_done_tx: std::sync::mpsc::Sender<Result<PathBuf, String>>,
    pub anim_done_rx: std::sync::mpsc::Receiver<Result<PathBuf, String>>,
    pub file_browser: crate::dialogs::FileBrowser,
    /// The .afx file the session was last saved to / loaded from.
    pub state_path: Option<PathBuf>,
    /// Id of the 3D view `viewport` currently holds (0 = main view).
    pub active_view: u32,
    /// Inactive 3D views.
    pub views: std::collections::BTreeMap<u32, crate::viewport::Viewport>,
    /// Visible nodes of each inactive view.
    pub view_visibility: std::collections::HashMap<u32, std::collections::HashSet<aurafx_pipeline::NodeId>>,
    pub next_view_id: u32,
    pub pending_activate: Option<u32>,
    pub camera_match_from: Option<u32>,
    /// Frame the visible data once this view becomes active (views made for a single object).
    pub fit_view_on_activate: Option<u32>,
    /// Panes currently in the layout (refreshed every frame).
    pub open_panes: Vec<Pane>,
    /// Layout changes requested by menus, applied after the menu bar is drawn.
    pub layout_actions: Vec<LayoutAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutPreset {
    Full,
    /// Everything in one tab strip (phones, small windows).
    Compact,
    Visualize,
    Analyze,
    Script,
    Simulate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutAction {
    Show(Pane),
    Hide(Pane),
    Preset(LayoutPreset),
    /// Add a 3D view beside (or below, when `vertical`) view `from`.
    SplitView {
        from: u32,
        vertical: bool,
        /// Show only this object in the new view (otherwise copy the visible objects of `from`);
        /// the flag keeps it visible in the other views instead of moving it.
        only: Option<(NodeId, bool)>,
    },
}

/// Panes that share a tab group in the default layout; used to re-open a closed pane in a sensible place.
const PANE_GROUPS: &[&[Pane]] = &[
    &[Pane::Pipeline],
    &[Pane::Properties, Pane::Information],
    &[Pane::Viewport],
    &[Pane::Python, Pane::Charts, Pane::Spreadsheet, Pane::Log],
    &[Pane::Assistant, Pane::Jobs],
];

/// Put `new` next to `anchor` in direction `dir` (before or after), giving it `share` of the space.
fn split_beside(tree: &mut egui_tiles::Tree<Pane>, anchor: egui_tiles::TileId, new: egui_tiles::TileId, dir: egui_tiles::LinearDir, before: bool, share: f32) {
    let parent = tree.tiles.parent_of(anchor);
    if let Some(pid) = parent {
        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(lin))) = tree.tiles.get_mut(pid) {
            if lin.dir == dir {
                let idx = lin.children.iter().position(|c| *c == anchor).unwrap_or(0);
                lin.children.insert(if before { idx } else { idx + 1 }, new);
                let a = lin.shares[anchor];
                lin.shares.set_share(anchor, a * (1.0 - share));
                lin.shares.set_share(new, a * share);
                return;
            }
        }
    }
    let kids = if before { vec![new, anchor] } else { vec![anchor, new] };
    let mut lin = egui_tiles::Linear::new(dir, kids);
    lin.shares.set_share(anchor, 1.0 - share);
    lin.shares.set_share(new, share);
    let lin_id = tree.tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(lin)));
    match parent {
        Some(pid) => {
            if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(pid) {
                // Keeps position and share of the replaced child.
                let _ = c.replace_child(anchor, lin_id);
            }
        }
        None => tree.root = Some(lin_id),
    }
}

pub fn show_pane(tree: &mut egui_tiles::Tree<Pane>, pane: Pane) {
    use egui_tiles::LinearDir::{Horizontal, Vertical};
    if tree.tiles.find_pane(&pane).is_none() {
        let group = PANE_GROUPS.iter().find(|g| g.contains(&pane)).copied().unwrap_or(&[]);
        let neighbor = group.iter().filter(|p| **p != pane).find_map(|p| tree.tiles.find_pane(p));
        let id = tree.tiles.insert_pane(pane);
        if let Some(par) = neighbor.and_then(|n| tree.tiles.parent_of(n)) {
            if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(par) {
                c.add_child(id);
            }
        } else {
            let tabs = tree.tiles.insert_tab_tile(vec![id]);
            // Anchor on the 3D view's tab group (or the root when the view is closed).
            let anchor = tree.tiles.find_pane(&Pane::Viewport).and_then(|v| tree.tiles.parent_of(v)).or(tree.root());
            match anchor {
                Some(a) => match pane {
                    Pane::Python | Pane::Charts | Pane::Spreadsheet | Pane::Log => split_beside(tree, a, tabs, Vertical, false, 0.3),
                    Pane::Assistant | Pane::Jobs => split_beside(tree, a, tabs, Horizontal, false, 0.28),
                    Pane::Pipeline | Pane::Properties | Pane::Information => {
                        // Join the left column if the other left-side group is open.
                        let other = [Pane::Pipeline, Pane::Properties, Pane::Information]
                            .iter()
                            .filter(|p| **p != pane)
                            .find_map(|p| tree.tiles.find_pane(p))
                            .and_then(|t| tree.tiles.parent_of(t));
                        match other {
                            Some(o) => split_beside(tree, o, tabs, Vertical, pane == Pane::Pipeline, 0.4),
                            None => split_beside(tree, a, tabs, Horizontal, true, 0.22),
                        }
                    }
                    Pane::Viewport | Pane::View(_) => split_beside(tree, a, tabs, Horizontal, false, 0.5),
                },
                None => tree.root = Some(tabs),
            }
        }
    }
    tree.make_active(|_, t| matches!(t, egui_tiles::Tile::Pane(p) if *p == pane));
}

pub fn hide_pane(tree: &mut egui_tiles::Tree<Pane>, pane: Pane) {
    if let Some(id) = tree.tiles.find_pane(&pane) {
        tree.remove_recursively(id);
    }
}

pub fn compact_tree() -> egui_tiles::Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();
    let panes: Vec<_> =
        [Pane::Viewport, Pane::Pipeline, Pane::Properties, Pane::Charts, Pane::Spreadsheet, Pane::Assistant, Pane::Jobs, Pane::Information, Pane::Log]
            .into_iter()
            .filter(|p| *p != Pane::Python || cfg!(feature = "python"))
            .map(|p| tiles.insert_pane(p))
            .collect();
    let root = tiles.insert_tab_tile(panes);
    #[allow(deprecated)]
    let id = egui::Id::new("aurafx_layout");
    egui_tiles::Tree::new(id, root, tiles)
}

pub fn apply_preset(tree: &mut egui_tiles::Tree<Pane>, preset: LayoutPreset) {
    if preset == LayoutPreset::Compact {
        *tree = compact_tree();
        return;
    }
    *tree = default_tree();
    let (hide, front): (&[Pane], &[Pane]) = match preset {
        LayoutPreset::Full | LayoutPreset::Compact => (&[], &[]),
        LayoutPreset::Visualize => (&[Pane::Python, Pane::Charts, Pane::Spreadsheet, Pane::Log, Pane::Assistant, Pane::Jobs, Pane::Information], &[]),
        LayoutPreset::Analyze => (&[Pane::Jobs, Pane::Log], &[Pane::Charts, Pane::Information]),
        LayoutPreset::Script => (&[Pane::Jobs, Pane::Spreadsheet, Pane::Charts], &[Pane::Python, Pane::Assistant]),
        LayoutPreset::Simulate => (&[Pane::Spreadsheet], &[Pane::Jobs, Pane::Log]),
    };
    for p in hide {
        hide_pane(tree, *p);
    }
    for p in front {
        show_pane(tree, *p);
    }
}

pub struct AuraApp {
    pub state: State,
    pub tree: egui_tiles::Tree<Pane>,
    layout_checked: bool,
}

pub fn default_tree() -> egui_tiles::Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();
    let pipeline = tiles.insert_pane(Pane::Pipeline);
    let props = tiles.insert_pane(Pane::Properties);
    let info = tiles.insert_pane(Pane::Information);
    let viewport = tiles.insert_pane(Pane::Viewport);
    let python = tiles.insert_pane(Pane::Python);
    let log = tiles.insert_pane(Pane::Log);
    let sheet = tiles.insert_pane(Pane::Spreadsheet);
    let charts = tiles.insert_pane(Pane::Charts);
    let assistant = tiles.insert_pane(Pane::Assistant);
    let jobs = tiles.insert_pane(Pane::Jobs);

    let props_tabs = tiles.insert_tab_tile(vec![props, info]);
    let mut left = egui_tiles::Linear::new(egui_tiles::LinearDir::Vertical, vec![pipeline, props_tabs]);
    left.shares.set_share(pipeline, 0.35);
    left.shares.set_share(props_tabs, 0.65);
    let left = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(left)));

    let bottom = tiles.insert_tab_tile(vec![python, charts, sheet, log]);
    let mut center = egui_tiles::Linear::new(egui_tiles::LinearDir::Vertical, vec![viewport, bottom]);
    center.shares.set_share(viewport, 0.72);
    center.shares.set_share(bottom, 0.28);
    let center = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(center)));

    let right = tiles.insert_tab_tile(vec![assistant, jobs]);
    let mut root = egui_tiles::Linear::new(egui_tiles::LinearDir::Horizontal, vec![left, center, right]);
    root.shares.set_share(left, 0.2);
    root.shares.set_share(center, 0.56);
    root.shares.set_share(right, 0.24);
    let root = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(root)));
    #[allow(deprecated)]
    let id = egui::Id::new("aurafx_layout");
    egui_tiles::Tree::new(id, root, tiles)
}

impl AuraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>, script: Option<PathBuf>) -> Self {
        let settings = Settings::load();
        aurafx_jobs::set_hosts(settings.hosts.clone());
        for m in &settings.user_colormaps {
            aurafx_pipeline::colormap::register_user(m.clone());
        }
        crate::theme::install_fonts(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx, settings.theme, settings.ui_scale);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let session = Session::shared();
        spawn_pipeline_worker(session.clone(), cc.egui_ctx.clone());
        let python = PythonWorker::spawn(session.clone(), cc.egui_ctx.clone());
        let viewport = Viewport::new(cc, settings.background);
        let tree = cc.storage.and_then(|s| s.get_string("layout_v4")).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(default_tree);
        let assistant = Assistant::new(&settings);
        let (anim_tx, anim_rx) = std::sync::mpsc::channel();
        let mut state = State {
            session,
            settings,
            python,
            console: Console::default(),
            assistant,
            viewport,
            palette: Palette::default(),
            status: "Ready. Open a file (Ctrl+O), add a source, or press Ctrl+K.".into(),
            show_settings: false,
            show_about: false,
            show_shortcuts: false,
            rename_target: None,
            playing: false,
            play_fps: 10.0,
            last_play_step: 0.0,
            pending_open: files,
            spreadsheet_assoc: Association::Points,
            spreadsheet_page: 0,
            spreadsheet_only_selected: true,
            pipeline_selection: Vec::new(),
            toasts: Vec::new(),
            jobs_panel: crate::jobs_panel::JobsPanel::default(),
            charts: Default::default(),
            file_browser: Default::default(),
            state_path: None,
            anim_dialog: Default::default(),
            cmap_editor: Default::default(),
            plugins: Default::default(),
            server: Default::default(),
            anim_job: None,
            anim_done_tx: anim_tx,
            anim_done_rx: anim_rx,
            active_view: 0,
            views: Default::default(),
            view_visibility: Default::default(),
            next_view_id: 1,
            pending_activate: None,
            camera_match_from: None,
            fit_view_on_activate: None,
            open_panes: Pane::ALL.to_vec(),
            layout_actions: Vec::new(),
        };
        // Plugins first, so startup scripts and restored sessions can use their filters.
        state.sync_plugins(false);
        if let Some(p) = script {
            match std::fs::read_to_string(&p) {
                Ok(code) => state.python.run(code, "startup"),
                Err(e) => tracing::error!("cannot read script {}: {e}", p.display()),
            }
        }
        let mut app = Self { state, tree, layout_checked: false };
        let max_id =
            app.tree.tiles.iter().filter_map(|(_, t)| if let egui_tiles::Tile::Pane(Pane::View(id)) = t { Some(*id) } else { None }).max().unwrap_or(0);
        app.state.next_view_id = max_id + 1;
        app
    }
}

impl State {
    pub fn toast(&mut self, msg: impl Into<String>, error: bool) {
        let now = self.now();
        self.toasts.push((msg.into(), now, error));
    }

    fn now(&self) -> f64 {
        self.viewport.time_seconds()
    }

    pub fn open_file(&mut self, path: PathBuf) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if path.extension().map(|e| e.eq_ignore_ascii_case("afx")).unwrap_or(false) {
            self.load_state(path);
            return;
        }
        let r = self.session.lock().open_file(&path);
        match r {
            Ok(_) => {
                self.settings.push_recent(path.clone());
                self.settings.save();
                self.status = format!("Opened {}", path.display());
            }
            Err(e) => {
                self.toast(format!("Could not open {}: {e:#}", path.display()), true);
            }
        }
    }

    pub fn undo(&mut self) {
        match self.session.lock().undo() {
            Some(l) => self.status = format!("Undid: {l}"),
            None => self.status = "Nothing to undo".into(),
        }
    }

    pub fn redo(&mut self) {
        match self.session.lock().redo() {
            Some(l) => self.status = format!("Redid: {l}"),
            None => self.status = "Nothing to redo".into(),
        }
    }

    pub fn camera_state(&self) -> aurafx_pipeline::CameraState {
        let c = &self.viewport.camera;
        aurafx_pipeline::CameraState {
            eye: c.eye.to_array(),
            target: c.target.to_array(),
            up: c.up.to_array(),
            fov_y_deg: c.fov_y_deg,
            orthographic: c.orthographic,
            ortho_height: c.ortho_height,
        }
    }

    pub fn save_state_to(&mut self, path: PathBuf) {
        let cam = self.camera_state();
        let r = self.session.lock().save_state(&path, Some(cam));
        match r {
            Ok(()) => {
                self.state_path = Some(path.clone());
                self.settings.push_recent(path.clone());
                self.settings.save();
                self.toast(format!("Saved {}", path.display()), false);
            }
            Err(e) => self.toast(format!("Could not save state: {e:#}"), true),
        }
    }

    /// Save to the current state file, or ask for one.
    pub fn save_state_dialog(&mut self, save_as: bool) {
        if let (false, Some(p)) = (save_as, self.state_path.clone()) {
            self.save_state_to(p);
            return;
        }
        if let Some(p) = crate::dialogs::FileDialog::new().add_filter("auraFX state", &["afx"]).set_file_name("session.afx").save_file() {
            let p = if p.extension().is_none() { p.with_extension("afx") } else { p };
            self.save_state_to(p);
        }
    }

    pub fn load_state(&mut self, path: PathBuf) {
        let r = self.session.lock().load_state(&path);
        match r {
            Ok(_) => {
                self.state_path = Some(path.clone());
                self.settings.push_recent(path.clone());
                self.settings.save();
                self.status = format!("Loaded state {}", path.display());
            }
            Err(e) => self.toast(format!("State loaded with problems: {e:#}"), true),
        }
    }

    /// Write the active node's data to a file chosen by the user.
    pub fn save_data_dialog(&mut self) {
        let (name, ds) = {
            let s = self.session.lock();
            match s.active_node() {
                Some(n) => (n.name.clone(), n.output.clone()),
                None => (String::new(), None),
            }
        };
        let Some(ds) = ds else {
            self.toast("Select a node with data to save", true);
            return;
        };
        let formats = aurafx_io::writers::writable_formats(&ds);
        let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let mut dlg = crate::dialogs::FileDialog::new().set_file_name(format!("{stem}.{}", formats.first().map(|f| f.0).unwrap_or("csv")));
        for (e, label) in &formats {
            dlg = dlg.add_filter(*label, &[*e]);
        }
        if let Some(p) = dlg.save_file() {
            let p = if p.extension().is_none() { p.with_extension(formats.first().map(|f| f.0).unwrap_or("csv")) } else { p };
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let multi = self.session.lock().times.len() > 1;
            let r = if multi && matches!(ext.as_str(), "exo" | "e" | "ex2" | "g") {
                // Exodus holds every time step in one file.
                let mut s = self.session.lock();
                match s.active {
                    Some(id) => s.save_exodus_all_times(id, &p).map(|n| format!("Saved {} ({n} time steps)", p.display())),
                    None => Err(anyhow::anyhow!("nothing selected")),
                }
            } else {
                aurafx_io::writers::write_file(&ds, &p).map(|_| format!("Saved {}", p.display()))
            };
            match r {
                Ok(msg) => self.toast(msg, false),
                Err(e) => self.toast(format!("{e:#}"), true),
            }
        }
    }

    /// Write an embedded sample into the documents folder and open it.
    pub fn open_sample(&mut self, name: &str) {
        let bytes: &[u8] = match name {
            "disk_out_ref.ex2" => include_bytes!("../../../testdata/disk_out_ref.ex2"),
            "lagrange_annulus.vtu" => include_bytes!("../../../testdata/lagrange_annulus.vtu"),
            _ => return,
        };
        let dir = crate::dialogs::documents_dir().join("samples");
        let path = dir.join(name);
        if !path.exists() {
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes)) {
                self.toast(format!("Cannot write sample: {e}"), true);
                return;
            }
        }
        self.open_file(path);
    }

    pub fn last_session_path() -> PathBuf {
        aurafx_jobs::cache_dir().join("last_session.afx")
    }

    pub fn open_file_dialog(&mut self) {
        if !crate::dialogs::NATIVE {
            self.file_browser.open = true;
            return;
        }
        let mut exts: Vec<String> = aurafx_io::all_extensions();
        exts.push("afx".into());
        let refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        let mut dlg = crate::dialogs::FileDialog::new().add_filter("All supported", &refs);
        for (e, label) in aurafx_io::supported_formats() {
            dlg = dlg.add_filter(label, e);
        }
        for r in aurafx_io::plugin_readers() {
            let e: Vec<&str> = r.extensions.iter().map(|s| s.as_str()).collect();
            dlg = dlg.add_filter(&r.label, &e);
        }
        dlg = dlg.add_filter("auraFX state", &["afx"]);
        dlg = dlg.add_filter("All files", &["*"]);
        if let Some(last) = self.settings.recent_files.first().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(last);
        }
        if let Some(paths) = dlg.pick_files() {
            for p in paths {
                self.open_file(p);
            }
        }
    }

    pub fn add_filter(&mut self, id: &str) {
        let mut s = self.session.lock();
        let def = aurafx_filters::find(id);
        self.pipeline_selection.retain(|n| s.nodes.contains_key(n));
        let multi = def.as_ref().is_some_and(|d| d.multi.is_some()) && self.pipeline_selection.len() >= 2;
        let r = match (def.as_ref().map(|d| d.is_source), s.active) {
            (Some(true), _) => s.add_source(id),
            // Several nodes selected in the Pipeline browser: they become the filter's inputs.
            (Some(false), _) if multi => {
                let inputs = std::mem::take(&mut self.pipeline_selection);
                s.add_filter_multi(id, &inputs)
            }
            (Some(false), Some(a)) => s.add_filter(id, a),
            (Some(false), None) => Err(anyhow::anyhow!("Select a dataset first (or open a file) to apply {}", def.as_ref().unwrap().label)),
            (None, _) => Err(anyhow::anyhow!("Unknown filter {id}")),
        };
        drop(s);
        match r {
            Ok(_) => self.status = format!("Added {}", def.map(|d| d.label).unwrap_or(id)),
            Err(e) => self.toast(format!("{e:#}"), true),
        }
    }

    pub fn save_screenshot_dialog(&mut self) {
        if let Some(path) = crate::dialogs::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("aurafx.png").save_file() {
            self.viewport.request_screenshot(path, None);
        }
    }

    pub fn run_python(&mut self, code: &str, origin: &str) {
        self.python.run(code, origin);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let (undo, redo, save_state) = ctx.input_mut(|i| {
            (
                !typing && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                !typing
                    && (i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)
                        || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
            )
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if save_state {
            self.save_state_dialog(false);
        }
        let (open, palette, screenshot, reset_cam, del, play, next, prev, settings) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::O),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::K) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Space),
                i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S),
                !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::R),
                !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Delete),
                !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Period),
                !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Comma),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma),
            )
        });
        if open {
            self.open_file_dialog();
        }
        if palette {
            self.palette.open = !self.palette.open;
        }
        if screenshot {
            self.save_screenshot_dialog();
        }
        if reset_cam {
            self.session.lock().camera_request = Some(CameraRequest::ResetToBounds);
        }
        if del {
            let active = self.session.lock().active;
            if let Some(a) = active {
                self.session.lock().remove_node(a);
            }
        }
        if play {
            self.playing = !self.playing;
        }
        if next {
            self.session.lock().step_time(1);
        }
        if prev {
            self.session.lock().step_time(-1);
        }
        if settings {
            self.show_settings = true;
        }
        // Dropped files.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for p in dropped {
            self.open_file(p);
        }
    }

    fn tick_playback(&mut self, ctx: &egui::Context) {
        if !self.playing {
            return;
        }
        let now = self.now();
        if now - self.last_play_step >= 1.0 / self.play_fps.max(0.1) as f64 {
            let mut s = self.session.lock();
            if s.times.len() < 2 {
                self.playing = false;
            } else if !s.is_busy() {
                s.step_time(1);
                self.last_play_step = now;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.add(egui::Image::new(egui::include_image!("../../../assets/icons/aurafx-32.png")).fit_to_exact_size(egui::vec2(20.0, 20.0)));
            ui.menu_button("File", |ui| {
                if ui.button("Open…    Ctrl+O").clicked() {
                    ui.close();
                    self.open_file_dialog();
                }
                if ui
                    .button(format!("{} Open on Server…", crate::icons::CLOUD))
                    .on_hover_text("Read and filter a file on another machine (over SSH); only what you view comes back")
                    .clicked()
                {
                    ui.close();
                    self.server.open = true;
                }
                let has_data = self.session.lock().active_node().map(|n| n.output.is_some()).unwrap_or(false);
                if ui.add_enabled(has_data, egui::Button::new("Save Data…")).on_hover_text("Write the selected node's data (VTK, CSV, STL, PLY)").clicked() {
                    ui.close();
                    self.save_data_dialog();
                }
                ui.separator();
                if ui.button("Save State    Ctrl+S").on_hover_text("Save the pipeline, display settings and camera to an .afx file").clicked() {
                    ui.close();
                    self.save_state_dialog(false);
                }
                if ui.button("Save State As…").clicked() {
                    ui.close();
                    self.save_state_dialog(true);
                }
                let last = Self::last_session_path();
                if ui
                    .add_enabled(last.exists(), egui::Button::new("Restore Last Session"))
                    .on_hover_text("Reopen the pipeline you had when auraFX was last closed")
                    .clicked()
                {
                    ui.close();
                    self.load_state(last);
                }
                ui.menu_button("Open Sample Data", |ui| {
                    if ui.button("Heated disk in a flow (Exodus)").on_hover_text("ParaView's disk_out_ref: temperature, pressure, velocity").clicked() {
                        ui.close();
                        self.open_sample("disk_out_ref.ex2");
                    }
                    if ui
                        .button("Curved quarter cylinder (Lagrange cells)")
                        .on_hover_text("Four cubic Lagrange hexahedra; try Display › Curved cells and Tessellate")
                        .clicked()
                    {
                        ui.close();
                        self.open_sample("lagrange_annulus.vtu");
                    }
                    if ui.button("Wavelet volume (generated)").clicked() {
                        ui.close();
                        self.add_filter("wavelet");
                    }
                    if ui.button("Sphere (generated)").clicked() {
                        ui.close();
                        self.add_filter("sphere");
                    }
                });
                ui.menu_button("Open Recent", |ui| {
                    if self.settings.recent_files.is_empty() {
                        ui.label("No recent files");
                    }
                    let recent = self.settings.recent_files.clone();
                    for p in recent {
                        if ui.button(p.display().to_string()).clicked() {
                            ui.close();
                            self.open_file(p);
                        }
                    }
                    ui.separator();
                    if ui.button("Clear list").clicked() {
                        self.settings.recent_files.clear();
                        self.settings.save();
                    }
                });
                ui.separator();
                if ui.button("Save Screenshot…    Ctrl+Shift+S").clicked() {
                    ui.close();
                    self.save_screenshot_dialog();
                }
                if ui.button("Animation…").on_hover_text("Time steps, a camera orbit or keyframes, saved as GIF, MP4 or PNG frames").clicked() {
                    ui.close();
                    self.anim_dialog.open = true;
                }
                if ui.button("Export Python trace…").clicked() {
                    ui.close();
                    let trace = self.session.lock().trace.join("\n");
                    if let Some(p) = crate::dialogs::FileDialog::new().add_filter("Python", &["py"]).set_file_name("aurafx_trace.py").save_file() {
                        let _ = std::fs::write(p, format!("import aurafx as afx\n\n{trace}\n"));
                    }
                }
                ui.separator();
                if ui.button("Settings…    Ctrl+,").clicked() {
                    ui.close();
                    self.show_settings = true;
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let (ul, rl) = {
                    let s = self.session.lock();
                    (s.undo_label().map(|l| l.to_string()), s.redo_label().map(|l| l.to_string()))
                };
                if ui.add_enabled(ul.is_some(), egui::Button::new(format!("Undo {}    Ctrl+Z", ul.clone().unwrap_or_default()))).clicked() {
                    ui.close();
                    self.undo();
                }
                if ui.add_enabled(rl.is_some(), egui::Button::new(format!("Redo {}    Ctrl+Shift+Z", rl.clone().unwrap_or_default()))).clicked() {
                    ui.close();
                    self.redo();
                }
                ui.separator();
                let active = self.session.lock().active;
                if ui.add_enabled(active.is_some(), egui::Button::new("Delete active node    Del")).clicked() {
                    ui.close();
                    if let Some(a) = active {
                        self.session.lock().remove_node(a);
                    }
                }
                if ui.add_enabled(active.is_some(), egui::Button::new("Rename active node")).clicked() {
                    ui.close();
                    if let Some(a) = active {
                        let name = self.session.lock().nodes.get(&a).map(|n| n.name.clone()).unwrap_or_default();
                        self.rename_target = Some((a, name));
                    }
                }
                ui.separator();
                if ui.button("Plugins…").on_hover_text("Python plugins that add filters, sources and readers").clicked() {
                    ui.close();
                    self.plugins.open = true;
                }
                if ui.button("Settings…    Ctrl+,").clicked() {
                    ui.close();
                    self.show_settings = true;
                }
            });
            ui.menu_button("Sources", |ui| {
                for f in aurafx_filters::registry().iter().filter(|f| f.is_source) {
                    if ui.button(f.label).on_hover_text(f.description).clicked() {
                        ui.close();
                        self.add_filter(f.id);
                    }
                }
            });
            ui.menu_button("Filters", |ui| {
                let mut cats: Vec<&str> = aurafx_filters::registry().iter().filter(|f| !f.is_source).map(|f| f.category).collect();
                cats.dedup();
                for cat in cats {
                    ui.menu_button(cat, |ui| {
                        for f in aurafx_filters::registry().iter().filter(|f| !f.is_source && f.category == cat) {
                            if ui.button(f.label).on_hover_text(f.description).clicked() {
                                ui.close();
                                self.add_filter(f.id);
                            }
                        }
                        if cat == "Temporal" {
                            ui.separator();
                            if ui.button("Plot Over Time…").on_hover_text("Open the Charts panel's over-time tools").clicked() {
                                ui.close();
                                self.layout_actions.push(LayoutAction::Show(Pane::Charts));
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button("Search…    Ctrl+K").clicked() {
                    ui.close();
                    self.palette.open = true;
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Reset camera    R").clicked() {
                    ui.close();
                    self.session.lock().camera_request = Some(CameraRequest::ResetToBounds);
                }
                for (label, dir, up) in [
                    ("+X", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                    ("-X", [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                    ("+Y", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
                    ("-Y", [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
                    ("+Z", [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
                    ("-Z", [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
                    ("Isometric", [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]),
                ] {
                    if ui.button(format!("Look from {label}")).clicked() {
                        ui.close();
                        self.session.lock().camera_request = Some(CameraRequest::View { direction: dir, up });
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.viewport.camera.orthographic, "Orthographic projection");
                ui.checkbox(&mut self.settings.show_orientation_axes, "Orientation axes");
                if ui.button(format!("{} Color Map Editor…", crate::icons::PENCIL_SIMPLE)).clicked() {
                    ui.close();
                    self.cmap_editor.open = true;
                }
                {
                    let mut s = self.session.lock();
                    let mut t = s.show_time_label;
                    if ui.checkbox(&mut t, "Time label").changed() {
                        s.show_time_label = t;
                    }
                }
                ui.menu_button("Annotations", |ui| {
                    let id = ui.make_persistent_id("new_note");
                    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut text).hint_text("text").desired_width(160.0));
                    });
                    ui.data_mut(|d| d.insert_temp(id, text.clone()));
                    ui.horizontal_wrapped(|ui| {
                        for corner in ["upper left", "upper right", "lower right", "upper center", "lower center"] {
                            if ui.small_button(corner).clicked() && !text.trim().is_empty() {
                                self.session.lock().annotations.push(aurafx_pipeline::Annotation { text: text.clone(), corner: corner.into(), size: 16.0 });
                                ui.data_mut(|d| d.remove::<String>(id));
                            }
                        }
                    });
                    let n = self.session.lock().annotations.len();
                    if ui.add_enabled(n > 0, egui::Button::new(format!("Clear {n} annotation(s)"))).clicked() {
                        self.session.lock().annotations.clear();
                    }
                });
                ui.separator();
                ui.label("Theme");
                for m in crate::theme::ThemeMode::ALL {
                    if ui.radio(self.settings.theme == m, m.label()).clicked() {
                        self.settings.theme = m;
                        self.settings.dark_theme = m != crate::theme::ThemeMode::Light;
                        crate::theme::apply(ui.ctx(), m, self.settings.ui_scale);
                        self.settings.save();
                    }
                }
                ui.separator();
                let dark = ui.ctx().theme() == egui::Theme::Dark;
                ui.horizontal(|ui| {
                    ui.label(if dark { "3D background (dark theme)" } else { "3D background (light theme)" });
                    let mut c = self.settings.background_for(dark);
                    if ui.color_edit_button_rgb(&mut c).changed() {
                        if dark {
                            self.settings.background = c;
                        } else {
                            self.settings.background_light = c;
                        }
                        self.settings.save();
                    }
                    if ui.small_button("Default").clicked() {
                        let d = Settings::default();
                        if dark {
                            self.settings.background = d.background;
                        } else {
                            self.settings.background_light = d.background_light;
                        }
                        self.settings.save();
                    }
                });
            });
            ui.menu_button("Windows", |ui| {
                for p in Pane::ALL {
                    let mut open = self.open_panes.contains(&p);
                    if ui.checkbox(&mut open, format!("{} {}", p.icon(), p.title())).clicked() {
                        self.layout_actions.push(if open { LayoutAction::Show(p) } else { LayoutAction::Hide(p) });
                    }
                }
                ui.separator();
                ui.label(RichText::new("3D views").weak());
                if ui.button(format!("{} New view side by side", crate::icons::COLUMNS)).clicked() {
                    self.layout_actions.push(LayoutAction::SplitView { from: self.active_view, vertical: false, only: None });
                    ui.close();
                }
                if ui.button(format!("{} New view above / below", crate::icons::ROWS)).clicked() {
                    self.layout_actions.push(LayoutAction::SplitView { from: self.active_view, vertical: true, only: None });
                    ui.close();
                }
                let all_linked = self.viewport.linked && self.views.values().all(|v| v.linked);
                let mut linked = all_linked;
                if ui
                    .add_enabled(!self.views.is_empty(), egui::Checkbox::new(&mut linked, format!("{} Link all cameras", crate::icons::LINK)))
                    .on_hover_text("Rotating, zooming or choosing +X/+Y/+Z/Iso in one view applies to all linked views")
                    .changed()
                {
                    self.link_all_views(linked);
                }
                if ui.add_enabled(!self.views.is_empty(), egui::Button::new("Match all views to the active camera")).clicked() {
                    self.camera_match_from = Some(self.active_view);
                    ui.close();
                }
                ui.separator();
                ui.label(RichText::new("Layouts").weak());
                for (label, preset, tip) in [
                    ("Full workbench", LayoutPreset::Full, "Every panel"),
                    ("Visualize", LayoutPreset::Visualize, "Pipeline, properties and a large 3D view"),
                    ("Analyze", LayoutPreset::Analyze, "3D view with charts, spreadsheet and statistics"),
                    ("Script", LayoutPreset::Script, "Python console and assistant beside the 3D view"),
                    ("Simulate", LayoutPreset::Simulate, "Simulation runner, logs and GPUs"),
                    ("Compact (tabs only)", LayoutPreset::Compact, "Every panel as a tab: best on small screens"),
                ] {
                    if ui.button(label).on_hover_text(tip).clicked() {
                        self.layout_actions.push(LayoutAction::Preset(preset));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Reset layout").clicked() {
                    self.layout_actions.push(LayoutAction::Preset(LayoutPreset::Full));
                    ui.close();
                }
            });
            ui.menu_button("Simulate", |ui| {
                if ui.button("Run a simulation…").clicked() {
                    ui.close();
                    self.status = "Use the Simulations panel to configure and run a job.".into();
                }
                ui.label(RichText::new("Templates: LAMMPS, MOOSE, OpenFOAM, Python, custom").weak());
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    ui.close();
                    self.show_shortcuts = true;
                }
                if ui.button("Python API cheat sheet").clicked() {
                    ui.close();
                    self.console.show_help = true;
                }
                if ui.button("About auraFX").clicked() {
                    ui.close();
                    self.show_about = true;
                }
            });
            if ui.available_width() < 260.0 {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("{} Ask AI", crate::icons::SPARKLE)).clicked() {
                    self.assistant.focus_input = true;
                }
                if ui.button(format!("{} Search  Ctrl+K", crate::icons::MAGNIFYING_GLASS)).clicked() {
                    self.palette.open = true;
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("{} Open", crate::icons::FOLDER_OPEN)).on_hover_text("Open data file (Ctrl+O)").clicked() {
                self.open_file_dialog();
            }
            ui.separator();
            for (label, id, tip) in [
                ("Slice", "slice", "Cut with a plane"),
                ("Clip", "clip", "Remove part of the data"),
                ("Contour", "contour", "Isosurfaces"),
                ("Threshold", "threshold", "Keep a value range"),
                ("Calculator", "calculator", "Compute arrays"),
                ("Glyph", "glyph", "Arrows at points"),
                ("Warp", "warp_by_vector", "Deform by vectors"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.add_filter(id);
                }
            }
            ui.separator();
            if ui.button(format!("{} Reset", crate::icons::ARROWS_COUNTER_CLOCKWISE)).on_hover_text("Reset camera (R)").clicked() {
                self.session.lock().camera_request = Some(CameraRequest::ResetToBounds);
            }
            for (label, dir, up) in [
                ("+X", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                ("+Y", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
                ("+Z", [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
                ("Iso", [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]),
            ] {
                if ui.small_button(label).clicked() {
                    self.session.lock().camera_request = Some(CameraRequest::View { direction: dir, up });
                }
            }
            ui.separator();
            // Active node display quick controls.
            let (active, repr, color_by, arrays, cmap) = {
                let s = self.session.lock();
                match s.active_node() {
                    Some(n) => {
                        let mut arrays: Vec<(Association, String, usize)> = Vec::new();
                        if let Some(i) = &n.info {
                            arrays.extend(i.point_arrays.iter().map(|a| (Association::Points, a.name.clone(), a.ncomp)));
                            arrays.extend(i.cell_arrays.iter().map(|a| (Association::Cells, a.name.clone(), a.ncomp)));
                        }
                        (Some(n.id), n.display.representation, n.display.color_by.clone(), arrays, n.display.colormap.clone())
                    }
                    None => (None, Representation::Surface, None, vec![], String::new()),
                }
            };
            if let Some(id) = active {
                let mut r = repr;
                egui::ComboBox::from_id_salt("tb_repr").selected_text(r.label()).width(150.0).show_ui(ui, |ui| {
                    for x in Representation::ALL {
                        ui.selectable_value(&mut r, x, x.label());
                    }
                });
                if r != repr {
                    self.session.lock().set_display(id, |d| d.representation = r);
                }
                let current = color_by
                    .as_ref()
                    .map(|c| format!("{}{}", c.name, if c.assoc == Association::Cells { " (cells)" } else { "" }))
                    .unwrap_or_else(|| "Solid Color".into());
                let mut new_cb: Option<Option<ColorBy>> = None;
                egui::ComboBox::from_id_salt("tb_color").selected_text(current).width(180.0).show_ui(ui, |ui| {
                    if ui.selectable_label(color_by.is_none(), "Solid Color").clicked() {
                        new_cb = Some(None);
                    }
                    for (assoc, name, ncomp) in &arrays {
                        let label = format!(
                            "{}{}{}",
                            if *assoc == Association::Points { format!("{} ", crate::icons::CIRCLE) } else { format!("{} ", crate::icons::SQUARE) },
                            name,
                            if *ncomp > 1 { format!(" ({ncomp})") } else { String::new() }
                        );
                        let sel = color_by.as_ref().map(|c| c.name == *name && c.assoc == *assoc).unwrap_or(false);
                        if ui.selectable_label(sel, label).clicked() {
                            new_cb = Some(Some(ColorBy { assoc: *assoc, name: name.clone(), component: None }));
                        }
                    }
                });
                if let Some(cb) = new_cb {
                    self.session.lock().color_by(id, cb);
                }
                if color_by.is_some() {
                    let mut m = cmap.clone();
                    crate::widgets::colormap_combo(ui, "tb_cmap", &mut m, 150.0);
                    if m != cmap {
                        self.session.lock().set_display(id, |d| d.colormap = m);
                    }
                }
            }
        });
    }

    fn time_bar(&mut self, ui: &mut egui::Ui) {
        let (n, idx, t) = {
            let s = self.session.lock();
            (s.times.len(), s.time_index, s.current_time())
        };
        if n < 2 {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} Time", crate::icons::CLOCK)).strong());
            if ui.button(crate::icons::SKIP_BACK).on_hover_text("First").clicked() {
                self.session.lock().set_time_index(0);
            }
            if ui.button(crate::icons::CARET_LEFT).on_hover_text("Previous (,)").clicked() {
                self.session.lock().step_time(-1);
            }
            if ui.button(if self.playing { crate::icons::PAUSE } else { crate::icons::PLAY }).on_hover_text("Play / pause (Space)").clicked() {
                self.playing = !self.playing;
            }
            if ui.button(crate::icons::CARET_RIGHT).on_hover_text("Next (.)").clicked() {
                self.session.lock().step_time(1);
            }
            if ui.button(crate::icons::SKIP_FORWARD).on_hover_text("Last").clicked() {
                self.session.lock().set_time_index(n - 1);
            }
            let mut i = idx;
            if ui.add(egui::Slider::new(&mut i, 0..=n - 1).show_value(false)).changed() {
                self.session.lock().set_time_index(i);
            }
            ui.label(format!("step {}/{}  t = {}", idx + 1, n, t.map(|v| aurafx_data::info::g(v)).unwrap_or_default()));
            ui.add(egui::DragValue::new(&mut self.play_fps).range(0.5..=60.0).speed(0.5).suffix(" fps"));
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (busy, nodes, pts, cells) = {
                let s = self.session.lock();
                let (mut p, mut c) = (0usize, 0usize);
                for n in s.nodes.values().filter(|n| n.visible) {
                    if let Some(i) = &n.info {
                        p += i.n_points;
                        c += i.n_cells;
                    }
                }
                (s.is_busy(), s.nodes.len(), p, c)
            };
            if busy {
                ui.spinner();
                ui.label("Working…");
            } else {
                ui.label(RichText::new(crate::icons::CHECK_CIRCLE).color(crate::theme::OK));
            }
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} | {} nodes | {} points, {} cells visible | {:.0} fps",
                    self.viewport.adapter_name(),
                    nodes,
                    pts,
                    cells,
                    self.viewport.fps()
                ));
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings").open(&mut open).resizable(true).default_width(520.0).show(ctx, |ui| {
            ui.heading("Appearance");
            ui.horizontal(|ui| {
                ui.label("Theme");
                let before = self.settings.theme;
                egui::ComboBox::from_id_salt("theme_mode").selected_text(self.settings.theme.label()).show_ui(ui, |ui| {
                    for m in crate::theme::ThemeMode::ALL {
                        ui.selectable_value(&mut self.settings.theme, m, m.label());
                    }
                });
                if before != self.settings.theme {
                    self.settings.dark_theme = self.settings.theme != crate::theme::ThemeMode::Light;
                    crate::theme::apply(ctx, self.settings.theme, self.settings.ui_scale);
                }
                ui.label("UI scale");
                if ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.7..=2.0)).drag_stopped() {
                    crate::theme::apply(ctx, self.settings.theme, self.settings.ui_scale);
                }
            });
            ui.separator();
            ui.heading("AI assistant");
            self.assistant.settings_ui(ui, &mut self.settings);
            ui.separator();
            ui.heading("Python");
            ui.label(format!("Interpreter: {}", self.python.version.lock()));
            ui.label("Packages are installed into the interpreter above (pip install numpy scipy …).");
            ui.separator();
            if ui.button("Save").clicked() {
                self.settings.save();
                self.toast("Settings saved", false);
            }
        });
        if !open {
            self.show_settings = false;
            self.settings.save();
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if self.show_about {
            let mut open = true;
            egui::Window::new("About auraFX").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../../../assets/icons/aurafx-128.png")).fit_to_exact_size(egui::vec2(96.0, 96.0)));
                    ui.heading(format!("auraFX {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Scientific visualization, analysis and simulation workbench written in Rust.");
                    ui.label(format!("Renderer: {}", self.viewport.adapter_name()));
                    ui.label(format!("Python: {}", self.python.version.lock()));
                    ui.hyperlink("https://github.com/aashiquear/auraFX");
                });
            });
            self.show_about = open;
        }
        if self.show_shortcuts {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).show(ctx, |ui| {
                egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                    for (k, v) in [
                        ("Ctrl+O", "Open file"),
                        ("Ctrl+K / Ctrl+Space", "Command palette (search filters, sources, actions)"),
                        ("Ctrl+Shift+S", "Save screenshot"),
                        ("R", "Reset camera"),
                        ("Delete", "Delete active node"),
                        ("Space", "Play / pause time"),
                        (", / .", "Previous / next time step"),
                        ("Left drag", "Rotate"),
                        ("Middle drag / Shift+drag", "Pan"),
                        ("Right drag / wheel", "Zoom"),
                        ("Double click", "Reset camera"),
                        ("Ctrl+Enter", "Run code in Python console"),
                        ("Ctrl+,", "Settings"),
                    ] {
                        ui.label(RichText::new(k).monospace());
                        ui.label(v);
                        ui.end_row();
                    }
                });
            });
            self.show_shortcuts = open;
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = self.now();
        self.toasts.retain(|(_, t, _)| now - t < 6.0);
        if self.toasts.is_empty() {
            return;
        }
        #[allow(deprecated)]
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            for (msg, _, err) in &self.toasts {
                let fill = if *err { Color32::from_rgb(120, 40, 40) } else { Color32::from_rgb(40, 80, 50) };
                egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(RichText::new(msg).color(Color32::WHITE));
                });
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

struct TreeBehavior<'a> {
    state: &'a mut State,
}

impl egui_tiles::Behavior<Pane> for TreeBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: egui_tiles::TileId, pane: &mut Pane) -> egui_tiles::UiResponse {
        match pane {
            Pane::Pipeline => crate::panels::pipeline_ui(self.state, ui),
            Pane::Properties => crate::panels::properties_ui(self.state, ui),
            Pane::Viewport => self.state.view_pane_ui(ui, 0),
            Pane::View(id) => self.state.view_pane_ui(ui, *id),
            Pane::Python => crate::console::console_ui(self.state, ui),
            Pane::Assistant => crate::assistant::assistant_ui(self.state, ui),
            Pane::Log => crate::panels::log_ui(self.state, ui),
            Pane::Spreadsheet => crate::panels::spreadsheet_ui(self.state, ui),
            Pane::Information => crate::panels::information_ui(self.state, ui),
            Pane::Jobs => crate::jobs_panel::jobs_ui(self.state, ui),
            Pane::Charts => crate::charts::charts_ui(self.state, ui),
        }
        egui_tiles::UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        let active = match pane {
            Pane::Viewport => Some(0),
            Pane::View(id) => Some(*id),
            _ => None,
        };
        if let Some(id) = active {
            let many = !self.state.views.is_empty();
            let name = crate::views::view_title(id);
            let mark = if many && id == self.state.active_view { " (active)" } else { "" };
            let link = if many
                && (if id == self.state.active_view { self.state.viewport.linked } else { self.state.views.get(&id).map(|v| v.linked).unwrap_or(false) })
            {
                format!(" {}", crate::icons::LINK)
            } else {
                String::new()
            };
            return format!("{} {name}{link}{mark}", pane.icon()).into();
        }
        format!("{} {}", pane.icon(), pane.title()).into()
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions { all_panes_must_have_tabs: true, ..Default::default() }
    }

    fn is_tab_closable(&self, _tiles: &egui_tiles::Tiles<Pane>, _tile_id: egui_tiles::TileId) -> bool {
        true
    }
}

impl eframe::App for AuraApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx_owned = root_ui.ctx().clone();
        let ctx = &ctx_owned;
        if !self.layout_checked {
            self.layout_checked = true;
            // Phones and small windows start with the compact layout.
            if ctx.content_rect().width() < 820.0 {
                self.tree = compact_tree();
            }
        }
        self.state.open_panes = Pane::ALL.iter().copied().filter(|p| self.tree.tiles.find_pane(p).is_some()).collect();
        let st = &mut self.state;
        st.viewport.frame_begin(ctx, frame);
        st.viewport.settings.background = st.settings.background_for(ctx.theme() == egui::Theme::Dark);
        {
            let cam = st.camera_state();
            let mut s = st.session.lock();
            s.camera_state = Some(cam);
            if let Some(c) = s.pending_camera.take() {
                drop(s);
                let vc = &mut st.viewport.camera;
                vc.eye = glam::DVec3::from_array(c.eye);
                vc.target = glam::DVec3::from_array(c.target);
                vc.up = glam::DVec3::from_array(c.up);
                vc.fov_y_deg = c.fov_y_deg;
                vc.orthographic = c.orthographic;
                vc.ortho_height = c.ortho_height;
                st.viewport.mark_camera_initialized();
            }
        }
        st.handle_shortcuts(ctx);
        st.tick_playback(ctx);
        // Open files passed on the command line.
        let pending = std::mem::take(&mut st.pending_open);
        for p in pending {
            st.open_file(p);
        }
        // Python results.
        for r in st.python.poll() {
            st.console.push_result(&r);
            if r.origin == "assistant" || r.origin == "agent" {
                st.assistant.on_python_result(&r);
            }
            if let Some(e) = &r.output.error {
                if r.origin != "console" {
                    st.toast(format!("Python error: {}", e.lines().last().unwrap_or("")), true);
                }
            }
        }
        st.assistant.poll(ctx);
        // Open results of finished jobs that asked for it.
        let to_open: Vec<(String, Vec<PathBuf>)> = {
            let jobs = aurafx_jobs::global();
            let mut m = jobs.lock();
            m.jobs
                .iter_mut()
                .filter(|j| j.outputs_pending_load)
                .map(|j| {
                    j.outputs_pending_load = false;
                    (j.spec.name.clone(), j.outputs.clone())
                })
                .collect()
        };
        for (name, outs) in to_open {
            st.toast(format!("{name} finished: opening {} result file(s)", outs.len().min(8)), false);
            for o in outs.into_iter().take(8) {
                st.open_file(o);
            }
        }
        if aurafx_jobs::global().lock().jobs.iter().any(|j| j.state.is_active()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        egui::Panel::top("menu").show(root_ui, |ui| st.menu_bar(ui));
        egui::Panel::top("toolbar").show(root_ui, |ui| {
            st.toolbar(ui);
            st.time_bar(ui);
        });
        egui::Panel::bottom("status").show(root_ui, |ui| st.status_bar(ui));

        // Commands from Python / the AI assistant.
        let cmds = std::mem::take(&mut self.state.session.lock().app_commands);
        for c in cmds {
            let (verb, arg) = c.split_once(' ').unwrap_or((c.as_str(), ""));
            let find = |a: &str| Pane::ALL.iter().copied().find(|p| p.title().eq_ignore_ascii_case(a) || format!("{p:?}").eq_ignore_ascii_case(a));
            match verb {
                "theme" => {
                    let m = match arg {
                        "light" => crate::theme::ThemeMode::Light,
                        "system" => crate::theme::ThemeMode::System,
                        _ => crate::theme::ThemeMode::Dark,
                    };
                    self.state.settings.theme = m;
                    self.state.settings.dark_theme = m != crate::theme::ThemeMode::Light;
                    crate::theme::apply(ctx, m, self.state.settings.ui_scale);
                    self.state.settings.save();
                }
                "layout" => {
                    let p = match arg {
                        "visualize" => LayoutPreset::Visualize,
                        "analyze" => LayoutPreset::Analyze,
                        "script" => LayoutPreset::Script,
                        "simulate" => LayoutPreset::Simulate,
                        "compact" => LayoutPreset::Compact,
                        _ => LayoutPreset::Full,
                    };
                    self.state.layout_actions.push(LayoutAction::Preset(p));
                }
                "hide" => {
                    if let Some(p) = find(arg) {
                        self.state.layout_actions.push(LayoutAction::Hide(p));
                    }
                }
                "show" if arg.eq_ignore_ascii_case("plugins") => {
                    self.state.plugins.open = true;
                }
                "show" if arg.eq_ignore_ascii_case("server") || arg.eq_ignore_ascii_case("open on server") => {
                    self.state.server.open = true;
                }
                "show" if arg.eq_ignore_ascii_case("colormap") || arg.eq_ignore_ascii_case("color map editor") => {
                    self.state.cmap_editor.open = true;
                }
                "show" if arg.eq_ignore_ascii_case("animation") => {
                    self.state.anim_dialog.open = true;
                    self.state.anim_dialog.mode = crate::animation::AnimMode::Keyframes;
                }
                "show" => {
                    if let Some(p) = find(arg) {
                        self.state.layout_actions.push(LayoutAction::Show(p));
                    }
                }
                "split" => {
                    let vertical = matches!(arg, "down" | "below" | "vertical" | "up");
                    self.state.layout_actions.push(LayoutAction::SplitView { from: self.state.active_view, vertical, only: None });
                }
                // "send <node> <view>|right|down [keep]": move (or with keep, also show) an object in a view.
                "send" => {
                    let mut it = arg.split_whitespace();
                    let node = it.next().and_then(|n| n.parse::<NodeId>().ok());
                    let target = it.next().unwrap_or("right");
                    let keep = it.next() == Some("keep");
                    match (node, target) {
                        (Some(node), "right" | "down") => {
                            self.state.layout_actions.push(LayoutAction::SplitView {
                                from: self.state.active_view,
                                vertical: target == "down",
                                only: Some((node, keep)),
                            });
                        }
                        (Some(node), v) => match v.parse::<u32>() {
                            Ok(view) if self.state.view_ids().contains(&view) => {
                                if keep {
                                    self.state.set_shown_in(view, node, true);
                                } else {
                                    self.state.move_to_view(node, view);
                                }
                            }
                            _ => self.state.toast(format!("No 3D view {v}"), true),
                        },
                        _ => {}
                    }
                }
                "link" => self.state.link_all_views(arg != "off"),
                "match" => self.state.camera_match_from = Some(self.state.active_view),
                "activate" => {
                    if let Ok(id) = arg.parse::<u32>() {
                        self.state.pending_activate = Some(id);
                    }
                }
                "ask" | "ask-agent" => {
                    let desc = self.state.session.lock().describe();
                    if verb == "ask-agent" {
                        self.state.settings.ai_agent_mode = true;
                        self.state.assistant.agent_active = true;
                        self.state.assistant.reset_agent_steps();
                    }
                    self.state.assistant.send(arg.to_string(), desc);
                    self.state.layout_actions.push(LayoutAction::Show(Pane::Assistant));
                }
                _ => tracing::warn!("unknown app command {c:?}"),
            }
        }
        // Apply layout changes requested from menus.
        let had_actions = !self.state.layout_actions.is_empty();
        for a in std::mem::take(&mut self.state.layout_actions) {
            match a {
                LayoutAction::Show(p) => show_pane(&mut self.tree, p),
                LayoutAction::Hide(p) => hide_pane(&mut self.tree, p),
                LayoutAction::Preset(pr) => apply_preset(&mut self.tree, pr),
                LayoutAction::SplitView { from, vertical, only } => {
                    let anchor = self.tree.tiles.find_pane(&crate::views::pane_for(from)).or_else(|| self.tree.tiles.find_pane(&Pane::Viewport));
                    let id = self.state.new_view_with(only);
                    let new_pane = self.tree.tiles.insert_pane(Pane::View(id));
                    let tabs = self.tree.tiles.insert_tab_tile(vec![new_pane]);
                    match anchor.and_then(|a| self.tree.tiles.parent_of(a)).or(anchor) {
                        Some(a) => split_beside(
                            &mut self.tree,
                            a,
                            tabs,
                            if vertical { egui_tiles::LinearDir::Vertical } else { egui_tiles::LinearDir::Horizontal },
                            false,
                            0.5,
                        ),
                        None => self.tree.root = Some(tabs),
                    }
                    self.state.pending_activate = Some(id);
                }
            }
        }
        if self.tree.root().is_none() {
            self.tree = default_tree();
        }
        if had_actions {
            tracing::debug!("layout: {:#?}", self.tree);
        }

        let fill = if ctx.theme() == egui::Theme::Dark { crate::theme::dark_visuals().panel_fill } else { crate::theme::light_visuals().panel_fill };
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin::same(2)).fill(fill)).show(root_ui, |ui| {
            let mut behavior = TreeBehavior { state: &mut self.state };
            self.tree.ui(&mut behavior, ui);
        });

        let open_views: Vec<u32> = self
            .tree
            .tiles
            .iter()
            .filter_map(|(_, t)| match t {
                egui_tiles::Tile::Pane(Pane::Viewport) => Some(0),
                egui_tiles::Tile::Pane(Pane::View(id)) => Some(*id),
                _ => None,
            })
            .collect();
        self.state.views_end_frame(&open_views);
        let st = &mut self.state;
        st.animation_tick(ctx);
        st.animation_poll();
        crate::animation::dialog_ui(st, ctx);
        crate::colormap_editor::editor_ui(st, ctx);
        st.plugins_tick(ctx);
        crate::plugins_ui::plugins_ui(st, ctx);
        crate::server_ui::server_ui(st, ctx);
        if let Some(p) = crate::dialogs::file_browser_ui(&mut st.file_browser, ctx) {
            st.open_file(p);
        }
        crate::palette::palette_ui(st, ctx);
        st.settings_window(ctx);
        st.about_window(ctx);
        st.toasts_ui(ctx);
        // Rename dialog.
        if let Some((id, mut name)) = st.rename_target.take() {
            let mut keep = true;
            let mut done = false;
            egui::Window::new("Rename").collapsible(false).resizable(false).show(ctx, |ui| {
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        done = true;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
            if done {
                st.session.lock().rename(id, name);
            } else if keep {
                st.rename_target = Some((id, name));
            }
        }
        st.viewport.frame_end(ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(s) = serde_json::to_string(&self.tree) {
            storage.set_string("layout_v4", s);
        }
        self.state.settings.save();
        // Keep the last pipeline so it can be restored next time.
        let cam = self.state.camera_state();
        let s = self.state.session.lock();
        if !s.nodes.is_empty() {
            let p = State::last_session_path();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Err(e) = s.save_state(&p, Some(cam)) {
                tracing::warn!("could not save last session: {e:#}");
            }
        }
    }
}
