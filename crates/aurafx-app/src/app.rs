//! Application state, layout and top-level chrome (menu bar, toolbar, status bar).

use crate::assistant::Assistant;
use crate::console::Console;
use crate::menus::MenuExt;
use crate::palette::Palette;
use crate::settings::Settings;
use crate::touch::FitScreen;
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
    /// A notebook of Python and text cells.
    Notebook,
    /// An additional 3D view.
    View(u32),
    /// An additional chart view (`aurafx_pipeline::charts`; the Charts panel is view 0).
    Chart(u32),
}

impl Pane {
    pub const ALL: [Pane; 11] = [
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
        Pane::Notebook,
    ];
    /// Whether the pane has anything to show in this build (the Python console and the notebook
    /// need the embedded Python, which the tablet and phone builds do not have).
    pub fn available(self) -> bool {
        match self {
            Pane::Python | Pane::Notebook => cfg!(feature = "python"),
            _ => true,
        }
    }
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
            Pane::Notebook => "Notebook",
            Pane::View(_) => "3D View",
            Pane::Chart(_) => "Chart",
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
            Pane::Notebook => crate::icons::FILE_PY,
            Pane::View(_) => crate::icons::CUBE,
            Pane::Chart(_) => crate::icons::WAVE_SINE,
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
    /// What the 3D view's right-click menu was opened on.
    pub view_menu_hit: crate::view_menu::ViewMenuHit,
    pub status: String,
    pub show_settings: bool,
    pub show_about: bool,
    pub show_welcome: bool,
    /// The report of a crash of the previous run, to tell the user about.
    pub crash_report: Option<PathBuf>,
    /// When the session was last saved for recovery, and its revision then.
    pub autosaved: (std::time::Instant, u64),
    /// Help > About auraFX > Third-party licences, and its find box.
    pub show_licences: bool,
    pub licence_find: String,
    pub show_shortcuts: bool,
    /// An action waiting for its new key (Settings › Keyboard).
    pub recording_shortcut: Option<String>,
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
    pub notebook: crate::notebook::Notebook,
    pub plugin_panels: crate::plugin_panels::PluginPanels,
    pub charts: crate::charts::ChartsState,
    pub anim_dialog: crate::animation::AnimDialog,
    pub screenshot_dialog: crate::screenshot_ui::ScreenshotDialog,
    pub cmap_editor: crate::colormap_editor::ColormapEditor,
    pub goal_stats: crate::goal_stats::GoalStats,
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
    /// The CAD environment's interface state.
    pub cad: crate::cad::CadUi,
    /// The window that inserts a source with its standard settings or changed ones.
    pub source_dialog: Option<crate::source_dialog::SourceDialog>,
    /// Chart views whose panes were open last frame (a pane closed by the user closes its view).
    pub open_chart_panes: Vec<u32>,
    /// Pictures of the views as laid out (`capture`).
    pub capture: crate::capture::Capture,
    /// The touch interfaces' own state (the phone's menu list, the view on the whole screen).
    pub touch: crate::touch::TouchUi,
    /// The layout follows the shape of the window (Windows > Layouts > Automatic).
    pub auto_layout: bool,
    /// The shape the automatic layout was last built for.
    pub layout_class: Option<crate::touch::LayoutClass>,
}

/// One of the top menus: its name and the function that adds its items.
pub struct TopMenu {
    pub label: &'static str,
    /// Shown in the accent color (the CAD menu, while a part is edited).
    pub accent: bool,
    pub ui: fn(&mut State, &mut egui::Ui),
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
    /// Follow the shape of the window: the layout is rebuilt when the window (or the screen)
    /// turns from wide to tall or back.
    Automatic,
    /// The 3D view on the left, every panel as a tab on the right.
    Wide,
    /// The 3D view on top, every panel as a tab below.
    Tall,
    /// The 3D view on top, two tab groups side by side below.
    TallTwo,
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
    /// Open chart view `n` (already in the session) beside the other charts.
    OpenChart(u32),
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
                    Pane::Python | Pane::Notebook | Pane::Charts | Pane::Spreadsheet | Pane::Log | Pane::Chart(_) => {
                        split_beside(tree, a, tabs, Vertical, false, 0.3)
                    }
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

/// A tab group of the panes in `panes` that are in `open` (all of them when `open` is `None`)
/// and have something to show in this build; `None` when that leaves nothing.
fn tab_group(tiles: &mut egui_tiles::Tiles<Pane>, panes: &[Pane], open: Option<&[Pane]>) -> Option<egui_tiles::TileId> {
    let ids: Vec<_> = panes.iter().copied().filter(|p| p.available() && open.is_none_or(|o| o.contains(p))).map(|p| tiles.insert_pane(p)).collect();
    (!ids.is_empty()).then(|| tiles.insert_tab_tile(ids))
}

/// The simulation runner starts programs on this machine or over SSH, which a tablet's or a
/// phone's system does not allow: the automatic layouts leave it out there (Windows > Simulations
/// still opens it).
fn jobs_in_touch_layouts() -> bool {
    !cfg!(any(target_os = "ios", target_os = "android"))
}

/// The layout for a window of this shape (`crate::touch::LayoutClass`). With `open`, only those
/// panes are shown (a layout rebuilt because the screen was turned keeps what was open), and the
/// further 3D views, chart views and the notebook among them get a place too.
pub fn class_tree(class: crate::touch::LayoutClass, open: Option<&[Pane]>) -> egui_tiles::Tree<Pane> {
    use crate::touch::LayoutClass;
    use egui_tiles::LinearDir::{Horizontal, Vertical};
    let extra: Vec<Pane> = open.unwrap_or(&[]).iter().copied().filter(|p| matches!(p, Pane::View(_) | Pane::Chart(_) | Pane::Notebook)).collect();
    if matches!(class, LayoutClass::Workbench | LayoutClass::TabletWide) {
        let mut tree = default_tree();
        if class == LayoutClass::TabletWide {
            // A tablet is narrower than a desktop window and its controls are larger: the side
            // columns get more of the width.
            if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(lin))) = tree.root().and_then(|r| tree.tiles.get_mut(r)) {
                let kids = lin.children.clone();
                for (k, share) in kids.iter().zip([0.24, 0.46, 0.30]) {
                    lin.shares.set_share(*k, share);
                }
            }
        }
        for p in Pane::ALL {
            let leave_out = !p.available() || (p == Pane::Jobs && class == LayoutClass::TabletWide && !jobs_in_touch_layouts());
            if tree.tiles.find_pane(&p).is_some() && (open.is_some_and(|o| !o.contains(&p)) || (class == LayoutClass::TabletWide && leave_out)) {
                hide_pane(&mut tree, p);
            }
        }
        for p in extra.into_iter().filter(|p| p.available()) {
            show_pane(&mut tree, p);
        }
        return tree;
    }
    let mut tiles = egui_tiles::Tiles::default();
    // The 3D views share the first group.
    let views: Vec<Pane> = std::iter::once(Pane::Viewport).chain(extra.iter().copied().filter(|p| matches!(p, Pane::View(_)))).collect();
    let others: Vec<Pane> = extra.iter().copied().filter(|p| !matches!(p, Pane::View(_))).collect();
    let view = tab_group(&mut tiles, &views, open);
    let jobs: &[Pane] = if jobs_in_touch_layouts() { &[Pane::Jobs] } else { &[] };
    let mut children: Vec<(egui_tiles::TileId, f32)> = Vec::new();
    let dir = if class == LayoutClass::PhoneWide { Horizontal } else { Vertical };
    if let Some(v) = view {
        children.push((v, if class == LayoutClass::PhoneWide { 0.56 } else { 0.5 }));
    }
    if class == LayoutClass::TabletTall {
        let left: Vec<Pane> = [Pane::Pipeline, Pane::Charts, Pane::Spreadsheet, Pane::Python, Pane::Log].into_iter().chain(others).collect();
        let right: Vec<Pane> = [Pane::Properties, Pane::Information, Pane::Assistant].into_iter().chain(jobs.iter().copied()).collect();
        let groups: Vec<_> = [tab_group(&mut tiles, &left, open), tab_group(&mut tiles, &right, open)].into_iter().flatten().collect();
        match groups.len() {
            0 => {}
            1 => children.push((groups[0], 0.5)),
            _ => {
                let row = tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(egui_tiles::Linear::new(Horizontal, groups))));
                children.push((row, 0.5));
            }
        }
    } else {
        // A phone: one tab group. The simulation runner and the Python console want more room
        // than a phone has; Windows opens them.
        let mut all: Vec<Pane> = vec![Pane::Pipeline, Pane::Properties, Pane::Information, Pane::Charts, Pane::Spreadsheet, Pane::Assistant, Pane::Log];
        if let Some(o) = open {
            all.extend([Pane::Jobs, Pane::Python].into_iter().filter(|p| o.contains(p)));
        }
        all.extend(others);
        if let Some(g) = tab_group(&mut tiles, &all, open) {
            children.push((g, if class == LayoutClass::PhoneWide { 0.44 } else { 0.5 }));
        }
    }
    if children.is_empty() {
        // Everything was closed: start again from the whole layout.
        return class_tree(class, None);
    }
    let root = match children.len() {
        1 => children[0].0,
        _ => {
            let mut lin = egui_tiles::Linear::new(dir, children.iter().map(|c| c.0).collect());
            for (id, share) in &children {
                lin.shares.set_share(*id, *share);
            }
            tiles.insert_new(egui_tiles::Tile::Container(egui_tiles::Container::Linear(lin)))
        }
    };
    #[allow(deprecated)]
    let id = egui::Id::new("aurafx_layout");
    egui_tiles::Tree::new(id, root, tiles)
}

/// The panes of a layout, and those that are the front tab of their group.
fn panes_of(tree: &egui_tiles::Tree<Pane>) -> (Vec<Pane>, Vec<Pane>) {
    let open: Vec<Pane> = tree.tiles.iter().filter_map(|(_, t)| if let egui_tiles::Tile::Pane(p) = t { Some(*p) } else { None }).collect();
    let front: Vec<Pane> = tree
        .tiles
        .iter()
        .filter_map(|(_, t)| match t {
            egui_tiles::Tile::Container(egui_tiles::Container::Tabs(tabs)) => tabs.active.and_then(|a| tree.tiles.get_pane(&a)).copied(),
            _ => None,
        })
        .collect();
    (open, front)
}

/// Rebuild `tree` for a window of another shape, keeping which panes are open and, where the
/// new groups allow it, which tabs are in front.
pub fn reshape_tree(tree: &mut egui_tiles::Tree<Pane>, class: crate::touch::LayoutClass) {
    let (open, front) = panes_of(tree);
    *tree = class_tree(class, Some(&open));
    for p in front.into_iter().filter(|p| !matches!(p, Pane::Viewport)) {
        tree.make_active(|_, t| matches!(t, egui_tiles::Tile::Pane(q) if *q == p));
    }
}

