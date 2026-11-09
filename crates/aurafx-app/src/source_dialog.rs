//! Adding a source: a small window that says what the source makes with its standard settings and
//! lets the user insert it as it is, or change its basic settings first (sizes, counts, resolution;
//! for a CAD Model, the starter part and its dimensions). "Don't ask again" in the window (or in
//! Settings) inserts sources straight away.

use crate::app::State;
use aurafx_filters::{ParamKind, ParamSpec, ParamValue};
use egui::RichText;

/// Settings of a source that are not basic: long texts and documents edited elsewhere.
const NOT_BASIC: &[&str] = &["document", "parameters", "configuration", "script"];

pub struct SourceDialog {
    pub id: &'static str,
    /// Change the settings before inserting (false: the summary of the standard ones).
    pub customize: bool,
    /// The settings as edited: key and value, for the basic settings only.
    pub values: Vec<(ParamSpec, ParamValue)>,
    /// A CAD Model: the starter part ("" = the plate) and the plate's dimensions.
    pub cad_start: String,
    pub cad_dims: [f64; 3],
    pub dont_ask: bool,
}

impl SourceDialog {
    pub fn new(id: &'static str) -> Option<Self> {
        let def = aurafx_filters::find(id)?;
        let values = def
            .params
            .iter()
            .filter(|p| !p.advanced && !NOT_BASIC.contains(&p.key.as_str()) && !matches!(p.kind, ParamKind::Code | ParamKind::Expression))
            .map(|p| (p.clone(), p.default.clone()))
            .collect();
        Some(Self { id, customize: false, values, cad_start: String::new(), cad_dims: [40.0, 30.0, 8.0], dont_ask: false })
    }

    /// The settings that differ from the standard ones, ready for `set_param`.
    fn changes(&self) -> Vec<(String, ParamValue)> {
        let mut out: Vec<(String, ParamValue)> = self.values.iter().filter(|(p, v)| *v != p.default).map(|(p, v)| (p.key.clone(), v.clone())).collect();
        if self.id == "cad_model" && self.customize {
            let doc = if self.cad_start.is_empty() {
                let mut d = aurafx_filters::cad::default_document();
                for (name, v) in ["width", "depth", "thickness"].iter().zip(self.cad_dims) {
                    d.set_parameter(name, &format!("{v} mm"));
                }
                d
            } else {
                aurafx_cad::templates().into_iter().find(|(t, _)| *t == self.cad_start).map(|(_, d)| d).unwrap_or_else(aurafx_filters::cad::default_document)
            };
            out.push(("document".into(), ParamValue::Str(aurafx_cad::CadFile::new(doc).to_json())));
        }
        out
    }
}

