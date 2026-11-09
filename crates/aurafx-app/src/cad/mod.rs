//! The CAD modeling environment's part of the app (roadmap section 13). One mode of the one
//! window: while a CAD Model is being edited the toolbar becomes the **ribbon**, the Pipeline
//! panel the **FeatureManager**, Properties the **PropertyManager**, a **timeline bar** appears
//! at the bottom, and everything else is marked stale because the pipeline is paused. The
//! document being edited lives in the session (`CadEdit`); this module draws it and turns
//! clicks into the same operations Python makes (`aurafx_cad::edit`).

mod conflicts;
mod feature_manager;
mod pick;
mod planes;
mod property_manager;
mod ribbon;
mod sketch;
mod sketch_dims;
mod sketch_snap;
mod timeline;

use crate::app::{LayoutAction, Pane, State};
use aurafx_cad::Id;
use aurafx_pipeline::NodeId;
use egui::RichText;
use serde_json::{json, Value};

pub use conflicts::hold_banner;
pub use feature_manager::feature_manager_ui;
pub use pick::{click as pick_click, draw_overlay as pick_overlay, open as open_picker, picking, PickKind};
pub use planes::draw_overlay as planes_overlay;
pub use sketch::{begin_on_frame as sketch_on_frame, Tool as SketchTool};

/// Open the New reference plane form.
pub fn open_plane_form(state: &mut State) {
    state.cad.plane_form = Some(planes::PlaneForm::default());
}
pub use property_manager::property_manager_ui;
pub use ribbon::{check_fit, ribbon_ui};
pub use sketch::{begin_edit as sketch_edit, begin_from_spec as sketch_from_spec, begin_new as sketch_new, canvas_ui as sketch_canvas_ui, sketching};
pub use timeline::timeline_ui;

/// The ribbon's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RibbonTab {
    Sketch,
    #[default]
    Features,
    Modify,
    Inspect,
    Assemble,
    Files,
}

impl RibbonTab {
    pub const ALL: [RibbonTab; 6] = [RibbonTab::Sketch, RibbonTab::Features, RibbonTab::Modify, RibbonTab::Inspect, RibbonTab::Assemble, RibbonTab::Files];
    pub fn label(self) -> &'static str {
        match self {
            RibbonTab::Sketch => "Sketch",
            RibbonTab::Features => "Features",
            RibbonTab::Modify => "Modify",
            RibbonTab::Inspect => "Inspect",
            RibbonTab::Assemble => "Assemble",
            RibbonTab::Files => "Files",
        }
    }
}

type EvalResult = (NodeId, u64, Result<(aurafx_data::DataSet, aurafx_pipeline::CadSummary), String>);

/// The environment's interface state (the document itself is in the session).
pub struct CadUi {
    pub tab: RibbonTab,
    tx: std::sync::mpsc::Sender<EvalResult>,
    rx: std::sync::mpsc::Receiver<EvalResult>,
    /// The revision an evaluation was started for, and whether one is running.
    requested: u64,
    running: bool,
    /// "Apply, discard or keep editing?" (Esc).
    pub leave_dialog: bool,
    /// Density for the mass readout (g/cm³) and the section height of the Inspect tab.
    pub density: f64,
    pub section_z: f64,
    pub section_area: Option<f64>,
    /// A feature being renamed in the FeatureManager.
    pub rename: Option<(Id, String)>,
    pub new_param: (String, String),
    /// Apply is waiting for the user's answer about the nodes below the part.
    pub apply_dialog: Option<conflicts::ApplyDialog>,
    /// Re-run the nodes below right after Apply (the dialog's checkbox).
    pub rerun_now: bool,
    /// The sketch open on the canvas.
    pub sketch: Option<sketch::SketchEdit>,
    /// The edge or face picker of the Modify tab.
    pub pick: Option<pick::Pick>,
    /// The New reference plane form.
    pub plane_form: Option<planes::PlaneForm>,
    /// The component selected in the FeatureManager (by name).
    pub component: Option<String>,
    /// The overlaps found by the last "Check fit": the two parts and the volume in mm³.
    pub fit: Option<Vec<(String, String, f64)>>,
}

