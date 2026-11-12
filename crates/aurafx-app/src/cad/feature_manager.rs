//! The model tree: the part's tree (dimensions, features in order, finishing modifiers,
//! configurations) in place of the Pipeline panel while the environment is open.

use super::{editing, expr_field, number_or_expr, op};
use crate::app::State;
use aurafx_cad::{BooleanOp, Id};
use egui::RichText;
use serde_json::{json, Value};

enum Act {
    Op(&'static str, Value),
    Select(Option<Id>),
    Rollback(Option<usize>),
    Rename(Id, String),
    EditSketch(Id),
    /// Change a sketch kept with the part.
    EditSaved(String),
    /// Edit a component on its own (its name), or go back to the assembly.
    Focus(Option<String>),
    Pick(super::PickKind),
}

fn kind_label(f: &aurafx_cad::Feature) -> String {
    let kind = serde_json::to_value(&f.shape).ok().and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_string))).unwrap_or_default();
    match kind.as_str() {
        "SketchExtrude" | "Extrude" => "extruded sketch".into(),
        "SketchRevolve" | "Revolve" => "revolved sketch".into(),
        "SketchSweep" | "Sweep" => "sweep".into(),
        "SketchLoft" | "Loft" => "loft".into(),
        "Brep" => "imported body".into(),
        k => k.to_lowercase(),
    }
}

