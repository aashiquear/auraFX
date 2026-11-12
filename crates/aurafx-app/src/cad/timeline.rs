//! The timeline bar at the bottom of the environment: the features in order with the
//! rollback marker, suppress toggles and reordering, and the state of the last build.

use super::{editing, op};
use crate::app::State;
use egui::RichText;
use serde_json::json;

pub fn timeline_ui(state: &mut State, ui: &mut egui::Ui) {
    if !editing(state) {
        return;
    }
    let (features, rollback, selected, summary, error, undo, redo, building) = {
        let s = state.session.lock();
        let Some(e) = s.cad_edit.as_ref() else { return };
        (
            e.active().features.iter().map(|f| (f.id, f.name.clone(), f.suppressed)).collect::<Vec<_>>(),
            e.rollback,
            e.selected,
            e.summary.clone(),
            e.error.clone(),
            e.undo.len(),
            e.redo.len(),
            e.evaluated_revision < e.revision,
        )
    };
    let n = features.len();
    let upto = rollback.unwrap_or(n);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Timeline").strong());
        if ui.add_enabled(undo > 0, egui::Button::new("Undo")).clicked() {
            state.session.lock().cad_env_undo();
        }
        if ui.add_enabled(redo > 0, egui::Button::new("Redo")).clicked() {
            state.session.lock().cad_env_redo();
        }
        ui.separator();
        egui::ScrollArea::horizontal().id_salt("cad timeline").max_width(ui.available_width() - 330.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, (id, name, suppressed)) in features.iter().enumerate() {
                    if i == upto {
                        ui.label(RichText::new("▮ rollback").color(crate::theme::ACCENT).small());
                    }
                    let beyond = i >= upto;
                    let mut text = RichText::new(name);
                    if *suppressed {
                        text = text.strikethrough().weak();
                    } else if beyond {
                        text = text.weak();
                    }
                    let r = ui.selectable_label(selected == Some(*id), text).on_hover_text("Click to select. Right-click: suppress, roll back here, move");
                    if r.clicked() {
                        state.session.lock().cad_env_select(Some(*id));
                    }
                    r.context_menu(|ui| {
                        if ui.button(if *suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                            ui.close();
                            op(state, "suppress", json!({"feature": name, "suppressed": !suppressed}));
                        }
                        if ui.button("Roll back to here").on_hover_text("Build only the features up to and including this one").clicked() {
                            ui.close();
                            state.session.lock().cad_env_rollback(Some(i + 1));
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Move earlier")).clicked() {
                            ui.close();
                            op(state, "move", json!({"feature": name, "to": i.saturating_sub(1)}));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("Move later")).clicked() {
                            ui.close();
                            op(state, "move", json!({"feature": name, "to": i + 1}));
                        }
                    });
                    if i + 1 < n {
                        ui.label(RichText::new("›").weak());
                    }
                }
                if upto >= n && n > 0 {
                    ui.label(RichText::new("▮").color(crate::theme::ACCENT)).on_hover_text("The rollback marker is at the end: everything is built");
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if n > 0 {
                if ui.add_enabled(upto > 0, egui::Button::new("◀")).on_hover_text("Roll the marker back one feature").clicked() {
                    state.session.lock().cad_env_rollback(Some(upto - 1));
                }
                if ui.add_enabled(upto < n, egui::Button::new("▶")).on_hover_text("Roll the marker forward one feature").clicked() {
                    state.session.lock().cad_env_rollback(if upto + 1 >= n { None } else { Some(upto + 1) });
                }
            }
            if n == 0 {
                ui.label(RichText::new("an empty part").weak().small());
            } else if let Some(e) = &error {
                ui.label(RichText::new(e).color(crate::theme::ERROR).small());
            } else if building {
                ui.label(RichText::new("building…").weak().small());
            } else if let Some(s) = &summary {
                ui.label(RichText::new(format!("{} kernel · {} ms · volume {} mm³", s.kernel, s.ms.round(), super::num_text(s.volume))).weak().small());
            }
        });
    });
}
