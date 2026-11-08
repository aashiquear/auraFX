//! Picking edges and faces of the exact part from lists (stage C7): the ribbon's Modify tab opens a
//! picker in the PropertyManager that lists the edges (with the faces on either side, their
//! length and a point on each) or the faces of the part as the B-rep kernel builds it; the chosen
//! ones become a *Fillet*, *Chamfer*, *Shell*, *Draft*, *Mirror* or *Pattern on a face* feature
//! after the features before them.

use super::op;
use crate::app::State;
use egui::RichText;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Fillet,
    Chamfer,
    Shell,
    /// Taper the chosen faces.
    Draft,
    /// Reflect the part about the chosen flat face.
    MirrorFace,
    /// Lay holes or bosses out on the chosen flat face.
    PatternFace,
}

impl PickKind {
    /// One face is chosen (a radio list), not several.
    fn single(self) -> bool {
        matches!(self, PickKind::MirrorFace | PickKind::PatternFace)
    }
    fn faces(self) -> bool {
        !matches!(self, PickKind::Fillet | PickKind::Chamfer)
    }
}

/// What the pattern dialog collects.
pub struct PatternForm {
    pub layout: usize,
    pub boss: bool,
    pub count: String,
    pub spacing: String,
    pub radius: String,
    pub diameter: String,
    pub depth: String,
}

impl Default for PatternForm {
    fn default() -> Self {
        Self { layout: 0, boss: false, count: "4".into(), spacing: "12".into(), radius: "15".into(), diameter: "6".into(), depth: "8".into() }
    }
}

pub struct Pick {
    pub kind: PickKind,
    /// Label, a point on it, and a hint (length, area).
    pub items: Vec<(String, [f64; 3], String)>,
    /// The outward direction of each item when it is a face.
    pub dirs: Vec<[f64; 3]>,
    /// The polyline of each item when it is an edge (empty for a face).
    pub lines: Vec<Vec<[f64; 3]>>,
    pub chosen: Vec<bool>,
    pub size: String,
    pub form: PatternForm,
    /// Where the neutral plane of a draft sits: the bottom of the part.
    pub bottom: f64,
}

/// List the edges or faces and open the picker.
pub fn open(state: &mut State, kind: PickKind) {
    let doc = state.session.lock().cad_edit.as_ref().map(|e| e.document_to_build());
    let Some(doc) = doc else { return };
    let bottom = state.session.lock().cad_edit.as_ref().and_then(|e| e.summary.as_ref()).map(|s| s.bounds.0[2]).unwrap_or(0.0);
    let mut dirs: Vec<[f64; 3]> = Vec::new();
    let mut lines: Vec<Vec<[f64; 3]>> = Vec::new();
    let items: Result<Vec<(String, [f64; 3], String)>, String> = match kind {
        PickKind::Fillet | PickKind::Chamfer => aurafx_cad::kernel::brep::list_edges(&doc)
            .map(|v| {
                v.into_iter()
                    .map(|e| {
                        lines.push(e.polyline.clone());
                        (
                            format!("{}", if e.faces.is_empty() { "edge".to_string() } else { e.faces.join(" × ") }),
                            e.mid,
                            format!("{} mm{}", super::num_text(e.length), if e.straight { "" } else { ", curved" }),
                        )
                    })
                    .collect()
            })
            .map_err(|e| format!("{e}")),
        PickKind::Shell | PickKind::Draft | PickKind::MirrorFace | PickKind::PatternFace => {
            aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &doc, 48, true)
                .map(|ev| {
                    let flat_only = matches!(kind, PickKind::MirrorFace | PickKind::PatternFace);
                    ev.faces()
                        .filter(|(_, f)| !flat_only || f.kind == aurafx_cad::FaceKind::Plane)
                        .filter_map(|(_, f)| {
                            ev.face_pick(&f.name).map(|p| {
                                dirs.push(f.direction);
                                (f.name.clone(), p, format!("{} mm²", super::num_text(f.area)))
                            })
                        })
                        .collect()
                })
                .map_err(|e| format!("{e}"))
        }
    };
    match items {
        Ok(items) if !items.is_empty() => {
            let n = items.len();
            let size = match kind {
                PickKind::Shell => "2",
                PickKind::Draft => "3",
                _ => "1",
            };
            dirs.resize(n, [0.0, 0.0, 1.0]);
            lines.resize(n, Vec::new());
            state.cad.pick = Some(Pick { kind, items, dirs, lines, chosen: vec![false; n], size: size.into(), form: PatternForm::default(), bottom });
        }
        Ok(_) => state.toast("Nothing to pick".to_string(), true),
        Err(e) => state.toast(if kind.faces() { format!("{e}") } else { format!("Picking needs the part to be built by the B-rep kernel: {e}") }, true),
    }
}