/// A short text of a value, for the summary of the standard settings.
fn show(v: &ParamValue) -> String {
    match v {
        ParamValue::Bool(b) => if *b { "on" } else { "off" }.into(),
        ParamValue::Int(i) => i.to_string(),
        ParamValue::Float(x) => aurafx_data::info::g(*x),
        ParamValue::Vec3(a) => format!("{} × {} × {}", aurafx_data::info::g(a[0]), aurafx_data::info::g(a[1]), aurafx_data::info::g(a[2])),
        ParamValue::Floats(a) => a.iter().map(|x| aurafx_data::info::g(*x)).collect::<Vec<_>>().join(", "),
        ParamValue::Str(s) if s.len() > 40 => format!("{}…", &s[..40]),
        ParamValue::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

/// Open the window for a source, or insert it at once when the user asked not to be asked.
pub fn insert_source(state: &mut State, id: &'static str) {
    if state.settings.ask_source_settings {
        if let Some(d) = SourceDialog::new(id) {
            state.source_dialog = Some(d);
            return;
        }
    }
    add_now(state, id, Vec::new());
}

fn add_now(state: &mut State, id: &'static str, changes: Vec<(String, ParamValue)>) {
    state.add_filter(id);
    let node = state.session.lock().active;
    if let Some(n) = node {
        let mut s = state.session.lock();
        if s.nodes.get(&n).is_some_and(|x| matches!(&x.kind, aurafx_pipeline::NodeKind::Source { def } if def.id == id)) {
            for (k, v) in changes {
                s.set_param(n, &k, v);
            }
        }
    }
    // A new CAD Model opens the CAD environment on it.
    if id == "cad_model" {
        crate::cad::enter_active(state);
    }
}

pub fn window(state: &mut State, ctx: &egui::Context) {
    let Some(d) = state.source_dialog.as_ref() else { return };
    let Some(def) = aurafx_filters::find(d.id) else {
        state.source_dialog = None;
        return;
    };
    let mut open = true;
    let (mut insert, mut cancel) = (false, false);
    egui::Window::new(format!("Add {}", def.label)).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
        ctx,
        |ui| {
            ui.set_max_width(420.0);
            ui.label(def.description);
            ui.add_space(6.0);
            let customize = state.source_dialog.as_ref().unwrap().customize;
            if !customize {
                ui.label(RichText::new("With the standard settings:").strong());
                let d = state.source_dialog.as_ref().unwrap();
                if d.id == "cad_model" {
                    ui.label("a plate 40 × 30 × 8 mm with the dimensions width, depth and thickness, opened in the CAD environment");
                }
                egui::Grid::new("source defaults").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    for (p, v) in d.values.iter().take(10) {
                        ui.label(RichText::new(&p.label).weak());
                        ui.label(show(v));
                        ui.end_row();
                    }
                });
                if d.values.len() > 10 {
                    ui.label(RichText::new(format!("and {} more", d.values.len() - 10)).weak().small());
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(format!("{} Insert with these settings", crate::icons::CHECK_CIRCLE)).strong()).clicked() {
                        insert = true;
                    }
                    if ui.button(format!("{} Change settings first…", crate::icons::PENCIL_SIMPLE)).clicked() {
                        state.source_dialog.as_mut().unwrap().customize = true;
                    }
                });
            } else {
                ui.label(RichText::new("Basic settings (the rest are in Properties after inserting):").strong());
                let d = state.source_dialog.as_mut().unwrap();
                if d.id == "cad_model" {
                    egui::Grid::new("cad start").num_columns(2).show(ui, |ui| {
                        ui.label("start from");
                        let shown = if d.cad_start.is_empty() { "Plate".to_string() } else { d.cad_start.clone() };
                        egui::ComboBox::from_id_salt("cad start combo").selected_text(shown).width(240.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.cad_start, String::new(), "Plate");
                            for (t, _) in aurafx_cad::templates() {
                                ui.selectable_value(&mut d.cad_start, t.to_string(), t);
                            }
                        });
                        ui.end_row();
                        if d.cad_start.is_empty() {
                            for (label, v) in ["width", "depth", "thickness"].iter().zip(d.cad_dims.iter_mut()) {
                                ui.label(*label);
                                ui.add(egui::DragValue::new(v).range(0.1..=10000.0).speed(0.5).suffix(" mm"));
                                ui.end_row();
                            }
                        }
                    });
                }
                let mut edits: Vec<(usize, ParamValue)> = Vec::new();
                let values: Vec<(ParamSpec, ParamValue)> = state.source_dialog.as_ref().unwrap().values.clone();
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    egui::Grid::new("source params").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        for (i, (p, v)) in values.iter().enumerate() {
                            let l = ui.label(&p.label);
                            if !p.help.is_empty() {
                                l.on_hover_text(&p.help);
                            }
                            if let Some(nv) = crate::panels::param_widget(state, ui, u64::MAX, p, v, &[]) {
                                edits.push((i, nv));
                            }
                            ui.end_row();
                        }
                    });
                });
                if let Some(d) = state.source_dialog.as_mut() {
                    for (i, v) in edits {
                        d.values[i].1 = v;
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(format!("{} Insert", crate::icons::CHECK_CIRCLE)).strong()).clicked() {
                        insert = true;
                    }
                    if ui.button("Standard settings").on_hover_text("Put every setting back to its standard value").clicked() {
                        let id = state.source_dialog.as_ref().unwrap().id;
                        state.source_dialog = SourceDialog::new(id).map(|mut d| {
                            d.customize = true;
                            d
                        });
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(d) = state.source_dialog.as_mut() {
                    ui.checkbox(&mut d.dont_ask, "Don't ask again (insert sources straight away)")
                        .on_hover_text("Turn it back on in Settings › General › Ask before adding a source");
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );
    if insert {
        let d = state.source_dialog.take().unwrap();
        if d.dont_ask {
            state.settings.ask_source_settings = false;
            state.settings.save();
        }
        let changes = d.changes();
        add_now(state, d.id, changes);
    } else if cancel || !open {
        state.source_dialog = None;
    }
}
