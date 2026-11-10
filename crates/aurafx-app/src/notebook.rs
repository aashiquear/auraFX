//! The Notebook panel: a Jupyter-style notebook inside the app. Markdown and Python cells, each
//! run in the app's Python (the same interpreter as the Python console, with `afx`), its printed
//! output, value, error and pictures (`afx.snapshot()`, `afx.display(...)`) shown below it; open
//! and save `.ipynb` files (nbformat 4), so a notebook moves between this panel and Jupyter.

use crate::app::State;
use crate::worker::{PythonRequest, RunOutput};
use base64::Engine;
use egui::{Color32, RichText};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Code,
    Markdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    Text(String),
    Error(String),
    /// A picture (MIME type and bytes).
    Image(String, Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub kind: CellKind,
    pub source: String,
    pub outputs: Vec<Output>,
    pub count: Option<u64>,
    /// Markdown cells show rendered until double-clicked.
    editing: bool,
}

impl Cell {
    pub fn code(src: &str) -> Cell {
        Cell { kind: CellKind::Code, source: src.into(), outputs: vec![], count: None, editing: false }
    }
    pub fn markdown(src: &str) -> Cell {
        Cell { kind: CellKind::Markdown, source: src.into(), outputs: vec![], count: None, editing: false }
    }
}

pub struct Notebook {
    pub cells: Vec<Cell>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    queue: VecDeque<usize>,
    running: Option<(usize, Receiver<RunOutput>)>,
    count: u64,
    selected: usize,
    md_cache: egui_commonmark::CommonMarkCache,
    textures: std::collections::HashMap<(usize, usize, usize), egui::TextureHandle>,
    pub error: Option<String>,
    /// A cell that just finished: scroll so its output shows.
    scroll_to: Option<usize>,
}

impl Default for Notebook {
    fn default() -> Self {
        Notebook {
            cells: vec![
                Cell::markdown("# Notebook\nCells run in auraFX's Python: `afx` drives the window. **Shift+Enter** runs a cell."),
                Cell::code("w = afx.wavelet()\nw.set(color_by=\"RTData\")\nafx.view(\"iso\"); afx.reset_camera()\nafx.snapshot(width=480, height=320)"),
            ],
            path: None,
            dirty: false,
            queue: VecDeque::new(),
            running: None,
            count: 0,
            selected: 1,
            md_cache: Default::default(),
            textures: Default::default(),
            error: None,
            scroll_to: None,
        }
    }
}

/// The text of a notebook field that is a string or a list of lines.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect(),
        _ => String::new(),
    }
}

/// Lines with their newlines, as nbformat stores sources.
fn lines_of(s: &str) -> Value {
    let mut v: Vec<String> = s.split_inclusive('\n').map(str::to_string).collect();
    if v.is_empty() {
        v.push(String::new());
    }
    json!(v)
}

