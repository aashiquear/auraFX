//! Session: the pipeline graph plus everything the UI, Python and AI need to see.

use crate::display::{ColorBy, DisplayProps};
use anyhow::{anyhow, Result};
use aurafx_data::{DataSet, DataSetInfo, TimeSeries};
use aurafx_filters::{FilterDef, Params};
use aurafx_io::Loaded;
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub type NodeId = u64;
pub type SharedSession = Arc<Mutex<Session>>;

/// Memory a temporal filter keeps between runs (e.g. particles already advanced).
#[derive(Clone, Default)]
pub struct TemporalState(pub Arc<std::sync::Mutex<Option<Box<dyn std::any::Any + Send>>>>);

impl std::fmt::Debug for TemporalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TemporalState")
    }
}

/// The upstream pipeline of a temporal filter in a form that can be evaluated at any time on
/// a worker thread: readers load the step for the requested time, filters re-run.
enum Plan {
    /// Does not change with time.
    Fixed(Arc<DataSet>),
    /// A reader with time steps; `current` is the step it already holds.
    Series { series: TimeSeries, current: Option<(usize, Arc<DataSet>)>, piece: Option<(usize, usize)> },
    /// An ordinary filter (the first input is the pipeline input).
    Run { def: Arc<FilterDef>, params: Params, inputs: Vec<Plan> },
    /// A temporal filter further upstream.
    Temporal { def: Arc<FilterDef>, params: Params, input: Box<Plan>, extra: Vec<Arc<DataSet>>, state: TemporalState },
}

type PlanCache = std::sync::Mutex<std::collections::HashMap<(usize, u64), Arc<DataSet>>>;

impl Plan {
    fn times(&self) -> Vec<f64> {
        match self {
            Plan::Fixed(_) => Vec::new(),
            Plan::Series { series, .. } => series.times.clone(),
            Plan::Run { inputs, .. } => union_times(inputs.iter().map(|p| p.times())),
            Plan::Temporal { def, params, input, .. } => (def.temporal.unwrap().output_times)(params, &input.times()),
        }
    }

    fn key(&self, h: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        match self {
            Plan::Fixed(d) => (Arc::as_ptr(d) as usize).hash(h),
            Plan::Series { series, .. } => {
                for (t, s) in series.times.iter().zip(&series.steps) {
                    t.to_bits().hash(h);
                    match s {
                        aurafx_data::TimeStepRef::Loaded(d) => (Arc::as_ptr(d) as usize).hash(h),
                        other => format!("{other:?}").hash(h),
                    }
                }
            }
            Plan::Run { def, params, inputs } => {
                def.id.hash(h);
                serde_json::to_string(params).unwrap_or_default().hash(h);
                for i in inputs {
                    i.key(h);
                }
            }
            Plan::Temporal { def, params, input, extra, .. } => {
                def.id.hash(h);
                serde_json::to_string(params).unwrap_or_default().hash(h);
                input.key(h);
                for e in extra {
                    (Arc::as_ptr(e) as usize).hash(h);
                }
            }
        }
    }

    fn fingerprint(&self) -> u64 {
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.key(&mut h);
        h.finish()
    }

    fn eval(&self, t: f64, cache: &PlanCache) -> Result<Arc<DataSet>> {
        let ck = (self as *const Plan as usize, t.to_bits());
        if let Some(d) = cache.lock().unwrap().get(&ck) {
            return Ok(d.clone());
        }
        let out = match self {
            Plan::Fixed(d) => return Ok(d.clone()),
            Plan::Series { series, current, piece } => {
                let eps = 1e-12 * (1.0 + t.abs());
                let k = series.times.iter().rposition(|&x| x <= t + eps).unwrap_or(0);
                match current {
                    Some((i, d)) if *i == k => d.clone(),
                    _ => aurafx_io::load_step_piece(series.steps.get(k).ok_or_else(|| anyhow!("time step {k} missing"))?, *piece)?,
                }
            }
            Plan::Run { def, params, inputs } => {
                let ins: Vec<Arc<DataSet>> = inputs.iter().map(|p| p.eval(t, cache)).collect::<Result<_>>()?;
                let refs: Vec<&DataSet> = ins.iter().map(|d| &**d).collect();
                Arc::new(def.run_multi(&refs, params)?)
            }
            Plan::Temporal { def, params, input, extra, state } => Arc::new(run_temporal(def, params, input, extra, state, t, cache)?),
        };
        cache.lock().unwrap().insert(ck, out.clone());
        Ok(out)
    }
}

/// Server pictures are composited as opaque surfaces: translucent, volume-rendered and
/// Surface LIC objects (a screen-space pass) are drawn here.
fn server_can_draw(d: &DisplayProps) -> bool {
    !matches!(d.representation, crate::display::Representation::Volume | crate::display::Representation::SurfaceLic) && d.opacity >= 1.0
}

/// Built-in filters run on a server; Python and plugin filters only here.
pub fn runs_on_server(def: &FilterDef) -> bool {
    def.ext.is_none() && !def.is_source && !matches!(def.id, "programmable_filter" | "programmable_source" | "python_calculator")
}

fn union_times(lists: impl Iterator<Item = Vec<f64>>) -> Vec<f64> {
    let mut t: Vec<f64> = lists.flatten().collect();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    t.dedup_by(|a, b| (*a - *b).abs() <= 1e-12 * (1.0 + a.abs()));
    t
}

fn run_temporal(def: &FilterDef, params: &Params, input: &Plan, extra: &[Arc<DataSet>], state: &TemporalState, t: f64, cache: &PlanCache) -> Result<DataSet> {
    let times = input.times();
    let fetch = |tau: f64| input.eval(tau, cache);
    let ctx = aurafx_filters::temporal::TemporalCtx { time: t, input_times: &times, fetch: &fetch, extra, state: &state.0, key: input.fingerprint() };
    def.run_temporal(&ctx, params)
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Reader {
        path: PathBuf,
        loaded: Loaded,
    },
    /// A file on another machine, read (and filtered) by an aurafx-server there.
    Remote {
        host: aurafx_jobs::Host,
        path: String,
        times: Vec<f64>,
    },
    Source {
        def: Arc<FilterDef>,
    },
    Filter {
        def: Arc<FilterDef>,
    },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub params: Params,
    pub input: Option<NodeId>,
    /// Further inputs of multi-input filters (Append Datasets, Resample With Dataset).
    pub extra_inputs: Vec<NodeId>,
    pub visible: bool,
    pub display: DisplayProps,
    pub output: Option<Arc<DataSet>>,
    pub info: Option<DataSetInfo>,
    pub error: Option<String>,
    pub dirty: bool,
    pub running: bool,
    pub last_exec_ms: f64,
    /// Which time step index the current output corresponds to (readers only).
    pub output_time_index: Option<usize>,
    /// Whether the automatic "color by active scalar" default has been applied.
    pub auto_colored: bool,
    /// Reload automatically when the file changes (live view of a running simulation).
    pub watch: bool,
    /// When watching, jump to the newest time step after a reload.
    pub follow_latest: bool,
    pub(crate) file_signature: u64,
    /// Kept between runs of temporal filters.
    pub temporal_state: TemporalState,
    /// (upstream + parameter fingerprint, time) of a temporal filter's current output, so a
    /// time change does not re-run filters whose result does not depend on it.
    pub(crate) temporal_key: Option<(u64, f64)>,
    /// Set when the node runs on a server: what came back.
    pub remote: Option<RemoteStatus>,
    /// Bring the whole output of a server node over, however big.
    pub fetch_full: bool,
}

/// How a node that runs on a server was delivered.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteStatus {
    pub host: String,
    pub payload: crate::remote::Payload,
    /// Size of the full output on the server.
    pub full_bytes: u64,
    /// Bytes transferred (compressed).
    pub sent_bytes: u64,
    pub server_ms: f64,
}

