//! Panels from Python plugins (`@afx.panel("Title")`): each open panel is a window drawn from the
//! items its function last gave; a press or a changed value runs the function again (on the
//! Python thread) with that event, and its new items replace the old.

use crate::app::State;
use crate::worker::PythonRequest;
use egui::RichText;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;

#[derive(Default)]
pub struct PanelView {
    pub items: Vec<Value>,
    pub error: Option<String>,
    pending: Option<Receiver<String>>,
    /// Values being edited (a text field before Enter), by name.
    edits: BTreeMap<String, Value>,
    textures: BTreeMap<String, egui::TextureHandle>,
}

#[derive(Default)]
pub struct PluginPanels {
    pub titles: Vec<String>,
    titles_rx: Option<Receiver<Vec<String>>>,
    pub open: BTreeMap<String, PanelView>,
}

impl PluginPanels {
    /// Ask the Python thread for the panel titles (after plugins load).
    pub fn refresh_titles(&mut self, python: &std::sync::mpsc::Sender<PythonRequest>) {
        let (tx, rx) = std::sync::mpsc::channel();
        if python.send(PythonRequest::PanelTitles(tx)).is_ok() {
            self.titles_rx = Some(rx);
        }
    }

    /// Open a panel (draws it by running its function once).
    pub fn open_panel(&mut self, title: &str, python: &std::sync::mpsc::Sender<PythonRequest>) {
        let v = self.open.entry(title.to_string()).or_default();
        send(v, title, "", python);
    }
}

fn send(v: &mut PanelView, title: &str, event: &str, python: &std::sync::mpsc::Sender<PythonRequest>) {
    let (tx, rx) = std::sync::mpsc::channel();
    if python.send(PythonRequest::Panel(title.to_string(), event.to_string(), tx)).is_ok() {
        v.pending = Some(rx);
    }
}

/// Draw the open plugin panels.
pub fn panels_ui(state: &mut State, ctx: &egui::Context) {
    let python = state.python.sender();
    let pp = &mut state.plugin_panels;
    if let Some(rx) = &pp.titles_rx {
        if let Ok(t) = rx.try_recv() {
            pp.titles = t;
            pp.titles_rx = None;
        }
    }
    let mut close = Vec::new();
    let titles: Vec<String> = pp.open.keys().cloned().collect();
    for title in titles {
        let v = pp.open.get_mut(&title).unwrap();
        if let Some(rx) = &v.pending {
            match rx.try_recv() {
                Ok(text) => {
                    let r: Value = serde_json::from_str(&text).unwrap_or(json!({"items": [], "error": "the panel gave no answer"}));
                    v.items = r["items"].as_array().cloned().unwrap_or_default();
                    v.error = r["error"].as_str().map(str::to_string);
                    v.pending = None;
                    v.edits.clear();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
                Err(_) => v.pending = None,
            }
        }
        let mut open = true;
        let mut event: Option<Value> = None;
        egui::Window::new(&title).id(egui::Id::new(("plugin panel", &title))).open(&mut open).default_width(300.0).show(ctx, |ui| {
            if v.pending.is_some() {
                ui.spinner();
            }
            for it in v.items.clone() {
                let name = it["name"].as_str().unwrap_or("").to_string();
                match it["kind"].as_str().unwrap_or("") {
                    "label" => {
                        ui.label(it["text"].as_str().unwrap_or(""));
                    }
                    "separator" => {
                        ui.separator();
                    }
                    "button" => {
                        let t = it["text"].as_str().unwrap_or("").to_string();
                        if ui.button(&t).clicked() {
                            event = Some(json!({"kind": "button", "name": t}));
                        }
                    }
                    "slider" | "number" => {
                        let integer = it["int"].as_bool().unwrap_or(false);
                        let mut x = v.edits.get(&name).and_then(|e| e.as_f64()).unwrap_or_else(|| it["value"].as_f64().unwrap_or(0.0));
                        ui.horizontal(|ui| {
                            ui.label(&name);
                            let r = if it["kind"] == "slider" {
                                let (lo, hi) = (it["min"].as_f64().unwrap_or(0.0), it["max"].as_f64().unwrap_or(1.0));
                                let mut s = egui::Slider::new(&mut x, lo..=hi);
                                if integer {
                                    s = s.integer();
                                }
                                ui.add(s)
                            } else {
                                let mut d = egui::DragValue::new(&mut x).speed(if integer { 1.0 } else { 0.01 });
                                if integer {
                                    d = d.fixed_decimals(0);
                                }
                                ui.add(d)
                            };
                            if r.changed() {
                                v.edits.insert(name.clone(), json!(x));
                            }
                            // Run the panel when the value is let go of (not on every step of a drag).
                            if r.drag_stopped() || (r.changed() && !r.dragged()) || r.lost_focus() {
                                let val = if integer { json!(x.round() as i64) } else { json!(x) };
                                event = Some(json!({"kind": "set", "name": name, "value": val}));
                            }
                        });
                    }
                    "checkbox" => {
                        let mut b = it["value"].as_bool().unwrap_or(false);
                        if ui.checkbox(&mut b, &name).changed() {
                            event = Some(json!({"kind": "set", "name": name, "value": b}));
                        }
                    }
                    "text" => {
                        let mut t =
                            v.edits.get(&name).and_then(|e| e.as_str().map(str::to_string)).unwrap_or_else(|| it["value"].as_str().unwrap_or("").to_string());
                        ui.horizontal(|ui| {
                            ui.label(&name);
                            let r = ui.text_edit_singleline(&mut t);
                            if r.changed() {
                                v.edits.insert(name.clone(), json!(t));
                            }
                            if r.lost_focus() && v.edits.contains_key(&name) {
                                event = Some(json!({"kind": "set", "name": name, "value": t}));
                            }
                        });
                    }
                    "choice" => {
                        let cur = it["value"].as_str().unwrap_or("").to_string();
                        let opts: Vec<String> =
                            it["options"].as_array().map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_string)).collect()).unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(&name);
                            egui::ComboBox::from_id_salt(("plugin choice", &title, &name)).selected_text(&cur).show_ui(ui, |ui| {
                                for o in &opts {
                                    if ui.selectable_label(*o == cur, o).clicked() && *o != cur {
                                        event = Some(json!({"kind": "set", "name": name, "value": o}));
                                    }
                                }
                            });
                        });
                    }
                    "image" => {
                        let path = it["path"].as_str().unwrap_or("").to_string();
                        if !v.textures.contains_key(&path) {
                            if let Ok(img) = image::open(&path) {
                                let rgba = img.to_rgba8();
                                let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                                v.textures.insert(path.clone(), ui.ctx().load_texture(format!("plugin-{path}"), ci, Default::default()));
                            }
                        }
                        match v.textures.get(&path) {
                            Some(t) => {
                                let s = t.size_vec2();
                                let w = s.x.min(ui.available_width());
                                ui.image((t.id(), egui::vec2(w, s.y * w / s.x.max(1.0))));
                            }
                            None => {
                                ui.label(RichText::new(format!("(no picture at {path})")).weak());
                            }
                        }
                    }
                    other => {
                        ui.label(RichText::new(format!("(unknown item {other})")).weak());
                    }
                }
            }
            if let Some(e) = &v.error {
                ui.add(egui::Label::new(RichText::new(e).monospace().small().color(crate::theme::ERROR)).wrap());
            }
        });
        if let Some(e) = event {
            if v.pending.is_none() {
                v.textures.clear();
                send(v, &title, &e.to_string(), &python);
            }
        }
        if !open {
            close.push(title);
        }
    }
    for t in close {
        pp.open.remove(&t);
    }
}