/// Cells from an `.ipynb` file (nbformat 4).
pub fn from_ipynb(text: &str) -> anyhow::Result<Vec<Cell>> {
    let nb: Value = serde_json::from_str(text)?;
    if nb["nbformat"].as_u64().unwrap_or(0) < 4 {
        anyhow::bail!("only notebooks in format 4 are read (this one says {})", nb["nbformat"]);
    }
    let mut out = Vec::new();
    for c in nb["cells"].as_array().cloned().unwrap_or_default() {
        let src = text_of(&c["source"]);
        let mut cell = match c["cell_type"].as_str() {
            Some("code") => Cell::code(&src),
            _ => Cell::markdown(&src),
        };
        cell.count = c["execution_count"].as_u64();
        for o in c["outputs"].as_array().cloned().unwrap_or_default() {
            match o["output_type"].as_str() {
                Some("stream") => cell.outputs.push(Output::Text(text_of(&o["text"]))),
                Some("error") => cell.outputs.push(Output::Error(
                    o["traceback"].as_array().map(|t| t.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default(),
                )),
                Some("execute_result" | "display_data") => {
                    let d = &o["data"];
                    if let Some(png) = d["image/png"].as_str() {
                        if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(png.replace('\n', "")) {
                            cell.outputs.push(Output::Image("image/png".into(), b));
                            continue;
                        }
                    }
                    if !d["text/plain"].is_null() {
                        cell.outputs.push(Output::Text(text_of(&d["text/plain"])));
                    }
                }
                _ => {}
            }
        }
        out.push(cell);
    }
    Ok(out)
}

/// The cells as an `.ipynb` file (nbformat 4.5, the auraFX kernel).
pub fn to_ipynb(cells: &[Cell]) -> String {
    let cells: Vec<Value> = cells
        .iter()
        .enumerate()
        .map(|(i, c)| match c.kind {
            CellKind::Markdown => json!({"cell_type": "markdown", "id": format!("cell-{i}"), "metadata": {}, "source": lines_of(&c.source)}),
            CellKind::Code => {
                let outputs: Vec<Value> = c
                    .outputs
                    .iter()
                    .map(|o| match o {
                        Output::Text(t) => json!({"output_type": "stream", "name": "stdout", "text": lines_of(t)}),
                        Output::Error(e) => {
                            let last = e.lines().last().unwrap_or("");
                            let (n, v) = last.split_once(": ").unwrap_or(("Error", last));
                            json!({"output_type": "error", "ename": n, "evalue": v, "traceback": e.lines().collect::<Vec<_>>()})
                        }
                        Output::Image(m, b) => json!({"output_type": "display_data", "data": {m.as_str(): base64::engine::general_purpose::STANDARD.encode(b)}, "metadata": {}}),
                    })
                    .collect();
                json!({"cell_type": "code", "id": format!("cell-{i}"), "execution_count": c.count, "metadata": {}, "outputs": outputs, "source": lines_of(&c.source)})
            }
        })
        .collect();
    let nb = json!({
        "cells": cells,
        "metadata": {
            "kernelspec": {"display_name": "auraFX (Python)", "language": "python", "name": "aurafx"},
            "language_info": {"name": "python", "file_extension": ".py", "mimetype": "text/x-python"},
        },
        "nbformat": 4,
        "nbformat_minor": 5,
    });
    serde_json::to_string_pretty(&nb).unwrap_or_default() + "\n"
}

/// What a run gave, as cell outputs.
fn outputs_of(o: RunOutput) -> Vec<Output> {
    let mut v = Vec::new();
    if !o.stdout.is_empty() {
        v.push(Output::Text(o.stdout));
    }
    if !o.stderr.is_empty() {
        v.push(Output::Text(o.stderr));
    }
    for (m, b) in o.displays {
        if m.starts_with("image/") && m != "image/svg+xml" {
            v.push(Output::Image(m, b));
        } else {
            v.push(Output::Text(String::from_utf8_lossy(&b).into_owned()));
        }
    }
    if let Some(val) = o.value {
        v.push(Output::Text(val));
    }
    if let Some(e) = o.error {
        v.push(Output::Error(e));
    }
    v
}

impl Notebook {
    fn run_cells(&mut self, cells: impl IntoIterator<Item = usize>) {
        for i in cells {
            if self.cells.get(i).is_some_and(|c| c.kind == CellKind::Code) && !self.queue.contains(&i) {
                self.queue.push_back(i);
            }
        }
    }

    /// Start the next queued cell and collect a finished one.
    fn pump(&mut self, python: &std::sync::mpsc::Sender<PythonRequest>, ctx: &egui::Context) {
        if let Some((i, rx)) = &self.running {
            match rx.try_recv() {
                Ok(out) => {
                    let i = *i;
                    if let Some(c) = self.cells.get_mut(i) {
                        c.outputs = outputs_of(out);
                    }
                    self.textures.retain(|k, _| k.0 != i);
                    self.running = None;
                    self.scroll_to = Some(i);
                    self.dirty = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    return;
                }
                Err(_) => self.running = None,
            }
        }
        if let Some(i) = self.queue.pop_front() {
            let (tx, rx) = std::sync::mpsc::channel();
            let src = self.cells[i].source.clone();
            if python.send(PythonRequest::RunFor(src, tx)).is_ok() {
                self.count += 1;
                self.cells[i].count = Some(self.count);
                self.cells[i].outputs.clear();
                self.running = Some((i, rx));
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
    }

    /// Queue every code cell.
    pub fn run_all(&mut self) {
        let n = self.cells.len();
        self.run_cells(0..n);
    }

    pub fn open(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|t| from_ipynb(&t)) {
            Ok(cells) => {
                self.cells = cells;
                self.path = Some(path);
                self.dirty = false;
                self.textures.clear();
                self.queue.clear();
                self.selected = 0;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e:#}", path.display())),
        }
    }

    pub fn save(&mut self, path: PathBuf) {
        match std::fs::write(&path, to_ipynb(&self.cells)) {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }
}

/// The Notebook panel.
pub fn notebook_ui(state: &mut State, ui: &mut egui::Ui) {
    let python = state.python.sender();
    let nb = &mut state.notebook;
    nb.pump(&python, ui.ctx());
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::icons::with("New")).on_hover_text("An empty notebook").clicked() {
            *nb = Notebook { cells: vec![Cell::code("")], selected: 0, ..Default::default() };
        }
        if ui.button(crate::icons::with("Open…")).clicked() {
            if let Some(p) = crate::dialogs::FileDialog::new().add_filter("Jupyter notebook", &["ipynb"]).pick_file() {
                nb.open(p);
            }
        }
        if ui.button(crate::icons::with("Save")).on_hover_text("As a Jupyter notebook (.ipynb)").clicked() {
            match nb.path.clone() {
                Some(p) => nb.save(p),
                None => {
                    if let Some(p) = crate::dialogs::FileDialog::new().add_filter("Jupyter notebook", &["ipynb"]).set_file_name("notebook.ipynb").save_file() {
                        nb.save(if p.extension().is_none() { p.with_extension("ipynb") } else { p });
                    }
                }
            }
        }
        ui.separator();
        if ui.button(format!("{} Run all", crate::icons::PLAY)).clicked() {
            let n = nb.cells.len();
            nb.run_cells(0..n);
        }
        if ui.button("Clear outputs").clicked() {
            for c in nb.cells.iter_mut() {
                c.outputs.clear();
                c.count = None;
            }
            nb.textures.clear();
        }
        ui.separator();
        if ui.button("+ Code").clicked() {
            let at = (nb.selected + 1).min(nb.cells.len());
            nb.cells.insert(at, Cell::code(""));
            nb.selected = at;
        }
        if ui.button("+ Text").on_hover_text("A Markdown cell").clicked() {
            let at = (nb.selected + 1).min(nb.cells.len());
            let mut c = Cell::markdown("");
            c.editing = true;
            nb.cells.insert(at, c);
            nb.selected = at;
        }
        let name = nb.path.as_ref().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "untitled".into());
        ui.label(RichText::new(format!("{name}{}", if nb.dirty { " •" } else { "" })).weak());
        if nb.running.is_some() || !nb.queue.is_empty() {
            ui.spinner();
        }
    });
    if let Some(e) = &nb.error {
        ui.colored_label(crate::theme::ERROR, e);
    }
    ui.separator();
    let mut action: Option<(usize, &'static str)> = None;
    let running = nb.running.as_ref().map(|r| r.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for i in 0..nb.cells.len() {
            let sel = nb.selected == i;
            let frame = egui::Frame::group(ui.style()).stroke(egui::Stroke::new(
                if sel { 1.5 } else { 0.5 },
                if sel { crate::theme::ACCENT } else { ui.visuals().widgets.noninteractive.bg_stroke.color },
            ));
            let shown = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let c = &nb.cells[i];
                    let tag = match (c.kind, running == Some(i), nb.queue.contains(&i)) {
                        (CellKind::Markdown, ..) => "text".to_string(),
                        (_, true, _) => "[*]".into(),
                        (_, _, true) => "[ ]".into(),
                        _ => c.count.map(|n| format!("[{n}]")).unwrap_or_else(|| "[ ]".into()),
                    };
                    ui.label(RichText::new(tag).monospace().weak());
                    if c.kind == CellKind::Code && ui.small_button(crate::icons::PLAY).on_hover_text("Run (Shift+Enter)").clicked() {
                        action = Some((i, "run"));
                    }
                    if ui.small_button("↑").on_hover_text("Move up").clicked() {
                        action = Some((i, "up"));
                    }
                    if ui.small_button("↓").on_hover_text("Move down").clicked() {
                        action = Some((i, "down"));
                    }
                    if ui.small_button("×").on_hover_text("Delete the cell").clicked() {
                        action = Some((i, "delete"));
                    }
                });
                let c = &mut nb.cells[i];
                let mut clicked = false;
                match c.kind {
                    CellKind::Markdown if !c.editing => {
                        let r = ui.scope(|ui| {
                            egui_commonmark::CommonMarkViewer::new().show(
                                ui,
                                &mut nb.md_cache,
                                if c.source.trim().is_empty() { "*(empty text cell: double-click to write)*" } else { &c.source },
                            );
                        });
                        let resp = ui.interact(r.response.rect, ui.id().with(("md", i)), egui::Sense::click());
                        if resp.double_clicked() {
                            c.editing = true;
                        }
                        clicked |= resp.clicked();
                    }
                    _ => {
                        let r = ui.add(
                            egui::TextEdit::multiline(&mut c.source)
                                .code_editor()
                                .desired_rows(if c.kind == CellKind::Code { 2 } else { 3 })
                                .desired_width(f32::INFINITY)
                                .hint_text(if c.kind == CellKind::Code { "Python (afx drives the window)" } else { "Markdown" }),
                        );
                        if r.changed() {
                            nb.dirty = true;
                        }
                        clicked |= r.gained_focus() || r.clicked();
                        if r.has_focus() && ui.input_mut(|inp| inp.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter)) {
                            if c.kind == CellKind::Markdown {
                                c.editing = false;
                                action = Some((i, "next"));
                            } else {
                                action = Some((i, "run_next"));
                            }
                        }
                    }
                }
                if clicked {
                    nb.selected = i;
                }
                // Outputs.
                for (k, o) in nb.cells[i].outputs.iter().enumerate() {
                    match o {
                        Output::Text(t) => {
                            ui.label(RichText::new(t.trim_end()).monospace());
                        }
                        Output::Error(e) => {
                            ui.label(RichText::new(e.trim_end()).monospace().color(crate::theme::ERROR));
                        }
                        Output::Image(_, bytes) => {
                            let key = (i, k, bytes.len());
                            if !nb.textures.contains_key(&key) {
                                if let Ok(img) = image::load_from_memory(bytes) {
                                    let rgba = img.to_rgba8();
                                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                                    nb.textures.insert(key, ui.ctx().load_texture(format!("nb-{i}-{k}"), ci, Default::default()));
                                }
                            }
                            match nb.textures.get(&key) {
                                Some(t) => {
                                    let s = t.size_vec2();
                                    let w = s.x.min(ui.available_width());
                                    ui.image((t.id(), egui::vec2(w, s.y * w / s.x.max(1.0))));
                                }
                                None => {
                                    ui.colored_label(Color32::GRAY, "(a picture that could not be read)");
                                }
                            }
                        }
                    }
                }
            });
            if nb.scroll_to == Some(i) {
                ui.scroll_to_rect(shown.response.rect, Some(egui::Align::BOTTOM));
                nb.scroll_to = None;
            }
        }
    });
    if let Some((i, what)) = action {
        match what {
            "run" => nb.run_cells([i]),
            "run_next" => {
                nb.run_cells([i]);
                if i + 1 >= nb.cells.len() {
                    nb.cells.push(Cell::code(""));
                }
                nb.selected = i + 1;
            }
            "next" => nb.selected = (i + 1).min(nb.cells.len().saturating_sub(1)),
            "up" if i > 0 => {
                nb.cells.swap(i, i - 1);
                nb.selected = i - 1;
                nb.textures.clear();
            }
            "down" if i + 1 < nb.cells.len() => {
                nb.cells.swap(i, i + 1);
                nb.selected = i + 1;
                nb.textures.clear();
            }
            "delete" => {
                nb.cells.remove(i);
                if nb.cells.is_empty() {
                    nb.cells.push(Cell::code(""));
                }
                nb.selected = nb.selected.min(nb.cells.len() - 1);
                nb.textures.clear();
                nb.queue.retain(|q| *q != i);
            }
            _ => {}
        }
        nb.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipynb_round_trip() {
        let mut a = Cell::code("x = 1\nprint(x)\n");
        a.count = Some(3);
        a.outputs = vec![
            Output::Text("1\n".into()),
            Output::Image("image/png".into(), vec![137, 80, 78, 71]),
            Output::Error("Traceback\nZeroDivisionError: division by zero".into()),
        ];
        let cells = vec![Cell::markdown("# Title\ntext"), a];
        let text = to_ipynb(&cells);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["nbformat"], 4);
        assert_eq!(v["metadata"]["kernelspec"]["name"], "aurafx");
        assert_eq!(v["cells"][1]["outputs"][2]["ename"], "ZeroDivisionError");
        let back = from_ipynb(&text).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!((back[0].kind, back[0].source.as_str()), (CellKind::Markdown, "# Title\ntext"));
        assert_eq!((back[1].source.as_str(), back[1].count), ("x = 1\nprint(x)\n", Some(3)));
        assert_eq!(back[1].outputs, cells[1].outputs);
        // A notebook written by Jupyter (sources as line lists, execute_result text).
        let j = r#"{"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
            {"cell_type": "code", "execution_count": 1, "metadata": {}, "source": ["a = 2\n", "a * 3"],
             "outputs": [{"output_type": "execute_result", "execution_count": 1, "data": {"text/plain": ["6"]}, "metadata": {}}]}]}"#;
        let c = from_ipynb(j).unwrap();
        assert_eq!((c[0].source.as_str(), c[0].outputs.clone()), ("a = 2\na * 3", vec![Output::Text("6".into())]));
    }
}
