//! Picking edges and faces of the exact part from lists (stage C7): the ribbon's Modify tab opens a
//! picker in the PropertyManager that lists the edges (with the faces on either side, their
//! length and a point on each) or the faces of the part as the B-rep kernel builds it; the chosen
//! ones become a *Fillet*, *Chamfer* or *Shell* feature after the features before them.

use super::op;
use crate::app::State;
use egui::RichText;
use serde_json::json;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Fillet,
    Chamfer,
    Shell,
}

pub struct Pick {
    pub kind: PickKind,
    /// Label, a point on it, and a hint (length, area).
    pub items: Vec<(String, [f64; 3], String)>,
    pub chosen: Vec<bool>,
    pub size: String,
}

/// List the edges or faces and open the picker.
pub fn open(state: &mut State, kind: PickKind) {
    let doc = state.session.lock().cad_edit.as_ref().map(|e| e.document_to_build());
    let Some(doc) = doc else { return };
    let items: Result<Vec<(String, [f64; 3], String)>, String> = match kind {
        PickKind::Fillet | PickKind::Chamfer => aurafx_cad::kernel::brep::list_edges(&doc)
            .map(|v| {
                v.into_iter()
                    .map(|e| {
                        (
                            format!("{}", if e.faces.is_empty() { "edge".to_string() } else { e.faces.join(" × ") }),
                            e.mid,
                            format!("{} mm{}", super::num_text(e.length), if e.straight { "" } else { ", curved" }),
                        )
                    })
                    .collect()
            })
            .map_err(|e| format!("{e}")),
        PickKind::Shell => aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &doc, 48, true)
            .map(|ev| ev.faces().filter_map(|(_, f)| ev.face_pick(&f.name).map(|p| (f.name.clone(), p, format!("{} mm²", super::num_text(f.area))))).collect())
            .map_err(|e| format!("{e}")),
    };
    match items {
        Ok(items) if !items.is_empty() => {
            let n = items.len();
            state.cad.pick = Some(Pick { kind, items, chosen: vec![false; n], size: if kind == PickKind::Shell { "2" } else { "1" }.into() });
        }
        Ok(_) => state.toast("Nothing to pick".to_string(), true),
        Err(e) => state.toast(format!("Picking needs the part to be built by the B-rep kernel: {e}"), true),
    }
}

pub fn ui(state: &mut State, ui: &mut egui::Ui) {
    let Some(p) = state.cad.pick.as_mut() else { return };
    let title = match p.kind {
        PickKind::Fillet => "Fillet: choose the edges to round",
        PickKind::Chamfer => "Chamfer: choose the edges to cut",
        PickKind::Shell => "Shell: choose the faces to open (none: a closed hollow body)",
    };
    ui.label(RichText::new(title).strong());
    ui.horizontal(|ui| {
        ui.label(if p.kind == PickKind::Shell { "wall" } else { "size" });
        ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut p.size));
        ui.label(RichText::new("mm, or an expression of the dimensions").weak().small());
    });
    egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| {
        for (i, (label, _, hint)) in p.items.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut p.chosen[i], label);
                ui.label(RichText::new(hint).weak().small());
            });
        }
    });
    let n = p.chosen.iter().filter(|c| **c).count();
    let (mut apply, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        let ok = n > 0 || p.kind == PickKind::Shell;
        if ui.add_enabled(ok, egui::Button::new(RichText::new(format!("Apply ({n} chosen)")).strong())).clicked() {
            apply = true;
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    if apply {
        let picks: Vec<[f64; 3]> = p.items.iter().zip(&p.chosen).filter(|(_, c)| **c).map(|((_, pt, _), _)| *pt).collect();
        let size = super::number_or_expr(&p.size);
        let kind = p.kind;
        state.cad.pick = None;
        match kind {
            PickKind::Fillet => op(state, "edge_blend", json!({"edges": picks, "size": size})),
            PickKind::Chamfer => op(state, "edge_blend", json!({"edges": picks, "size": size, "chamfer": true})),
            PickKind::Shell => op(state, "shell_faces", json!({"faces": picks, "thickness": size})),
        };
    } else if cancel {
        state.cad.pick = None;
    }
}
