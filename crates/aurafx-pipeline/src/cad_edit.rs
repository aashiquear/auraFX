//! The CAD environment's side of the session (roadmap section 13, stage C3). While a CAD Model
//! node is being edited the session holds a *working copy* of its document; the scheduler
//! pauses (nothing downstream re-runs on every edit); the environment evaluates the working
//! copy itself and shows the result as the node's output; **Apply** hands the document to the
//! node as one undo step and lets the pipeline run, **Discard** puts everything back as it was.
//! Edits inside the environment have their own undo (one step per operation).

use crate::session::{LogLevel, NodeId, Session};
use anyhow::{anyhow, bail, Result};
use crate::cad_check::{self, CheckReport, FixMode, RunMode};
use aurafx_cad::diff::GeometrySnapshot;
use aurafx_cad::{CadFile, Document, Id};
use aurafx_data::{DataSet, DataSetInfo};
use std::sync::Arc;

/// A summary of the last evaluation of the working copy.
#[derive(Debug, Clone, Default)]
pub struct CadSummary {
    pub kernel: String,
    pub volume: f64,
    pub area: f64,
    pub centroid: [f64; 3],
    pub bounds: ([f64; 3], [f64; 3]),
    /// Face name, kind, area, owning feature.
    pub faces: Vec<(String, String, f64, Id)>,
    pub ms: f64,
}

/// The state of a CAD edit in progress.
pub struct CadEdit {
    pub node: NodeId,
    /// The working copy: operations change this, never the node, until Apply.
    pub document: Document,
    /// What the node had when the environment was entered (restored by Discard).
    pub(crate) original_text: String,
    pub(crate) original_output: Option<Arc<DataSet>>,
    pub(crate) original_info: Option<DataSetInfo>,
    pub(crate) original_error: Option<String>,
    /// Bumps with every change of the working copy; the environment evaluates when it moves.
    pub revision: u64,
    /// Only the first `n` features are built (the timeline's rollback marker); `None`: all.
    pub rollback: Option<usize>,
    /// The feature selected in the FeatureManager.
    pub selected: Option<Id>,
    pub undo: Vec<Document>,
    pub redo: Vec<Document>,
    /// The last evaluation: its revision, summary and error.
    pub evaluated_revision: u64,
    pub summary: Option<CadSummary>,
    pub error: Option<String>,
    /// Whether the working copy differs from what the node holds.
    pub changed: bool,
    /// The part as the nodes below it were built against (for the downstream check).
    pub baseline: Option<GeometrySnapshot>,
}

impl CadEdit {
    /// The document the environment builds: the working copy up to the rollback marker.
    pub fn document_to_build(&self) -> Document {
        let mut d = self.document.clone();
        if let Some(n) = self.rollback {
            d.features.truncate(n.min(d.features.len()));
        }
        d
    }
}

/// What Apply did.
#[derive(Debug, Clone)]
pub struct CadApplied {
    pub node: NodeId,
    pub features: usize,
    pub kept_editing: bool,
    /// Fixes applied to nodes below the part.
    pub fixes: usize,
    /// Nodes below the part that wait for `cad_run_held` (when the run was postponed).
    pub held: usize,
}

impl Session {
    /// The node being edited in the CAD environment, if any.
    pub fn cad_editing(&self) -> Option<NodeId> {
        self.cad_edit.as_ref().map(|e| e.node)
    }

    /// Enter the environment on a CAD Model node. The pipeline pauses; one undo step is
    /// recorded now so that undoing the whole session of edits is a single step.
    pub fn cad_enter(&mut self, id: NodeId) -> Result<()> {
        if let Some(e) = &self.cad_edit {
            if e.node == id {
                return Ok(());
            }
            bail!("the CAD environment is already open on '{}': apply or discard it first", self.nodes.get(&e.node).map(|n| n.name.as_str()).unwrap_or("another part"));
        }
        let document = self.cad_document(id)?;
        // What the nodes below were built against, for the check at Apply.
        let baseline = self
            .cad_effective_document(id)
            .ok()
            .and_then(|d| aurafx_filters::cad::evaluate(&d, 40, true).ok().map(|ev| GeometrySnapshot::of(&ev, &d)));
        self.checkpoint("Edit CAD");
        let n = self.nodes.get_mut(&id).ok_or_else(|| anyhow!("no such node"))?;
        let edit = CadEdit {
            node: id,
            document,
            original_text: n.params.str("document"),
            original_output: n.output.clone(),
            original_info: n.info.clone(),
            original_error: n.error.clone(),
            revision: 1,
            rollback: None,
            selected: None,
            undo: Vec::new(),
            redo: Vec::new(),
            evaluated_revision: 0,
            summary: None,
            error: None,
            changed: false,
            baseline,
        };
        n.visible = true;
        self.active = Some(id);
        self.cad_edit = Some(edit);
        self.log(LogLevel::Info, "CAD environment: pipeline paused while editing");
        self.touch();
        Ok(())
    }