impl Default for CadUi {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tab: RibbonTab::default(),
            tx,
            rx,
            requested: 0,
            running: false,
            leave_dialog: false,
            density: 7.85,
            section_z: 0.0,
            section_area: None,
            rename: None,
            new_param: (String::new(), String::new()),
            apply_dialog: None,
            rerun_now: true,
            sketch: None,
            pick: None,
            plane_form: None,
            component: None,
            fit: None,
        }
    }
}

/// Whether the environment is open.
pub fn editing(state: &State) -> bool {
    state.session.lock().cad_edit.is_some()
}

/// Enter the environment on a CAD Model node.
pub fn enter(state: &mut State, id: NodeId) {
    let r = state.session.lock().cad_enter(id);
    match r {
        Ok(()) => {
            state.cad.requested = 0;
            state.cad.running = false;
            state.layout_actions.push(LayoutAction::Show(Pane::Pipeline));
            state.layout_actions.push(LayoutAction::Show(Pane::Properties));
            state.status = "CAD environment: the pipeline is paused until you apply or discard".into();
        }
        Err(e) => state.toast(format!("{e:#}"), true),
    }
}

/// Enter on the active node when it is a CAD Model.
pub fn enter_active(state: &mut State) {
    let id = state.session.lock().active;
    if let Some(id) = id {
        enter(state, id);
    }
}

/// Run one operation on the working copy; a refusal is shown as a message.
pub fn op(state: &mut State, name: &str, args: Value) -> Option<Value> {
    let r = state.session.lock().cad_env_edit(name, &args);
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            state.toast(format!("{e:#}"), true);
            None
        }
    }
}

/// Apply (and leave, or keep editing): the downstream check first; when the nodes below are
/// affected a dialog asks what to do.
pub fn apply(state: &mut State, keep: bool) {
    state.cad.leave_dialog = false;
    conflicts::request_apply(state, keep);
}

/// Apply with the given fixes and run mode, without asking.
pub fn apply_now(state: &mut State, keep: bool, fix: aurafx_pipeline::FixMode, run: aurafx_pipeline::RunMode) {
    state.cad.sketch = None;
    let r = state.session.lock().cad_apply_with(keep, fix, run);
    match r {
        Ok(a) => {
            state.status = match (a.kept_editing, a.fixes, a.held) {
                (true, ..) => "CAD: applied; still editing".into(),
                (false, 0, 0) => "CAD: applied; the pipeline resumes".into(),
                (false, f, 0) => format!("CAD: applied with {f} fix(es); the pipeline resumes"),
                (false, _, h) => format!("CAD: applied; {h} node(s) below wait to be re-run"),
            }
        }
        Err(e) => state.toast(format!("{e:#}"), true),
    }
    state.cad.leave_dialog = false;
}

/// Leave without applying.
pub fn discard(state: &mut State) {
    state.cad.sketch = None;
    state.cad.pick = None;
    let r = state.session.lock().cad_discard();
    match r {
        Ok(()) => state.status = "CAD: changes discarded".into(),
        Err(e) => state.toast(format!("{e:#}"), true),
    }
    state.cad.leave_dialog = false;
}

