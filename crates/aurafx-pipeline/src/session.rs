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

#[derive(Debug, Clone)]
pub enum NodeKind {
    Reader { path: PathBuf, loaded: Loaded },
    Source { def: Arc<FilterDef> },
    Filter { def: Arc<FilterDef> },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub params: Params,
    pub input: Option<NodeId>,
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
}

impl Node {
    pub fn new(id: NodeId, name: String, kind: NodeKind, params: Params, input: Option<NodeId>) -> Self {
        Node {
            id,
            name,
            kind,
            params,
            input,
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
        }
    }

    pub fn def(&self) -> Option<&Arc<FilterDef>> {
        match &self.kind {
            NodeKind::Source { def } | NodeKind::Filter { def } => Some(def),
            NodeKind::Reader { .. } => None,
        }
    }
    pub fn type_label(&self) -> String {
        match &self.kind {
            NodeKind::Reader { path, .. } => path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default(),
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
}

enum WorkKind {
    LoadStep { step: aurafx_data::TimeStepRef, index: usize },
    Static(Arc<DataSet>),
    Run { def: Arc<FilterDef>, params: Params, input: Option<Arc<DataSet>> },
}

impl WorkItem {
    /// Execute (on any thread). Returns the output or an error.
    pub fn execute(&self) -> Result<Arc<DataSet>> {
        match &self.kind {
            WorkKind::Static(ds) => Ok(ds.clone()),
            WorkKind::LoadStep { step, .. } => aurafx_io::load_step(step),
            WorkKind::Run { def, params, input } => def.run(input.as_deref(), params).map(Arc::new),
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
    /// Set by Python/AI: (path, width, height). The UI fulfils it and clears it.
    pub screenshot_request: Option<(PathBuf, Option<u32>, Option<u32>)>,
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

    fn touch(&mut self) {
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
        let loaded = aurafx_io::read_file(&path).map_err(|e| {
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
        };
        // ParaView-style: hide the input when adding a filter that replaces its look.
        if matches!(
            def.id,
            "slice"
                | "clip"
                | "contour"
                | "threshold"
                | "extract_surface"
                | "warp_by_vector"
                | "warp_by_scalar"
                | "transform"
                | "calculator"
                | "cell_data_to_point_data"
                | "point_data_to_cell_data"
                | "merge_blocks"
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
        self.touch();
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
            self.touch();
        }
    }

    pub fn set_display(&mut self, id: NodeId, f: impl FnOnce(&mut DisplayProps)) {
        let Some(n) = self.nodes.get(&id) else { return };
        let mut d = n.display.clone();
        f(&mut d);
        if d != n.display {
            let label = format!("Display {}", n.name);
            self.checkpoint(label);
            self.nodes.get_mut(&id).unwrap().display = d;
            self.touch();
        }
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
        while let Some(cur) = stack.pop() {
            if let Some(n) = self.nodes.get_mut(&cur) {
                n.dirty = true;
            }
            for n in self.nodes.values() {
                if n.input == Some(cur) {
                    stack.push(n.id);
                }
            }
        }
        self.generation += 1;
        self.touch();
    }

    // ------------------------------------------------------------------ time

    fn rebuild_times(&mut self) {
        let mut t: Vec<f64> = self.nodes.values().filter_map(|n| n.series()).flat_map(|s| s.times.iter().copied()).collect();
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        t.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        self.times = t;
        if self.time_index >= self.times.len() {
            self.time_index = self.times.len().saturating_sub(1);
        }
    }

    pub fn current_time(&self) -> Option<f64> {
        self.times.get(self.time_index).copied()
    }

    pub fn set_time_index(&mut self, idx: usize) {
        let idx = idx.min(self.times.len().saturating_sub(1));
        if idx != self.time_index {
            self.time_index = idx;
            let readers: Vec<NodeId> = self.nodes.values().filter(|n| n.series().is_some()).map(|n| n.id).collect();
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
                    WorkKind::LoadStep { step: s.steps[index].clone(), index }
                }
                NodeKind::Source { def } => WorkKind::Run { def: def.clone(), params: n.params.clone(), input: None },
                NodeKind::Filter { def } => {
                    WorkKind::Run { def: def.clone(), params: n.params.clone(), input: n.input.and_then(|i| self.nodes[&i].output.clone()) }
                }
            };
            let nm = self.nodes.get_mut(&id).unwrap();
            nm.running = true;
            nm.error = None;
            self.touch();
            return Some(WorkItem { node: id, generation: self.generation, kind });
        }
        None
    }

    /// Store the result of a work item.
    pub fn complete(&mut self, item: WorkItem, result: Result<Arc<DataSet>>, elapsed_ms: f64) {
        let Some(n) = self.nodes.get_mut(&item.node) else { return };
        n.running = false;
        n.last_exec_ms = elapsed_ms;
        match result {
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
    pub fn temporal_statistics(&mut self, id: NodeId) -> Result<NodeId> {
        if self.times.len() < 2 {
            return Err(anyhow!("temporal statistics need a time series"));
        }
        let saved = self.time_index;
        let name = self.nodes.get(&id).map(|n| n.name.clone()).ok_or_else(|| anyhow!("node {id} does not exist"))?;
        struct Acc {
            sum: Vec<f64>,
            sq: Vec<f64>,
            min: Vec<f64>,
            max: Vec<f64>,
            ncomp: usize,
        }
        let mut template: Option<DataSet> = None;
        // (leaf index, is_cell, array name) -> accumulator
        let mut accs: indexmap::IndexMap<(usize, bool, String), Acc> = indexmap::IndexMap::new();
        let steps = self.times.len();
        for t in 0..steps {
            self.set_time_index(t);
            self.update_blocking();
            let n = self.nodes.get(&id).ok_or_else(|| anyhow!("node removed"))?;
            let ds = n.output.clone().ok_or_else(|| anyhow!("{}: {}", n.name, n.error.clone().unwrap_or_else(|| "no output".into())))?;
            let leaves = aurafx_filters::selection::flat_leaves(&ds);
            if template.is_none() {
                template = Some((*ds).clone());
            }
            for (li, (leaf, _, _)) in leaves.iter().enumerate() {
                for (is_cell, attrs) in [(false, leaf.point_data()), (true, leaf.cell_data())] {
                    let Some(attrs) = attrs else { continue };
                    for a in attrs.iter().filter(|a| a.scalar_type() != aurafx_data::ScalarType::String) {
                        let len = a.data.len();
                        let e = accs.entry((li, is_cell, a.name.clone())).or_insert_with(|| Acc {
                            sum: vec![0.0; len],
                            sq: vec![0.0; len],
                            min: vec![f64::INFINITY; len],
                            max: vec![f64::NEG_INFINITY; len],
                            ncomp: a.ncomp,
                        });
                        if e.sum.len() != len {
                            continue; // topology changed; skip this array at this step
                        }
                        for i in 0..len {
                            let v = a.data.get_f64(i);
                            e.sum[i] += v;
                            e.sq[i] += v * v;
                            e.min[i] = e.min[i].min(v);
                            e.max[i] = e.max[i].max(v);
                        }
                    }
                }
            }
        }
        self.set_time_index(saved);
        let mut out = template.ok_or_else(|| anyhow!("no data"))?;
        let nf = steps as f64;
        let fill = |leaf: &mut DataSet, li: usize, accs: &indexmap::IndexMap<(usize, bool, String), Acc>| {
            for ((l, is_cell, name), a) in accs {
                if *l != li {
                    continue;
                }
                let attrs = if *is_cell { leaf.cell_data_mut() } else { leaf.point_data_mut() };
                let Some(attrs) = attrs else { continue };
                attrs.remove(name);
                let mean: Vec<f64> = a.sum.iter().map(|s| s / nf).collect();
                let std: Vec<f64> = a.sq.iter().zip(&mean).map(|(q, m)| (q / nf - m * m).max(0.0).sqrt()).collect();
                attrs.insert(aurafx_data::Array::new(format!("{name}_average"), a.ncomp, aurafx_data::ArrayData::F64(mean)));
                attrs.insert(aurafx_data::Array::new(format!("{name}_minimum"), a.ncomp, aurafx_data::ArrayData::F64(a.min.clone())));
                attrs.insert(aurafx_data::Array::new(format!("{name}_maximum"), a.ncomp, aurafx_data::ArrayData::F64(a.max.clone())));
                attrs.insert(aurafx_data::Array::new(format!("{name}_stddev"), a.ncomp, aurafx_data::ArrayData::F64(std)));
            }
        };
        match &mut out {
            DataSet::MultiBlock(mb) => {
                fn walk(mb: &mut aurafx_data::MultiBlock, li: &mut usize, f: &mut dyn FnMut(&mut DataSet, usize)) {
                    for (_, b) in mb.blocks.iter_mut() {
                        match b {
                            Some(DataSet::MultiBlock(inner)) => walk(inner, li, f),
                            Some(d) => {
                                f(d, *li);
                                *li += 1;
                            }
                            None => {}
                        }
                    }
                }
                let mut li = 0;
                walk(mb, &mut li, &mut |d, i| fill(d, i, &accs));
            }
            d => fill(d, 0, &accs),
        }
        Ok(self.add_data(&format!("{name} temporal statistics"), out))
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
        self.nodes.values().filter(|n| n.visible).filter_map(|n| n.output.as_ref()).fold(aurafx_data::Bounds::EMPTY, |b, d| b.union(&d.bounds()))
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
