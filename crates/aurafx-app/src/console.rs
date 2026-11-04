//! Python console panel.

use crate::app::State;
use crate::worker::PythonResult;
use egui::{Color32, RichText};

#[derive(Default)]
pub struct Console {
    pub input: String,
    pub entries: Vec<Entry>,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub completions: Vec<String>,
    pub show_help: bool,
    pub script_path: Option<std::path::PathBuf>,
}

pub struct Entry {
    pub code: String,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub value: Option<String>,
    pub ms: f64,
    pub origin: String,
}

impl Console {
    pub fn push_result(&mut self, r: &PythonResult) {
        self.entries.push(Entry {
            code: r.code.clone(),
            stdout: r.output.stdout.clone(),
            stderr: r.output.stderr.clone(),
            error: r.output.error.clone(),
            value: r.output.value.clone(),
            ms: r.elapsed.as_secs_f64() * 1000.0,
            origin: r.origin.clone(),
        });
        if self.entries.len() > 500 {
            self.entries.remove(0);
        }
    }
}

pub const CHEAT_SHEET: &str = r#"# auraFX Python quick reference
m = afx.load("data.vtu")                  # open a file (returns a Node)
s = afx.sphere(radius=2)                  # any source: afx.<source>(**params)
n = m.slice(normal=[0, 0, 1])             # any filter: node.<filter>(**params)
n = m.contour(values=[0.5, 1.0])
n = m.threshold(array="T", lower=300)
n = m.clip(type="Sphere", radius=1.5)
n = m.calculator(expression="mag(velocity)", result="speed")
n = m.glyph(orient="velocity", shape="Arrow")
n.color_by("T", cmap="Inferno", range=(0, 100))
n.set(opacity=0.5, representation="Wireframe", name="my slice")
n.hide(); n.show(); n.delete()
n.info()           # dict with points, cells, bounds, arrays
n.arrays()         # {'points': [...], 'cells': [...]}
a = n.array("T")   # numpy array (copy)
p = n.points       # (N, 3) numpy array
n.add_array("T2", a * 2)
afx.filters()      # list every filter/source and its parameters
afx.help_filter("slice")
afx.time(3); afx.times()
afx.view("iso"); afx.reset_camera(); afx.screenshot("shot.png")
afx.describe()     # text summary of the whole pipeline
"#;

pub fn console_ui(state: &mut State, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button(format!("{} Run  (Ctrl+Enter)", crate::icons::PLAY)).clicked() {
            submit(state);
        }
        if ui.button("Run script…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Python", &["py"]).pick_file() {
                match std::fs::read_to_string(&p) {
                    Ok(code) => {
                        state.console.script_path = Some(p);
                        state.run_python(&code, "console");
                    }
                    Err(e) => state.toast(format!("{e}"), true),
                }
            }
        }
        if ui.button("Clear").clicked() {
            state.console.entries.clear();
        }
        if ui.button("Help").clicked() {
            state.console.show_help = !state.console.show_help;
        }
        if ui.button("Trace").on_hover_text("Insert the Python equivalent of everything done in the UI so far").clicked() {
            let trace = state.session.lock().trace.join("\n");
            state.console.input = trace;
        }
        if state.python.busy.load(std::sync::atomic::Ordering::Relaxed) {
            ui.spinner();
            ui.label("running…");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(state.python.version.lock().split_whitespace().next().map(|v| format!("Python {v}")).unwrap_or_default()).weak().small());
        });
    });
    if state.console.show_help {
        egui::Window::new("Python quick reference").open(&mut state.console.show_help).default_width(620.0).show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut CHEAT_SHEET.to_string()).code_editor().desired_width(f32::INFINITY).interactive(false));
            });
        });
    }
    let input_h = 76.0;
    let out_h = (ui.available_height() - input_h).max(40.0);
    egui::ScrollArea::vertical().id_salt("py_out").auto_shrink([false, false]).max_height(out_h).stick_to_bottom(true).show(ui, |ui| {
        if state.console.entries.is_empty() {
            ui.label(RichText::new("Python console. `afx` is the auraFX module, `np` is numpy. Try: afx.sphere().color_by('Normals')").weak());
        }
        for e in &state.console.entries {
            for (i, line) in e.code.lines().enumerate() {
                ui.label(RichText::new(format!("{} {}", if i == 0 { ">>>" } else { "..." }, line)).monospace().color(Color32::from_rgb(140, 190, 255)));
            }
            if !e.stdout.is_empty() {
                ui.label(RichText::new(e.stdout.trim_end()).monospace());
            }
            if !e.stderr.is_empty() {
                ui.label(RichText::new(e.stderr.trim_end()).monospace().color(Color32::from_rgb(240, 190, 80)));
            }
            if let Some(v) = &e.value {
                ui.label(RichText::new(v).monospace().color(Color32::from_gray(200)));
            }
            if let Some(err) = &e.error {
                ui.label(RichText::new(err.trim_end()).monospace().color(Color32::from_rgb(255, 110, 110)));
            }
            if e.ms > 50.0 {
                ui.label(RichText::new(format!("({:.0} ms, {})", e.ms, e.origin)).weak().small());
            }
        }
    });
    ui.separator();
    // Completion popup.
    if !state.console.completions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Tab:").weak());
            let comps = state.console.completions.clone();
            for c in comps.iter().take(30) {
                if ui.small_button(c).clicked() {
                    apply_completion(&mut state.console, c);
                }
            }
        });
    }
    let resp = ui.add(
        egui::TextEdit::multiline(&mut state.console.input)
            .code_editor()
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("Python… (Ctrl+Enter to run, Tab to complete, Up/Down for history)"),
    );
    if resp.has_focus() {
        let (run, tab, up, down) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                !state.console.input.contains('\n') && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                !state.console.input.contains('\n') && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            )
        });
        if run {
            submit(state);
        }
        if tab {
            let prefix = current_token(&state.console.input);
            let comps = state.python.complete(&prefix);
            if comps.len() == 1 {
                apply_completion(&mut state.console, &comps[0]);
                state.console.completions.clear();
            } else {
                state.console.completions = comps;
            }
        }
        if up || down {
            let n = state.console.history.len();
            if n > 0 {
                let pos = match (state.console.history_pos, up) {
                    (None, true) => Some(n - 1),
                    (Some(p), true) => Some(p.saturating_sub(1)),
                    (Some(p), false) if p + 1 < n => Some(p + 1),
                    _ => None,
                };
                state.console.history_pos = pos;
                state.console.input = pos.map(|p| state.console.history[p].clone()).unwrap_or_default();
            }
        }
    }
}

fn current_token(input: &str) -> String {
    let tail: String = input.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect();
    tail.chars().rev().collect()
}

fn apply_completion(c: &mut Console, completion: &str) {
    let tok = current_token(&c.input);
    let part = tok.rsplit('.').next().unwrap_or("");
    let keep = c.input.len() - part.len();
    c.input.truncate(keep);
    c.input.push_str(completion);
    c.completions.clear();
}

fn submit(state: &mut State) {
    let code = state.console.input.trim().to_string();
    if code.is_empty() {
        return;
    }
    state.console.history.push(code.clone());
    state.console.history_pos = None;
    state.console.input.clear();
    state.console.completions.clear();
    state.run_python(&code, "console");
}