/// Every frame: collect finished builds, start a new one when the document changed, and ask
/// what to do on Esc.
pub fn tick(state: &mut State, ctx: &egui::Context) {
    while let Ok((node, rev, result)) = state.cad.rx.try_recv() {
        state.cad.running = false;
        let _ = state.session.lock().cad_env_deliver(node, rev, result);
        ctx.request_repaint();
    }
    let job = state.session.lock().cad_env_job();
    let Some((doc, overrides, rev, node)) = job else {
        state.cad.requested = 0;
        state.cad.running = false;
        return;
    };
    if rev != state.cad.requested && !state.cad.running {
        let res = {
            let s = state.session.lock();
            s.nodes.get(&node).map(|n| n.params.i64("resolution", 64)).unwrap_or(64).clamp(24, 96) as usize
        };
        state.cad.requested = rev;
        state.cad.running = true;
        let tx = state.cad.tx.clone();
        let ctx2 = ctx.clone();
        std::thread::Builder::new()
            .name("aurafx-cad".into())
            .spawn(move || {
                let r = aurafx_pipeline::cad_edit::evaluate_document(doc, &overrides, res);
                let _ = tx.send((node, rev, r));
                ctx2.request_repaint();
            })
            .ok();
    }
    if state.cad.running {
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
    }
    // Keyboard: Alt+1..6 ribbon tabs, Ctrl+Enter apply, Delete removes the selected feature, F2 renames it.
    if !ctx.text_edit_focused() && !state.cad.dialog_open() && !state.cad.leave_dialog {
        let keys = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6];
        for (k, tab) in keys.iter().zip(RibbonTab::ALL) {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, *k)) {
                state.cad.tab = tab;
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            apply(state, false);
            return;
        }
        if state.cad.sketch.is_none() {
            let sel = state.session.lock().cad_edit.as_ref().and_then(|e| e.selected.and_then(|id| e.document.feature(id).map(|f| (id, f.name.clone()))));
            if let Some((id, name)) = sel {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                    op(state, "remove", json!({"feature": name}));
                    state.session.lock().cad_env_select(None);
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
                    state.cad.rename = Some((id, name));
                }
            }
        }
    }
    if !ctx.text_edit_focused()
        && !state.cad.leave_dialog
        && !state.cad.dialog_open()
        && state.cad.sketch.is_none()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        state.cad.leave_dialog = true;
    }
}

/// The "leave the CAD environment?" dialog (Esc).
pub fn dialogs_ui(state: &mut State, ctx: &egui::Context) {
    conflicts::dialog_ui(state, ctx);
    if !state.cad.leave_dialog || !editing(state) {
        state.cad.leave_dialog = false;
        return;
    }
    let changed = state.session.lock().cad_edit.as_ref().map(|e| e.changed).unwrap_or(false);
    egui::Window::new("Leave the CAD environment?").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(if changed { "The part has changes. Apply hands them to the pipeline; discard throws them away." } else { "The part has no changes." });
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                apply(state, false);
            }
            if ui.button(if changed { "Discard" } else { "Leave" }).clicked() {
                discard(state);
            }
            if ui.button("Keep editing").clicked() {
                state.cad.leave_dialog = false;
            }
        });
    });
}

/// A grey banner on views and panels that show data the paused pipeline no longer updates.
pub fn stale_banner(state: &State, ui: &mut egui::Ui) {
    if !editing(state) {
        return;
    }
    egui::Frame::NONE.fill(egui::Color32::from_gray(70)).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.label(RichText::new("Pipeline paused while editing CAD: this shows the data as it was.").color(egui::Color32::from_gray(225)).small());
    });
}

/// The CAD menu, shown in the menu bar while the environment is open.
pub fn menu_ui(state: &mut State, ui: &mut egui::Ui) {
    let (can_undo, can_redo, changed) = {
        let s = state.session.lock();
        s.cad_edit.as_ref().map(|e| (!e.undo.is_empty(), !e.redo.is_empty(), e.changed)).unwrap_or_default()
    };
    if ui.button("Apply and close").on_hover_text("Hand the part to the pipeline and leave").clicked() {
        ui.close();
        apply(state, false);
    }
    if ui.button("Apply and keep editing").clicked() {
        ui.close();
        apply(state, true);
    }
    if ui.button(if changed { "Discard changes" } else { "Close" }).clicked() {
        ui.close();
        discard(state);
    }
    ui.separator();
    if ui.add_enabled(can_undo, egui::Button::new("Undo    Ctrl+Z")).clicked() {
        ui.close();
        state.session.lock().cad_env_undo();
    }
    if ui.add_enabled(can_redo, egui::Button::new("Redo    Ctrl+Y")).clicked() {
        ui.close();
        state.session.lock().cad_env_redo();
    }
    ui.separator();
    for t in RibbonTab::ALL {
        if ui.button(format!("{} tab", t.label())).clicked() {
            ui.close();
            state.cad.tab = t;
        }
    }
}