pub fn feature_manager_ui(state: &mut State, ui: &mut egui::Ui) {
    if !editing(state) {
        return;
    }
    let (doc, selected, rollback, summary, focus, assembly) = {
        let s = state.session.lock();
        let Some(e) = s.cad_edit.as_ref() else { return };
        (e.active().clone(), e.selected, e.rollback, e.summary.clone(), e.focused().map(|c| c.name.clone()), e.document.name.clone())
    };
    let values = doc.parameter_values().ok();
    let mut acts: Vec<Act> = Vec::new();
    // A component edited on its own: where one is, and the way back.
    if let Some(c) = &focus {
        ui.horizontal(|ui| {
            let top = if assembly.is_empty() { "Assembly".to_string() } else { assembly.clone() };
            if ui.link(RichText::new(format!("{} {top}", crate::icons::CUBE))).on_hover_text("Back to the assembly").clicked() {
                acts.push(Act::Focus(None));
            }
            ui.label(RichText::new("›").weak());
            ui.label(RichText::new(c).strong());
        });
        ui.label(
            RichText::new(
                "This component alone, in its own coordinates: every tool works on its features. The rest of the assembly is hidden until you go back.",
            )
            .weak()
            .small(),
        );
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} {}", crate::icons::CUBE, if doc.name.is_empty() { "Part" } else { &doc.name })).strong());
        if let Some(s) = &summary {
            ui.label(RichText::new(format!("{} mm³", super::num_text(s.volume))).weak().small());
        }
    });
    super::guide::guide_ui(state, ui);
    let (build_error, approximate) = {
        let s = state.session.lock();
        let e = s.cad_edit.as_ref();
        (e.and_then(|e| e.error.clone()), e.and_then(|e| e.summary.as_ref()).map(|m| m.approximate.clone()).unwrap_or_default())
    };
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // ---------------------------------------------------------------- dimensions
        egui::CollapsingHeader::new(RichText::new(format!("Dimensions ({})", doc.parameters.len())).strong())
            .id_salt("fm dims")
            .default_open(doc.features.len() < 2)
            .show(ui, |ui| {
                egui::Grid::new("fm dims grid").num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
                    for p in &doc.parameters {
                        ui.label(&p.name).on_hover_text(if p.comment.is_empty() { "A named dimension of the part" } else { &p.comment });
                        if let Some(t) = expr_field(ui, ("fm dim", &p.name), &p.expr, 100.0) {
                            acts.push(Act::Op("parameter", json!({"name": p.name, "expr": t})));
                        }
                        ui.horizontal(|ui| {
                            match values.as_ref().and_then(|v| v.get(&p.name)) {
                                Some(v) => ui.label(RichText::new(format!("= {}", super::num_text(*v))).weak().small()),
                                None => ui.label(RichText::new("?").color(crate::theme::ERROR)),
                            };
                            if ui.small_button("×").on_hover_text("Remove this dimension").clicked() {
                                acts.push(Act::Op("remove_parameter", json!({"name": p.name})));
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    let (n, e) = &mut state.cad.new_param;
                    ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(n).hint_text("name"));
                    ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(e).hint_text("40 mm"));
                    if ui.small_button("Add").clicked() && !n.trim().is_empty() && !e.trim().is_empty() {
                        acts.push(Act::Op("parameter", json!({"name": n.trim(), "expr": e.trim()})));
                        n.clear();
                        e.clear();
                    }
                });
            });
        // ---------------------------------------------------------------- sketches kept with the part
        if !doc.sketches.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Sketches ({})", doc.sketches.len())).strong()).id_salt("fm sketches").default_open(true).show(ui, |ui| {
                ui.label(RichText::new("Kept for sweeps (a profile and a path) and lofts (sections).").weak().small());
                for k in &doc.sketches {
                    let (closed, open) = (k.sketch.profiles().len(), k.sketch.open_paths().len());
                    let what = match (closed, open) {
                        (0, 0) => "nothing to use yet".to_string(),
                        (c, 0) => format!("{c} closed outline(s)"),
                        (0, _) => "an open path".to_string(),
                        (c, _) => format!("{c} closed outline(s) and an open path"),
                    };
                    let users: Vec<&str> = doc
                        .features
                        .iter()
                        .filter(|f| match &f.shape {
                            aurafx_cad::Shape::SketchSweep { profile, path, .. } => profile == &k.name || path == &k.name,
                            aurafx_cad::Shape::SketchLoft { sections } => sections.contains(&k.name),
                            _ => false,
                        })
                        .map(|f| f.name.as_str())
                        .collect();
                    ui.horizontal(|ui| {
                        let r = ui.selectable_label(false, &k.name).on_hover_text(format!(
                            "{what}, on {}{}\nClick to change it on the sketch canvas.",
                            k.sketch.plane_label(),
                            if users.is_empty() { String::new() } else { format!("\nUsed by {}", users.join(", ")) }
                        ));
                        if r.clicked() {
                            acts.push(Act::EditSaved(k.name.clone()));
                        }
                        ui.label(RichText::new(what.clone()).weak().small());
                        if ui
                            .add_enabled(users.is_empty(), egui::Button::new("×").small())
                            .on_hover_text("Remove this sketch")
                            .on_disabled_hover_text(format!("Used by {}: remove that feature, or give it another sketch, first", users.join(", ")))
                            .clicked()
                        {
                            acts.push(Act::Op("sketch_remove", json!({"name": k.name})));
                        }
                    });
                }
            });
        }
        // ---------------------------------------------------------------- features
        egui::CollapsingHeader::new(RichText::new(format!("Features ({})", doc.features.len())).strong()).id_salt("fm features").default_open(true).show(
            ui,
            |ui| {
                if doc.features.is_empty() {
                    ui.label(RichText::new("No feature yet. Use the buttons above, the ribbon's Sketch and Features tabs, or press S to search.").weak().small());
                }
                let upto = rollback.unwrap_or(doc.features.len());
                for (i, f) in doc.features.iter().enumerate() {
                    if i == upto && rollback.is_some() {
                        ui.label(RichText::new("▮ rollback marker").color(crate::theme::ACCENT).small());
                    }
                    ui.horizontal(|ui| {
                        let mut on = !f.suppressed;
                        if ui.checkbox(&mut on, "").on_hover_text("Include this feature (off: suppressed)").changed() {
                            acts.push(Act::Op("suppress", json!({"feature": f.name, "suppressed": !on})));
                        }
                        if state.cad.rename.as_ref().is_some_and(|(id, _)| *id == f.id) {
                            let (_, text) = state.cad.rename.as_mut().unwrap();
                            let r = ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(text));
                            r.request_focus();
                            if r.lost_focus() {
                                let (id, t) = state.cad.rename.take().unwrap();
                                if ui.input(|i| i.key_pressed(egui::Key::Enter)) || !t.trim().is_empty() && t != f.name {
                                    acts.push(Act::Rename(id, t));
                                }
                            }
                            return;
                        }
                        let verb = match f.op {
                            BooleanOp::Add => "adds",
                            BooleanOp::Cut => "cuts",
                            BooleanOp::Intersect => "intersects",
                        };
                        let mut text = RichText::new(&f.name);
                        // The feature the build error names, and those built only roughly.
                        let failed = build_error.as_ref().filter(|e| !f.suppressed && e.contains(&format!("'{}'", f.name)));
                        let rough = approximate.iter().find(|a| a.contains(&f.name));
                        if f.suppressed {
                            text = text.weak().strikethrough();
                        } else if failed.is_some() {
                            text = text.color(crate::theme::ERROR);
                        } else if i >= upto {
                            text = text.weak();
                        }
                        let mut hover = format!("{} {}\nRight-click: rename, suppress, roll back, move, delete. Double-click: rename.", verb, kind_label(f));
                        if let Some(e) = failed {
                            hover = format!("This feature stops the part from building:\n{e}\n\nChange its numbers on the right, suppress it (untick the box), or undo.");
                        } else if let Some(a) = rough {
                            hover = format!("{hover}\n\nShown roughly: {a}");
                        }
                        let r = ui.selectable_label(selected == Some(f.id), text).on_hover_text(hover);
                        if failed.is_some() {
                            ui.label(RichText::new(crate::icons::WARNING).color(crate::theme::ERROR));
                        } else if rough.is_some() {
                            ui.label(RichText::new("≈").weak()).on_hover_text("Built roughly by this kernel; the exact kernel builds it at Apply");
                        }
                        ui.label(RichText::new(format!("{verb} {}", kind_label(f))).weak().small());
                        if r.clicked() {
                            acts.push(Act::Select(Some(f.id)));
                        }
                        if r.double_clicked() {
                            state.cad.rename = Some((f.id, f.name.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Rename").clicked() {
                                ui.close();
                                state.cad.rename = Some((f.id, f.name.clone()));
                            }
                            if matches!(f.shape, aurafx_cad::Shape::SketchExtrude { .. } | aurafx_cad::Shape::SketchRevolve { .. })
                                && ui.button("Edit sketch…").clicked()
                            {
                                ui.close();
                                acts.push(Act::EditSketch(f.id));
                            }
                            // A sketch on a face: another face for it (the repair when its own is gone), or none.
                            if let Some(sk) = f.shape.sketch() {
                                let on = sk.face.clone();
                                if ui
                                    .button(if on.is_some() { "Pick another face…" } else { "Put its sketch on a face…" })
                                    .on_hover_text(match &on {
                                        Some(n) => format!("The sketch sits on the face {n} and follows it. Choose another flat face of the part as it is before this feature."),
                                        None => "Choose a flat face: the sketch then stays on that face when the part changes".to_string(),
                                    })
                                    .clicked()
                                {
                                    ui.close();
                                    acts.push(Act::Pick(super::PickKind::RepairFace(f.id)));
                                }
                                if on.is_some() && ui.button("Detach from its face").on_hover_text("The sketch stays where it is and no longer follows the face").clicked() {
                                    ui.close();
                                    acts.push(Act::Op("sketch_face", json!({"feature": f.name, "face": Value::Null})));
                                }
                            }
                            if ui.button(if f.suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                                ui.close();
                                acts.push(Act::Op("suppress", json!({"feature": f.name, "suppressed": !f.suppressed})));
                            }
                            if focus.is_none() && ui.button("Roll back to here").clicked() {
                                ui.close();
                                acts.push(Act::Rollback(Some(i + 1)));
                            }
                            if ui.add_enabled(i > 0, egui::Button::new("Move up")).clicked() {
                                ui.close();
                                acts.push(Act::Op("move", json!({"feature": f.name, "to": i - 1})));
                            }
                            if ui.add_enabled(i + 1 < doc.features.len(), egui::Button::new("Move down")).clicked() {
                                ui.close();
                                acts.push(Act::Op("move", json!({"feature": f.name, "to": i + 1})));
                            }
                            ui.separator();
                            if ui.button("Delete").clicked() {
                                ui.close();
                                acts.push(Act::Op("remove", json!({"feature": f.name})));
                            }
                        });
                    });
                }
                if rollback.is_some() && ui.small_button("Roll forward to the end").clicked() {
                    acts.push(Act::Rollback(None));
                }
            },
        );
        // ---------------------------------------------------------------- components
        if !doc.components.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Components ({})", doc.components.len())).strong())
                .id_salt("fm components")
                .default_open(true)
                .show(ui, |ui| {
                    for c in &doc.components {
                        let on_it = state.cad.component.as_deref() == Some(c.name.as_str());
                        ui.horizontal(|ui| {
                            let mut vis = c.visible;
                            if ui.checkbox(&mut vis, "").on_hover_text("Show this part (off: left out of the assembly)").changed() {
                                acts.push(Act::Op("component_visible", json!({"component": c.name, "visible": vis})));
                            }
                            if state.cad.component_rename.as_ref().is_some_and(|(n, _)| *n == c.name) {
                                let (_, text) = state.cad.component_rename.as_mut().unwrap();
                                let r = ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(text));
                                r.request_focus();
                                if r.lost_focus() {
                                    let (old, new) = state.cad.component_rename.take().unwrap();
                                    if !new.trim().is_empty() && new.trim() != old {
                                        acts.push(Act::Op("component_rename", json!({"component": old, "to": new.trim()})));
                                    }
                                }
                                return;
                            }
                            let mut text = RichText::new(&c.name);
                            if !c.visible {
                                text = text.weak().strikethrough();
                            }
                            let is_fixed = doc.mates.iter().find(|m| m.kind == aurafx_cad::MateKind::Fixed && m.a.component == c.name).map(|m| m.name.clone());
                            let r = ui.selectable_label(on_it, text).on_hover_text(format!(
                                "{} ({} features)\nClick to move or turn it; double-click to edit its own features; right-click for more",
                                c.document.name,
                                c.document.features.len()
                            ));
                            if is_fixed.is_some() {
                                ui.label(RichText::new("fixed").weak().small()).on_hover_text("A fixed mate holds this part where it is placed");
                            }
                            if r.double_clicked() {
                                acts.push(Act::Focus(Some(c.name.clone())));
                            } else if r.clicked() {
                                state.cad.component = if on_it { None } else { Some(c.name.clone()) };
                            }
                            r.context_menu(|ui| {
                                if ui.button("Edit component").on_hover_text("Its own features in the tree, the feature panel and the ribbon, the part shown alone").clicked() {
                                    ui.close();
                                    acts.push(Act::Focus(Some(c.name.clone())));
                                }
                                if ui.button("Rename").clicked() {
                                    ui.close();
                                    state.cad.component_rename = Some((c.name.clone(), c.name.clone()));
                                }
                                match &is_fixed {
                                    Some(m) => {
                                        if ui.button("Let it move").on_hover_text("Remove the fixed mate").clicked() {
                                            ui.close();
                                            acts.push(Act::Op("mate_remove", json!({"mate": m})));
                                        }
                                    }
                                    None => {
                                        if ui.button("Fix in place").on_hover_text("A fixed mate: this part stays where it is, the others move to meet their mates").clicked() {
                                            ui.close();
                                            acts.push(Act::Op("mate", json!({"kind": "fixed", "a": c.name})));
                                        }
                                    }
                                }
                                ui.separator();
                                if ui.button("Remove").clicked() {
                                    ui.close();
                                    acts.push(Act::Op("component_remove", json!({"component": c.name})));
                                }
                            });
                            if ui.small_button("×").on_hover_text("Remove this part from the assembly").clicked() {
                                acts.push(Act::Op("component_remove", json!({"component": c.name})));
                            }
                        });
                        if on_it {
                            egui::Grid::new(("fm component", c.id.0)).num_columns(6).spacing([6.0, 2.0]).show(ui, |ui| {
                                let mut at = c.placement.translate;
                                let mut rot = c.placement.rotate_deg;
                                let mut moved = false;
                                for (label, v) in ["x", "y", "z"].into_iter().zip(at.iter_mut()) {
                                    ui.label(RichText::new(label).weak().small());
                                    moved |= ui.add(egui::DragValue::new(v).speed(0.5).suffix(" mm")).changed();
                                }
                                ui.end_row();
                                for (label, v) in ["rx", "ry", "rz"].into_iter().zip(rot.iter_mut()) {
                                    ui.label(RichText::new(label).weak().small());
                                    moved |= ui.add(egui::DragValue::new(v).speed(1.0).suffix("°")).changed();
                                }
                                if moved {
                                    acts.push(Act::Op("component_move", json!({"component": c.name, "at": at, "rotate": rot})));
                                }
                            });
                            // With mates the numbers above are where the part starts from; this is where they put it.
                            let mated = doc.mates.iter().any(|m| !m.suppressed && (m.a.component == c.name || m.b.component == c.name));
                            if let Some((_, at, rot)) = summary.as_ref().filter(|_| mated).and_then(|m| m.placements.iter().find(|p| p.0 == c.name)) {
                                ui.label(
                                    RichText::new(format!(
                                        "its mates put it at ({}, {}, {}), turned ({}, {}, {})°",
                                        super::num_text(at[0]),
                                        super::num_text(at[1]),
                                        super::num_text(at[2]),
                                        super::num_text(rot[0]),
                                        super::num_text(rot[1]),
                                        super::num_text(rot[2])
                                    ))
                                    .weak()
                                    .small(),
                                )
                                .on_hover_text("The position and turn above are where the part starts from; it moves as little as its mates allow");
                            }
                        }
                    }
                });
        }
        // ---------------------------------------------------------------- mates
        if !doc.mates.is_empty() {
            let status = summary.as_ref().map(|m| m.mates.clone()).unwrap_or_default();
            egui::CollapsingHeader::new(RichText::new(format!("Mates ({})", doc.mates.len())).strong()).id_salt("fm mates").default_open(true).show(ui, |ui| {
                match summary.as_ref().and_then(|m| m.mate_dof) {
                    Some(0) => ui.label(RichText::new("The mated parts are fully held.").weak().small()),
                    Some(d) => ui.label(RichText::new(format!("{d} way(s) left to move: add mates to hold the parts, or leave them free.")).weak().small()),
                    None => ui.label(RichText::new("Solved whenever the assembly is built.").weak().small()),
                };
                for m in &doc.mates {
                    let error = status.iter().find(|x| x.0 == m.name).and_then(|x| x.1.clone());
                    ui.horizontal(|ui| {
                        let mut on = !m.suppressed;
                        if ui.checkbox(&mut on, "").on_hover_text("Use this mate (off: left out)").changed() {
                            acts.push(Act::Op("mate_set", json!({"mate": m.name, "suppressed": !on})));
                        }
                        let what = match m.kind {
                            aurafx_cad::MateKind::Fixed => format!("{} stays in place", m.a.component),
                            k => format!("{}: {} and {}", k.label(), m.a.label(), m.b.label()),
                        };
                        let mut text = RichText::new(&m.name);
                        if m.suppressed {
                            text = text.weak().strikethrough();
                        } else if error.is_some() {
                            text = text.color(crate::theme::ERROR);
                        }
                        let r = ui.label(text);
                        match &error {
                            Some(e) if !m.suppressed => {
                                r.on_hover_text(format!("{what}\n\nNot met: {e}"));
                                ui.label(RichText::new(crate::icons::WARNING).color(crate::theme::ERROR)).on_hover_text(e);
                            }
                            _ => {
                                r.on_hover_text(&what);
                            }
                        }
                        if ui.small_button("×").on_hover_text("Remove this mate").clicked() {
                            acts.push(Act::Op("mate_remove", json!({"mate": m.name})));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        ui.label(RichText::new(match m.kind {
                            aurafx_cad::MateKind::Fixed => "stays in place".to_string(),
                            k => format!("{} · {} · {}", k.label(), m.a.label(), m.b.label()),
                        })
                        .weak()
                        .small());
                    });
                    if m.kind.has_value() && !m.suppressed {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);
                            // Dragging moves the assembly: the parts follow as it is built again.
                            let mut v = m.value;
                            let angle = m.kind == aurafx_cad::MateKind::Angle;
                            let slider = if angle {
                                egui::Slider::new(&mut v, 0.0..=180.0).suffix("°")
                            } else {
                                let top = (m.value.abs() * 2.0).max(50.0);
                                egui::Slider::new(&mut v, 0.0..=top).suffix(" mm")
                            };
                            let r = ui.add(slider.clamping(egui::SliderClamping::Never)).on_hover_text(match &m.expr {
                                Some(e) => format!("Follows the dimension expression {e}; dragging gives it a number of its own"),
                                None => "Drag to move the assembly; type a number for an exact value".to_string(),
                            });
                            if r.changed() && (v - m.value).abs() > 1e-9 {
                                acts.push(Act::Op("mate_set", json!({"mate": m.name, "value": v})));
                            }
                            if let Some(e) = &m.expr {
                                ui.label(RichText::new(format!("= {e}")).weak().small());
                            }
                            {
                                let mut flip = m.flip;
                                if ui.checkbox(&mut flip, "flush").on_hover_text("The two faces look the same way instead of at each other").changed() {
                                    acts.push(Act::Op("mate_set", json!({"mate": m.name, "flip": flip})));
                                }
                            }
                        });
                    }
                }
            });
        }
        // ---------------------------------------------------------------- finish
        if !doc.finish.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Finish ({})", doc.finish.len())).strong()).id_salt("fm finish").default_open(true).show(
                ui,
                |ui| {
                    ui.label(RichText::new("Modifiers on the whole part").weak().small());
                    for (i, m) in doc.finish.iter().enumerate() {
                        let v = serde_json::to_value(m).unwrap_or(Value::Null);
                        let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("?").to_string();
                        ui.horizontal(|ui| {
                            ui.label(&kind);
                            if let Some(o) = v.as_object() {
                                for (k, val) in o.iter().filter(|(k, _)| k.as_str() != "kind") {
                                    if let Some(x) = val.as_f64() {
                                        ui.label(RichText::new(k).weak().small());
                                        if let Some(t) = expr_field(ui, ("fm finish", i, k), &super::num_text(x), 56.0) {
                                            if let Ok(n) = t.parse::<f64>() {
                                                acts.push(Act::Op("set_finish", json!({"index": i, "field": k, "value": n})));
                                            }
                                        }
                                    }
                                }
                            }
                            if ui.small_button("×").clicked() {
                                acts.push(Act::Op("remove_finish", json!({"index": i})));
                            }
                        });
                    }
                },
            );
        }
        // ---------------------------------------------------------------- configurations
        if !doc.configurations.is_empty() || !doc.parameters.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("Configurations ({})", doc.configurations.len())).strong())
                .id_salt("fm cfg")
                .show(ui, |ui| {
                    ui.label(RichText::new("Named sets of dimension values: variants of one part.").weak().small());
                    let active = doc.active_configuration.clone();
                    if ui.selectable_label(active.is_none(), "(the dimensions as listed above)").clicked() {
                        acts.push(Act::Op("activate", json!({"name": Value::Null})));
                    }
                    for c in &doc.configurations {
                        ui.horizontal(|ui| {
                            let r = ui.selectable_label(active.as_deref() == Some(c.name.as_str()), &c.name);
                            r.clone().on_hover_text(c.overrides.iter().map(|p| format!("{} = {}", p.name, p.expr)).collect::<Vec<_>>().join("\n"));
                            if r.clicked() {
                                acts.push(Act::Op("activate", json!({"name": c.name})));
                            }
                            if ui.small_button("×").on_hover_text("Remove this configuration").clicked() {
                                acts.push(Act::Op("remove_configuration", json!({"name": c.name})));
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let name = &mut state.cad.new_configuration;
                        ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(name).hint_text("name, e.g. large"));
                        let ok = !name.trim().is_empty() && !doc.parameters.is_empty();
                        if ui
                            .add_enabled(ok, egui::Button::new("Save values"))
                            .on_hover_text("Keep the dimensions' present values under this name (an existing name is replaced). To change a configuration: set the dimensions, then save under its name again.")
                            .on_disabled_hover_text("Type a name; the part needs at least one dimension")
                            .clicked()
                        {
                            let overrides: serde_json::Map<String, Value> = doc.parameters.iter().map(|p| (p.name.clone(), json!(p.expr))).collect();
                            acts.push(Act::Op("configuration", json!({"name": name.trim(), "overrides": overrides})));
                            name.clear();
                        }
                    });
                });
        }
    });
    for a in acts {
        match a {
            Act::Op(name, args) => {
                op(state, name, args);
            }
            Act::Select(id) => state.session.lock().cad_env_select(id),
            Act::EditSketch(id) => super::sketch_edit(state, id),
            Act::EditSaved(name) => super::sketch_saved(state, &name),
            Act::Focus(c) => super::focus(state, c.as_deref()),
            Act::Pick(kind) => super::pick::open(state, kind),
            Act::Rollback(n) => state.session.lock().cad_env_rollback(n),
            Act::Rename(id, text) => {
                if let Some(f) = doc.feature(id) {
                    op(state, "rename", json!({"feature": f.name, "to": text}));
                }
            }
        }
    }
    let _ = number_or_expr;
}