pub fn ui(state: &mut State, ui: &mut egui::Ui) {
    let Some(p) = state.cad.pick.as_mut() else { return };
    let title = match p.kind {
        PickKind::Fillet => "Fillet: choose the edges to round",
        PickKind::Chamfer => "Chamfer: choose the edges to cut",
        PickKind::Shell => "Shell: choose the faces to open (none: a closed hollow body)",
        PickKind::Draft => "Draft: choose the side faces to taper (they stay fixed where the part meets the bottom plane)",
        PickKind::MirrorFace => "Mirror: choose the flat face to reflect the part about",
        PickKind::PatternFace => "Pattern: choose the flat face to lay holes or bosses out on",
    };
    ui.label(RichText::new(title).strong());
    ui.label(RichText::new("Click the part in the 3D view to choose (click again to un-choose), or tick the list.").weak().small());
    match p.kind {
        PickKind::MirrorFace => {}
        PickKind::PatternFace => {
            let f = &mut p.form;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut f.boss, false, "Holes");
                ui.selectable_value(&mut f.boss, true, "Bosses");
                ui.separator();
                for (i, l) in ["Row", "Grid", "Circle"].iter().enumerate() {
                    ui.selectable_value(&mut f.layout, i, *l);
                }
            });
            egui::Grid::new("pattern form").num_columns(2).show(ui, |ui| {
                let field = |ui: &mut egui::Ui, label: &str, text: &mut String, tip: &str| {
                    ui.label(label);
                    ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(text)).on_hover_text(tip);
                    ui.end_row();
                };
                field(ui, "diameter", &mut f.diameter, "mm");
                field(ui, if f.boss { "height" } else { "depth" }, &mut f.depth, "mm");
                field(
                    ui,
                    if f.layout == 1 { "count (x, y)" } else { "count" },
                    &mut f.count,
                    if f.layout == 1 { "two numbers: 3, 2" } else { "how many copies" },
                );
                if f.layout == 2 {
                    field(ui, "radius", &mut f.radius, "mm, of the circle about the face's center");
                } else {
                    field(ui, if f.layout == 1 { "spacing (x, y)" } else { "spacing" }, &mut f.spacing, "mm between copies (a grid takes two numbers: 15, 20)");
                }
            });
            ui.label(RichText::new("The first copy sits at the face's center; the row runs along the face's x axis.").weak().small());
        }
        _ => {
            ui.horizontal(|ui| {
                ui.label(match p.kind {
                    PickKind::Shell => "wall",
                    PickKind::Draft => "angle",
                    _ => "size",
                });
                ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut p.size));
                ui.label(RichText::new(if p.kind == PickKind::Draft { "degrees" } else { "mm, or an expression of the dimensions" }).weak().small());
            });
        }
    }
    let single = p.kind.single();
    egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| {
        for i in 0..p.items.len() {
            ui.horizontal(|ui| {
                let label = p.items[i].0.clone();
                if single {
                    if ui.radio(p.chosen[i], &label).clicked() {
                        p.chosen.iter_mut().for_each(|c| *c = false);
                        p.chosen[i] = true;
                    }
                } else {
                    ui.checkbox(&mut p.chosen[i], &label);
                }
                ui.label(RichText::new(&p.items[i].2).weak().small());
            });
        }
    });
    let n = p.chosen.iter().filter(|c| **c).count();
    let (mut apply, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        let ok = n > 0 || p.kind == PickKind::Shell;
        let ok = ok && !(p.kind.single() && n != 1);
        if ui.add_enabled(ok, egui::Button::new(RichText::new(format!("Apply ({n} chosen)")).strong())).clicked() {
            apply = true;
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    if apply {
        let picks: Vec<[f64; 3]> = p.items.iter().zip(&p.chosen).filter(|(_, c)| **c).map(|((_, pt, _), _)| *pt).collect();
        let first = p.chosen.iter().position(|c| *c);
        let (face_name, face_dir) = first.map(|i| (p.items[i].0.clone(), p.dirs[i])).unwrap_or_default();
        let size = super::number_or_expr(&p.size);
        let kind = p.kind;
        let (form, bottom) = (std::mem::take(&mut p.form), p.bottom);
        state.cad.pick = None;
        match kind {
            PickKind::Draft => op(state, "draft", json!({"faces": picks, "angle": size, "neutral_origin": [0, 0, bottom], "neutral_normal": [0, 0, 1]})),
            PickKind::MirrorFace => op(state, "mirror_plane", json!({"origin": picks[0], "normal": face_dir, "name": format!("Mirror about {face_name}")})),
            PickKind::PatternFace => {
                let num = |t: &str| super::number_or_expr(t);
                let two = |t: &str| -> Value {
                    let v: Vec<f64> = t.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
                    json!([v.first().copied().unwrap_or(2.0), v.get(1).copied().unwrap_or(v.first().copied().unwrap_or(2.0))])
                };
                let layout = ["linear", "grid", "circular"][form.layout.min(2)];
                let mut a = json!({"face": face_name, "kind": if form.boss { "boss" } else { "hole" }, "diameter": num(&form.diameter), "layout": layout});
                a[if form.boss { "height" } else { "depth" }] = num(&form.depth);
                match form.layout {
                    1 => {
                        a["counts"] = two(&form.count);
                        a["spacing"] = two(&form.spacing);
                    }
                    2 => {
                        a["count"] = json!(form.count.trim().parse::<u64>().unwrap_or(4));
                        a["radius"] = num(&form.radius);
                    }
                    _ => {
                        a["count"] = json!(form.count.trim().parse::<u64>().unwrap_or(3));
                        a["spacing"] = num(&form.spacing);
                    }
                }
                op(state, "face_pattern", a)
            }
            PickKind::Fillet => op(state, "edge_blend", json!({"edges": picks, "size": size})),
            PickKind::Chamfer => op(state, "edge_blend", json!({"edges": picks, "size": size, "chamfer": true})),
            PickKind::Shell => op(state, "shell_faces", json!({"faces": picks, "thickness": size})),
        };
    } else if cancel {
        state.cad.pick = None;
    }
}

// ------------------------------------------------------------------ picking in the 3D view

/// The squared distance from `p` to the segment `a`..`b`.
fn dist2_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let (ab, ap) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [p[0] - a[0], p[1] - a[1], p[2] - a[2]]);
    let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    let t = if l2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (0..3).map(|k| (ap[k] - t * ab[k]).powi(2)).sum()
}