    fn edit_mut(&mut self) -> Result<&mut CadEdit> {
        self.cad_edit.as_mut().ok_or_else(|| anyhow!("the CAD environment is not open (node.edit_cad() opens it)"))
    }

    /// One operation on the working copy (see `aurafx_cad::edit`), with its own undo step.
    pub fn cad_env_edit(&mut self, op: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        let var = self.cad_edit.as_ref().map(|e| e.node).map(|id| self.py_var(id));
        let e = self.edit_mut()?;
        let before = e.document.clone();
        let out = aurafx_cad::edit::apply_op(&mut e.document, op, args).map_err(|x| anyhow!("{x}"))?;
        if e.document != before {
            e.undo.push(before);
            if e.undo.len() > 200 {
                e.undo.remove(0);
            }
            e.redo.clear();
            e.revision += 1;
            e.changed = true;
            // A new feature appears at the end: build everything again.
            if matches!(op, "add" | "sketch_extrude" | "sketch_revolve") {
                e.rollback = None;
            }
            if let Some(id) = out.get("id").and_then(|i| i.as_u64()) {
                e.selected = Some(Id(id));
            }
        }
        if let Some(v) = var {
            self.trace.push(format!("{v}.cad.edit({}, {})", crate::session::py_str(op), crate::session::json_to_py(args)));
        }
        Ok(out)
    }

    /// Replace the whole working copy (the PropertyManager's raw edits, Open in the Files tab).
    pub fn cad_env_set_document(&mut self, doc: Document) -> Result<()> {
        let e = self.edit_mut()?;
        if e.document != doc {
            let before = std::mem::replace(&mut e.document, doc);
            e.undo.push(before);
            e.redo.clear();
            e.revision += 1;
            e.changed = true;
        }
        Ok(())
    }

    pub fn cad_env_undo(&mut self) -> Option<()> {
        let e = self.cad_edit.as_mut()?;
        let prev = e.undo.pop()?;
        let cur = std::mem::replace(&mut e.document, prev);
        e.redo.push(cur);
        e.revision += 1;
        e.changed = true;
        Some(())
    }

    pub fn cad_env_redo(&mut self) -> Option<()> {
        let e = self.cad_edit.as_mut()?;
        let next = e.redo.pop()?;
        let cur = std::mem::replace(&mut e.document, next);
        e.undo.push(cur);
        e.revision += 1;
        e.changed = true;
        Some(())
    }

    /// Select a feature of the working copy.
    pub fn cad_env_select(&mut self, feature: Option<Id>) {
        if let Some(e) = self.cad_edit.as_mut() {
            e.selected = feature;
        }
    }

    /// The timeline's rollback marker: build only the first `n` features (`None`: all).
    pub fn cad_env_rollback(&mut self, n: Option<usize>) {
        if let Some(e) = self.cad_edit.as_mut() {
            let n = n.filter(|n| *n < e.document.features.len());
            if e.rollback != n {
                e.rollback = n;
                e.revision += 1;
            }
        }
    }

    /// Evaluate the working copy now (on the calling thread) and show it as the node's output.
    /// The app calls this from a worker thread through [`Session::cad_env_evaluate_with`] so the
    /// window stays responsive; scripts call it directly.
    pub fn cad_env_evaluate(&mut self, resolution: usize) -> Result<CadSummary> {
        let (doc, overrides, rev, node) = {
            let e = self.cad_edit.as_ref().ok_or_else(|| anyhow!("the CAD environment is not open"))?;
            (e.document_to_build(), self.cad_overrides(e.node), e.revision, e.node)
        };
        let result = evaluate_document(doc, &overrides, resolution);
        self.cad_env_deliver(node, rev, result)
    }

