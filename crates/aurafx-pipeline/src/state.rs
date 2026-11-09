//! Snapshots of the pipeline: undo/redo and `.afx` state files.

use crate::display::DisplayProps;
use crate::session::{Chart, LogLevel, Node, NodeId, NodeKind, Session};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_filters::Params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum KindSnap {
    Reader(PathBuf),
    /// A file on a server: (host, path there).
    Remote(aurafx_jobs::Host, String),
    Source(String),
    Filter(String),
}

#[derive(Debug, Clone)]
pub struct NodeSnap {
    pub id: NodeId,
    pub name: String,
    pub kind: KindSnap,
    pub params: Params,
    pub input: Option<NodeId>,
    pub extra_inputs: Vec<NodeId>,
    pub visible: bool,
    pub display: DisplayProps,
    pub auto_colored: bool,
}

/// Everything needed to rebuild the pipeline (not the computed data).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub nodes: Vec<NodeSnap>,
    pub active: Option<NodeId>,
    pub time_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub fov_y_deg: f64,
    pub orthographic: bool,
    pub ortho_height: f64,
}

pub(crate) fn kind_snap(n: &Node) -> KindSnap {
    match &n.kind {
        NodeKind::Reader { path, .. } => KindSnap::Reader(path.clone()),
        NodeKind::Remote { host, path, .. } => KindSnap::Remote(host.clone(), path.clone()),
        NodeKind::Source { def } => KindSnap::Source(def.id.to_string()),
        NodeKind::Filter { def } => KindSnap::Filter(def.id.to_string()),
    }
}