/// The name of the CAD face under a point of the node's surface, by the nearest triangle.
fn face_at(ds: &aurafx_data::DataSet, p: [f64; 3]) -> Option<String> {
    let aurafx_data::DataSet::PolyData(pd) = ds else { return None };
    let ids = pd.cell_data.get("FaceId")?;
    let names = match &pd.field_data.get("FaceNames")?.data {
        aurafx_data::ArrayData::String(v) => v,
        _ => return None,
    };
    let mut best: Option<(f64, usize)> = None;
    for (ci, (_, c)) in pd.polys.iter().enumerate() {
        if c.len() != 3 {
            continue;
        }
        let q: Vec<[f64; 3]> = c.iter().map(|i| pd.points.get(*i as usize)).collect();
        // The distance to the triangle's edges, or zero when the point is over its inside.
        let d = (0..3).map(|k| dist2_segment(p, q[k], q[(k + 1) % 3])).fold(f64::INFINITY, f64::min);
        let n = [
            (q[1][1] - q[0][1]) * (q[2][2] - q[0][2]) - (q[1][2] - q[0][2]) * (q[2][1] - q[0][1]),
            (q[1][2] - q[0][2]) * (q[2][0] - q[0][0]) - (q[1][0] - q[0][0]) * (q[2][2] - q[0][2]),
            (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[1][1] - q[0][1]) * (q[2][0] - q[0][0]),
        ];
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-300);
        let off = ((p[0] - q[0][0]) * n[0] + (p[1] - q[0][1]) * n[1] + (p[2] - q[0][2]) * n[2]) / nl;
        // Inside test by barycentric signs in the plane.
        let inside = (0..3).all(|k| {
            let (a, b) = (q[k], q[(k + 1) % 3]);
            let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let w = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
            let c = [e[1] * w[2] - e[2] * w[1], e[2] * w[0] - e[0] * w[2], e[0] * w[1] - e[1] * w[0]];
            c[0] * n[0] + c[1] * n[1] + c[2] * n[2] >= -1e-9 * nl
        });
        let score = if inside { off * off } else { d + off * off };
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, ci));
        }
    }
    let (_, ci) = best?;
    names.get(ids.get(ci, 0) as usize).cloned()
}

/// Whether a picker is open (a click in the 3D view then chooses an edge or a face).
pub fn picking(state: &State) -> bool {
    state.cad.pick.is_some()
}