    /// The input of an off-thread evaluation: the document to build, the overrides, the revision.
    pub fn cad_env_job(&self) -> Option<(Document, Vec<(String, String)>, u64, NodeId)> {
        let e = self.cad_edit.as_ref()?;
        Some((e.document_to_build(), self.cad_overrides(e.node), e.revision, e.node))
    }

    /// Take the result of an evaluation of revision `rev` (ignored when the working copy has
    /// moved on): the surface becomes the node's output so every view shows it.
    pub fn cad_env_deliver(&mut self, node: NodeId, rev: u64, result: Result<(DataSet, CadSummary), String>) -> Result<CadSummary> {
        let Some(e) = self.cad_edit.as_mut().filter(|e| e.node == node) else { bail!("the CAD environment closed") };
        if rev < e.revision {
            bail!("stale");
        }
        e.evaluated_revision = rev;
        match result {
            Ok((surface, summary)) => {
                e.error = None;
                e.summary = Some(summary.clone());
                let info = DataSetInfo::of(&surface);
                let first = self.nodes.get(&node).is_some_and(|n| n.output.as_ref().is_none_or(|o| !matches!(&**o, DataSet::PolyData(_))));
                if let Some(n) = self.nodes.get_mut(&node) {
                    n.info = Some(info);
                    n.output = Some(Arc::new(surface));
                    n.error = None;
                    n.dirty = false;
                }
                if first {
                    self.camera_request = Some(crate::session::CameraRequest::ResetToBounds);
                }
                self.data_revision += 1;
                self.touch();
                Ok(summary)
            }
            Err(msg) => {
                e.error = Some(msg.clone());
                self.touch();
                bail!("{msg}")
            }
        }
    }

    /// The downstream check of the working copy against the part the nodes below were built on:
    /// what changed, what the nodes refer to that did, and the default fixes.
    pub fn cad_check(&self) -> Result<CheckReport> {
        let e = self.cad_edit.as_ref().ok_or_else(|| anyhow!("the CAD environment is not open (node.edit_cad() opens it)"))?;
        let baseline = e.baseline.clone().ok_or_else(|| anyhow!("the part as it was could not be built, so nothing can be compared"))?;
        let mut doc = e.document.clone();
        for (name, expr) in self.cad_overrides(e.node) {
            if doc.parameters.iter().any(|p| p.name == name) {
                doc.set_parameter(&name, &expr);
            }
        }
        let ev = aurafx_filters::cad::evaluate(&doc, 40, true).map_err(|x| anyhow!("the part does not build: {x:#}"))?;
        let new = GeometrySnapshot::of(&ev, &doc);
        Ok(cad_check::collect(self, e.node, &baseline, &new))
    }

    /// The assistant's prompt for fixes to a report.
    pub fn cad_check_prompt(&self, report: &CheckReport) -> String {
        cad_check::ai_prompt(self, report)
    }

    /// Hand the working copy to the node and apply the default fixes: see [`Session::cad_apply_with`].
    pub fn cad_apply(&mut self, keep_editing: bool) -> Result<CadApplied> {
        self.cad_apply_with(keep_editing, FixMode::Default, RunMode::Now)
    }