pub fn apply_preset(tree: &mut egui_tiles::Tree<Pane>, preset: LayoutPreset) {
    if preset == LayoutPreset::Compact {
        *tree = compact_tree();
        return;
    }
    // The layouts by shape. (Automatic is applied by the app, which knows the window.)
    let by_shape = match preset {
        LayoutPreset::Wide => Some(crate::touch::LayoutClass::PhoneWide),
        LayoutPreset::Tall => Some(crate::touch::LayoutClass::PhoneTall),
        LayoutPreset::TallTwo => Some(crate::touch::LayoutClass::TabletTall),
        _ => None,
    };
    if let Some(class) = by_shape {
        *tree = class_tree(class, None);
        return;
    }
    *tree = default_tree();
    let (hide, front): (&[Pane], &[Pane]) = match preset {
        LayoutPreset::Full | LayoutPreset::Compact | LayoutPreset::Automatic | LayoutPreset::Wide | LayoutPreset::Tall | LayoutPreset::TallTwo => (&[], &[]),
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
    /// A touch interface is shown (its layout is kept apart from the desktop's).
    touch_layout: bool,
    /// The layout of the other kind of interface: the desktop's while a touch interface is
    /// shown, the touch one's otherwise.
    other_tree: Option<egui_tiles::Tree<Pane>>,
    /// Whether the layout follows the window's shape in the other kind of interface.
    other_auto: bool,
    /// The layout was restored from the last run: an automatic layout built over it keeps which
    /// panels were open.
    keep_open: bool,
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
        let welcome_at_start = settings.show_welcome && files.is_empty() && script.is_none() && crate::JUPYTER_CONNECTION.get().is_none();
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
        // Run as a Jupyter kernel: the notebook's cells go to this Python; shutting the kernel
        // down closes the window.
        if let Some(conn) = crate::JUPYTER_CONNECTION.get() {
            let ctx = cc.egui_ctx.clone();
            let close = Box::new(move || ctx.send_viewport_cmd(egui::ViewportCommand::Close));
            match crate::jupyter::start(conn, python.sender(), close) {
                Ok(()) => session.lock().log(
                    aurafx_pipeline::LogLevel::Info,
                    "Running as a Jupyter kernel: cells from the notebook run in the Python console's interpreter".to_string(),
                ),
                Err(e) => session.lock().log(aurafx_pipeline::LogLevel::Error, format!("Jupyter kernel: {e:#}")),
            }
        }
        let viewport = Viewport::new(cc, settings.background);
        let stored_tree: Option<egui_tiles::Tree<Pane>> = cc.storage.and_then(|s| s.get_string("layout_v4")).and_then(|s| serde_json::from_str(&s).ok());
        let keep_open = stored_tree.is_some();
        let tree = stored_tree.unwrap_or_else(default_tree);
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
            view_menu_hit: Default::default(),
            status: "Ready. Open a file (Ctrl+O), add a source, or press Ctrl+K.".into(),
            show_settings: false,
            show_about: false,
            // The welcome window at a plain start (not with a file, a script or as a kernel).
            show_welcome: welcome_at_start,
            crash_report: crate::crash::take_last(),
            autosaved: (std::time::Instant::now(), 0),
            show_licences: false,
            licence_find: String::new(),
            show_shortcuts: false,
            recording_shortcut: None,
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
            notebook: Default::default(),
            plugin_panels: Default::default(),
            charts: Default::default(),
            file_browser: Default::default(),
            state_path: None,
            anim_dialog: Default::default(),
            screenshot_dialog: Default::default(),
            cmap_editor: Default::default(),
            goal_stats: Default::default(),
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
            cad: Default::default(),
            source_dialog: None,
            open_chart_panes: Vec::new(),
            capture: Default::default(),
            touch: Default::default(),
            // The desktop's layout follows the window only when the user chose Automatic.
            auto_layout: cc.storage.and_then(|s| s.get_string("layout_auto")).is_some_and(|s| s == "true"),
            layout_class: None,
        };
        // Scripts see that an app with a 3D view is running before the first frame.
        state.session.lock().view_ids = vec![0];
        {
            let ctx = cc.egui_ctx.clone();
            state.session.lock().wake_ui = Some(std::sync::Arc::new(move || ctx.request_repaint()));
        }
        // Plugins first, so startup scripts and restored sessions can use their filters.
        state.sync_plugins(false);
        if let Some(p) = script {
            match std::fs::read_to_string(&p) {
                Ok(code) => state.python.run(code, "startup"),
                Err(e) => tracing::error!("cannot read script {}: {e}", p.display()),
            }
        }
        // The touch interfaces keep their own layout, which follows the screen's shape unless a
        // named layout was chosen.
        let other_tree = cc.storage.and_then(|s| s.get_string("layout_touch_v1")).and_then(|s| serde_json::from_str(&s).ok());
        let other_auto = cc.storage.and_then(|s| s.get_string("layout_touch_auto")).is_none_or(|s| s != "false");
        let mut app = Self { state, tree, layout_checked: false, touch_layout: false, other_tree, other_auto, keep_open };
        let max_id =
            app.tree.tiles.iter().filter_map(|(_, t)| if let egui_tiles::Tile::Pane(Pane::View(id)) = t { Some(*id) } else { None }).max().unwrap_or(0);
        app.state.next_view_id = max_id + 1;
        app
    }

    /// Going from the desktop interface to a touch one or back: each has its own layout, and its
    /// own choice of following the window's shape (a tablet or a phone does unless a layout was
    /// chosen by name; the desktop only when Automatic was chosen).
    fn sync_interface_layout(&mut self, mode: crate::touch::Mode) {
        if mode.is_touch() == self.touch_layout {
            return;
        }
        self.touch_layout = mode.is_touch();
        std::mem::swap(&mut self.state.auto_layout, &mut self.other_auto);
        let mine = std::mem::replace(&mut self.tree, default_tree());
        self.keep_open = false;
        if let Some(t) = self.other_tree.replace(mine) {
            self.tree = t;
            self.keep_open = true;
        } else if self.touch_layout {
            // No touch layout yet: the one for the screen's shape.
            self.state.auto_layout = true;
        }
        self.state.layout_class = None;
        self.state.touch.menu_open = false;
        self.state.touch.view_maximized = false;
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
        // In an open sketch, undo is about the sketch.
        if let Some(e) = self.cad.sketch.as_mut() {
            self.status = if e.undo() { "Undid the last change of the sketch".into() } else { "Nothing to undo in this sketch".into() };
            return;
        }
        if crate::cad::editing(self) {
            let done = self.session.lock().cad_env_undo().is_some();
            self.status = if done { "Undid the last CAD edit".into() } else { "Nothing to undo in the CAD environment".into() };
            return;
        }
        match self.session.lock().undo() {
            Some(l) => self.status = format!("Undid: {l}"),
            None => self.status = "Nothing to undo".into(),
        }
    }

    pub fn redo(&mut self) {
        if let Some(e) = self.cad.sketch.as_mut() {
            self.status = if e.redo() { "Redid the change of the sketch".into() } else { "Nothing to redo in this sketch".into() };
            return;
        }
        if crate::cad::editing(self) {
            let done = self.session.lock().cad_env_redo().is_some();
            self.status = if done { "Redid the CAD edit".into() } else { "Nothing to redo in the CAD environment".into() };
            return;
        }
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

    /// Build a pipeline from a pipeline file chosen by the user.
    pub fn import_pipeline_dialog(&mut self) {
        let Some(p) = crate::dialogs::FileDialog::new().add_filter("Pipelines", &["json", "dream3d"]).pick_file() else { return };
        let mut s = self.session.lock();
        let input = s.active;
        let r = s.import_pipeline(&p, input);
        drop(s);
        match r {
            Ok(r) => {
                let msg = if r.skipped.is_empty() {
                    format!("Imported {} steps", r.nodes.len())
                } else {
                    format!("Imported {} steps; skipped: {}", r.nodes.len(), r.skipped.iter().map(|(a, b)| format!("{a} ({b})")).collect::<Vec<_>>().join("; "))
                };
                let mut s = self.session.lock();
                for (a, b) in &r.skipped {
                    s.log(aurafx_pipeline::session::LogLevel::Warn, format!("pipeline step {a} skipped: {b}"));
                }
                drop(s);
                self.toast(msg, !r.skipped.is_empty());
            }
            Err(e) => self.toast(format!("{e:#}"), true),
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
            let _ = &ext;
            let r = if multi && aurafx_pipeline::Session::holds_all_times(&p) {
                // Exodus, VTKHDF, EnSight, Xdmf and CGNS hold every time step.
                let mut s = self.session.lock();
                match s.active {
                    Some(id) => s.save_all_times(id, &p).map(|n| format!("Saved {} ({n} time steps)", p.display())),
                    None => Err(anyhow::anyhow!("nothing selected")),
                }
            } else if ext == "dream3d" {
                // Records the pipeline that made the data.
                let mut s = self.session.lock();
                match s.active {
                    Some(id) => s.save_data(id, &p).map(|_| format!("Saved {} with its pipeline", p.display())),
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
        let view = self.viewport.size();
        self.screenshot_dialog.show_for(view);
    }

    pub fn run_python(&mut self, code: &str, origin: &str) {
        self.python.run(code, origin);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        // Recording a new key for an action (Settings › Keyboard): the next key pressed, Esc cancels.
        if let Some(action) = self.recording_shortcut.clone() {
            let got = ctx.input_mut(|i| {
                let ev = i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                });
                if let Some((k, m)) = ev {
                    i.consume_key(m, k);
                }
                ev
            });
            if let Some((k, m)) = got {
                if k != egui::Key::Escape {
                    let _ = self.settings.shortcuts.set(&action, vec![crate::shortcuts::combo_text(m, k)]);
                    self.settings.save();
                }
                self.recording_shortcut = None;
            }
            return;
        }
        let sc = self.settings.shortcuts.clone();
        use crate::shortcuts::pressed;
        let (undo, redo, save_state) =
            ctx.input_mut(|i| (!typing && pressed(i, &sc, "undo", typing), !typing && pressed(i, &sc, "redo", typing), pressed(i, &sc, "save_state", typing)));
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
                pressed(i, &sc, "open", typing),
                pressed(i, &sc, "palette", typing),
                pressed(i, &sc, "screenshot", typing),
                pressed(i, &sc, "reset_camera", typing),
                pressed(i, &sc, "delete_node", typing),
                pressed(i, &sc, "play", typing),
                pressed(i, &sc, "next_step", typing),
                pressed(i, &sc, "prev_step", typing),
                pressed(i, &sc, "settings", typing),
            )
        });
        // View directions bound to keys.
        for (action, dir, up) in [
            ("view_x", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ("view_y", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            ("view_z", [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ("view_iso", [-1.0, -1.0, -1.0], [0.0, 0.0, 1.0]),
        ] {
            if ctx.input_mut(|i| pressed(i, &sc, action, typing)) {
                self.session.lock().camera_request = Some(CameraRequest::View { direction: dir, up });
            }
        }
        // Vim-style view keys: h l turn about the up axis, j k tilt, + - zoom, 0 resets.
        if sc.vim_keys && !typing {
            let k = |i: &mut egui::InputState, key: egui::Key| i.consume_key(egui::Modifiers::NONE, key);
            let (h, l, j, kk, plus, minus, zero) = ctx.input_mut(|i| {
                (
                    k(i, egui::Key::H),
                    k(i, egui::Key::L),
                    k(i, egui::Key::J),
                    k(i, egui::Key::K),
                    k(i, egui::Key::Plus) || k(i, egui::Key::Equals),
                    k(i, egui::Key::Minus),
                    k(i, egui::Key::Num0),
                )
            });
            let cam = &mut self.viewport.camera;
            if h {
                cam.rotate(-1.0 / 24.0, 0.0);
            }
            if l {
                cam.rotate(1.0 / 24.0, 0.0);
            }
            if j {
                cam.rotate(0.0, 1.0 / 24.0);
            }
            if kk {
                cam.rotate(0.0, -1.0 / 24.0);
            }
            if plus {
                cam.zoom(1.2);
            }
            if minus {
                cam.zoom(1.0 / 1.2);
            }
            if zero {
                self.session.lock().camera_request = Some(CameraRequest::ResetToBounds);
            }
            if h || l || j || kk || plus || minus {
                ctx.request_repaint();
            }
        }
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

    /// The top menus, in order. The menu bar, the tablet's overflow menu and the phone's menu
    /// list all show these, so they cannot drift apart.
    pub(crate) fn top_menus(&self) -> Vec<TopMenu> {
        let mut m = vec![
            TopMenu { label: "File", accent: false, ui: Self::menu_file },
            TopMenu { label: "Edit", accent: false, ui: Self::menu_edit },
            TopMenu { label: "Sources", accent: false, ui: Self::menu_sources },
        ];
        if crate::cad::editing(self) {
            m.push(TopMenu { label: "CAD", accent: true, ui: |s, ui| crate::cad::menu_ui(s, ui) });
        }
        m.extend([
            TopMenu { label: "Filters", accent: false, ui: Self::menu_filters },
            TopMenu { label: "View", accent: false, ui: Self::menu_view },
            TopMenu { label: "Windows", accent: false, ui: Self::menu_windows },
            TopMenu { label: "Simulate", accent: false, ui: Self::menu_simulate },
            TopMenu { label: "Remote", accent: false, ui: Self::menu_remote },
            TopMenu { label: "Model", accent: false, ui: Self::menu_model },
            TopMenu { label: "Help", accent: false, ui: Self::menu_help },
        ]);
        m
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.add(egui::Image::new(egui::include_image!("../../../assets/icons/aurafx-32.png")).fit_to_exact_size(egui::vec2(20.0, 20.0)));
            for m in self.top_menus() {
                if m.accent {
                    ui.scroll_menu(RichText::new(m.label).color(crate::theme::ACCENT), |ui| (m.ui)(self, ui));
                } else {
                    ui.scroll_menu(m.label, |ui| (m.ui)(self, ui));
                }
            }
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

    /// The items of the Sources menu.
    fn menu_sources(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = crate::menus::source_menu(ui) {
            crate::source_dialog::insert_source(self, id);
        }
    }

    /// The items of the File menu.
    fn menu_file(&mut self, ui: &mut egui::Ui) {
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
        if ui.button("Import Pipeline…").on_hover_text("Build a pipeline from a pipeline file (.json) or the pipeline a .dream3d file records; it starts from the selected data when its files are not found").clicked() {
            ui.close();
            self.import_pipeline_dialog();
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
        ui.scroll_menu("Open Sample Data", |ui| {
            if ui
                .button("Heated disk in a flow (Exodus)")
                .on_hover_text("The disk_out_ref sample: a heated disk in a flow, with temperature, pressure and velocity")
                .clicked()
            {
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
        ui.scroll_menu("Open Recent", |ui| {
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
        if ui.button("Export Scene…").on_hover_text("The visible objects as a glTF (.glb) or OBJ file, for Blender, web viewers, slides").clicked() {
            ui.close();
            if let Some(path) = crate::dialogs::FileDialog::new()
                .add_filter("glTF binary", &["glb"])
                .add_filter("Wavefront OBJ (vertex colors)", &["obj"])
                .add_filter("X3D", &["x3d"])
                .add_filter("VRML 2.0", &["wrl"])
                .add_filter("Web page you can turn (HTML)", &["html"])
                .set_file_name("scene.glb")
                .save_file()
            {
                self.viewport.request_screenshot(aurafx_pipeline::session::ScreenshotRequest::new(path));
            }
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
    }

    /// The items of the Edit menu.
    fn menu_edit(&mut self, ui: &mut egui::Ui) {
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
    }

    /// The items of the Filters menu.
    fn menu_filters(&mut self, ui: &mut egui::Ui) {
        let ctx_info = crate::menus::node_context(self, None);
        match &ctx_info {
            Some((info, timed)) => {
                if let Some(id) = crate::menus::suggestion_buttons(ui, info, *timed) {
                    self.add_filter(id);
                }
            }
            None => {
                ui.label(RichText::new("Open a file or add a source first: filters work on the selected object.").weak().small());
                ui.separator();
            }
        }
        if let Some(id) = crate::menus::filter_menu_for(ui, ctx_info.as_ref().map(|c| &c.0), |_| true) {
            self.add_filter(id);
        }
        ui.separator();
        if ui
            .button(format!("{} Describe a filter…", crate::icons::SPARKLE))
            .on_hover_text("Say in words what you want to see; the assistant adds the filters that show it")
            .clicked()
        {
            ui.close();
            self.assistant.describe_open = true;
        }
        if ui.button("Plot Over Time…").on_hover_text("Open the Charts panel's over-time tools").clicked() {
            ui.close();
            self.layout_actions.push(LayoutAction::Show(Pane::Charts));
        }
        ui.separator();
        if ui.button("Search…    Ctrl+K").clicked() {
            ui.close();
            self.palette.open = true;
        }
    }

    /// The items of the View menu.
    fn menu_view(&mut self, ui: &mut egui::Ui) {
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
        ui.scroll_menu(format!("{} Lighting", crate::icons::CIRCLE_HALF), |ui| {
            crate::lighting_ui::lighting_ui(ui, &mut self.settings.lighting);
            if !self.settings.lighting.lights.is_empty() {
                ui.checkbox(&mut self.settings.show_light_widgets, "Show lights in the view").on_hover_text("Drag the sun handles to aim the lights");
            }
        });
        if ui
            .button(format!("{} New Chart View", crate::icons::WAVE_SINE))
            .on_hover_text("A chart of the selected object's arrays beside the other charts (choose its object in the chart's first box)")
            .clicked()
        {
            ui.close();
            let active = self.session.lock().active;
            self.session.lock().new_chart_view(active);
        }
        if ui.button(format!("{} Color Map Editor…", crate::icons::PENCIL_SIMPLE)).clicked() {
            ui.close();
            self.cmap_editor.open = true;
        }
        if ui
            .button(format!("{} Goal Statistics…", crate::icons::DOTS_NINE))
            .on_hover_text("Edit the grain size and shape distributions of a synthetic microstructure with live plots, then pack it")
            .clicked()
        {
            ui.close();
            self.goal_stats.open = true;
        }
        {
            let mut s = self.session.lock();
            let mut t = s.show_time_label;
            if ui.checkbox(&mut t, "Time label").changed() {
                s.show_time_label = t;
            }
        }
        ui.scroll_menu("Annotations", |ui| {
            let id = ui.make_persistent_id("new_note");
            let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut text).hint_text("text").desired_width(160.0));
            });
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
            ui.horizontal_wrapped(|ui| {
                for corner in ["upper left", "upper right", "lower right", "upper center", "lower center"] {
                    if crate::touch::small_button(ui, corner).clicked() && !text.trim().is_empty() {
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
            if crate::touch::small_button(ui, "Default").clicked() {
                let d = Settings::default();
                if dark {
                    self.settings.background = d.background;
                } else {
                    self.settings.background_light = d.background_light;
                }
                self.settings.save();
            }
        });
    }

    /// The items of the Windows menu.
    fn menu_windows(&mut self, ui: &mut egui::Ui) {
        for p in Pane::ALL {
            // A tablet or a phone does not list the panels its build has nothing to show in.
            if crate::touch::mode().is_touch() && !p.available() {
                continue;
            }
            let mut open = self.open_panes.contains(&p);
            if ui.checkbox(&mut open, format!("{} {}", p.icon(), p.title())).clicked() {
                self.layout_actions.push(if open { LayoutAction::Show(p) } else { LayoutAction::Hide(p) });
            }
        }
        ui.separator();
        ui.menu_button("Plugin panels", |ui| {
            if ui.button(format!("{} Look again", crate::icons::ARROW_CLOCKWISE)).on_hover_text("Panels added by @afx.panel in Python plugins").clicked() {
                let tx = self.python.sender();
                self.plugin_panels.refresh_titles(&tx);
            }
            if self.plugin_panels.titles.is_empty() {
                ui.label(RichText::new("None yet: a Python plugin adds one with @afx.panel(\"Title\")").weak());
            }
            for t in self.plugin_panels.titles.clone() {
                let open = self.plugin_panels.open.contains_key(&t);
                if ui.selectable_label(open, &t).clicked() {
                    if open {
                        self.plugin_panels.open.remove(&t);
                    } else {
                        let tx = self.python.sender();
                        self.plugin_panels.open_panel(&t, &tx);
                    }
                    ui.close();
                }
            }
        });
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
        ui.label(RichText::new("Layouts by window shape").weak());
        if ui
            .radio(self.auto_layout, "Automatic")
            .on_hover_text("Follow the window: a wide one gets the workbench, a tall one the view on top and the panels below, a small one a single group of tabs. Turning a tablet or a phone changes the layout with it.")
            .clicked()
        {
            self.layout_actions.push(LayoutAction::Preset(LayoutPreset::Automatic));
            ui.close();
        }
        for (label, preset, tip) in [
            (
                "Wide (view left, tabs right)",
                LayoutPreset::Wide,
                "The 3D view on the left and every panel as a tab on the right: a phone held wide, a low window",
            ),
            ("Tall (view on top, tabs below)", LayoutPreset::Tall, "The 3D view on top and every panel as a tab below: a phone held tall, a narrow window"),
            ("Tall, two tab groups", LayoutPreset::TallTwo, "The 3D view on top and two groups of tabs side by side below: a tablet held tall, a tall window"),
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
    }

    /// The items of the Simulate menu.
    fn menu_simulate(&mut self, ui: &mut egui::Ui) {
        if ui.button("Run a simulation…").clicked() {
            ui.close();
            self.status = "Use the Simulations panel to configure and run a job.".into();
        }
        ui.label(RichText::new("Templates: LAMMPS, MOOSE, OpenFOAM, Python, custom").weak());
    }

    /// The items of the Remote menu.
    fn menu_remote(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(format!("{} Open data on another machine…", crate::icons::CLOUD))
            .on_hover_text("Add a machine, install aurafx-server there, choose where the picture is drawn, and open a file on it")
            .clicked()
        {
            ui.close();
            self.server.open = true;
        }
        if ui.button(format!("{} Machines and jobs…", crate::icons::DESKTOP_TOWER)).on_hover_text("The Simulations panel: machines, jobs, files").clicked() {
            ui.close();
            self.layout_actions.push(LayoutAction::Show(Pane::Jobs));
        }
    }

    /// The items of the Model menu.
    fn menu_model(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Make what a solver needs").weak().small());
        if ui
            .button(format!("{} New CAD part", crate::icons::CUBE))
            .on_hover_text("A parametric part: sketches, extrusions, holes, fillets. Opens the CAD environment, which says what to do next.")
            .clicked()
        {
            ui.close();
            crate::welcome::new_cad_part(self);
        }
        if ui
            .button(format!("{} Open a CAD file…", crate::icons::FOLDER_OPEN))
            .on_hover_text("STEP, IGES, 3MF, .sc or .afxcad: opens as a CAD Model you can edit")
            .clicked()
        {
            ui.close();
            self.open_file_dialog();
        }
        {
            // The selected object as the first body of a CAD part: a Geometry source, any closed surface.
            let (active, is_cad) = {
                let s = self.session.lock();
                (s.active.filter(|a| s.nodes.get(a).is_some_and(|n| n.output.is_some())), s.active.is_some_and(|a| s.is_cad_model(a)))
            };
            if ui
                .add_enabled(active.is_some() && !crate::cad::editing(self), egui::Button::new(format!("{} Edit in CAD…", crate::icons::PENCIL_SIMPLE)))
                .on_hover_text(if is_cad {
                    "Open the CAD environment on the selected part"
                } else {
                    "Start a CAD part from the selected object: its solid (a Geometry source's bodies, any closed surface) becomes the first body, to add to and cut from. The object itself is hidden, not changed."
                })
                .on_disabled_hover_text("Select an object in the Pipeline panel first: a CAD Model, a Geometry source or anything with a closed surface")
                .clicked()
            {
                ui.close();
                if let Some(id) = active {
                    crate::cad::enter_from(self, id);
                }
            }
        }
        if ui
            .button(format!("{} New geometry (commands)", crate::icons::CUBE))
            .on_hover_text("Solids from short commands, with menus that insert them and the steps to a hexahedral or tetrahedral mesh")
            .clicked()
        {
            ui.close();
            crate::welcome::new_geometry(self);
        }
        ui.separator();
        ui.label(RichText::new("Meshing filters (on the selected object)").weak().small());
        let ctx_info = crate::menus::node_context(self, None);
        if let Some(id) = crate::menus::filter_menu_for(ui, ctx_info.as_ref().map(|c| &c.0), |f| f.category == "Mesh" || f.id == "mesh_quality") {
            self.add_filter(id);
        }
    }

    /// The items of the Help menu.
    fn menu_help(&mut self, ui: &mut egui::Ui) {
        if ui.button("Welcome").on_hover_text("Where to start, and a few things worth knowing").clicked() {
            ui.close();
            self.show_welcome = true;
        }
        if ui.button("User guide").on_hover_text("Every feature with pictures, in your browser").clicked() {
            ui.close();
            crate::welcome::open_guide(ui.ctx(), "index.html");
        }
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
    }

    /// The tablet's menu bar: the same menus with room for a finger. Those that do not fit the
    /// width go into a last menu instead of being cut off.
    fn menu_bar_tablet(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.spacing_mut().button_padding = egui::vec2(10.0, 8.0);
            ui.add(egui::Image::new(egui::include_image!("../../../assets/icons/aurafx-32.png")).fit_to_exact_size(egui::vec2(24.0, 24.0)));
            let menus = self.top_menus();
            let gap = ui.spacing().item_spacing.x;
            let pad = 2.0 * ui.spacing().button_padding.x + gap;
            let widths: Vec<f32> =
                menus.iter().map(|m| egui::WidgetText::from(m.label).into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button).size().x + pad).collect();
            // The two buttons on the right, and the overflow menu when it is needed.
            let button = ui.spacing().interact_size.x.max(44.0) + gap;
            let room = ui.available_width() - 2.0 * button;
            let mut shown = menus.len();
            if widths.iter().sum::<f32>() > room {
                let mut used = button;
                shown = 0;
                for w in &widths {
                    if used + w > room {
                        break;
                    }
                    used += w;
                    shown += 1;
                }
            }
            let title = |m: &TopMenu| if m.accent { RichText::new(m.label).color(crate::theme::ACCENT) } else { RichText::new(m.label) };
            for m in &menus[..shown] {
                ui.scroll_menu(title(m), |ui| (m.ui)(self, ui));
            }
            if shown < menus.len() {
                ui.scroll_menu(RichText::new(crate::icons::DOTS_THREE).size(20.0), |ui| {
                    for m in &menus[shown..] {
                        ui.scroll_menu(title(m), |ui| (m.ui)(self, ui));
                    }
                })
                .response
                .on_hover_text("More menus");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.search_and_ask(ui));
        });
    }

    /// The two buttons at the right of a touch interface's top bar: the assistant and the search.
    fn search_and_ask(&mut self, ui: &mut egui::Ui) {
        let icon = |i: &str| egui::Button::new(RichText::new(i).size(20.0)).min_size(egui::vec2(44.0, crate::touch::TOUCH_HEIGHT));
        if ui.add(icon(crate::icons::SPARKLE)).on_hover_text("Ask AI: the Assistant panel").clicked() {
            self.assistant.focus_input = true;
            self.layout_actions.push(LayoutAction::Show(Pane::Assistant));
            self.touch.menu_open = false;
            self.touch.view_maximized = false;
        }
        if ui.add(icon(crate::icons::MAGNIFYING_GLASS)).on_hover_text("Search filters, sources and actions").clicked() {
            self.palette.open = true;
            self.touch.menu_open = false;
        }
        if self.touch.view_maximized && ui.add(icon(crate::icons::CORNERS_IN)).on_hover_text("Show the panels again").clicked() {
            self.touch.view_maximized = false;
        }
    }

    /// The phone's top bar: one menu button, the name, the search and the assistant.
    fn menu_bar_phone(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let open = self.touch.menu_open;
            let icon = if open { crate::icons::X } else { crate::icons::LIST };
            let b = egui::Button::new(RichText::new(icon).size(22.0)).min_size(egui::vec2(48.0, crate::touch::TOUCH_HEIGHT)).selected(open);
            if ui.add(b).on_hover_text("Menu: File, Edit, Sources, Filters, View, Windows and the others").clicked() {
                self.touch.menu_open = !open;
            }
            ui.add(egui::Image::new(egui::include_image!("../../../assets/icons/aurafx-32.png")).fit_to_exact_size(egui::vec2(24.0, 24.0)));
            if crate::cad::editing(self) {
                ui.label(RichText::new("CAD").strong().color(crate::theme::ACCENT));
            } else {
                ui.label(RichText::new("auraFX").strong());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.search_and_ask(ui));
        });
    }

    /// The phone's menu: the top menus as a list over the whole screen. A menu's name opens its
    /// items below it; choosing an item closes the list.
    fn menu_list_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("menu list").auto_shrink([false, false]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                // Rows like a menu's: no button frames.
                let v = &mut ui.style_mut().visuals.widgets;
                v.inactive.weak_bg_fill = Color32::TRANSPARENT;
                v.inactive.bg_stroke = egui::Stroke::NONE;
                for m in self.top_menus() {
                    let open = self.touch.menu_section == Some(m.label);
                    let caret = if open { crate::icons::CARET_DOWN } else { crate::icons::CARET_RIGHT };
                    let mut title = RichText::new(format!("{caret}   {}", m.label)).size(17.0).strong();
                    if m.accent {
                        title = title.color(crate::theme::ACCENT);
                    }
                    if ui.add(egui::Button::new(title).min_size(egui::vec2(0.0, 46.0))).clicked() {
                        self.touch.menu_section = if open { None } else { Some(m.label) };
                    }
                    if open {
                        let chosen = ui
                            .scope_builder(egui::UiBuilder::new().closable(), |ui| {
                                ui.indent(("menu list", m.label), |ui| (m.ui)(self, ui));
                                ui.should_close()
                            })
                            .inner;
                        if chosen {
                            self.touch.menu_open = false;
                        }
                    }
                    ui.separator();
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        if crate::touch::mode().is_touch() {
            // One row that scrolls sideways instead of being cut off or taking several rows.
            egui::ScrollArea::horizontal().id_salt("toolbar").show(ui, |ui| ui.horizontal(|ui| self.toolbar_items(ui)));
            return;
        }
        ui.horizontal_wrapped(|ui| self.toolbar_items(ui));
    }

    fn toolbar_items(&mut self, ui: &mut egui::Ui) {
        {
            if ui.button(format!("{} Open", crate::icons::FOLDER_OPEN)).on_hover_text("Open data file (Ctrl+O)").clicked() {
                self.open_file_dialog();
            }
            ui.separator();
            let tool_ctx = crate::menus::node_context(self, None);
            for (label, id, tip) in [
                ("Slice", "slice", "Cut with a plane"),
                ("Clip", "clip", "Remove part of the data"),
                ("Contour", "contour", "Isosurfaces"),
                ("Threshold", "threshold", "Keep a value range"),
                ("Calculator", "calculator", "Compute arrays"),
                ("Glyph", "glyph", "Arrows at points"),
                ("Warp", "warp_by_vector", "Deform by vectors"),
            ] {
                // Faded when it is unlikely to work on the selected object, with the reason.
                let why_not = match (&tool_ctx, aurafx_filters::find(id)) {
                    (Some((info, _)), Some(def)) => aurafx_filters::suggest::applicable(&def, info).err(),
                    (None, _) => Some("select an object in the Pipeline first (open a file or add a source)".to_string()),
                    _ => None,
                };
                let r = match &why_not {
                    Some(w) => ui.button(RichText::new(crate::icons::with(label)).weak()).on_hover_text(format!("{tip}\nMay not work now: {w}.")),
                    None => ui.button(crate::icons::with(label)).on_hover_text(tip),
                };
                if r.clicked() {
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
                if crate::touch::small_button(ui, crate::icons::with(label)).on_hover_text(format!("Look from {label}")).clicked() {
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
        }
    }

    fn time_bar(&mut self, ui: &mut egui::Ui) {
        let (n, idx, t) = {
            let s = self.session.lock();
            (s.times.len(), s.time_index, s.current_time())
        };
        if n < 2 {
            return;
        }
        if crate::touch::mode().is_touch() {
            egui::ScrollArea::horizontal().id_salt("time bar").show(ui, |ui| ui.horizontal(|ui| self.time_bar_items(ui, n, idx, t)));
            return;
        }
        ui.horizontal(|ui| self.time_bar_items(ui, n, idx, t));
    }

    fn time_bar_items(&mut self, ui: &mut egui::Ui, n: usize, idx: usize, t: Option<f64>) {
        {
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
        }
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
            if crate::touch::mode().is_touch() {
                // One line whatever the width: the message, cut short when it does not fit, and
                // on a tablet the counts on the right.
                let counts = format!("{nodes} nodes | {pts} points, {cells} cells | {:.0} fps", self.viewport.fps());
                if crate::touch::mode().is_phone() {
                    ui.add(egui::Label::new(&self.status).truncate()).on_hover_text(format!("{}\n{counts}", self.status));
                } else {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(counts);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(egui::Label::new(&self.status).truncate()));
                    });
                }
                return;
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
        egui::Window::new("Settings").open(&mut open).resizable(true).default_width(520.0).show_fit(ctx, |ui| {
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
            ui.checkbox(&mut self.settings.ask_source_settings, "Ask before adding a source")
                .on_hover_text("Adding a source opens a window to insert it with its standard settings or change them first");
            ui.separator();
            ui.heading("Interface");
            ui.horizontal(|ui| {
                ui.label("Interface");
                let before = self.settings.interface;
                egui::ComboBox::from_id_salt("interface_mode").selected_text(self.settings.interface.label()).show_ui(ui, |ui| {
                    for (c, tip) in [
                        (crate::touch::InterfaceChoice::Automatic, "Desktop on a computer; Tablet or Phone on a tablet or a phone, by the size of its screen"),
                        (crate::touch::InterfaceChoice::Desktop, "Menus, panels and windows for a mouse and a keyboard"),
                        (crate::touch::InterfaceChoice::Tablet, "Controls sized for a finger, windows that stay on the screen, layouts that follow how the screen is held"),
                        (crate::touch::InterfaceChoice::Phone, "One menu button, the 3D view and one group of tabs"),
                    ] {
                        ui.selectable_value(&mut self.settings.interface, c, c.label()).on_hover_text(tip);
                    }
                });
                if before != self.settings.interface {
                    self.settings.save();
                }
                let now = crate::touch::mode();
                let note = if crate::touch::mode_from_env() {
                    format!("(now {}, set by AURAFX_INTERFACE)", now.label())
                } else {
                    format!("(now {})", now.label())
                };
                ui.label(RichText::new(note).weak().small());
            });
            if crate::touch::mode().is_touch() {
                if ui
                    .checkbox(&mut self.settings.keyboard_room, "Keep room for the on-screen keyboard")
                    .on_hover_text("While you type, the lower part of the screen is left free for the keyboard where the system does not say how tall it is. Turn it off with a hardware keyboard.")
                    .changed()
                {
                    self.settings.save();
                }
            }
            ui.separator();
            ui.heading("Keyboard");
            ui.label(RichText::new("Type keys like Ctrl+Shift+S, R, F5 or Space (several with commas), or press Record and then the key.").weak().small());
            egui::Grid::new("keyboard settings").num_columns(3).striped(true).show(ui, |ui| {
                for (a, label, _) in crate::shortcuts::ACTIONS {
                    ui.label(*label);
                    let mut text = self.settings.shortcuts.of(a).join(", ");
                    let r = ui.add_sized([170.0, 20.0], egui::TextEdit::singleline(&mut text));
                    if r.changed() {
                        let combos: Vec<String> = text.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
                        if self.settings.shortcuts.set(a, combos).is_ok() {
                            self.settings.save();
                        }
                    }
                    let rec = self.recording_shortcut.as_deref() == Some(*a);
                    if ui.selectable_label(rec, if rec { "Press a key…" } else { "Record" }).clicked() {
                        self.recording_shortcut = if rec { None } else { Some(a.to_string()) };
                    }
                    ui.end_row();
                }
            });
            for (c, a, b) in self.settings.shortcuts.clashes() {
                ui.colored_label(crate::theme::ERROR, format!("{c} is bound to both {a} and {b}: the first one wins"));
            }
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut self.settings.shortcuts.vim_keys, "Vim-style view keys")
                    .on_hover_text("h and l turn the view, j and k tilt it, + and - zoom, 0 resets")
                    .changed()
                {
                    self.settings.save();
                }
                if ui.button("Defaults").on_hover_text("Put every key back as it was").clicked() {
                    self.settings.shortcuts = Default::default();
                    self.settings.save();
                }
            });
            ui.separator();
            ui.heading("Performance");
            ui.horizontal(|ui| {
                let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
                ui.label("Filter threads");
                let mut all = self.settings.threads == 0;
                if ui.checkbox(&mut all, format!("all {cores} cores")).changed() {
                    self.settings.threads = if all { 0 } else { (cores / 2).max(1) };
                }
                if !all {
                    ui.add(egui::Slider::new(&mut self.settings.threads, 1..=cores));
                }
                ui.label(RichText::new(format!("(now {}; changes apply at the next start)", rayon::current_num_threads())).weak().small());
            });
            ui.horizontal(|ui| {
                let dev = aurafx_filters::gpu::device_name();
                let label = match &dev {
                    Some(d) => format!("Use the GPU for filters ({d})"),
                    None => "Use the GPU for filters (none found)".to_string(),
                };
                if ui
                    .add_enabled(dev.is_some(), egui::Checkbox::new(&mut self.settings.gpu_filters, label))
                    .on_hover_text(
                        "Contour and Slice on big grids: the GPU finds the cells they can cross and the CPU cuts those, so the results are the same.",
                    )
                    .changed()
                {
                    aurafx_filters::gpu::set_enabled(self.settings.gpu_filters);
                    self.settings.save();
                }
                if dev.is_some()
                    && ui.button("Benchmark").on_hover_text("Time Contour and Slice on a 201^3 grid with and without the GPU (a few seconds).").clicked()
                {
                    let r = aurafx_filters::gpu::benchmark(100);
                    aurafx_filters::gpu::set_enabled(self.settings.gpu_filters);
                    let text = r.iter().map(|(n, c, g)| format!("{n}: CPU {:.0} ms, GPU {:.0} ms", c * 1e3, g * 1e3)).collect::<Vec<_>>().join("; ");
                    self.status = format!("GPU benchmark: {text}");
                    tracing::info!("GPU benchmark: {text}");
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
            egui::Window::new("About auraFX").open(&mut open).collapsible(false).resizable(false).show_fit(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../../../assets/icons/aurafx-128.png")).fit_to_exact_size(egui::vec2(96.0, 96.0)));
                    ui.heading(format!("auraFX {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Scientific visualization, analysis and simulation workbench written in Rust.");
                    ui.label(format!("Renderer: {}", self.viewport.adapter_name()));
                    ui.label(format!("Python: {}", self.python.version.lock()));
                    ui.hyperlink("https://github.com/aashiquear/auraFX");
                    ui.add_space(6.0);
                    ui.label("auraFX includes open-source software. The B-rep CAD kernel and the CAD file formats come from SolidCraft and solidcore.");
                    if ui.button(format!("{} Third-party licences...", crate::icons::FILE_TEXT)).clicked() {
                        self.show_licences = true;
                    }
                });
            });
            self.show_about = open;
        }
        if self.show_licences {
            // The notices the licences of the bundled software ask to be passed on (scripts/licences.sh).
            static LINES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
            let lines = LINES.get_or_init(|| include_str!("../../../THIRD-PARTY-NOTICES.txt").lines().collect());
            let mut open = true;
            egui::Window::new("Third-party licences").open(&mut open).default_size([680.0, 520.0]).fit_own_scroll(ctx).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} Find", crate::icons::MAGNIFYING_GLASS));
                    ui.add(egui::TextEdit::singleline(&mut self.licence_find).hint_text("a crate or a licence, e.g. Apache-2.0").desired_width(260.0));
                });
                let find = self.licence_find.trim().to_lowercase();
                let shown: Vec<&str> =
                    if find.is_empty() { lines.clone() } else { lines.iter().copied().filter(|l| l.to_lowercase().contains(&find)).collect() };
                ui.weak(format!("{} of {} lines. The same text is THIRD-PARTY-NOTICES.txt next to the program.", shown.len(), lines.len()));
                let row = ui.text_style_height(&egui::TextStyle::Monospace);
                egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, row, shown.len(), |ui, range| {
                    for l in &shown[range] {
                        ui.add(egui::Label::new(egui::RichText::new(*l).monospace()).extend());
                    }
                });
            });
            self.show_licences = open;
        }
        if self.show_shortcuts {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).show_fit(ctx, |ui| {
                egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                    for (a, label, _) in crate::shortcuts::ACTIONS {
                        let keys = self.settings.shortcuts.of(a);
                        if keys.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(keys.join(" / ")).monospace());
                        ui.label(*label);
                        ui.end_row();
                    }
                    for (k, v) in [
                        ("Left drag", "Rotate"),
                        ("Middle drag / Shift+drag", "Pan"),
                        ("Right drag / wheel", "Zoom"),
                        ("Double click", "Reset camera"),
                        ("Ctrl+Enter", "Run code in Python console"),
                        ("CAD: Ctrl+Enter", "Apply the part (in the CAD environment)"),
                        ("CAD: Esc", "Leave the CAD environment (asks), or end a sketch chain"),
                        ("CAD: Ctrl+Z / Ctrl+Y", "Undo / redo one CAD edit"),
                        ("CAD: Delete", "Delete the selected feature (or the selected sketch shapes)"),
                        ("CAD: Alt+1 … Alt+6", "Ribbon tab: Sketch, Features, Modify, Inspect, Assemble, Files"),
                        ("CAD: F2", "Rename the selected feature"),
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
        // Nothing over the views while a picture of them is taken.
        if self.toasts.is_empty() || self.capture.busy() {
            return;
        }
        #[allow(deprecated)]
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            for (msg, _, err) in &self.toasts {
                let fill = if *err { Color32::from_rgb(120, 40, 40) } else { Color32::from_rgb(40, 80, 50) };
                egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
                    if crate::touch::mode().is_touch() {
                        ui.set_max_width((ctx.content_rect().width() - 56.0).clamp(120.0, 420.0));
                    } else {
                        ui.set_max_width(420.0);
                    }
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
        if crate::capture::is_view(*pane) {
            self.state.capture.pane_rects.push((*pane, ui.max_rect()));
        }
        match pane {
            Pane::Pipeline if crate::cad::editing(self.state) => crate::cad::feature_manager_ui(self.state, ui),
            Pane::Properties if crate::cad::editing(self.state) => crate::cad::property_manager_ui(self.state, ui),
            Pane::Pipeline => crate::panels::pipeline_ui(self.state, ui),
            Pane::Properties => crate::panels::properties_ui(self.state, ui),
            Pane::Viewport => self.state.view_pane_ui(ui, 0),
            Pane::View(id) => {
                if *id != self.state.active_view {
                    crate::cad::stale_banner(self.state, ui);
                }
                self.state.view_pane_ui(ui, *id)
            }
            Pane::Python => crate::console::console_ui(self.state, ui),
            Pane::Notebook => crate::notebook::notebook_ui(self.state, ui),
            Pane::Assistant => crate::assistant::assistant_ui(self.state, ui),
            Pane::Log => crate::panels::log_ui(self.state, ui),
            Pane::Spreadsheet => {
                crate::cad::stale_banner(self.state, ui);
                crate::panels::spreadsheet_ui(self.state, ui)
            }
            Pane::Information => crate::panels::information_ui(self.state, ui),
            Pane::Jobs => crate::jobs_panel::jobs_ui(self.state, ui),
            Pane::Charts => {
                crate::cad::stale_banner(self.state, ui);
                crate::charts::charts_ui(self.state, ui)
            }
            Pane::Chart(id) => {
                crate::cad::stale_banner(self.state, ui);
                crate::charts::chart_view_ui(self.state, ui, *id)
            }
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
        match pane {
            Pane::Charts => return format!("{} {}", pane.icon(), crate::charts::chart_title(self.state, 0)).into(),
            Pane::Chart(id) => return format!("{} {}", pane.icon(), crate::charts::chart_title(self.state, *id)).into(),
            _ => {}
        }
        if crate::cad::editing(self.state) {
            match pane {
                Pane::Pipeline => return format!("{} Model tree", pane.icon()).into(),
                Pane::Properties => return format!("{} Feature panel", pane.icon()).into(),
                _ => {}
            }
        }
        format!("{} {}", pane.icon(), pane.title()).into()
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions { all_panes_must_have_tabs: true, ..Default::default() }
    }

    fn is_tab_closable(&self, _tiles: &egui_tiles::Tiles<Pane>, _tile_id: egui_tiles::TileId) -> bool {
        true
    }

    // Tabs, their close buttons and the bars between panels sized for a finger in the touch
    // interfaces (the other values are the dock's own).
    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        if crate::touch::mode().is_touch() {
            crate::touch::TOUCH_HEIGHT
        } else {
            24.0
        }
    }

    fn close_button_outer_size(&self) -> f32 {
        if crate::touch::mode().is_touch() {
            20.0
        } else {
            12.0
        }
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        if crate::touch::mode().is_touch() {
            4.0
        } else {
            1.0
        }
    }
}

impl eframe::App for AuraApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx_owned = root_ui.ctx().clone();
        let ctx = &ctx_owned;
        let mode = crate::touch::resolve(ctx, self.state.settings.interface);
        crate::touch::sync_style(ctx, mode, self.state.settings.theme, self.state.settings.ui_scale);
        self.sync_interface_layout(mode);
        if !self.layout_checked {
            self.layout_checked = true;
            // Phones and small windows start with the compact layout.
            if !mode.is_touch() && !self.state.auto_layout && ctx.content_rect().width() < 820.0 {
                self.tree = compact_tree();
            }
        }
        if self.state.auto_layout {
            // The layout follows the shape of the whole window (not of what an on-screen
            // keyboard leaves of it).
            let class = crate::touch::layout_class(mode, ctx.viewport_rect().size());
            if self.state.layout_class != Some(class) {
                if self.state.layout_class.is_some() || self.keep_open {
                    reshape_tree(&mut self.tree, class);
                } else {
                    self.tree = class_tree(class, None);
                }
                self.state.layout_class = Some(class);
            }
        }
        self.keep_open = false;
        self.state.open_panes = Pane::ALL.iter().copied().filter(|p| self.tree.tiles.find_pane(p).is_some()).collect();
        let st = &mut self.state;
        st.viewport.frame_begin(ctx, frame);
        st.viewport.settings.background = st.settings.background_for(ctx.theme() == egui::Theme::Dark);
        if st.viewport.settings.lighting != st.settings.lighting {
            st.viewport.settings.lighting = st.settings.lighting.clone();
        }
        for v in st.views.values_mut() {
            if v.settings.lighting != st.settings.lighting {
                v.settings.lighting = st.settings.lighting.clone();
            }
        }
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
            // Numbered outputs open together as one series: skip the ones a series covers.
            let mut covered: std::collections::HashSet<PathBuf> = Default::default();
            let mut opened = 0;
            for o in outs {
                let canon = std::fs::canonicalize(&o).unwrap_or(o.clone());
                if covered.contains(&canon) || opened >= 8 {
                    continue;
                }
                opened += 1;
                let file = o.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                let before = st.session.lock().active;
                st.open_file(o);
                // Name the result after its run.
                let mut s = st.session.lock();
                if let Some(id) = s.active.filter(|a| Some(*a) != before) {
                    s.rename(id, format!("{name}: {file}"));
                    if let Some(aurafx_pipeline::session::NodeKind::Reader { loaded: aurafx_io::Loaded::Series(series), .. }) =
                        s.nodes.get(&id).map(|n| &n.kind)
                    {
                        for step in &series.steps {
                            if let aurafx_data::TimeStepRef::File(p) = step {
                                covered.insert(std::fs::canonicalize(p).unwrap_or(p.clone()));
                            }
                        }
                    }
                }
            }
        }
        if aurafx_jobs::global().lock().jobs.iter().any(|j| j.state.is_active()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        crate::cad::tick(st, ctx);
        // Keep the pipeline on disk as it changes (at most every 30 s), so a crash or a power cut
        // loses little: File > Restore Last Session brings it back.
        if st.autosaved.0.elapsed().as_secs() >= 30 {
            st.autosaved.0 = std::time::Instant::now();
            let cam = st.camera_state();
            let s = st.session.lock();
            if s.revision != st.autosaved.1 && !s.nodes.is_empty() && s.cad_edit.is_none() {
                let p = State::last_session_path();
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                if s.save_state(&p, Some(cam)).is_ok() {
                    st.autosaved.1 = s.revision;
                }
            }
        }
        crate::source_dialog::window(st, ctx);
        crate::assistant::describe_window(st, ctx);
        {
            let session = st.session.clone();
            st.assistant.tick_view(&session);
        }
        let cad_open = crate::cad::editing(st);
        if cad_open && !st.touch.cad_open && mode.is_phone() {
            st.toast("CAD modeling is made for a larger screen. On a phone every command is still here: the ribbon's rows scroll sideways, and the model tree and the feature panel are tabs.", false);
        }
        st.touch.cad_open = cad_open;
        // Nothing of the app under the system's status bar, cut-out or gesture bar (a tablet or a
        // phone; no strips on a desktop system).
        crate::touch::safe_area_strips(root_ui);
        if !mode.is_touch() {
            st.touch.menu_open = false;
            st.touch.view_maximized = false;
        }
        egui::Panel::top("menu").show(root_ui, |ui| match mode {
            crate::touch::Mode::Desktop => st.menu_bar(ui),
            crate::touch::Mode::Tablet => st.menu_bar_tablet(ui),
            crate::touch::Mode::Phone => st.menu_bar_phone(ui),
        });
        // The phone's menu list and a view on the whole screen put the tool rows away.
        let rows = !(st.touch.menu_open && mode.is_phone()) && !st.touch.view_maximized;
        if rows {
            egui::Panel::top("toolbar").show(root_ui, |ui| {
                if cad_open {
                    crate::cad::ribbon_ui(st, ui);
                } else {
                    st.toolbar(ui);
                    st.time_bar(ui);
                }
            });
            egui::Panel::bottom("status").show(root_ui, |ui| st.status_bar(ui));
            if cad_open {
                egui::Panel::bottom("cad_timeline").show(root_ui, |ui| crate::cad::timeline_ui(st, ui));
            }
        }

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
                        "high contrast" | "high-contrast" | "contrast" => crate::theme::ThemeMode::HighContrast,
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
                        "automatic" | "auto" => LayoutPreset::Automatic,
                        "wide" => LayoutPreset::Wide,
                        "tall" => LayoutPreset::Tall,
                        "tall2" | "tall two" => LayoutPreset::TallTwo,
                        _ => LayoutPreset::Full,
                    };
                    self.state.layout_actions.push(LayoutAction::Preset(p));
                }
                "hide" => {
                    if let Some(p) = find(arg) {
                        self.state.layout_actions.push(LayoutAction::Hide(p));
                    }
                }
                "show" if arg.trim().eq_ignore_ascii_case("welcome") => self.state.show_welcome = true,
                // "show menu [name]": the phone's menu list, with that menu's items shown.
                "show" if arg.trim().to_ascii_lowercase().starts_with("menu") => {
                    let name = arg.trim()[4..].trim();
                    self.state.touch.menu_open = mode.is_phone();
                    self.state.touch.menu_section = self.state.top_menus().iter().find(|m| m.label.eq_ignore_ascii_case(name)).map(|m| m.label);
                }
                // "show whole view": the 3D view on the whole screen (the touch interfaces).
                "show" if arg.trim().eq_ignore_ascii_case("whole view") => self.state.touch.view_maximized = mode.is_touch(),
                // "show cad search [text]": the CAD command search with that text typed.
                "show" if arg.trim().to_ascii_lowercase().starts_with("cad search") => {
                    if crate::cad::editing(&self.state) {
                        self.state.cad.search =
                            Some(crate::cad::CommandSearch { query: arg.trim()[10..].trim().to_string(), selected: 0, at: Some(egui::pos2(520.0, 230.0)) });
                    }
                }
                // "show palette [text]": the command palette, with that text typed.
                "show" if arg.trim().to_ascii_lowercase().starts_with("palette") => {
                    self.state.palette.open = true;
                    self.state.palette.query = arg.trim()[7..].trim().to_string();
                }
                "show" if arg.eq_ignore_ascii_case("plugins") => {
                    self.state.plugins.open = true;
                }
                "lighting" => {
                    // A JSON object of lighting options (afx.lighting).
                    if let Err(e) = crate::lighting_ui::apply_json(&mut self.state.settings.lighting, arg) {
                        tracing::warn!("lighting: {e:#}");
                    }
                }
                "show" if arg.eq_ignore_ascii_case("server") || arg.eq_ignore_ascii_case("open on server") => {
                    self.state.server.open = true;
                }
                "show" if arg.eq_ignore_ascii_case("colormap") || arg.eq_ignore_ascii_case("color map editor") => {
                    self.state.cmap_editor.open = true;
                }
                "show" if arg.eq_ignore_ascii_case("goal statistics") => self.state.goal_stats.open = true,
                "show" if arg.eq_ignore_ascii_case("settings") => self.state.show_settings = true,
                // "show notebook run": the Notebook panel with every cell run.
                // "show notebook run [file.ipynb]": the Notebook panel (with that notebook) with every cell run.
                "show" if arg.trim().to_ascii_lowercase().starts_with("notebook run") => {
                    self.state.layout_actions.push(LayoutAction::Show(Pane::Notebook));
                    let f = arg.trim()[12..].trim();
                    if !f.is_empty() {
                        self.state.notebook.open(PathBuf::from(f));
                    }
                    self.state.notebook.run_all();
                }
                // "show plugin <title>": a plugin panel.
                "show" if arg.trim().to_ascii_lowercase().starts_with("plugin ") => {
                    let t = arg.trim()[7..].trim().to_string();
                    let tx = self.state.python.sender();
                    self.state.plugin_panels.open_panel(&t, &tx);
                }
                // The Save Screenshot window; "screenshot example" also chooses a 16 : 9 picture on
                // a gradient (the user guide's picture of the window).
                "show" if arg.trim().to_ascii_lowercase().starts_with("screenshot") => {
                    self.state.save_screenshot_dialog();
                    if arg.trim().eq_ignore_ascii_case("screenshot example") {
                        let o = &mut self.state.screenshot_dialog.options;
                        o.bg = crate::picture_options::Bg::Gradient;
                        o.shape = crate::picture_options::Shape::Ratio(16, 9);
                        (o.width, o.height) = (1920, 1080);
                    }
                }
                "show" if arg.eq_ignore_ascii_case("animation") => {
                    self.state.anim_dialog.open = true;
                    self.state.anim_dialog.mode = crate::animation::AnimMode::Keyframes;
                }
                "show" if arg.trim().eq_ignore_ascii_case("remote") => self.state.server.open = true,
                // "show remote 8 srun": the Remote window with a process count and a launcher.
                "show" if arg.trim().to_ascii_lowercase().starts_with("remote ") => {
                    // "show remote 8 srun [folder]": also the folder the window browses.
                    let w: Vec<&str> = arg.trim()[7..].split_whitespace().collect();
                    self.state.server.set_processes(w.first().and_then(|n| n.parse().ok()).unwrap_or(1), w.get(1).copied().unwrap_or(""));
                    if let Some(f) = w.get(2) {
                        self.state.server.path = f.to_string();
                        self.state.server.entries.clear();
                    }
                    self.state.server.open = true;
                }
                // "assistant_view_on": tick the assistant's camera box (for the guide's picture).
                "assistant_view_on" => {
                    self.state.assistant.include_view = true;
                    self.state.assistant.input = "What stands out in this view?".into();
                }
                // "remote_window <folder>": the Remote window, browsing that folder.
                // "remote_processes <n> [launcher]": the process count and launcher of the Remote window.
                "remote_processes" => {
                    let (n, l) = arg.trim().split_once(' ').unwrap_or((arg.trim(), ""));
                    self.state.server.set_processes(n.parse().unwrap_or(1), l.trim());
                }
                "remote_window" => {
                    self.state.server.path = arg.trim().to_string();
                    self.state.server.entries.clear();
                    self.state.server.open = true;
                }
                // "source_dialog <id>": the window that adds a source (its standard or changed settings).
                "source_dialog" => {
                    if let Some(def) = aurafx_filters::find(arg.trim()).filter(|d| d.is_source) {
                        self.state.source_dialog = crate::source_dialog::SourceDialog::new(def.id);
                    }
                }
                // "about [licences]": the About window, or its third-party licences.
                "about" => {
                    self.state.show_about = true;
                    self.state.show_licences = arg.trim() == "licences";
                }
                // "licence_find <text>": the find box of the third-party licences.
                "licence_find" => {
                    self.state.show_about = true;
                    self.state.show_licences = true;
                    self.state.licence_find = arg.trim().to_string();
                }
                "show" => {
                    if let Some(p) = find(arg) {
                        self.state.layout_actions.push(LayoutAction::Show(p));
                    }
                }
                // "chart_open <n>" / "chart_close <n>": the pane of chart view n (0 = the Charts panel).
                "chart_open" | "chart_close" => {
                    if let Ok(id) = arg.trim().parse::<u32>() {
                        let pane = if id == 0 { Pane::Charts } else { Pane::Chart(id) };
                        self.state.layout_actions.push(match (verb, id) {
                            ("chart_open", 0) => LayoutAction::Show(pane),
                            ("chart_open", _) => LayoutAction::OpenChart(id),
                            _ => LayoutAction::Hide(pane),
                        });
                    }
                }
                // "sim_template <id>": choose a template in the Simulations panel.
                "sim_template" => {
                    if let Some(i) = aurafx_jobs::templates::TEMPLATES.iter().position(|t| t.id == arg.trim() || t.label.eq_ignore_ascii_case(arg.trim())) {
                        let t = &aurafx_jobs::templates::TEMPLATES[i];
                        let p = &mut self.state.jobs_panel;
                        p.template = i;
                        p.exe = t.default_exe.into();
                        p.input = t.default_input.into();
                        p.outputs = t.outputs.into();
                        p.name = format!("{} run", t.label);
                        p.command_override = None;
                    }
                    if let Some(p) = find("Simulations") {
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
                // What the Apply button does: the downstream check, then the dialog when needed.
                "cad_apply" => {
                    if crate::cad::editing(&self.state) {
                        crate::cad::apply(&mut self.state, arg == "keep");
                    }
                }
                // "cad_pick fillet|chamfer|shell|draft|mirror|pattern": open the edge or face picker; "cad_tab <name>": a ribbon tab.
                "cad_pick" => {
                    if crate::cad::editing(&self.state) {
                        // "cad_pick repair <feature>": the faces a feature's sketch can go on.
                        let repair = arg.trim().strip_prefix("repair ").and_then(|name| {
                            self.state.session.lock().cad_edit.as_ref().and_then(|e| e.active().features.iter().find(|f| f.name == name.trim()).map(|f| f.id))
                        });
                        let kind = match arg.trim() {
                            _ if repair.is_some() => crate::cad::PickKind::RepairFace(repair.unwrap()),
                            "chamfer" => crate::cad::PickKind::Chamfer,
                            "shell" => crate::cad::PickKind::Shell,
                            "draft" => crate::cad::PickKind::Draft,
                            "mirror" => crate::cad::PickKind::MirrorFace,
                            "pattern" => crate::cad::PickKind::PatternFace,
                            "sketch" => crate::cad::PickKind::SketchFace,
                            "mate" => crate::cad::PickKind::Mate,
                            _ => crate::cad::PickKind::Fillet,
                        };
                        crate::cad::open_picker(&mut self.state, kind);
                    }
                }
                // "cad_pick_choose <label>": tick an item of the open picker; "cad_sketch_saved <name>": open the
                // canvas on a sketch kept with the part; "cad_edit_in <node id>": Edit in CAD on any node.
                "cad_pick_choose" => crate::cad::pick_choose(&mut self.state, arg.trim()),
                "cad_sketch_saved" => {
                    if crate::cad::editing(&self.state) {
                        crate::cad::sketch_saved(&mut self.state, arg.trim());
                    }
                }
                "cad_edit_in" => {
                    if let Ok(id) = arg.trim().parse::<u64>() {
                        crate::cad::enter_from(&mut self.state, id);
                    }
                }
                // "cad_component <name>": select a component in the feature tree; "cad_fit": run Check fit.
                "cad_component" => {
                    if crate::cad::editing(&self.state) {
                        self.state.cad.component = Some(arg.trim().to_string());
                    }
                }
                "cad_fit" => {
                    if crate::cad::editing(&self.state) {
                        crate::cad::check_fit(&mut self.state);
                    }
                }
                "cad_tab" => {
                    if let Some(t) = crate::cad::RibbonTab::ALL.iter().find(|t| t.label().eq_ignore_ascii_case(arg.trim())) {
                        self.state.cad.tab = *t;
                    }
                }
                // "cad_sketch_new <json>": open the sketch canvas on a new sketch (plane, offset, items).
                "cad_sketch_new" => {
                    if crate::cad::editing(&self.state) {
                        let spec: serde_json::Value = serde_json::from_str(arg).unwrap_or_default();
                        let named = spec.get("plane").and_then(|p| p.as_str()).and_then(|n| {
                            self.state.session.lock().cad_edit.as_ref().and_then(|e| e.active().planes.iter().find(|p| p.name.eq_ignore_ascii_case(n)).cloned())
                        });
                        if spec.get("items").is_some() {
                            crate::cad::sketch_from_spec(&mut self.state, &spec);
                        } else if let Some(p) = named {
                            crate::cad::sketch_on_frame(&mut self.state, p.frame, Some(p.name), false);
                        } else {
                            let plane = match spec.get("plane").and_then(|p| p.as_str()).unwrap_or("XY").to_uppercase().as_str() {
                                "XZ" => aurafx_cad::SketchPlane::XZ,
                                "YZ" => aurafx_cad::SketchPlane::YZ,
                                _ => aurafx_cad::SketchPlane::XY,
                            };
                            crate::cad::sketch_new(&mut self.state, plane, spec.get("offset").and_then(|o| o.as_f64()).unwrap_or(0.0), false);
                        }
                    }
                }
                // "cad_sketch_cursor <x> <y> [line <point>]": for the guide's pictures, where the cursor
                // is on the sketch (sketch units) when the mouse is not over it, optionally drawing a
                // line from a point; "cad_sketch_dim_edit <i>": the value editor of dimension i;
                // "cad_plane_form": the New reference plane form.
                "cad_sketch_cursor" => {
                    let w: Vec<&str> = arg.split_whitespace().collect();
                    if let (Some(e), Some(x), Some(y)) =
                        (self.state.cad.sketch.as_mut(), w.first().and_then(|v| v.parse().ok()), w.get(1).and_then(|v| v.parse().ok()))
                    {
                        e.demo_cursor = Some(glam::DVec2::new(x, y));
                        if w.get(2) == Some(&"line") {
                            e.tool = crate::cad::SketchTool::Line;
                            e.set_chain(w.get(3).and_then(|v| v.parse().ok()));
                        }
                    }
                }
                "cad_sketch_dim_edit" => {
                    let first = self.state.cad.sketch.as_mut().and_then(|e| e.sketch.constraints.iter_mut().position(|c| c.dim_mut().is_some()));
                    let idx = if arg.trim() == "first" { first.ok_or(()) } else { arg.trim().parse::<usize>().map_err(|_| ()) };
                    if let (Some(e), Ok(i)) = (self.state.cad.sketch.as_mut(), idx) {
                        let cur =
                            e.sketch.constraints.get_mut(i).and_then(|c| c.dim_mut()).map(|d| d.expr.clone().unwrap_or_else(|| crate::cad::num_text(d.value)));
                        if let Some(t) = cur {
                            e.dim_edit = Some((i, t));
                        }
                    }
                }
                // "shortcut <action> <keys, comma separated>": bind keys to an action (empty: none).
                "shortcut" => {
                    let (a, k) = arg.split_once(' ').unwrap_or((arg, ""));
                    let combos: Vec<String> = k.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
                    match self.state.settings.shortcuts.set(a.trim(), combos) {
                        Ok(()) => self.state.settings.save(),
                        Err(e) => self.state.toast(e, true),
                    }
                }
                // "settings [section]": the Settings window; "assistant_prompt <name>": a saved prompt put in
                // the assistant's box (for the guide's pictures).
                "settings" => self.state.show_settings = true,
                "assistant_prompt" => {
                    let object = self.state.session.lock().active_node().map(|n| n.name.clone()).unwrap_or_else(|| "the data".into());
                    if let Some(p) = self.state.settings.ai.prompts.iter().find(|p| p.name.eq_ignore_ascii_case(arg.trim())) {
                        self.state.assistant.input = p.fill(&object);
                    }
                }
                "vim_keys" => {
                    self.state.settings.shortcuts.vim_keys = arg.trim() != "off";
                    self.state.settings.save();
                }
                "cad_plane_form" => {
                    if crate::cad::editing(&self.state) {
                        crate::cad::open_plane_form(&mut self.state);
                    }
                }
                // "cad_sketch_edit <feature>": open the sketch canvas on a feature's sketch.
                "cad_sketch_edit" => {
                    let id = self.state.session.lock().cad_edit.as_ref().and_then(|e| e.active().features.iter().find(|f| f.name == arg.trim()).map(|f| f.id));
                    match id {
                        Some(id) => crate::cad::sketch_edit(&mut self.state, id),
                        None => self.state.toast(format!("No feature named '{}'", arg.trim()), true),
                    }
                }
                "link" => self.state.link_all_views(arg != "off"),
                "match" => self.state.camera_match_from = Some(self.state.active_view),
                "activate" => {
                    if let Ok(id) = arg.parse::<u32>() {
                        self.state.pending_activate = Some(id);
                    }
                }
                "ask" | "ask-agent" | "ask-view" | "ask-agent-view" => {
                    tracing::info!("AI: {verb} from a script: {:.60}", arg);
                    let desc = self.state.session.lock().describe();
                    if verb.starts_with("ask-agent") {
                        self.state.settings.ai_agent_mode = true;
                        self.state.assistant.agent_active = true;
                        self.state.assistant.reset_agent_steps();
                    }
                    if verb.ends_with("-view") {
                        let session = self.state.session.clone();
                        self.state.assistant.send_with_view(arg.to_string(), desc, &session);
                    } else {
                        self.state.assistant.send(arg.to_string(), desc);
                    }
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
                LayoutAction::Preset(LayoutPreset::Automatic) => {
                    // Rebuilt for the window's shape at the start of the next frame.
                    self.state.auto_layout = true;
                    self.state.layout_class = None;
                }
                LayoutAction::Preset(pr) => {
                    // A layout chosen by name stays as it is when the window changes shape.
                    self.state.auto_layout = false;
                    self.state.layout_class = None;
                    apply_preset(&mut self.tree, pr)
                }
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
                LayoutAction::OpenChart(id) => {
                    let pane = Pane::Chart(id);
                    if self.tree.tiles.find_pane(&pane).is_none() {
                        // Beside the newest chart view (or the Charts panel), else below the 3D view.
                        let anchor = self
                            .state
                            .open_chart_panes
                            .iter()
                            .rev()
                            .find_map(|c| self.tree.tiles.find_pane(&Pane::Chart(*c)))
                            .or_else(|| self.tree.tiles.find_pane(&Pane::Charts));
                        let new_pane = self.tree.tiles.insert_pane(pane);
                        let tabs = self.tree.tiles.insert_tab_tile(vec![new_pane]);
                        match anchor.and_then(|a| self.tree.tiles.parent_of(a)) {
                            Some(a) => split_beside(&mut self.tree, a, tabs, egui_tiles::LinearDir::Horizontal, false, 0.5),
                            None => {
                                let a = self.tree.tiles.find_pane(&Pane::Viewport).and_then(|v| self.tree.tiles.parent_of(v)).or(self.tree.root());
                                match a {
                                    Some(a) => split_beside(&mut self.tree, a, tabs, egui_tiles::LinearDir::Vertical, false, 0.35),
                                    None => self.tree.root = Some(tabs),
                                }
                            }
                        }
                    }
                    self.tree.make_active(|_, t| matches!(t, egui_tiles::Tile::Pane(p) if *p == pane));
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
        self.state.capture_poll_session();
        self.state.capture_begin_frame();
        if mode.is_touch() && (self.state.touch.menu_open || self.state.touch.view_maximized) {
            let menu = self.state.touch.menu_open && mode.is_phone();
            egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin::same(if menu { 8 } else { 0 })).fill(fill)).show(root_ui, |ui| {
                if menu {
                    self.state.menu_list_ui(ui);
                } else {
                    // The active 3D view alone.
                    let id = self.state.active_view;
                    self.state.capture.pane_rects.push((crate::views::pane_for(id), ui.max_rect()));
                    self.state.view_pane_ui(ui, id);
                }
            });
        } else {
            egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin::same(2)).fill(fill)).show(root_ui, |ui| {
                let mut behavior = TreeBehavior { state: &mut self.state };
                self.tree.ui(&mut behavior, ui);
            });
        }
        self.state.capture_end_frame(ctx);

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
        // Chart panes and the session's chart views: a pane the user closed closes its view.
        let open_charts: Vec<u32> =
            self.tree.tiles.iter().filter_map(|(_, t)| if let egui_tiles::Tile::Pane(Pane::Chart(id)) = t { Some(*id) } else { None }).collect();
        {
            let closed: Vec<u32> = self.state.open_chart_panes.iter().copied().filter(|id| !open_charts.contains(id)).collect();
            if !closed.is_empty() {
                let mut s = self.state.session.lock();
                s.chart_views.retain(|v| !closed.contains(&v.id));
                s.revision += 1;
            }
        }
        self.state.open_chart_panes = open_charts;
        {
            let ids = self.state.view_ids();
            let mut s = self.state.session.lock();
            s.view_ids = ids;
            s.active_view = self.state.active_view;
            s.app_frame += 1;
            let a = &self.state.assistant;
            s.assistant = aurafx_pipeline::AssistantStatus {
                busy: a.busy(),
                agent: a.agent_active,
                messages: a.messages.len(),
                last: a.messages.iter().rev().find(|m| m.role == aurafx_ai::Role::Assistant).map(|m| m.content.clone()).unwrap_or_default(),
                error: a.error.clone(),
                frame: s.app_frame,
                history: a
                    .messages
                    .iter()
                    .rev()
                    .take(12)
                    .rev()
                    .map(|m| (format!("{:?}", m.role).to_lowercase(), m.content.chars().take(2000).collect::<String>()))
                    .collect(),
            };
        }
        let st = &mut self.state;
        st.animation_tick(ctx);
        st.animation_poll();
        crate::animation::dialog_ui(st, ctx);
        crate::screenshot_ui::dialog_ui(st, ctx);
        crate::colormap_editor::editor_ui(st, ctx);
        crate::goal_stats::window(st, ctx);
        crate::plugin_panels::panels_ui(st, ctx);
        st.plugins_tick(ctx);
        crate::plugins_ui::plugins_ui(st, ctx);
        crate::server_ui::server_ui(st, ctx);
        if let Some(p) = crate::dialogs::file_browser_ui(&mut st.file_browser, ctx) {
            st.open_file(p);
        }
        crate::cad::dialogs_ui(st, ctx);
        crate::welcome::window(st, ctx);
        crate::welcome::crash_window(st, ctx);
        crate::palette::palette_ui(st, ctx);
        st.settings_window(ctx);
        st.about_window(ctx);
        st.toasts_ui(ctx);
        // Rename dialog.
        if let Some((id, mut name)) = st.rename_target.take() {
            let mut keep = true;
            let mut done = false;
            egui::Window::new("Rename").collapsible(false).resizable(false).show_fit(ctx, |ui| {
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

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        crate::touch::raw_input_hook(ctx, raw_input, self.state.settings.keyboard_room);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // The desktop's layout and the touch interfaces' are kept apart: trying Phone on a
        // desktop machine leaves the desktop layout as it was.
        let (desktop, touch) = if self.touch_layout { (self.other_tree.as_ref(), Some(&self.tree)) } else { (Some(&self.tree), self.other_tree.as_ref()) };
        let (desktop_auto, touch_auto) = if self.touch_layout { (self.other_auto, self.state.auto_layout) } else { (self.state.auto_layout, self.other_auto) };
        if let Some(s) = desktop.and_then(|t| serde_json::to_string(t).ok()) {
            storage.set_string("layout_v4", s);
        }
        if let Some(s) = touch.and_then(|t| serde_json::to_string(t).ok()) {
            storage.set_string("layout_touch_v1", s);
        }
        storage.set_string("layout_auto", desktop_auto.to_string());
        storage.set_string("layout_touch_auto", touch_auto.to_string());
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