/// A click in the 3D view on the CAD part at `world`: tick the face under it, or, for an edge
/// picker, the edge of that face nearest to the click. Clicking a ticked item unticks it.
pub fn click(state: &mut State, node: aurafx_pipeline::NodeId, world: [f64; 3]) {
    let Some(ds) = state.session.lock().nodes.get(&node).and_then(|n| n.output.clone()) else { return };
    let Some(face) = face_at(&ds, world) else {
        state.toast("That is not a CAD face".to_string(), true);
        return;
    };
    let Some(p) = state.cad.pick.as_mut() else { return };
    let hit = if p.kind.faces() {
        p.items.iter().position(|(l, _, _)| *l == face)
    } else {
        // Of the edges that border the clicked face (their label lists the faces on either side),
        // the one the click is nearest to.
        let mut best: Option<(f64, usize)> = None;
        for (i, (label, mid, _)) in p.items.iter().enumerate() {
            if !label.split(" × ").any(|f| f == face) {
                continue;
            }
            let line = &p.lines[i];
            let d = if line.len() >= 2 {
                line.windows(2).map(|w| dist2_segment(world, w[0], w[1])).fold(f64::INFINITY, f64::min)
            } else {
                (0..3).map(|k| (world[k] - mid[k]).powi(2)).sum()
            };
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    };
    match hit {
        Some(i) if p.kind.single() => {
            p.chosen.iter_mut().for_each(|c| *c = false);
            p.chosen[i] = true;
        }
        Some(i) => p.chosen[i] = !p.chosen[i],
        None => {
            let msg = if p.kind.faces() {
                format!("'{face}' cannot be chosen here (a pattern or a mirror needs a flat face)")
            } else {
                format!("No edge of '{face}' to pick")
            };
            state.toast(msg, true);
        }
    }
}

/// Draw the chosen edges and faces over the 3D view.
pub fn draw_overlay(painter: &egui::Painter, cam: &aurafx_render::Camera, rect: egui::Rect, state: &State) {
    let Some(p) = state.cad.pick.as_ref() else { return };
    let col = egui::Color32::from_rgb(255, 140, 0);
    for (i, on) in p.chosen.iter().enumerate() {
        if !*on {
            continue;
        }
        if p.lines[i].len() >= 2 {
            let pts: Vec<egui::Pos2> = crate::pick::project_many(cam, rect, &p.lines[i]).into_iter().flatten().collect();
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], egui::Stroke::new(4.0, col));
            }
        } else if let Some(sp) = crate::pick::project(cam, rect, glam::DVec3::from_array(p.items[i].1)) {
            painter.circle(sp, 8.0, col.gamma_multiply(0.6), egui::Stroke::new(2.0, col));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurafx_cad::{Document, Kernel};
    use serde_json::json;

    #[test]
    fn the_face_under_a_click_is_found_by_its_name_and_the_edge_by_its_nearest_side() {
        let mut d = Document::new("block");
        aurafx_cad::edit::apply_op(&mut d, "add", &json!({"kind": "box", "name": "Block", "size": [20, 20, 20]})).unwrap();
        aurafx_cad::edit::apply_op(&mut d, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 6, "depth": 20, "at": [0, 0, 10]})).unwrap();
        let ev = aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &d, 48, true).unwrap();
        // A click on the top face away from the hole, on the hole's wall, and on the side.
        assert_eq!(face_at(&ev.surface, [6.0, 5.0, 10.0]).as_deref(), Some("Block z+"));
        assert_eq!(face_at(&ev.surface, [3.0, 0.0, 5.0]).as_deref(), Some("Bore side"));
        assert_eq!(face_at(&ev.surface, [10.0, 2.0, 0.0]).as_deref(), Some("Block x+"));
        // The edges bordering the top face: a click near the +x side picks the edge on it.
        let edges = aurafx_cad::kernel::brep::list_edges(&d).unwrap();
        let near = |p: [f64; 3]| -> usize {
            edges
                .iter()
                .enumerate()
                .filter(|(_, e)| e.faces.iter().any(|f| f == "Block z+"))
                .min_by(|(_, a), (_, b)| {
                    let da = a.polyline.windows(2).map(|w| dist2_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min);
                    let db = b.polyline.windows(2).map(|w| dist2_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min);
                    da.total_cmp(&db)
                })
                .map(|(i, _)| i)
                .unwrap()
        };
        let e = &edges[near([9.5, 3.0, 10.0])];
        assert!(e.faces.contains(&"Block x+".to_string()) && e.polyline.iter().all(|p| (p[0] - 10.0).abs() < 1e-6), "{e:?}");
        let _ = <aurafx_cad::kernel::brep::BrepKernel as Kernel>::name;
    }
}