impl Node {
    pub fn new(id: NodeId, name: String, kind: NodeKind, params: Params, input: Option<NodeId>) -> Self {
        Node {
            id,
            name,
            kind,
            params,
            input,
            extra_inputs: Vec::new(),
            visible: true,
            display: DisplayProps::default(),
            output: None,
            info: None,
            error: None,
            dirty: true,
            running: false,
            last_exec_ms: 0.0,
            output_time_index: None,
            auto_colored: false,
            watch: false,
            follow_latest: true,
            file_signature: 0,
            temporal_state: TemporalState::default(),
            temporal_key: None,
            remote: None,
            fetch_full: false,
        }
    }

    pub fn def(&self) -> Option<&Arc<FilterDef>> {
        match &self.kind {
            NodeKind::Source { def } | NodeKind::Filter { def } => Some(def),
            NodeKind::Reader { .. } | NodeKind::Remote { .. } => None,
        }
    }
    pub fn type_label(&self) -> String {
        match &self.kind {
            NodeKind::Reader { path, .. } => path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default(),
            NodeKind::Remote { path, host, .. } => {
                let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
                format!("{ext} on {}", host.name)
            }
            NodeKind::Source { def } | NodeKind::Filter { def } => def.label.to_string(),
        }
    }
    pub fn series(&self) -> Option<&TimeSeries> {
        match &self.kind {
            NodeKind::Reader { loaded: Loaded::Series(s), .. } => Some(s),
            _ => None,
        }
    }
}

/// A line series for the chart panel (from Python or "plot over time").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Chart {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub series: Vec<ChartSeries>,
    pub revision: u64,
}

/// A text label drawn over the 3D views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub text: String,
    /// "upper left", "upper right", "lower left", "lower right", "upper center", "lower center".
    pub corner: String,
    pub size: f32,
}

/// Cells or points selected in the 3D view (flattened indices across blocks).
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub node: NodeId,
    pub assoc: aurafx_filters::Association,
    pub ids: Vec<usize>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub when: f64,
}

/// A unit of execution handed to a worker thread.
pub struct WorkItem {
    pub node: NodeId,
    pub generation: u64,
    kind: WorkKind,
    /// Filled by remote work: the server's summary and delivery details.
    remote: Arc<parking_lot::Mutex<Option<crate::remote::Fetched>>>,
}

enum WorkKind {
    LoadStep {
        piece: Option<(usize, usize)>,
        step: aurafx_data::TimeStepRef,
        index: usize,
    },
    Static(Arc<DataSet>),
    Run {
        def: Arc<FilterDef>,
        params: Params,
        input: Option<Arc<DataSet>>,
        extra: Vec<Arc<DataSet>>,
    },
    Temporal {
        def: Arc<FilterDef>,
        params: Params,
        input: Plan,
        extra: Vec<Arc<DataSet>>,
        state: TemporalState,
        time: f64,
    },
    Remote {
        client: Arc<crate::remote::RemoteClient>,
        specs: Vec<crate::remote::NodeSpec>,
        time: Option<f64>,
        fetch: crate::remote::Fetch,
        max_bytes: u64,
        level: u32,
        max_surface: u64,
    },
}

impl WorkItem {
    /// Execute (on any thread). Returns the output or an error.
    pub fn execute(&self) -> Result<Arc<DataSet>> {
        match &self.kind {
            WorkKind::Static(ds) => Ok(ds.clone()),
            WorkKind::LoadStep { step, piece, .. } => aurafx_io::load_step_piece(step, *piece),
            WorkKind::Run { def, params, input, extra } if def.multi.is_some() => {
                let all: Vec<&DataSet> = input.iter().chain(extra.iter()).map(|d| &**d).collect();
                def.run_multi(&all, params).map(Arc::new)
            }
            WorkKind::Run { def, params, input, .. } => def.run(input.as_deref(), params).map(Arc::new),
            WorkKind::Temporal { def, params, input, extra, state, time } => {
                let cache = PlanCache::default();
                run_temporal(def, params, input, extra, state, *time, &cache).map(Arc::new)
            }
            WorkKind::Remote { client, specs, time, fetch, max_bytes, level, max_surface } => {
                let mut f = client.compute(specs.clone(), self.node, *time, *fetch, *max_bytes, *level, *max_surface)?;
                let data = Arc::new(std::mem::replace(&mut f.data, DataSet::PolyData(Default::default())));
                *self.remote.lock() = Some(f);
                Ok(data)
            }
        }
    }
}

#[derive(Default)]
pub struct Session {
    pub nodes: IndexMap<NodeId, Node>,
    pub(crate) next_id: NodeId,
    pub active: Option<NodeId>,
    /// Union of all time values across readers, sorted.
    pub times: Vec<f64>,
    pub time_index: usize,
    /// Bumped on any change that the UI/renderer should react to.
    pub revision: u64,
    /// Bumped whenever outputs change (renderer re-uploads).
    pub data_revision: u64,
    pub log: Vec<LogEntry>,
    started: Option<Instant>,
    generation: u64,
    /// Python-equivalent of UI actions, appended as they happen.
    pub trace: Vec<String>,
    pub camera_request: Option<CameraRequest>,
    /// Set by Python/AI. The UI fulfils it, clears it and reports in `screenshot_results`.
    pub screenshot_request: Option<ScreenshotRequest>,
    /// Finished screenshots: (request id, error).
    pub screenshot_results: Vec<(u64, Result<(), String>)>,
    pub next_screenshot_id: u64,
    /// Series plotted from scripts / time analysis.
    pub chart: Chart,
    /// UI commands from scripts/AI the app executes: "theme light", "layout analyze", "hide Log", "show Charts".
    pub app_commands: Vec<String>,
    pub(crate) undo_stack: Vec<(String, crate::state::Snapshot)>,
    pub(crate) redo_stack: Vec<(String, crate::state::Snapshot)>,
    pub(crate) last_checkpoint: Option<(String, f64)>,
    /// While > 0, changes are not recorded for undo (used while applying snapshots/scripts).
    pub suspend_undo: u32,
    pub(crate) loaded_cache: std::collections::HashMap<PathBuf, Loaded>,
    /// Camera from a loaded state file, applied by the UI.
    pub pending_camera: Option<crate::state::CameraState>,
    /// The UI's current camera, mirrored so scripts can save it.
    pub camera_state: Option<crate::state::CameraState>,
    pub selection: Option<Selection>,
    selection_counter: u64,
    pub annotations: Vec<Annotation>,
    /// Keyframe animation (camera keys and parameter tracks).
    pub animation: crate::animation::Animation,
    /// Show "t = …" in the 3D views.
    pub show_time_label: bool,
    /// Animation export requested by a script: (output path, fps, orbit frames or None for time steps).
    pub animation_request: Option<(PathBuf, f32, Option<usize>)>,
    /// Connections to aurafx-server processes, by host name.
    pub remote_clients: std::collections::HashMap<String, Arc<crate::remote::RemoteClient>>,
    /// Outputs of server nodes larger than this come back as their surface (0 = default).
    pub remote_max_bytes: u64,
    /// In a parallel server process: the piece (k, n) of every file this session reads.
    pub piece: Option<(usize, usize)>,
}

/// Default limit above which server outputs are sent as their surface.
pub const REMOTE_MAX_BYTES: u64 = 64 << 20;

/// A picture of the 3D view to save.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotRequest {
    pub path: PathBuf,
    /// Pixels (None: the view's size).
    pub size: Option<(u32, u32)>,
    /// Transparent background.
    pub transparent: bool,
    /// Reported back in `Session::screenshot_results` (0: nobody waits).
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CameraRequest {
    ResetToBounds,
    View { direction: [f64; 3], up: [f64; 3] },
}

impl Session {
    pub fn new() -> Self {
        Self { started: Some(Instant::now()), next_id: 1, ..Default::default() }
    }

    pub fn shared() -> SharedSession {
        Arc::new(Mutex::new(Self::new()))
    }

