//! The dialog Apply opens when the nodes below the part are affected: what changed, what they
//! refer to, the default fix and the assistant's fix for each, a choice per row, and the
//! "Re-run N downstream filters?" question.

use super::{editing, CadUi};
use crate::app::{LayoutAction, Pane, State};
use aurafx_pipeline::{CheckReport, Fix, FixMode, RunMode};
use egui::RichText;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Default,
    Assistant,
    Leave,
}

/// An Apply waiting for the user's answer.
pub struct ApplyDialog {
    pub keep: bool,
    pub report: CheckReport,
    pub choices: Vec<Choice>,
    /// The assistant's fixes, matched to conflicts by node and parameter.
    pub ai: Vec<Fix>,
    /// The number of chat messages when the assistant was asked (the answer is the next one).
    pub ai_waiting: Option<usize>,
    pub ai_error: Option<String>,
    pub show_diff: bool,
}

/// What the Apply button does: check, and either apply at once or ask.
pub fn request_apply(state: &mut State, keep: bool) {
    let report = state.session.lock().cad_check();
    match report {
        Ok(r) if !r.conflicts.is_empty() || !r.downstream.is_empty() => {
            let choices = r.conflicts.iter().map(|c| if c.fix.is_some() { Choice::Default } else { Choice::Leave }).collect();
            state.cad.apply_dialog = Some(ApplyDialog { keep, report: r, choices, ai: Vec::new(), ai_waiting: None, ai_error: None, show_diff: false });
        }
        // Nothing below, or the part as it was cannot be compared: apply.
        _ => super::apply_now(state, keep, FixMode::Default, RunMode::Now),
    }
}

fn ai_fix_for<'a>(ai: &'a [Fix], node: u64, param: &str) -> Option<&'a Fix> {
    ai.iter().find(|f| f.node == node && f.param == param)
}

