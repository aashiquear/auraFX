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
    /// An event that happened while the panel's function was still running for an earlier one
    /// (the latest such): sent when that answer arrives.
    queued: Option<String>,
    /// Values being edited (a text field before Enter) or set and not yet answered, by name.
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

impl PanelView {
    /// Something was pressed or changed: run the panel's function with it, now or (while it
    /// still runs for an earlier event) as soon as that answer is here. Of several events
    /// that wait, the latest is sent.
    fn event(&mut self, title: &str, event: &Value, python: &std::sync::mpsc::Sender<PythonRequest>) {
        if event["kind"] == "set" {
            // Shown as set until the function has answered for it.
            if let Some(name) = event["name"].as_str() {
                self.edits.insert(name.to_string(), event["value"].clone());
            }
        }
        if self.pending.is_some() {
            self.queued = Some(event.to_string());
        } else {
            self.textures.clear();
            send(self, title, &event.to_string(), python);
        }
    }

    /// Take the function's answer if it has arrived (and send the event that waited for it).
    /// Returns whether an answer is still awaited.
    fn poll(&mut self, title: &str, python: &std::sync::mpsc::Sender<PythonRequest>) -> bool {
        let Some(rx) = &self.pending else { return false };
        match rx.try_recv() {
            Ok(text) => {
                let r: Value = serde_json::from_str(&text).unwrap_or(json!({"items": [], "error": "the panel gave no answer"}));
                self.items = r["items"].as_array().cloned().unwrap_or_default();
                self.error = r["error"].as_str().map(str::to_string);
                self.pending = None;
                match self.queued.take() {
                    // What was changed meanwhile stays as the user left it until the function
                    // has seen it.
                    Some(e) => {
                        self.textures.clear();
                        send(self, title, &e, python);
                    }
                    None => self.edits.clear(),
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => {
                self.pending = None;
                if let Some(e) = self.queued.take() {
                    send(self, title, &e, python);
                }
            }
        }
        self.pending.is_some()
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
        if v.poll(&title, &python) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
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
                        let mut b = v.edits.get(&name).and_then(|e| e.as_bool()).unwrap_or_else(|| it["value"].as_bool().unwrap_or(false));
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
                        let cur =
                            v.edits.get(&name).and_then(|e| e.as_str().map(str::to_string)).unwrap_or_else(|| it["value"].as_str().unwrap_or("").to_string());
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
            v.event(&title, &e, &python);
        }
        if !open {
            close.push(title);
        }
    }
    for t in close {
        pp.open.remove(&t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A change made while the panel's function still runs is not lost: it is sent when the
    /// answer arrives (the latest of several), and stays shown until then.
    #[test]
    fn events_wait_for_the_pending_answer() {
        let (python, requests) = std::sync::mpsc::channel::<PythonRequest>();
        let next = || match requests.try_recv() {
            Ok(PythonRequest::Panel(_, event, reply)) => Some((event, reply)),
            _ => None,
        };
        let mut v = PanelView::default();
        v.event("P", &json!({"kind": "set", "name": "n", "value": 1}), &python);
        let (first, reply) = next().expect("the first event is sent at once");
        assert!(first.contains("\"value\":1"));
        // Two more while the first is not answered: nothing is sent yet.
        v.event("P", &json!({"kind": "set", "name": "n", "value": 2}), &python);
        v.event("P", &json!({"kind": "set", "name": "n", "value": 3}), &python);
        assert!(next().is_none());
        assert!(v.poll("P", &python), "still waiting");
        assert_eq!(v.edits["n"], json!(3), "the panel shows what was set last");
        // The answer (for value 1) arrives: the latest event goes out, the edit stays.
        reply.send(json!({"items": [{"kind": "number", "name": "n", "value": 1}]}).to_string()).unwrap();
        assert!(v.poll("P", &python), "waiting for the answer to the queued event");
        assert_eq!(v.items[0]["value"], json!(1));
        assert_eq!(v.edits["n"], json!(3));
        let (second, reply) = next().expect("the queued event is sent");
        assert!(second.contains("\"value\":3"), "{second}");
        assert!(next().is_none(), "the one in between is skipped");
        reply.send(json!({"items": [{"kind": "number", "name": "n", "value": 3}]}).to_string()).unwrap();
        assert!(!v.poll("P", &python));
        assert_eq!(v.items[0]["value"], json!(3));
        assert!(v.edits.is_empty() && v.queued.is_none());
    }
}