    /// Hand the working copy to the node: one undo step (the entry's), the fixes chosen for the
    /// conflicts, the pipeline resumes (or, with `RunMode::Later`, the nodes below wait until
    /// `cad_run_held`). With `keep_editing` the environment stays open.
    pub fn cad_apply_with(&mut self, keep_editing: bool, fix: FixMode, run: RunMode) -> Result<CadApplied> {
        let e = self.cad_edit.as_ref().ok_or_else(|| anyhow!("the CAD environment is not open"))?;
        let (node, doc) = (e.node, e.document.clone());
        // The model must build, or the pipeline would only show an error.
        let overrides = self.cad_overrides(node);
        evaluate_document(doc.clone(), &overrides, 16).map_err(|m| anyhow!("the part does not build: {m}"))?;
        let fixes = match fix {
            FixMode::None => Vec::new(),
            FixMode::Fixes(f) => f,
            FixMode::Default => self.cad_check().map(|r| r.default_fixes()).unwrap_or_default(),
        };
        let text = CadFile::new(doc.clone()).to_json();
        let n = self.nodes.get_mut(&node).ok_or_else(|| anyhow!("the node is gone"))?;
        n.params.set("document", aurafx_filters::ParamValue::Str(text.clone()));
        n.dirty = true;
        n.error = None;
        let features = doc.features.len();
        // The fixes are part of the same undo step as the edit.
        for f in &fixes {
            if let Some(nd) = self.nodes.get_mut(&f.node) {
                if nd.params.get(&f.param).is_some() {
                    nd.params.set_json(&f.param, &f.value);
                    nd.forget_reader_step();
                }
            }
        }
        if keep_editing {
            let ev = aurafx_filters::cad::evaluate(&self.cad_effective_document(node).unwrap_or(doc.clone()), 40, true).ok();
            let e = self.cad_edit.as_mut().unwrap();
            e.original_text = text;
            e.original_output = None;
            e.original_info = None;
            e.changed = false;
            if let Some(ev) = ev {
                let d = e.document.clone();
                e.baseline = Some(GeometrySnapshot::of(&ev, &d));
            }
        } else {
            self.cad_edit = None;
        }
        let var = self.py_var(node);
        self.trace.push(format!("{var}.cad.apply()"));
        self.log(LogLevel::Info, if keep_editing { "CAD: applied, still editing" } else { "CAD environment closed: the pipeline resumes" });
        self.mark_dirty(node);
        let below: Vec<NodeId> = self.downstream(node).into_iter().filter(|n| *n != node).collect();
        let held = if run == RunMode::Later && !below.is_empty() {
            self.cad_hold = Some(node);
            below.len()
        } else {
            0
        };
        Ok(CadApplied { node, features, kept_editing: keep_editing, fixes: fixes.len(), held })
    }

    /// Let the nodes that were held back after Apply run.
    pub fn cad_run_held(&mut self) {
        if let Some(n) = self.cad_hold.take() {
            self.mark_dirty(n);
        }
    }

    /// Leave without applying: the node, its output and the pipeline are as they were.
    pub fn cad_discard(&mut self) -> Result<()> {
        let e = self.cad_edit.take().ok_or_else(|| anyhow!("the CAD environment is not open"))?;
        if let Some(n) = self.nodes.get_mut(&e.node) {
            n.params.set("document", aurafx_filters::ParamValue::Str(e.original_text));
            n.output = e.original_output;
            n.info = e.original_info;
            n.error = e.original_error;
            n.dirty = n.output.is_none();
        }
        // The undo step recorded on entry has nothing to undo any more.
        if self.undo_label() == Some("Edit CAD") {
            self.undo_stack.pop();
        }
        self.data_revision += 1;
        self.touch();
        self.log(LogLevel::Info, "CAD environment closed: changes discarded");
        Ok(())
    }
}

/// Build `doc` (with parameter overrides) on the implicit kernel: its surface and a summary.
/// A free function so a worker thread can run it without the session.
pub fn evaluate_document(mut doc: Document, overrides: &[(String, String)], resolution: usize) -> Result<(DataSet, CadSummary), String> {
    let t0 = std::time::Instant::now();
    for (name, expr) in overrides {
        if doc.parameters.iter().any(|p| &p.name == name) {
            doc.set_parameter(name, expr);
        }
    }
    let ev = aurafx_filters::cad::evaluate(&doc, resolution, true).map_err(|e| format!("{e:#}"))?;
    let body = ev.bodies.first().ok_or_else(|| "the part has no body".to_string())?;
    let summary = CadSummary {
        kernel: ev.kernel.to_string(),
        volume: ev.volume(),
        area: body.area,
        centroid: body.centroid,
        bounds: body.bounds,
        faces: ev.faces().map(|(_, f)| (f.name.clone(), f.kind.label().to_string(), f.area, f.id.0)).collect(),
        ms: t0.elapsed().as_secs_f64() * 1000.0,
    };
    Ok((ev.surface, summary))
}