/// A text field for a number or an expression: returns the new text when the user commits it
/// (Enter, or leaving the field).
pub fn expr_field(ui: &mut egui::Ui, salt: impl std::hash::Hash + std::fmt::Debug, current: &str, width: f32) -> Option<String> {
    let key = ui.make_persistent_id(("cad field", salt));
    let mut text: String = ui.data_mut(|d| d.get_temp(key)).unwrap_or_else(|| current.to_string());
    let r = ui.add_sized([width, 20.0], egui::TextEdit::singleline(&mut text));
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<String>(key));
        if text.trim() != current.trim() && !text.trim().is_empty() {
            return Some(text.trim().to_string());
        }
    }
    None
}

/// A number as short text.
pub fn num_text(v: f64) -> String {
    let s = if v.abs() >= 1000.0 {
        format!("{v:.1}")
    } else if v.abs() >= 10.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.4}")
    };
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A JSON value for a typed number or expression.
pub fn number_or_expr(text: &str) -> Value {
    match text.trim().parse::<f64>() {
        Ok(x) => json!(x),
        Err(_) => json!(text.trim()),
    }
}

/// The Properties of a CAD Model node.
pub fn properties_ui(state: &mut State, ui: &mut egui::Ui, id: NodeId) {
    hold_banner(state, ui);
    let (doc, effective, overrides) = {
        let s = state.session.lock();
        (s.cad_document(id), s.cad_effective_document(id), s.cad_overrides(id))
    };
    let doc = match doc {
        Ok(d) => d,
        Err(e) => {
            ui.label(RichText::new(format!("{e:#}")).color(crate::theme::ERROR));
            return;
        }
    };
    let values = effective.as_ref().ok().and_then(|d| d.parameter_values().ok());
    if ui
        .button(format!("{} Edit in CAD…", crate::icons::PENCIL_SIMPLE))
        .on_hover_text("Open the CAD environment on this part: ribbon, feature tree, timeline")
        .clicked()
    {
        enter(state, id);
    }
    egui::CollapsingHeader::new(RichText::new("Dimensions").strong()).id_salt(("cad dims", id)).default_open(true).show(ui, |ui| {
        if doc.parameters.is_empty() {
            ui.label(RichText::new("The part has no named dimensions. Add them in Python: node.cad.parameter(\"width\", 40).").weak().small());
        }
        egui::Grid::new(("cad dims grid", id)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            for p in &doc.parameters {
                let over = overrides.iter().find(|(n, _)| n == &p.name).map(|(_, e)| e.clone());
                let text_id = ui.make_persistent_id(("cad dim", id, &p.name));
                let mut text: String = ui.data_mut(|d| d.get_temp(text_id)).unwrap_or_else(|| over.clone().unwrap_or_else(|| p.expr.clone()));
                ui.label(&p.name).on_hover_text(if p.comment.is_empty() { "A named dimension of the part" } else { &p.comment });
                ui.horizontal(|ui| {
                    let r = ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut text));
                    if r.changed() {
                        ui.data_mut(|d| d.insert_temp(text_id, text.clone()));
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || !i.pointer.any_down()) {
                        ui.data_mut(|d| d.remove_temp::<String>(text_id));
                        if text.trim() != over.clone().unwrap_or_else(|| p.expr.clone()).trim() {
                            let value = if text.trim() == p.expr.trim() { serde_json::Value::Null } else { serde_json::Value::String(text.trim().to_string()) };
                            let result = state.session.lock().cad_set_parameter(id, &p.name, &value);
                            if let Err(e) = result {
                                state.toast(format!("{e:#}"), true);
                            }
                        }
                    }
                    match values.as_ref().and_then(|v| v.get(&p.name)) {
                        Some(v) => ui.label(RichText::new(format!("= {} mm", trim_num(*v))).weak()),
                        None => ui.label(RichText::new("?").color(crate::theme::ERROR)),
                    };
                    if over.is_some() && ui.small_button("reset").on_hover_text("Use the document's own value again").clicked() {
                        ui.data_mut(|d| d.remove_temp::<String>(text_id));
                        let _ = state.session.lock().cad_set_parameter(id, &p.name, &serde_json::Value::Null);
                    }
                });
                ui.end_row();
            }
        });
        if let Err(e) = &effective {
            ui.label(RichText::new(format!("{e}")).color(crate::theme::ERROR).small());
        }
    });
    if !doc.configurations.is_empty() {
        let current = state.session.lock().nodes.get(&id).map(|n| n.params.str("configuration")).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Configuration");
            let mut sel = current.clone();
            egui::ComboBox::from_id_salt(("cad cfg", id)).selected_text(if sel.is_empty() { "(document)".to_string() } else { sel.clone() }).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut sel, String::new(), "(document)");
                    for c in &doc.configurations {
                        ui.selectable_value(&mut sel, c.name.clone(), &c.name);
                    }
                },
            );
            if sel != current {
                state.session.lock().set_param(id, "configuration", aurafx_filters::ParamValue::Str(sel));
            }
        });
    }
    egui::CollapsingHeader::new(RichText::new(format!("Features ({})", doc.features.len())).strong()).id_salt(("cad feats", id)).default_open(true).show(
        ui,
        |ui| {
            let mut toggle: Option<(String, bool)> = None;
            for f in &doc.features {
                ui.horizontal(|ui| {
                    let mut on = !f.suppressed;
                    if ui.checkbox(&mut on, "").on_hover_text("Include this feature (off: suppressed)").changed() {
                        toggle = Some((f.name.clone(), !on));
                    }
                    let kind = serde_json::to_value(&f.shape).ok().and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_string))).unwrap_or_default();
                    let kind = match kind.as_str() {
                        "SketchExtrude" | "Extrude" => "extruded sketch".to_string(),
                        "SketchRevolve" | "Revolve" => "revolved sketch".to_string(),
                        k => k.to_lowercase(),
                    };
                    let text = format!(
                        "{}  ·  {} {}",
                        f.name,
                        match f.op {
                            aurafx_cad::BooleanOp::Add => "adds",
                            aurafx_cad::BooleanOp::Cut => "cuts",
                            aurafx_cad::BooleanOp::Intersect => "intersects",
                        },
                        kind
                    );
                    ui.label(if f.suppressed { RichText::new(text).weak().strikethrough() } else { RichText::new(text) });
                });
            }
            if let Some((name, sup)) = toggle {
                let _ = state.session.lock().cad_edit(id, "suppress", &serde_json::json!({"feature": name, "suppressed": sup}));
            }
        },
    );
    let faces = {
        let s = state.session.lock();
        s.nodes.get(&id).and_then(|n| n.output.clone()).and_then(|o| match &*o {
            aurafx_data::DataSet::PolyData(p) => {
                let names = p.field_data.get("FaceNames")?;
                let vol = p.field_data.get("Volume").map(|v| v.get(0, 0));
                let area = p.field_data.get("SurfaceArea").map(|v| v.get(0, 0));
                let list = match &names.data {
                    aurafx_data::ArrayData::String(v) => v.clone(),
                    _ => Vec::new(),
                };
                Some((list, vol, area))
            }
            _ => None,
        })
    };
    if let Some((names, vol, area)) = faces {
        egui::CollapsingHeader::new(RichText::new(format!("Faces ({})", names.len())).strong()).id_salt(("cad faces", id)).show(ui, |ui| {
            if let (Some(v), Some(a)) = (vol, area) {
                ui.label(RichText::new(format!("volume {} mm³, surface {} mm²", trim_num(v), trim_num(a))).weak().small());
            }
            for (i, n) in names.iter().enumerate() {
                ui.label(RichText::new(format!("{i}  {n}")).small()).on_hover_text("Downstream filters refer to a face by this name or number (FaceId)");
            }
        });
    }
}

fn trim_num(v: f64) -> String {
    if v.abs() >= 1000.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