pub fn dialog_ui(state: &mut State, ctx: &egui::Context) {
    let Some(mut d) = state.cad.apply_dialog.take() else { return };
    if !editing(state) {
        return;
    }
    // The assistant's answer arrives as the next chat message.
    if let Some(base) = d.ai_waiting {
        if !state.assistant.busy() && state.assistant.messages.len() > base {
            let last = state.assistant.messages.last().map(|m| m.content.clone()).unwrap_or_default();
            d.ai_waiting = None;
            let parsed = aurafx_pipeline::cad_check::parse_ai_fixes(&state.session.lock(), &last);
            match parsed {
                Ok(f) => {
                    for (i, c) in d.report.conflicts.iter().enumerate() {
                        if ai_fix_for(&f, c.node, &c.param).is_some() {
                            d.choices[i] = Choice::Assistant;
                        }
                    }
                    d.ai = f;
                    d.ai_error = None;
                }
                Err(e) => d.ai_error = Some(format!("{e:#}")),
            }
        } else if let Some(e) = state.assistant.error.clone().filter(|_| !state.assistant.busy()) {
            d.ai_waiting = None;
            d.ai_error = Some(e);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    let n_below = d.report.downstream.len();
    let n_conf = d.report.conflicts.len();
    enum Done {
        Apply(FixMode, RunMode),
        Back,
        Discard,
        Ask,
    }
    let mut done: Option<Done> = None;
    let mut open = true;
    egui::Window::new(if n_conf > 0 {
        format!("Applying the CAD changes affects {n_below} downstream object(s)")
    } else {
        format!("Re-run {n_below} downstream filter(s)?")
    })
    .collapsible(false)
    .resizable(true)
    .default_width(720.0)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .open(&mut open)
    .show(ctx, |ui| {
        egui::CollapsingHeader::new("What changed in the part").id_salt("cad diff").default_open(n_conf == 0 || d.show_diff).show(ui, |ui| {
            let text = d.report.diff.describe();
            for line in text.lines() {
                ui.label(RichText::new(line).small().monospace());
            }
        });
        if n_conf > 0 {
            ui.add_space(4.0);
            egui::Grid::new("cad conflicts").striped(true).num_columns(6).spacing([10.0, 4.0]).show(ui, |ui| {
                for h in ["Object", "Parameter", "What changed", "Default fix", "Assistant's fix", "Use"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (i, c) in d.report.conflicts.iter().enumerate() {
                    ui.label(&c.node_name);
                    ui.label(&c.param_label).on_hover_text(&c.value);
                    let what = format!("'{}' {}{}", c.reference, c.kind.label(), if c.may_reference { " (in code)" } else { "" });
                    ui.label(what);
                    match &c.fix {
                        Some(f) => ui
                            .label(RichText::new(format!("{}{}", fix_text(&f.value), if f.check { "  (check)" } else { "" })).small())
                            .on_hover_text(&f.reason),
                        None => ui.label(RichText::new("none").weak()),
                    };
                    match ai_fix_for(&d.ai, c.node, &c.param) {
                        Some(f) => ui.label(RichText::new(fix_text(&f.value)).small()).on_hover_text(&f.reason),
                        None => ui.label(RichText::new(if d.ai_waiting.is_some() { "…" } else { "–" }).weak()),
                    };
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut d.choices[i], Choice::Default, "default").on_hover_text("Apply the default fix");
                        if ai_fix_for(&d.ai, c.node, &c.param).is_some() {
                            ui.selectable_value(&mut d.choices[i], Choice::Assistant, "assistant");
                        }
                        ui.selectable_value(&mut d.choices[i], Choice::Leave, "leave");
                    });
                    ui.end_row();
                }
            });
            if let Some(e) = &d.ai_error {
                ui.label(RichText::new(format!("Assistant: {e}")).color(crate::theme::ERROR).small());
            }
            ui.horizontal(|ui| {
                let asking = d.ai_waiting.is_some();
                if ui
                    .add_enabled(!asking, egui::Button::new(if asking { "Asking the assistant…" } else { "Ask the assistant" }))
                    .on_hover_text("Send the changes and the conflicts to the assistant and read its suggested fixes (it answers in the chat too)")
                    .clicked()
                {
                    done = Some(Done::Ask);
                }
                ui.checkbox(&mut state.cad.rerun_now, format!("re-run the {n_below} nodes below now"));
            });
        } else {
            ui.label(format!("{n_below} node(s) below the part depend on it. They run again with the new part."));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let run = if state.cad.rerun_now { RunMode::Now } else { RunMode::Later };
            if n_conf > 0 {
                if ui.button(RichText::new("Apply fixes and re-run").strong()).clicked() {
                    let fixes: Vec<Fix> = d
                        .report
                        .conflicts
                        .iter()
                        .zip(&d.choices)
                        .filter_map(|(c, ch)| match ch {
                            Choice::Default => c.fix.clone(),
                            Choice::Assistant => ai_fix_for(&d.ai, c.node, &c.param).cloned(),
                            Choice::Leave => None,
                        })
                        .collect();
                    done = Some(Done::Apply(FixMode::Fixes(fixes), run));
                }
                if ui.button("Apply without fixing").on_hover_text("The nodes that refer to changed things will show their own errors").clicked() {
                    done = Some(Done::Apply(FixMode::None, run));
                }
            } else {
                if ui.button(RichText::new("Run now").strong()).clicked() {
                    done = Some(Done::Apply(FixMode::Default, RunMode::Now));
                }
                if ui.button("Later").on_hover_text("Apply the part; the nodes below keep their results until you run them").clicked() {
                    done = Some(Done::Apply(FixMode::Default, RunMode::Later));
                }
            }
            if ui.button("Back to CAD").clicked() {
                done = Some(Done::Back);
            }
            if ui.button("Discard CAD changes").clicked() {
                done = Some(Done::Discard);
            }
        });
    });
    if !open {
        done = Some(Done::Back);
    }
    match done {
        None => state.cad.apply_dialog = Some(d),
        Some(Done::Back) => {}
        Some(Done::Discard) => super::discard(state),
        Some(Done::Apply(f, r)) => super::apply_now(state, d.keep, f, r),
        Some(Done::Ask) => {
            let (prompt, desc) = {
                let s = state.session.lock();
                (s.cad_check_prompt(&d.report), s.describe())
            };
            d.ai_waiting = Some(state.assistant.messages.len());
            state.assistant.send(prompt, desc);
            state.layout_actions.push(LayoutAction::Show(Pane::Assistant));
            state.cad.apply_dialog = Some(d);
        }
    }
}

fn fix_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            let s = s.replace('\n', " ");
            if s.chars().count() > 40 {
                format!("{}…", s.chars().take(40).collect::<String>())
            } else {
                s
            }
        }
        serde_json::Value::Array(a) if a.iter().all(|x| x.is_number()) => {
            format!("({})", a.iter().map(|x| super::num_text(x.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(", "))
        }
        other => other.to_string(),
    }
}

/// A banner on nodes whose re-run was postponed.
pub fn hold_banner(state: &mut State, ui: &mut egui::Ui) {
    let held = state.session.lock().cad_hold.map(|h| state.session.lock().downstream(h).len().saturating_sub(1));
    let Some(n) = held else { return };
    egui::Frame::NONE.fill(egui::Color32::from_rgb(90, 70, 30)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{n} node(s) below the CAD part were not re-run.")).small());
            if ui.small_button("Run now").clicked() {
                state.session.lock().cad_run_held();
            }
        });
    });
}

impl CadUi {
    pub fn dialog_open(&self) -> bool {
        self.apply_dialog.is_some()
    }
}