    pub(crate) fn now_pub(&self) -> f64 {
        self.now()
    }

    pub(crate) fn rebuild_times_pub(&mut self) {
        self.rebuild_times()
    }

    fn now(&self) -> f64 {
        self.started.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0)
    }

    pub fn log(&mut self, level: LogLevel, msg: impl Into<String>) {
        let message = msg.into();
        match level {
            LogLevel::Error => tracing::error!("{message}"),
            LogLevel::Warn => tracing::warn!("{message}"),
            LogLevel::Info => tracing::info!("{message}"),
        }
        self.log.push(LogEntry { level, message, when: self.now() });
        if self.log.len() > 5000 {
            self.log.drain(..1000);
        }
        self.revision += 1;
    }

    pub fn touch(&mut self) {
        self.revision += 1;
    }

    fn alloc(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn unique_name(&self, base: &str) -> String {
        if !self.nodes.values().any(|n| n.name == base) {
            return base.to_string();
        }
        for k in 2.. {
            let cand = format!("{base}{k}");
            if !self.nodes.values().any(|n| n.name == cand) {
                return cand;
            }
        }
        unreachable!()
    }

    // ------------------------------------------------------------------ building

    pub fn open_file(&mut self, path: impl AsRef<Path>) -> Result<NodeId> {
        let path = path.as_ref().to_path_buf();
        self.checkpoint(format!("Open {}", path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()));
        let loaded = aurafx_io::read_file_piece(&path, self.piece).map_err(|e| {
            self.log(LogLevel::Error, format!("Failed to open {}: {e:#}", path.display()));
            e
        })?;
        let id = self.alloc();
        let name = self.unique_name(path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "data".into()).as_str());
        let node = Node {
            id,
            name,
            kind: NodeKind::Reader { path: path.clone(), loaded },
            params: Params::default(),
            input: None,
            extra_inputs: Vec::new(),
            visible: true,
            display: DisplayProps::default(),
            output: None,
            info: None,
            error: None,
            dirty: true,
            running: false,
            last_exec_ms: 0.0,
            output_time_index: None,
            auto_colored: false,
            watch: false,
            follow_latest: true,
            file_signature: 0,
            temporal_state: TemporalState::default(),
            temporal_key: None,
            remote: None,
            fetch_full: false,
        };
        self.nodes.insert(id, node);
        self.active = Some(id);
        self.rebuild_times();
        self.camera_request = Some(CameraRequest::ResetToBounds);
        self.trace.push(format!("{} = afx.load({:?})", self.py_var(id), path.display().to_string()));
        self.log(LogLevel::Info, format!("Opened {}", path.display()));
        Ok(id)
    }

    pub fn add_source(&mut self, filter_id: &str) -> Result<NodeId> {
        let def = aurafx_filters::find(filter_id).ok_or_else(|| anyhow!("unknown source {filter_id}"))?;
        if !def.is_source {
            return Err(anyhow!("{} is a filter, not a source", def.label));
        }
        self.checkpoint(format!("Add {}", def.label));
        let id = self.alloc();
        let node = Node {
            id,
            name: self.unique_name(def.label),
            params: def.default_params(),
            kind: NodeKind::Source { def: def.clone() },
            input: None,
            extra_inputs: Vec::new(),
            visible: true,
            display: DisplayProps::default(),
            output: None,
            info: None,
            error: None,
            dirty: true,
            running: false,
            last_exec_ms: 0.0,
            output_time_index: None,
            auto_colored: false,
            watch: false,
            follow_latest: true,
            file_signature: 0,
            temporal_state: TemporalState::default(),
            temporal_key: None,
            remote: None,
            fetch_full: false,
        };
        self.nodes.insert(id, node);
        self.active = Some(id);
        self.camera_request = Some(CameraRequest::ResetToBounds);
        self.trace.push(format!("{} = afx.{}()", self.py_var(id), def.id));
        self.touch();
        Ok(id)
    }

    pub fn add_filter(&mut self, filter_id: &str, input: NodeId) -> Result<NodeId> {
        let def = aurafx_filters::find(filter_id).ok_or_else(|| anyhow!("unknown filter {filter_id}"))?;
        if def.is_source {
            return self.add_source(filter_id);
        }
        if !self.nodes.contains_key(&input) {
            return Err(anyhow!("input node {input} does not exist"));
        }
        self.checkpoint(format!("Add {}", def.label));
        let id = self.alloc();
        let mut display = self.nodes[&input].display.clone();
        display.representation = crate::display::Representation::Surface;
        let node = Node {
            id,
            name: self.unique_name(def.label),
            params: def.default_params(),
            kind: NodeKind::Filter { def: def.clone() },
            input: Some(input),
            extra_inputs: Vec::new(),
            visible: true,
            display,
            output: None,
            info: None,
            error: None,
            dirty: true,
            running: false,
            last_exec_ms: 0.0,
            output_time_index: None,
            auto_colored: false,
            watch: false,
            follow_latest: true,
            file_signature: 0,
            temporal_state: TemporalState::default(),
            temporal_key: None,
            remote: None,
            fetch_full: false,
        };
        // ParaView-style: hide the input when adding a filter that replaces its look.
        if matches!(
            def.id,
            "slice"
                | "clip"
                | "contour"
                | "threshold"
                | "extract_surface"
                | "tessellate"
                | "warp_by_vector"
                | "warp_by_scalar"
                | "transform"
                | "calculator"
                | "cell_data_to_point_data"
                | "point_data_to_cell_data"
                | "merge_blocks"
                | "temporal_interpolator"
                | "temporal_shift_scale"
                | "temporal_array_operator"
                | "temporal_statistics"
        ) {
            if let Some(n) = self.nodes.get_mut(&input) {
                n.visible = false;
            }
        }
        self.nodes.insert(id, node);
        self.active = Some(id);
        // Show table-like results where they can be read.
        match def.id {
            "histogram" | "plot_over_line" => self.app_commands.push("show Charts".into()),
            "descriptive_statistics" | "integrate_variables" | "probe_location" => self.app_commands.push("show Spreadsheet".into()),
            _ => {}
        }
        self.trace.push(format!("{} = {}.{}()", self.py_var(id), self.py_var(input), def.id));
        if def.temporal.is_some() {
            self.rebuild_times();
        }
        self.touch();
        Ok(id)
    }

    /// After plugins were (re)loaded: point nodes using these filter ids at the new definitions
    /// and re-run them.
    pub fn refresh_filter_defs(&mut self, ids: &[String]) {
        let mut dirty = Vec::new();
        for n in self.nodes.values_mut() {
            let def = match &mut n.kind {
                NodeKind::Source { def } | NodeKind::Filter { def } => def,
                NodeKind::Reader { .. } | NodeKind::Remote { .. } => continue,
            };
            if ids.iter().any(|i| i == def.id) {
                if let Some(new) = aurafx_filters::find(def.id) {
                    // Keep values of parameters that still exist; add new ones with defaults.
                    let mut params = new.default_params();
                    for spec in &new.params {
                        if let Some(v) = n.params.get(&spec.key) {
                            params.set(&spec.key, v.clone());
                        }
                    }
                    n.params = params;
                    *def = new;
                }
                dirty.push(n.id);
            }
        }
        for id in dirty {
            self.mark_dirty(id);
        }
    }

    /// Downstream nodes of `id` (through primary and extra inputs), including `id`.
    pub fn downstream(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i];
            for n in self.nodes.values() {
                if (n.input == Some(cur) || n.extra_inputs.contains(&cur)) && !out.contains(&n.id) {
                    out.push(n.id);
                }
            }
            i += 1;
        }
        out
    }

    /// Set the extra inputs of a multi-input filter (everything after its pipeline input).
    pub fn set_extra_inputs(&mut self, id: NodeId, inputs: Vec<NodeId>) -> Result<()> {
        let n = self.nodes.get(&id).ok_or_else(|| anyhow!("node {id} does not exist"))?;
        let def = n.def().cloned().ok_or_else(|| anyhow!("{} is not a filter", n.name))?;
        let m = def.multi.ok_or_else(|| anyhow!("{} takes a single input", def.label))?;
        if let Some(max) = m.max {
            if inputs.len() + 1 > max {
                return Err(anyhow!("{} takes at most {} input(s) besides its pipeline input", def.label, max - 1));
            }
        }
        let below = self.downstream(id);
        for e in &inputs {
            let x = self.nodes.get(e).ok_or_else(|| anyhow!("input node {e} does not exist"))?;
            if below.contains(e) {
                return Err(anyhow!("\"{}\" is computed from \"{}\", so it cannot also be its input", x.name, self.nodes[&id].name));
            }
        }
        if self.nodes[&id].extra_inputs == inputs {
            return Ok(());
        }
        self.checkpoint(format!("Inputs of {}", self.nodes[&id].name));
        let names: Vec<String> = inputs.iter().map(|e| self.py_var(*e)).collect();
        self.trace.push(format!("{}.set_inputs({})", self.py_var(id), names.join(", ")));
        self.nodes.get_mut(&id).unwrap().extra_inputs = inputs;
        self.mark_dirty(id);
        Ok(())
    }

    /// Add a multi-input filter on `inputs` (the first is its pipeline input).
    pub fn add_filter_multi(&mut self, filter_id: &str, inputs: &[NodeId]) -> Result<NodeId> {
        let first = *inputs.first().ok_or_else(|| anyhow!("no inputs given"))?;
        let trace_len = self.trace.len();
        let id = self.add_filter(filter_id, first)?;
        if inputs.len() > 1 {
            self.suspend_undo += 1;
            let r = self.set_extra_inputs(id, inputs[1..].to_vec());
            self.suspend_undo -= 1;
            if let Err(e) = r {
                self.remove_node(id);
                return Err(e);
            }
            // Appending replaces the look of its inputs, as in ParaView.
            if matches!(filter_id, "append_datasets" | "append_geometry" | "append_attributes") {
                for i in inputs {
                    if let Some(n) = self.nodes.get_mut(i) {
                        n.visible = false;
                    }
                }
            }
            let names: Vec<String> = inputs.iter().map(|e| self.py_var(*e)).collect();
            self.trace.truncate(trace_len);
            self.trace.push(format!("{} = afx.{}({})", self.py_var(id), filter_id, names.join(", ")));
        }
        Ok(id)
    }

    pub fn remove_node(&mut self, id: NodeId) {
        if let Some(n) = self.nodes.get(&id) {
            let label = format!("Delete {}", n.name);
            self.checkpoint(label);
        }
        let mut to_remove = vec![id];
        let mut i = 0;
        while i < to_remove.len() {
            let cur = to_remove[i];
            for n in self.nodes.values() {
                if n.input == Some(cur) && !to_remove.contains(&n.id) {
                    to_remove.push(n.id);
                }
            }
            i += 1;
        }
        let parent = self.nodes.get(&id).and_then(|n| n.input);
        let mut lost_input = Vec::new();
        for n in self.nodes.values_mut() {
            if !to_remove.contains(&n.id) && n.extra_inputs.iter().any(|e| to_remove.contains(e)) {
                n.extra_inputs.retain(|e| !to_remove.contains(e));
                lost_input.push(n.id);
            }
        }
        for l in lost_input {
            self.mark_dirty(l);
        }
        let mut forget: std::collections::HashMap<String, Vec<NodeId>> = Default::default();
        for r in &to_remove {
            if let Some(h) = self.remote_host(*r) {
                forget.entry(crate::remote::client_key(&h)).or_default().push(*r);
            }
        }
        for (h, ids) in forget {
            if let Some(c) = self.remote_clients.get(&h) {
                c.forget(ids);
            }
        }
        for r in &to_remove {
            if let Some(n) = self.nodes.shift_remove(r) {
                if let NodeKind::Reader { path, loaded } = n.kind {
                    self.loaded_cache.insert(path, loaded);
                }
            }
        }
        if let Some(p) = parent {
            if let Some(n) = self.nodes.get_mut(&p) {
                n.visible = true;
            }
        }
        if self.selection.as_ref().map(|s| to_remove.contains(&s.node)).unwrap_or(false) {
            self.selection = None;
        }
        if self.active.map(|a| to_remove.contains(&a)).unwrap_or(false) {
            self.active = parent.or_else(|| self.nodes.keys().last().copied());
        }
        self.rebuild_times();
        self.touch();
        self.data_revision += 1;
    }

    pub fn set_param(&mut self, id: NodeId, key: &str, value: aurafx_filters::ParamValue) {
        let label = self.nodes.get(&id).map(|n| format!("Edit {}", n.name));
        if let Some(n) = self.nodes.get(&id) {
            if n.params.get(key) != Some(&value) {
                self.checkpoint(label.unwrap_or_default());
            }
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            if n.params.get(key) != Some(&value) {
                n.params.set(key, value);
                self.mark_dirty(id);
            }
        }
    }

    pub fn set_param_json(&mut self, id: NodeId, key: &str, value: &serde_json::Value) {
        if let Some(label) = self.nodes.get(&id).map(|n| format!("Edit {}", n.name)) {
            self.checkpoint(label);
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.params.set_json(key, value);
            self.mark_dirty(id);
        }
    }

    pub fn set_visible(&mut self, id: NodeId, visible: bool) {
        if let Some(label) = self.nodes.get(&id).filter(|n| n.visible != visible).map(|n| format!("{} {}", if visible { "Show" } else { "Hide" }, n.name)) {
            self.checkpoint(label);
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.visible = visible;
            // A server node that only sent its summary now needs its data.
            if visible && n.remote.as_ref().is_some_and(|r| r.payload == crate::remote::Payload::None) {
                n.dirty = true;
            }
            self.touch();
        }
    }

    /// Replace all parameters of a node.
    pub fn set_params(&mut self, id: NodeId, params: Params) {
        if let Some(n) = self.nodes.get_mut(&id) {
            if n.params != params {
                n.params = params;
                self.mark_dirty(id);
            }
        }
    }

    /// Go to the last time step at or before `t`.
    pub fn set_time_value(&mut self, t: f64) {
        let eps = 1e-12 * (1.0 + t.abs());
        let idx = self.times.iter().rposition(|&x| x <= t + eps).unwrap_or(0);
        self.set_time_index(idx);
    }

    // ------------------------------------------------------------------ remote

    /// Open a file on another machine; an aurafx-server there reads it and runs the filters
    /// added below it. The data arrives (or its surface, when large) once it has run.
    pub fn open_remote(&mut self, host: aurafx_jobs::Host, path: &str) -> NodeId {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
        self.checkpoint(format!("Open {file} on {}", host.name));
        let id = self.alloc();
        let name = self.unique_name(if file.is_empty() { "data" } else { &file });
        let trace = format!("{} = afx.load({:?}, host={:?})", self.py_var(id), path, host.name);
        let mut node = Node::new(id, name, NodeKind::Remote { host: host.clone(), path: path.to_string(), times: Vec::new() }, Params::default(), None);
        node.dirty = true;
        self.nodes.insert(id, node);
        self.active = Some(id);
        self.trace.push(trace);
        self.log(LogLevel::Info, format!("Opening {path} on {}", host.name));
        self.touch();
        id
    }

    /// The host a node runs on: server readers, and built-in filters whose inputs all run on
    /// the same server. Python and plugin filters run here.
    pub fn remote_host(&self, id: NodeId) -> Option<aurafx_jobs::Host> {
        self.remote_host_at(id, 0)
    }

    fn remote_host_at(&self, id: NodeId, depth: usize) -> Option<aurafx_jobs::Host> {
        let n = self.nodes.get(&id)?;
        if depth > 256 {
            return None;
        }
        match &n.kind {
            NodeKind::Remote { host, .. } => Some(host.clone()),
            NodeKind::Filter { def } if runs_on_server(def) => {
                let h = self.remote_host_at(n.input?, depth + 1)?;
                for e in &n.extra_inputs {
                    if self.remote_host_at(*e, depth + 1)? != h {
                        return None;
                    }
                }
                Some(h)
            }
            _ => None,
        }
    }

    /// The recipe of a server node: it and everything upstream, inputs first.
    pub fn remote_specs(&self, id: NodeId) -> Vec<crate::remote::NodeSpec> {
        fn walk(s: &Session, id: NodeId, out: &mut Vec<crate::remote::NodeSpec>, seen: &mut std::collections::HashSet<NodeId>) {
            if !seen.insert(id) {
                return;
            }
            let Some(n) = s.nodes.get(&id) else { return };
            for i in n.input.iter().chain(n.extra_inputs.iter()) {
                walk(s, *i, out, seen);
            }
            let kind = match &n.kind {
                NodeKind::Remote { path, .. } => crate::remote::SpecKind::Reader { path: path.clone() },
                NodeKind::Filter { def } | NodeKind::Source { def } => crate::remote::SpecKind::Filter { id: def.id.to_string() },
                NodeKind::Reader { path, .. } => crate::remote::SpecKind::Reader { path: path.display().to_string() },
            };
            out.push(crate::remote::NodeSpec { id, kind, params: n.params.clone(), input: n.input, extra: n.extra_inputs.clone() });
        }
        let mut out = Vec::new();
        walk(self, id, &mut out, &mut Default::default());
        out
    }

    /// How much of a server node's output to bring over.
    fn remote_fetch(&self, id: NodeId) -> crate::remote::Fetch {
        use crate::remote::Fetch;
        let n = &self.nodes[&id];
        let local_child = self.nodes.values().any(|c| (c.input == Some(id) || c.extra_inputs.contains(&id)) && self.remote_host(c.id).is_none());
        if n.fetch_full || local_child {
            Fetch::Full
        } else if n.visible && n.display.draw_on == crate::display::DrawOn::Server && server_can_draw(&n.display) {
            Fetch::Draw
        } else if n.visible {
            Fetch::Auto
        } else {
            Fetch::Info
        }
    }

    /// The connection to a host's server (started on first use, on a worker thread).
    pub fn client_for(&mut self, host: &aurafx_jobs::Host) -> Arc<crate::remote::RemoteClient> {
        self.remote_clients.entry(crate::remote::client_key(host)).or_insert_with(|| crate::remote::RemoteClient::for_host(host.clone())).clone()
    }

    /// Bring the whole output of a server node over (for the spreadsheet or saving).
    pub fn set_fetch_full(&mut self, id: NodeId, on: bool) {
        if let Some(n) = self.nodes.get_mut(&id) {
            if n.fetch_full != on {
                n.fetch_full = on;
                if n.remote.is_some() {
                    n.dirty = true;
                }
                self.touch();
            }
        }
    }

    pub fn set_display(&mut self, id: NodeId, f: impl FnOnce(&mut DisplayProps)) {
        let Some(n) = self.nodes.get(&id) else { return };
        let mut d = n.display.clone();
        f(&mut d);
        if d != n.display {
            let label = format!("Display {}", n.name);
            self.checkpoint(label);
            let n = self.nodes.get_mut(&id).unwrap();
            // Where a server node is drawn decides what it sends.
            let refetch = (n.display.draw_on != d.draw_on || server_can_draw(&n.display) != server_can_draw(&d)) && n.remote.is_some();
            n.display = d;
            if refetch {
                n.dirty = true;
            }
            self.touch();
        }
    }

    /// Visible nodes the server draws (their data stays there), with their host.
    pub fn drawn_on_server(&self) -> Vec<(NodeId, aurafx_jobs::Host)> {
        self.nodes
            .values()
            .filter(|n| n.visible && n.remote.as_ref().is_some_and(|r| r.payload == crate::remote::Payload::Drawn))
            .filter_map(|n| self.remote_host(n.id).map(|h| (n.id, h)))
            .collect()
    }

    pub fn color_by(&mut self, id: NodeId, cb: Option<ColorBy>) {
        self.set_display(id, |d| {
            d.color_by = cb;
            d.range = None;
        });
    }

    pub fn rename(&mut self, id: NodeId, name: String) {
        if let Some(label) = self.nodes.get(&id).filter(|n| n.name != name).map(|n| format!("Rename {}", n.name)) {
            self.checkpoint(label);
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.name = name;
            self.touch();
        }
    }

    pub fn mark_dirty(&mut self, id: NodeId) {
        let mut stack = vec![id];
        let mut seen = std::collections::HashSet::new();
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur) {
                continue;
            }
            if let Some(n) = self.nodes.get_mut(&cur) {
                n.dirty = true;
            }
            for n in self.nodes.values() {
                if n.input == Some(cur) || n.extra_inputs.contains(&cur) {
                    stack.push(n.id);
                }
            }
        }
        self.generation += 1;
        // Temporal filters derive their time steps from their parameters and inputs.
        if self.nodes.get(&id).and_then(|n| n.def()).is_some_and(|d| d.temporal.is_some()) {
            self.rebuild_times();
        }
        self.touch();
    }

    // ------------------------------------------------------------------ time

    fn rebuild_times(&mut self) {
        let before = self.current_time();
        let ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        let t = union_times(ids.into_iter().map(|id| self.node_times(id, 0)));
        if t == self.times {
            return;
        }
        self.times = t;
        // Stay at the same time value when filters add or remove time steps.
        self.time_index = match before {
            Some(b) => self.times.iter().rposition(|&x| x <= b + 1e-12 * (1.0 + b.abs())).unwrap_or(0),
            None => 0,
        };
        if self.time_index >= self.times.len() {
            self.time_index = self.times.len().saturating_sub(1);
        }
    }

    /// Time steps of a node's output: a reader's steps, mapped through temporal filters,
    /// united over the inputs of ordinary filters.
    pub fn node_times(&self, id: NodeId, depth: usize) -> Vec<f64> {
        let Some(n) = self.nodes.get(&id) else { return Vec::new() };
        if depth > 256 {
            return Vec::new();
        }
        match &n.kind {
            NodeKind::Reader { loaded: Loaded::Series(s), .. } => s.times.clone(),
            NodeKind::Remote { times, .. } => times.clone(),
            NodeKind::Reader { .. } | NodeKind::Source { .. } => Vec::new(),
            NodeKind::Filter { def } => match def.temporal {
                Some(t) => (t.output_times)(&n.params, &n.input.map(|i| self.node_times(i, depth + 1)).unwrap_or_default()),
                None => union_times(n.input.iter().chain(n.extra_inputs.iter()).map(|&i| self.node_times(i, depth + 1))),
            },
        }
    }

    /// Whether a node's output can change with time (a reader with steps somewhere upstream).
    fn time_dependent(&self, id: NodeId, depth: usize) -> bool {
        let Some(n) = self.nodes.get(&id) else { return false };
        if depth > 256 {
            return false;
        }
        match &n.kind {
            NodeKind::Reader { loaded: Loaded::Series(_), .. } => true,
            NodeKind::Remote { times, .. } => !times.is_empty(),
            NodeKind::Reader { .. } | NodeKind::Source { .. } => false,
            NodeKind::Filter { .. } => n.input.iter().chain(n.extra_inputs.iter()).any(|&i| self.time_dependent(i, depth + 1)),
        }
    }

    /// Build the evaluable upstream pipeline of node `id` (all of it is clean when called).
    fn plan(&self, id: NodeId, depth: usize) -> Result<Plan> {
        let n = self.nodes.get(&id).ok_or_else(|| anyhow!("node {id} does not exist"))?;
        let data = || n.output.clone().ok_or_else(|| anyhow!("input \"{}\" has no data", n.name));
        if depth > 256 {
            return Err(anyhow!("pipeline too deep"));
        }
        if !self.time_dependent(id, 0) {
            return Ok(Plan::Fixed(data()?));
        }
        Ok(match &n.kind {
            NodeKind::Reader { loaded: Loaded::Series(s), .. } => {
                Plan::Series { series: s.clone(), current: n.output_time_index.zip(n.output.clone()), piece: self.piece }
            }
            NodeKind::Reader { .. } | NodeKind::Source { .. } | NodeKind::Remote { .. } => Plan::Fixed(data()?),
            NodeKind::Filter { def } if def.temporal.is_some() => Plan::Temporal {
                def: def.clone(),
                params: n.params.clone(),
                input: Box::new(self.plan(n.input.ok_or_else(|| anyhow!("no input"))?, depth + 1)?),
                extra: n
                    .extra_inputs
                    .iter()
                    .map(|e| self.nodes.get(e).and_then(|x| x.output.clone()).ok_or_else(|| anyhow!("an input has no data")))
                    .collect::<Result<_>>()?,
                state: n.temporal_state.clone(),
            },
            NodeKind::Filter { def } => Plan::Run {
                def: def.clone(),
                params: n.params.clone(),
                inputs: n.input.iter().chain(n.extra_inputs.iter()).map(|&i| self.plan(i, depth + 1)).collect::<Result<_>>()?,
            },
        })
    }

    pub fn current_time(&self) -> Option<f64> {
        self.times.get(self.time_index).copied()
    }

    pub fn set_time_index(&mut self, idx: usize) {
        let idx = idx.min(self.times.len().saturating_sub(1));
        if idx != self.time_index {
            self.time_index = idx;
            let readers: Vec<NodeId> = self
                .nodes
                .values()
                .filter(|n| n.series().is_some() || matches!(&n.kind, NodeKind::Remote { times, .. } if !times.is_empty()))
                .map(|n| n.id)
                .collect();
            for r in readers {
                self.mark_dirty(r);
            }
        }
    }

    pub fn step_time(&mut self, delta: i64) {
        let n = self.times.len() as i64;
        if n == 0 {
            return;
        }
        let idx = ((self.time_index as i64 + delta) % n + n) % n;
        self.set_time_index(idx as usize);
    }

    /// Step index of `series` for the current global time (last step at or before it).
    fn step_for(&self, series: &TimeSeries) -> usize {
        let Some(t) = self.current_time() else { return 0 };
        let mut best = 0;
        for (i, &st) in series.times.iter().enumerate() {
            if st <= t + 1e-12 {
                best = i;
            }
        }
        best
    }

    // ------------------------------------------------------------------ execution

    /// Pick the next node that can be executed (its input is clean) and mark it running.
    pub fn take_work(&mut self) -> Option<WorkItem> {
        let order: Vec<NodeId> = self.nodes.keys().copied().collect();
        for id in order {
            let n = &self.nodes[&id];
            if !n.dirty || n.running {
                continue;
            }
            if let Some(inp) = n.input {
                match self.nodes.get(&inp) {
                    Some(i) if !i.dirty && !i.running && i.output.is_some() => {}
                    _ => continue,
                }
            }
            if n.extra_inputs.iter().any(|e| self.nodes.get(e).map(|x| x.dirty || x.running).unwrap_or(false)) {
                continue;
            }
            if let Some(host) = self.remote_host(id) {
                let n = &self.nodes[&id];
                let (level, fetch) = (n.display.nonlinear_subdivision.min(6), self.remote_fetch(id));
                let time = if self.time_dependent(id, 0) { self.current_time() } else { None };
                let max_bytes = if self.remote_max_bytes == 0 { REMOTE_MAX_BYTES } else { self.remote_max_bytes };
                let specs = self.remote_specs(id);
                let client = self.client_for(&host);
                let nm = self.nodes.get_mut(&id).unwrap();
                nm.running = true;
                nm.error = None;
                self.touch();
                let d = &self.nodes[&id].display;
                let max_surface = if d.draw_on == crate::display::DrawOn::Local || !server_can_draw(d) { u64::MAX } else { max_bytes };
                let kind = WorkKind::Remote { client, specs, time, fetch, max_bytes, level, max_surface };
                return Some(WorkItem { node: id, generation: self.generation, kind, remote: Default::default() });
            }
            // A local filter needs the whole output of server inputs, not just their surface.
            let thin: Vec<NodeId> = n
                .input
                .iter()
                .chain(n.extra_inputs.iter())
                .copied()
                .filter(|i| self.nodes.get(i).and_then(|x| x.remote.as_ref()).is_some_and(|r| r.payload != crate::remote::Payload::Full))
                .collect();
            if !thin.is_empty() {
                for i in thin {
                    if let Some(x) = self.nodes.get_mut(&i) {
                        x.dirty = true;
                    }
                }
                continue;
            }
            let kind = match &n.kind {
                NodeKind::Reader { loaded: Loaded::Static(ds), .. } => WorkKind::Static(ds.clone()),
                NodeKind::Reader { loaded: Loaded::Series(s), .. } => {
                    let index = self.step_for(s);
                    if n.output_time_index == Some(index) && n.output.is_some() {
                        // Already have this step: just mark clean.
                        let gen = self.generation;
                        let nm = self.nodes.get_mut(&id).unwrap();
                        nm.dirty = false;
                        let _ = gen;
                        self.touch();
                        continue;
                    }
                    WorkKind::LoadStep { step: s.steps[index].clone(), index, piece: self.piece }
                }
                NodeKind::Source { def } => WorkKind::Run { def: def.clone(), params: n.params.clone(), input: None, extra: Vec::new() },
                NodeKind::Remote { .. } => continue,
                NodeKind::Filter { def } => {
                    let mut extra = Vec::new();
                    let mut missing = None;
                    for e in &n.extra_inputs {
                        match self.nodes.get(e) {
                            Some(x) if x.output.is_some() => extra.push(x.output.clone().unwrap()),
                            Some(x) => missing = Some(format!("input \"{}\" has no data", x.name)),
                            None => {}
                        }
                    }
                    if let Some(msg) = missing {
                        let nm = self.nodes.get_mut(&id).unwrap();
                        nm.error = Some(msg);
                        nm.output = None;
                        nm.info = None;
                        nm.dirty = false;
                        self.data_revision += 1;
                        self.touch();
                        continue;
                    }
                    if def.temporal.is_some() {
                        match n.input.ok_or_else(|| anyhow!("no input")).and_then(|i| self.plan(i, 0)) {
                            Ok(plan) => {
                                let time = self.current_time().or_else(|| plan.times().first().copied()).unwrap_or(0.0);
                                let key = {
                                    use std::hash::{Hash, Hasher};
                                    let mut h = std::collections::hash_map::DefaultHasher::new();
                                    plan.key(&mut h);
                                    serde_json::to_string(&n.params).unwrap_or_default().hash(&mut h);
                                    for e in &extra {
                                        (Arc::as_ptr(e) as usize).hash(&mut h);
                                    }
                                    h.finish()
                                };
                                let time_free = (def.temporal.unwrap().output_times)(&n.params, &plan.times()).is_empty();
                                if n.output.is_some() && n.temporal_key.is_some_and(|(k, t)| k == key && (time_free || t == time)) {
                                    let nm = self.nodes.get_mut(&id).unwrap();
                                    nm.dirty = false;
                                    self.touch();
                                    continue;
                                }
                                let (def, params, state) = (def.clone(), n.params.clone(), n.temporal_state.clone());
                                self.nodes.get_mut(&id).unwrap().temporal_key = Some((key, time));
                                WorkKind::Temporal { def, params, input: plan, extra, state, time }
                            }
                            Err(e) => {
                                let nm = self.nodes.get_mut(&id).unwrap();
                                nm.error = Some(format!("{e:#}"));
                                nm.output = None;
                                nm.info = None;
                                nm.dirty = false;
                                self.data_revision += 1;
                                self.touch();
                                continue;
                            }
                        }
                    } else {
                        WorkKind::Run { def: def.clone(), params: n.params.clone(), input: n.input.and_then(|i| self.nodes[&i].output.clone()), extra }
                    }
                }
            };
            let nm = self.nodes.get_mut(&id).unwrap();
            nm.running = true;
            nm.error = None;
            self.touch();
            return Some(WorkItem { node: id, generation: self.generation, kind, remote: Default::default() });
        }
        None
    }

    /// Store the result of a work item.
    pub fn complete(&mut self, item: WorkItem, result: Result<Arc<DataSet>>, elapsed_ms: f64) {
        let fetched = item.remote.lock().take();
        let host_name = match &item.kind {
            WorkKind::Remote { client, .. } => client.name.clone(),
            _ => String::new(),
        };
        let mut new_times = false;
        if let (Some(f), Some(n)) = (&fetched, self.nodes.get_mut(&item.node)) {
            if let NodeKind::Remote { times, .. } = &mut n.kind {
                if *times != f.times {
                    *times = f.times.clone();
                    new_times = true;
                }
                if n.output.is_none() && result.is_ok() {
                    self.camera_request = Some(CameraRequest::ResetToBounds);
                }
            }
        }
        if new_times {
            self.rebuild_times();
        }
        let Some(n) = self.nodes.get_mut(&item.node) else { return };
        n.running = false;
        n.last_exec_ms = elapsed_ms;
        let summary_only = fetched.as_ref().is_some_and(|f| matches!(f.payload, crate::remote::Payload::None | crate::remote::Payload::Drawn));
        match result {
            Ok(ds) if summary_only => {
                // The data stayed on the server: keep its summary, color later.
                let f = fetched.unwrap();
                n.info = f.info.clone();
                n.output = Some(ds);
                n.error = None;
                n.remote = Some(RemoteStatus {
                    host: host_name.clone(),
                    payload: f.payload,
                    full_bytes: f.full_bytes,
                    sent_bytes: f.sent_bytes,
                    server_ms: f.server_ms,
                });
                if item.generation == self.generation {
                    n.dirty = false;
                }
                self.data_revision += 1;
            }
            Ok(ds) => {
                // Friendly default: color by the active scalar the first time data arrives,
                // and fall back when an inherited color array does not exist in this output.
                let has = |cb: &ColorBy| {
                    let attrs = match cb.assoc {
                        aurafx_filters::Association::Points => ds.point_data(),
                        aurafx_filters::Association::Cells => ds.cell_data(),
                    };
                    let leaf_attrs = match &*ds {
                        DataSet::MultiBlock(mb) => mb.leaves().into_iter().next().and_then(|(_, d)| match cb.assoc {
                            aurafx_filters::Association::Points => d.point_data().cloned(),
                            aurafx_filters::Association::Cells => d.cell_data().cloned(),
                        }),
                        _ => attrs.cloned(),
                    };
                    leaf_attrs.map(|a| a.get(&cb.name).is_some()).unwrap_or(false)
                };
                let needs_default = match &n.display.color_by {
                    None => !n.auto_colored,
                    Some(cb) => !has(cb),
                };
                if needs_default {
                    n.display.color_by = aurafx_filters::default_array(&ds, false).map(|(assoc, name)| ColorBy { assoc, name, component: None });
                    n.display.range = None;
                }
                n.auto_colored = true;
                n.info = Some(DataSetInfo::of(&ds));
                n.remote = None;
                if let Some(f) = &fetched {
                    if f.info.is_some() {
                        n.info = f.info.clone();
                    }
                    n.remote = Some(RemoteStatus {
                        host: host_name.clone(),
                        payload: f.payload,
                        full_bytes: f.full_bytes,
                        sent_bytes: f.sent_bytes,
                        server_ms: f.server_ms,
                    });
                }
                n.output = Some(ds);
                n.error = None;
                if let WorkKind::LoadStep { index, .. } = item.kind {
                    n.output_time_index = Some(index);
                }
                // Only mark clean if nothing changed meanwhile.
                if item.generation == self.generation {
                    n.dirty = false;
                }
                let name = n.name.clone();
                self.data_revision += 1;
                self.log(LogLevel::Info, format!("{name}: done in {elapsed_ms:.0} ms"));
            }
            Err(e) => {
                let name = n.name.clone();
                n.temporal_key = None;
                n.error = Some(format!("{e:#}"));
                n.output = None;
                n.info = None;
                n.dirty = false;
                self.data_revision += 1;
                self.log(LogLevel::Error, format!("{name}: {e:#}"));
            }
        }
        self.touch();
    }

    /// Run everything that is dirty on the current thread (used by Python and tests).
    pub fn update_blocking(&mut self) {
        loop {
            let Some(item) = self.take_work() else { break };
            let t0 = Instant::now();
            let r = item.execute();
            self.complete(item, r, t0.elapsed().as_secs_f64() * 1000.0);
        }
    }

    pub fn is_busy(&self) -> bool {
        self.nodes.values().any(|n| n.dirty || n.running)
    }

    /// Evaluate a statistic of `array` on node `id` at every time step (re-running the pipeline),
    /// restoring the current time afterwards. `stat`: min, max, mean, sum, or `value` (field data / first tuple).
    pub fn over_time(&mut self, id: NodeId, array: &str, stat: &str) -> Result<(Vec<f64>, Vec<f64>)> {
        if self.times.is_empty() {
            return Err(anyhow!("no time steps loaded"));
        }
        let saved = self.time_index;
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for i in 0..self.times.len() {
            self.set_time_index(i);
            self.update_blocking();
            let n = self.nodes.get(&id).ok_or_else(|| anyhow!("node {id} does not exist"))?;
            let ds = n.output.clone().ok_or_else(|| anyhow!("{}: {}", n.name, n.error.clone().unwrap_or_else(|| "no output".into())))?;
            xs.push(self.times[i]);
            ys.push(stat_of(&ds, array, stat).ok_or_else(|| anyhow!("array {array:?} not found at step {i}"))?);
        }
        self.set_time_index(saved);
        Ok((xs, ys))
    }

    /// Add an in-memory dataset as a pipeline node (results computed by scripts or analyses).
    pub fn add_data(&mut self, name: &str, ds: DataSet) -> NodeId {
        self.checkpoint(format!("Add {name}"));
        let id = self.alloc();
        let path = PathBuf::from(format!("memory://{name}"));
        let mut node =
            Node::new(id, self.unique_name(name), NodeKind::Reader { path, loaded: Loaded::Static(Arc::new(ds)) }, aurafx_filters::Params::default(), None);
        node.dirty = true;
        self.nodes.insert(id, node);
        self.active = Some(id);
        self.touch();
        id
    }

    /// Per-point/cell mean, min, max and standard deviation of every array over all time steps.
    /// Write node `id` at every time step to one Exodus file. Returns the number of steps.
    pub fn save_exodus_all_times(&mut self, id: NodeId, path: &std::path::Path) -> Result<usize> {
        let saved = self.time_index;
        let n = self.times.len().max(1);
        let mut steps: Vec<(f64, Arc<DataSet>)> = Vec::with_capacity(n);
        let r = (|| -> Result<()> {
            for t in 0..n {
                if self.times.len() > 1 {
                    self.set_time_index(t);
                }
                self.update_blocking();
                let node = self.nodes.get(&id).ok_or_else(|| anyhow!("node {id} does not exist"))?;
                let ds = node.output.clone().ok_or_else(|| anyhow!("{}: {}", node.name, node.error.clone().unwrap_or_else(|| "no output".into())))?;
                steps.push((self.times.get(t).copied().unwrap_or(0.0), ds));
            }
            Ok(())
        })();
        if self.times.len() > 1 {
            self.set_time_index(saved);
        }
        r?;
        let refs: Vec<(f64, &DataSet)> = steps.iter().map(|(t, d)| (*t, &**d)).collect();
        aurafx_io::exodus_writer::write_exodus_steps(&refs, path)?;
        Ok(n)
    }

    /// Add a live Temporal Statistics filter (average/min/max/stddev over all time steps).
    pub fn temporal_statistics(&mut self, id: NodeId) -> Result<NodeId> {
        if self.node_times(id, 0).len() < 2 {
            return Err(anyhow!("temporal statistics need a time series"));
        }
        self.add_filter("temporal_statistics", id)
    }

    pub fn add_series(&mut self, name: impl Into<String>, x: Vec<f64>, y: Vec<f64>) {
        let name = name.into();
        self.chart.series.retain(|s| s.name != name);
        self.chart.series.push(ChartSeries { name, x, y });
        self.chart.revision += 1;
        self.app_commands.push("show Charts".into());
        self.touch();
    }

    pub fn set_selection(&mut self, node: NodeId, assoc: aurafx_filters::Association, mut ids: Vec<usize>) {
        ids.sort_unstable();
        ids.dedup();
        self.selection_counter += 1;
        self.selection = Some(Selection { node, assoc, ids, revision: self.selection_counter });
        self.touch();
    }

    pub fn clear_selection(&mut self) {
        if self.selection.take().is_some() {
            self.touch();
        }
    }

    /// Add an Extract Selection filter for the current selection.
    pub fn extract_selection(&mut self) -> Result<NodeId> {
        let sel = self.selection.clone().ok_or_else(|| anyhow!("nothing is selected"))?;
        let id = self.add_filter("extract_selection", sel.node)?;
        let n = self.nodes.get_mut(&id).unwrap();
        n.params
            .set("attribute", aurafx_filters::ParamValue::Str(if sel.assoc == aurafx_filters::Association::Points { "Points".into() } else { "Cells".into() }));
        n.params.set("ids", aurafx_filters::ParamValue::Floats(sel.ids.iter().map(|&i| i as f64).collect()));
        if let Some(src) = self.nodes.get_mut(&sel.node) {
            src.visible = true;
        }
        self.selection = None;
        self.mark_dirty(id);
        Ok(id)
    }

    /// Turn live reloading on or off for a reader node.
    pub fn set_watch(&mut self, id: NodeId, on: bool) {
        if let Some(n) = self.nodes.get_mut(&id) {
            if let NodeKind::Reader { path, .. } = &n.kind {
                n.file_signature = aurafx_io::signature(path);
                n.watch = on;
                self.touch();
            }
        }
    }

    /// Paths and signatures of watched readers (checked off the lock by the worker).
    pub fn watched(&self) -> Vec<(NodeId, PathBuf, u64)> {
        self.nodes
            .values()
            .filter(|n| n.watch)
            .filter_map(|n| match &n.kind {
                NodeKind::Reader { path, .. } if !path.to_string_lossy().starts_with("memory://") => Some((n.id, path.clone(), n.file_signature)),
                _ => None,
            })
            .collect()
    }

    /// Re-read a reader node whose file changed.
    pub fn reload_reader(&mut self, id: NodeId, loaded: Loaded, signature: u64) {
        let Some(n) = self.nodes.get_mut(&id) else { return };
        let follow = n.follow_latest;
        if let NodeKind::Reader { loaded: l, .. } = &mut n.kind {
            *l = loaded;
        }
        n.file_signature = signature;
        n.output_time_index = None;
        let name = n.name.clone();
        self.rebuild_times();
        if follow && !self.times.is_empty() {
            self.time_index = self.times.len() - 1;
        }
        self.mark_dirty(id);
        self.log(LogLevel::Info, format!("{name} changed on disk: reloaded ({} time steps)", self.times.len().max(1)));
    }

    // ------------------------------------------------------------------ queries

    pub fn active_node(&self) -> Option<&Node> {
        self.active.and_then(|id| self.nodes.get(&id))
    }

    pub fn find_by_name(&self, name: &str) -> Option<NodeId> {
        self.nodes.values().find(|n| n.name == name).map(|n| n.id)
    }

    pub fn py_var(&self, id: NodeId) -> String {
        let name = self.nodes.get(&id).map(|n| n.name.clone()).unwrap_or_else(|| format!("node{id}"));
        let mut s: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        if s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
            s = format!("n_{s}");
        }
        s
    }

    /// Visible-bounds of everything shown.
    pub fn visible_bounds(&self) -> aurafx_data::Bounds {
        self.nodes.values().filter(|n| n.visible).fold(aurafx_data::Bounds::EMPTY, |b, n| {
            // Server-drawn nodes have no local data; their summary has the bounds.
            let drawn = n.remote.as_ref().is_some_and(|r| r.payload == crate::remote::Payload::Drawn);
            match (drawn, &n.info, &n.output) {
                (true, Some(i), _) => b.union(&i.bounds),
                (_, _, Some(d)) => b.union(&d.bounds()),
                _ => b,
            }
        })
    }

    /// Text summary of the whole pipeline for the AI assistant and tooltips.
    pub fn describe(&self) -> String {
        let mut s = String::new();
        if self.nodes.is_empty() {
            return "The pipeline is empty: no files opened and no sources created.".into();
        }
        for n in self.nodes.values() {
            let vis = if n.visible { "visible" } else { "hidden" };
            s += &format!("- [{}] \"{}\" ({}), {}", n.id, n.name, n.type_label(), vis);
            if let Some(inp) = n.input {
                s += &format!(", input = [{}]", inp);
            }
            if !n.extra_inputs.is_empty() {
                let e: Vec<String> = n.extra_inputs.iter().map(|e| format!("[{e}]")).collect();
                s += &format!(", more inputs = {}", e.join(" "));
            }
            if self.active == Some(n.id) {
                s += ", ACTIVE";
            }
            if !n.params.0.is_empty() {
                let ps: Vec<String> = n.params.0.iter().map(|(k, v)| format!("{k}={}", serde_json::to_string(v).unwrap_or_default())).collect();
                s += &format!("\n    params: {}", ps.join(", "));
            }
            if let Some(cb) = &n.display.color_by {
                s += &format!("\n    colored by {} ({:?}), colormap {}", cb.name, cb.assoc, n.display.colormap);
            }
            if let Some(e) = &n.error {
                s += &format!("\n    ERROR: {e}");
            } else if let Some(info) = &n.info {
                s += "\n    ";
                s += &info.summary().replace('\n', "\n    ");
            }
            s.push('\n');
        }
        if !self.times.is_empty() {
            s += &format!("Time: {} steps, current index {} (t = {})\n", self.times.len(), self.time_index, self.current_time().unwrap_or(0.0));
        }
        s
    }
}