impl Session {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            nodes: self
                .nodes
                .values()
                .map(|n| NodeSnap {
                    id: n.id,
                    name: n.name.clone(),
                    kind: kind_snap(n),
                    params: n.params.clone(),
                    input: n.input,
                    extra_inputs: n.extra_inputs.clone(),
                    visible: n.visible,
                    display: n.display.clone(),
                    auto_colored: n.auto_colored,
                })
                .collect(),
            active: self.active,
            time_index: self.time_index,
        }
    }

    /// Replace the pipeline with `snap`, keeping computed outputs of nodes that did not change.
    pub fn apply_snapshot(&mut self, snap: &Snapshot) -> Result<()> {
        let mut old = std::mem::take(&mut self.nodes);
        let mut changed: Vec<NodeId> = Vec::new();
        let mut errors = Vec::new();
        for ns in &snap.nodes {
            let reuse = old.get(&ns.id).filter(|n| kind_snap(n) == ns.kind).is_some();
            let mut node = if reuse {
                old.shift_remove(&ns.id).unwrap()
            } else {
                match self.make_node(ns.id, &ns.kind) {
                    Ok(n) => n,
                    Err(e) => {
                        errors.push(format!("{}: {e:#}", ns.name));
                        continue;
                    }
                }
            };
            if !reuse || node.params != ns.params || node.input != ns.input || node.extra_inputs != ns.extra_inputs {
                changed.push(ns.id);
            }
            node.name = ns.name.clone();
            node.params = ns.params.clone();
            node.input = ns.input;
            node.extra_inputs = ns.extra_inputs.clone();
            node.visible = ns.visible;
            node.display = ns.display.clone();
            node.auto_colored = ns.auto_colored;
            self.nodes.insert(ns.id, node);
        }
        // Remember readers that went away so redo does not re-read files.
        for (_, n) in old {
            if let NodeKind::Reader { path, loaded } = n.kind {
                self.loaded_cache.insert(path, loaded);
            }
        }
        self.next_id = self.next_id.max(snap.nodes.iter().map(|n| n.id + 1).max().unwrap_or(1));
        self.active = snap.active.filter(|a| self.nodes.contains_key(a)).or_else(|| self.nodes.keys().last().copied());
        self.rebuild_times_pub();
        self.time_index = snap.time_index.min(self.times.len().saturating_sub(1));
        for id in changed {
            self.mark_dirty(id);
        }
        self.data_revision += 1;
        self.revision += 1;
        if !errors.is_empty() {
            bail!("some nodes could not be restored: {}", errors.join("; "));
        }
        Ok(())
    }

    fn make_node(&mut self, id: NodeId, kind: &KindSnap) -> Result<Node> {
        let kind = match kind {
            KindSnap::Reader(path) => {
                let loaded = match self.loaded_cache.get(path) {
                    Some(l) => l.clone(),
                    None => aurafx_io::read_file(path).with_context(|| format!("re-opening {}", path.display()))?,
                };
                NodeKind::Reader { path: path.clone(), loaded }
            }
            KindSnap::Remote(host, path) => NodeKind::Remote { host: host.clone(), path: path.clone(), times: Vec::new() },
            KindSnap::Source(f) => NodeKind::Source { def: aurafx_filters::find(f).ok_or_else(|| anyhow!("unknown source {f}"))? },
            KindSnap::Filter(f) => NodeKind::Filter { def: aurafx_filters::find(f).ok_or_else(|| anyhow!("unknown filter {f}"))? },
        };
        Ok(Node::new(id, String::new(), kind, Params::default(), None))
    }

    // ------------------------------------------------------------------ undo / redo

    /// Record the current state before a change. Edits with the same label within 0.6 s merge.
    pub fn checkpoint(&mut self, label: impl Into<String>) {
        if self.suspend_undo > 0 {
            return;
        }
        let label = label.into();
        let now = self.now_pub();
        if let Some((l, t)) = &self.last_checkpoint {
            if *l == label && now - *t < 0.6 {
                self.last_checkpoint = Some((label, now));
                return;
            }
        }
        let snap = self.snapshot();
        self.undo_stack.push((label.clone(), snap));
        if self.undo_stack.len() > 200 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.last_checkpoint = Some((label, now));
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|(l, _)| l.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|(l, _)| l.as_str())
    }

    pub fn undo(&mut self) -> Option<String> {
        let (label, snap) = self.undo_stack.pop()?;
        let cur = self.snapshot();
        self.redo_stack.push((label.clone(), cur));
        if let Err(e) = self.apply_snapshot(&snap) {
            self.log(LogLevel::Warn, format!("Undo: {e:#}"));
        }
        self.last_checkpoint = None;
        self.log(LogLevel::Info, format!("Undo {label}"));
        Some(label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let (label, snap) = self.redo_stack.pop()?;
        let cur = self.snapshot();
        self.undo_stack.push((label.clone(), cur));
        if let Err(e) = self.apply_snapshot(&snap) {
            self.log(LogLevel::Warn, format!("Redo: {e:#}"));
        }
        self.last_checkpoint = None;
        self.log(LogLevel::Info, format!("Redo {label}"));
        Some(label)
    }

    // ------------------------------------------------------------------ state files

    pub fn save_state(&self, path: &Path, camera: Option<CameraState>) -> Result<()> {
        let base = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let nodes: Vec<serde_json::Value> = self
            .nodes
            .values()
            .map(|n| {
                let (kind, what) = match &n.kind {
                    NodeKind::Reader { path, .. } => ("reader", path.display().to_string()),
                    NodeKind::Remote { path, .. } => ("remote", path.clone()),
                    NodeKind::Source { def } => ("source", def.id.to_string()),
                    NodeKind::Filter { def } => ("filter", def.id.to_string()),
                };
                let rel = match &n.kind {
                    NodeKind::Reader { path, .. } => pathdiff(path, &base),
                    _ => None,
                };
                let params: serde_json::Map<String, serde_json::Value> =
                    n.params.0.iter().map(|(k, v)| (k.clone(), aurafx_filters::param_to_json(v))).collect();
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "kind": kind,
                    "type": what,
                    "relative_path": rel,
                    "input": n.input,
                    "extra_inputs": n.extra_inputs,
                    "visible": n.visible,
                    "params": params,
                    "display": n.display,
                    "host": match &n.kind {
                        NodeKind::Remote { host, .. } => serde_json::to_value(host).ok(),
                        _ => None,
                    },
                })
            })
            .collect();
        let doc = serde_json::json!({
            "format": "aurafx-state",
            "version": 1,
            "app_version": env!("CARGO_PKG_VERSION"),
            "nodes": nodes,
            "active": self.active,
            "time_index": self.time_index,
            "camera": camera,
            "chart": self.chart,
            "chart_views": self.chart_views,
            "annotations": self.annotations,
            "animation": self.animation,
            "show_time_label": self.show_time_label,
        });
        std::fs::write(path, serde_json::to_string_pretty(&doc)?).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Load a state file, replacing the pipeline. Returns the saved camera, if any.
    pub fn load_state(&mut self, path: &Path) -> Result<Option<CameraState>> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let doc: serde_json::Value = serde_json::from_str(&text).context("state file is not valid JSON")?;
        if doc.get("format").and_then(|f| f.as_str()) != Some("aurafx-state") {
            bail!("{} is not an auraFX state file", path.display());
        }
        let base = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut snap = Snapshot {
            active: doc.get("active").and_then(|a| a.as_u64()),
            time_index: doc.get("time_index").and_then(|t| t.as_u64()).unwrap_or(0) as usize,
            nodes: vec![],
        };
        for n in doc.get("nodes").and_then(|n| n.as_array()).cloned().unwrap_or_default() {
            let id = n.get("id").and_then(|v| v.as_u64()).ok_or_else(|| anyhow!("node without id"))?;
            let what = n.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let kind = match n.get("kind").and_then(|v| v.as_str()) {
                Some("reader") => {
                    let abs = PathBuf::from(&what);
                    let rel = n.get("relative_path").and_then(|v| v.as_str()).map(|r| base.join(r));
                    KindSnap::Reader(if abs.exists() { abs } else { rel.filter(|r| r.exists()).unwrap_or(abs) })
                }
                Some("source") => KindSnap::Source(what.clone()),
                Some("remote") => {
                    let host = n.get("host").cloned().and_then(|h| serde_json::from_value(h).ok()).ok_or_else(|| anyhow!("remote node without host"))?;
                    KindSnap::Remote(host, what.clone())
                }
                _ => KindSnap::Filter(what.clone()),
            };
            let def = match &kind {
                KindSnap::Source(f) | KindSnap::Filter(f) => aurafx_filters::find(f),
                KindSnap::Reader(_) | KindSnap::Remote(..) => None,
            };
            let mut params = def.as_ref().map(|d| d.default_params()).unwrap_or_default();
            if let (Some(def), Some(obj)) = (&def, n.get("params").and_then(|p| p.as_object())) {
                for (k, v) in obj {
                    if let Some(spec) = def.params.iter().find(|s| &s.key == k) {
                        params.set(k, aurafx_filters::param_from_json(spec, v));
                    }
                }
            }
            // A reader on a server: its options are only known there, so they are kept as saved.
            if let (KindSnap::Remote(..), Some(obj)) = (&kind, n.get("params").and_then(|p| p.as_object())) {
                for (k, v) in obj {
                    params.set_json(k, v);
                }
            }
            // A reader's parameters are the options its file offers.
            if let (KindSnap::Reader(file), Some(obj)) = (&kind, n.get("params").and_then(|p| p.as_object())) {
                let specs = crate::session::reader_param_specs(file);
                for (k, v) in obj {
                    if let Some(spec) = specs.iter().find(|s| &s.key == k) {
                        params.set(k, aurafx_filters::param_from_json(spec, v));
                    }
                }
            }
            snap.nodes.push(NodeSnap {
                id,
                name: n.get("name").and_then(|v| v.as_str()).unwrap_or("node").to_string(),
                kind,
                params,
                input: n.get("input").and_then(|v| v.as_u64()),
                extra_inputs: n.get("extra_inputs").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64()).collect()).unwrap_or_default(),
                visible: n.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
                display: n.get("display").cloned().and_then(|d| serde_json::from_value(d).ok()).unwrap_or_default(),
                auto_colored: true,
            });
        }
        self.checkpoint("Open state");
        self.nodes.clear();
        let r = self.apply_snapshot(&snap);
        if let Some(c) = doc.get("chart").cloned().and_then(|c| serde_json::from_value::<Chart>(c).ok()) {
            self.chart = c;
        }
        self.chart_views = doc.get("chart_views").cloned().and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
        for v in self.chart_views.iter().filter(|v| v.id != 0) {
            self.app_commands.push(format!("chart_open {}", v.id));
        }
        self.animation = doc.get("animation").cloned().and_then(|a| serde_json::from_value(a).ok()).unwrap_or_default();
        self.annotations = doc.get("annotations").cloned().and_then(|a| serde_json::from_value(a).ok()).unwrap_or_default();
        self.show_time_label = doc.get("show_time_label").and_then(|v| v.as_bool()).unwrap_or(false);
        let cam = doc.get("camera").cloned().and_then(|c| serde_json::from_value::<CameraState>(c).ok());
        self.pending_camera = cam;
        self.log(LogLevel::Info, format!("Loaded state {}", path.display()));
        r.map(|_| cam)
    }
}

/// Relative path from `base` to `p` when both are absolute.
fn pathdiff(p: &Path, base: &Path) -> Option<String> {
    let pc: Vec<_> = p.components().collect();
    let bc: Vec<_> = base.components().collect();
    let common = pc.iter().zip(&bc).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..bc.len() {
        out.push("..");
    }
    for c in &pc[common..] {
        out.push(c);
    }
    Some(out.display().to_string())
}