/// Reduce an array (searched in point, cell, then field data of every leaf) to one number.
pub fn stat_of(ds: &DataSet, array: &str, stat: &str) -> Option<f64> {
    let mut vals: Vec<f64> = Vec::new();
    let mut visit = |d: &DataSet| {
        let a = d.point_data().and_then(|a| a.get(array)).or_else(|| d.cell_data().and_then(|a| a.get(array))).or_else(|| d.field_data().get(array));
        if let Some(a) = a {
            for i in 0..a.len() {
                let v = if a.ncomp == 1 { a.get(i, 0) } else { a.magnitude(i) };
                if v.is_finite() {
                    vals.push(v);
                }
            }
        }
    };
    match ds {
        DataSet::MultiBlock(mb) => {
            if let Some(a) = mb.field_data.get(array) {
                return Some(a.get(0, 0));
            }
            for (_, d) in mb.leaves() {
                visit(d);
            }
        }
        d => visit(d),
    }
    if vals.is_empty() {
        return None;
    }
    Some(match stat {
        "min" => vals.iter().cloned().fold(f64::INFINITY, f64::min),
        "max" => vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        "sum" => vals.iter().sum(),
        "value" | "first" => vals[0],
        _ => vals.iter().sum::<f64>() / vals.len() as f64,
    })
}
