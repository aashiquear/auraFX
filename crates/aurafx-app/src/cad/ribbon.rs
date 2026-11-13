//! The ribbon: tabs of buttons that add features and modifiers with sensible defaults (placed
//! on the top of the part, sized for a small part); their numbers are then edited in the
//! Feature panel.

use super::{editing, op, RibbonTab};
use crate::app::State;
use crate::menus::MenuExt;
use egui::RichText;
use serde_json::{json, Value};

/// Where a new feature goes by default: the middle of the top of the part built so far.
pub(super) fn top_center(state: &State) -> ([f64; 3], bool) {
    let s = state.session.lock();
    match s.cad_edit.as_ref().and_then(|e| e.summary.as_ref()).filter(|_| s.cad_edit.as_ref().is_some_and(|e| !e.active().features.is_empty())) {
        Some(sum) => {
            let (lo, hi) = sum.bounds;
            ([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, hi[2]], true)
        }
        None => ([0.0; 3], false),
    }
}

/// A feature name not used yet: "Box", "Box 2", ...
pub(super) fn unique(state: &State, base: &str) -> String {
    let s = state.session.lock();
    let names: Vec<String> = s.cad_edit.as_ref().map(|e| e.active().features.iter().map(|f| f.name.clone()).collect()).unwrap_or_default();
    if !names.iter().any(|n| n == base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !names.contains(n)).unwrap()
}

pub(super) fn selected_name(state: &State) -> Option<String> {
    let s = state.session.lock();
    let e = s.cad_edit.as_ref()?;
    let id = e.selected?;
    e.active().feature(id).map(|f| f.name.clone())
}

fn button(ui: &mut egui::Ui, label: &str, tip: &str) -> bool {
    ui.button(crate::icons::with(label)).on_hover_text(tip).clicked()
}

pub fn ribbon_ui(state: &mut State, ui: &mut egui::Ui) {
    if !editing(state) {
        return;
    }
    let (changed, can_apply) = {
        let s = state.session.lock();
        (s.cad_edit.as_ref().is_some_and(|e| e.changed), s.cad_edit.as_ref().is_some_and(|e| e.error.is_none()))
    };
    if crate::touch::mode().is_phone() {
        // A phone: the whole row scrolls sideways.
        let width = ui.available_width().max(HEAD_WIDTH_TOUCH);
        egui::ScrollArea::horizontal().id_salt("cad ribbon head").show(ui, |ui| {
            ui.set_max_width(width);
            ui.horizontal(|ui| ribbon_head(state, ui, changed, can_apply));
        });
    } else if crate::touch::mode().is_touch() {
        // A tablet: Apply and Discard stay in view on the right, the tabs scroll in what is left.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ribbon_apply(state, ui, changed, can_apply);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    egui::ScrollArea::horizontal().id_salt("cad ribbon head").show(ui, |ui| ui.horizontal(|ui| ribbon_tabs(state, ui)));
                });
            });
        });
    } else {
        ui.horizontal(|ui| ribbon_head(state, ui, changed, can_apply));
    }
    ui.separator();
    let (at, has_part) = top_center(state);
    let tab = state.cad.tab;
    let tab_ui = |ui: &mut egui::Ui| match tab {
        RibbonTab::Features => features_tab(state, ui, at, has_part),
        RibbonTab::Sketch => sketch_tab(state, ui, at, has_part),
        RibbonTab::Modify => modify_tab(state, ui),
        RibbonTab::Inspect => inspect_tab(state, ui),
        RibbonTab::Assemble => assemble_tab(state, ui),
        RibbonTab::Files => files_tab(state, ui),
    };
    if crate::touch::mode().is_touch() {
        egui::ScrollArea::horizontal().id_salt("cad ribbon").show(ui, |ui| ui.horizontal(tab_ui));
    } else {
        ui.horizontal_wrapped(tab_ui);
    }
}

/// The width the first row of the ribbon needs in the touch interfaces (its right-hand buttons
/// are laid out from the right, so a narrower screen scrolls a row of this width).
const HEAD_WIDTH_TOUCH: f32 = 1320.0;

/// The first row of the ribbon: the tabs and the command search, and Apply and Discard on the right.
fn ribbon_head(state: &mut State, ui: &mut egui::Ui, changed: bool, can_apply: bool) {
    ribbon_tabs(state, ui);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ribbon_apply(state, ui, changed, can_apply));
}

/// The ribbon's tabs and the command search.
fn ribbon_tabs(state: &mut State, ui: &mut egui::Ui) {
    ui.label(RichText::new(format!("{} CAD", crate::icons::CUBE)).strong().color(crate::theme::ACCENT));
    for (k, t) in RibbonTab::ALL.into_iter().enumerate() {
        if ui.selectable_label(state.cad.tab == t, crate::icons::with(t.label())).on_hover_text(format!("{}  (Alt+{})", t.about(), k + 1)).clicked() {
            state.cad.tab = t;
        }
    }
    if ui
        .button(format!("{} Search", crate::icons::MAGNIFYING_GLASS))
        .on_hover_text("Every CAD command by name: press S in the 3D view (or Ctrl+K), type a few letters, Enter")
        .clicked()
    {
        state.cad.search = Some(super::guide::Search::default());
    }
}

/// Discard, Apply and Apply, keep editing (laid out from the right).
fn ribbon_apply(state: &mut State, ui: &mut egui::Ui, changed: bool, can_apply: bool) {
    if ui.button(crate::icons::with("Discard")).on_hover_text("Leave the CAD environment; the part is as it was (Esc)").clicked() {
        super::discard(state);
    }
    let empty = state.session.lock().cad_edit.as_ref().is_some_and(|e| e.document.features.is_empty() && e.document.components.is_empty());
    let why_not = if empty {
        "The part is empty: add a sketch or a shape first (the Next strip at the top left offers them)."
    } else {
        "The part does not build (the reason is in red at the top of the right panel): fix or undo the last change first."
    };
    if ui
        .add_enabled(can_apply, egui::Button::new(crate::icons::with("Apply, keep editing")))
        .on_hover_text("Hand the part to the pipeline and stay here")
        .on_disabled_hover_text(why_not)
        .clicked()
    {
        super::apply(state, true);
    }
    let apply = egui::Button::new(RichText::new(format!("{} Apply", crate::icons::CHECK_CIRCLE)).strong());
    if ui
        .add_enabled(can_apply, apply)
        .on_hover_text("Hand the part to the pipeline and leave; the nodes below it re-run (Ctrl+Enter)")
        .on_disabled_hover_text(why_not)
        .clicked()
    {
        super::apply(state, false);
    }
    if changed {
        ui.label(RichText::new("unapplied changes").weak().small());
    }
}

fn features_tab(state: &mut State, ui: &mut egui::Ui, at: [f64; 3], has_part: bool) {
    let z = |dz: f64| [at[0], at[1], at[2] + dz];
    let (c, a) = (at, if has_part { 1.0 } else { 0.0 });
    let _ = c;
    if button(ui, "Box", "A 20 mm box on top of the part") {
        let name = unique(state, "Box");
        op(state, "add", json!({"kind": "box", "name": name, "size": [20, 20, 20], "at": z(10.0 * a)}));
    }
    if button(ui, "Cylinder", "A cylinder of radius 8 and height 20 standing on the part") {
        let name = unique(state, "Cylinder");
        op(state, "add", json!({"kind": "cylinder", "name": name, "radius": 8, "height": 20, "at": z(0.0)}));
    }
    if button(ui, "Sphere", "A sphere of radius 10") {
        let name = unique(state, "Sphere");
        op(state, "add", json!({"kind": "sphere", "name": name, "radius": 10, "at": z(10.0 * a)}));
    }
    if button(ui, "Cone", "A cone of base radius 10 and height 20") {
        let name = unique(state, "Cone");
        op(state, "add", json!({"kind": "cone", "name": name, "r_bottom": 10, "r_top": 2, "height": 20, "at": z(0.0)}));
    }
    if button(ui, "Torus", "A torus of major radius 12 and minor radius 3") {
        let name = unique(state, "Torus");
        op(state, "add", json!({"kind": "torus", "name": name, "major": 12, "minor": 3, "at": z(3.0 * a)}));
    }
    ui.separator();
    for (label, hole, tip) in [
        ("Hole", "simple", "A simple hole cut down from the top of the part"),
        ("Counterbore", "counterbore", "A hole with a flat-bottomed recess for a bolt head"),
        ("Countersink", "countersink", "A hole with a conical recess for a flat-head screw"),
        ("Tapped hole", "tapped", "A threaded hole (the thread is modelled; cosmetic threads are a plain tap-drill hole)"),
        ("Drilled hole", "drilled", "A blind hole with a drill point"),
    ] {
        if button(ui, label, tip) {
            let name = unique(state, "Hole");
            let mut a = json!({"kind": "hole", "name": name, "hole": hole, "diameter": 6, "depth": 10, "at": z(0.0), "pitch": 1.0, "cb_diameter": 11, "cb_depth": 4, "cs_diameter": 12, "cs_angle": 90});
            if hole == "tapped" {
                a["diameter"] = json!(8);
                a["pitch"] = json!(1.25);
            }
            op(state, "add", a);
        }
    }
    if button(ui, "Thread", "A helical thread (an external thread to add, or the void of an internal one to cut)") {
        let name = unique(state, "Thread");
        op(state, "add", json!({"kind": "thread", "name": name, "major_diameter": 8, "pitch": 1.25, "length": 12, "at": z(0.0)}));
    }
    ui.separator();
    // From sketches kept with the part (Sketch tab, Keep as a sketch).
    let (sweep, loft) = swept_choices(state);
    for (label, opname, choice, tip) in [
        (
            "Sweep",
            "sweep",
            sweep,
            "Sweep the closed outline of one kept sketch along the open path of another; choose the sketches and add or cut in the feature panel",
        ),
        ("Loft", "loft", loft, "Loft through the closed outlines of kept sketches on parallel planes; choose the sections and add or cut in the feature panel"),
    ] {
        let why = choice.as_ref().err().cloned().unwrap_or_default();
        if ui.add_enabled(choice.is_ok(), egui::Button::new(crate::icons::with(label))).on_hover_text(tip).on_disabled_hover_text(why).clicked() {
            if let Ok(mut a) = choice {
                a["name"] = json!(unique(state, label));
                op(state, opname, a);
            }
        }
    }
}

/// What the Sweep and Loft buttons would make from the sketches kept with the part: the
/// operation's arguments, or why the button is off (and what to draw first).
pub(super) fn swept_choices(state: &State) -> (Result<Value, String>, Result<Value, String>) {
    let s = state.session.lock();
    let Some(doc) = s.cad_edit.as_ref().map(|e| e.active().clone()) else { return (Err(String::new()), Err(String::new())) };
    drop(s);
    let used = |name: &str| {
        doc.features.iter().any(|f| match &f.shape {
            aurafx_cad::Shape::SketchSweep { profile, path, .. } => profile == name || path == name,
            aurafx_cad::Shape::SketchLoft { sections } => sections.iter().any(|x| x == name),
            _ => false,
        })
    };
    // Sketches no feature uses yet come first: the ones just drawn.
    let mut all: Vec<&aurafx_cad::SavedSketch> = doc.sketches.iter().collect();
    all.sort_by_key(|k| used(&k.name));
    let closed: Vec<&str> = all.iter().filter(|k| !k.sketch.profiles().is_empty()).map(|k| k.name.as_str()).collect();
    let open: Vec<&str> = all.iter().filter(|k| !k.sketch.open_paths().is_empty()).map(|k| k.name.as_str()).collect();
    let how = "Draw each on the Sketch tab and finish it with Keep as a sketch.";
    let sweep = if closed.is_empty() && open.is_empty() {
        Err(format!("A sweep needs two sketches kept with the part: a profile (a closed outline) and a path (an open line). {how}"))
    } else if open.is_empty() {
        Err(format!("A sweep needs a path too: a sketch with an open line (lines and arcs end to end), starting where the profile is. {how}"))
    } else if closed.is_empty() {
        Err(format!("A sweep needs a profile too: a sketch with a closed outline, on a plane the path leaves at right angles. {how}"))
    } else {
        let mut why = String::new();
        let mut found = None;
        'pairs: for p in &closed {
            for q in &open {
                match aurafx_cad::swept::sweep_geometry(&doc, "Sweep", p, q) {
                    Ok(_) => {
                        found = Some(json!({"profile": p, "path": q}));
                        break 'pairs;
                    }
                    Err(e) if why.is_empty() => why = e.to_string(),
                    Err(_) => {}
                }
            }
        }
        found.ok_or(why)
    };
    let loft = if closed.len() < 2 {
        Err(format!(
            "A loft needs two or more sketches kept with the part, each a closed outline, on parallel planes at different heights (set a sketch's Offset). The part has {}. {how}",
            closed.len()
        ))
    } else {
        // The first closed sketch and every other one parallel to it at a height of its own.
        let mut picked: Vec<String> = vec![closed[0].to_string()];
        for name in &closed[1..] {
            let mut tryout = picked.clone();
            tryout.push(name.to_string());
            if aurafx_cad::swept::loft_geometry(&doc, "Loft", &tryout).is_ok() {
                picked = tryout;
            }
        }
        if picked.len() >= 2 {
            Ok(json!({"sections": picked}))
        } else {
            Err("A loft needs its sketches on parallel planes at different heights: these are on one plane, or on planes that cross (set a sketch's Offset before drawing it).".to_string())
        }
    };
    (sweep, loft)
}

fn sketch_tab(state: &mut State, ui: &mut egui::Ui, at: [f64; 3], has_part: bool) {
    if super::sketching(state) {
        super::sketch::tools_ui(state, ui);
        return;
    }
    ui.label(RichText::new("New sketch on:").weak().small());
    for (plane, label, tip) in [
        (aurafx_cad::SketchPlane::XY, "Top (XY)", "Draw on the horizontal plane"),
        (aurafx_cad::SketchPlane::XZ, "Front (XZ)", "Draw on the front plane (x across, z up)"),
        (aurafx_cad::SketchPlane::YZ, "Right (YZ)", "Draw on the side plane (y across, z up)"),
    ] {
        if button(ui, label, tip) {
            super::sketch_new(state, plane, 0.0, false);
        }
    }
    if has_part && button(ui, "Top of the part", "Draw on the plane of the top of the part, to add a boss or cut a pocket") {
        super::sketch_new(state, aurafx_cad::SketchPlane::XY, at[2], false);
    }
    if has_part && button(ui, "A face…", "Choose any flat face of the part (click it in the 3D view) and draw on it") {
        super::pick::open(state, super::PickKind::SketchFace);
    }
    let planes = state.session.lock().cad_edit.as_ref().map(|e| e.active().planes.clone()).unwrap_or_default();
    for p in planes {
        if button(ui, &p.name, &format!("Draw on the reference plane {} ({})", p.name, p.how)) {
            super::sketch::begin_on_frame(state, p.frame, Some(p.name.clone()), false);
        }
    }
    if button(ui, "Plane…", "Make a reference plane to sketch on: moved or turned from a plane or a face, or through three points") {
        state.cad.plane_form = Some(super::planes::PlaneForm::default());
    }
    ui.separator();
    ui.label(RichText::new("or add a ready shape (edit its dimensions on the right):").weak().small());
    let off = if has_part { at[2] } else { 0.0 };
    let sk = |item: Value, plane: &str| json!({"plane": plane, "offset": if plane == "XY" { off } else { 0.0 }, "items": [item]});
    if button(ui, "Rectangle", "A 30 × 20 rectangle extruded 10 mm") {
        let name = unique(state, "Extrude");
        op(state, "sketch_extrude", json!({"name": name, "depth": 10, "sketch": sk(json!({"rectangle": {"center": [at[0], at[1]], "w": 30, "h": 20}}), "XY")}));
    }
    if button(ui, "Circle", "A circle of radius 10 extruded 10 mm") {
        let name = unique(state, "Extrude");
        op(state, "sketch_extrude", json!({"name": name, "depth": 10, "sketch": sk(json!({"circle": {"center": [at[0], at[1]], "r": 10}}), "XY")}));
    }
    if button(ui, "Hexagon", "A regular hexagon of radius 12 extruded 10 mm") {
        let name = unique(state, "Extrude");
        op(
            state,
            "sketch_extrude",
            json!({"name": name, "depth": 10, "sketch": sk(json!({"regular": {"center": [at[0], at[1]], "r": 12, "sides": 6}}), "XY")}),
        );
    }
    if button(ui, "Rectangle cut", "A 10 × 10 pocket cut through the top 5 mm") {
        let name = unique(state, "Pocket");
        op(
            state,
            "sketch_extrude",
            json!({"name": name, "op": "cut", "depth": 5, "reverse": true, "sketch": sk(json!({"rectangle": {"center": [at[0], at[1]], "w": 10, "h": 10}}), "XY")}),
        );
    }
    ui.separator();
    if button(ui, "Revolve profile", "A profile of straight lines turned about the vertical axis (a knob)") {
        let name = unique(state, "Revolve");
        let pl = json!({"polygon": [[0, 0], [12, 0], [12, 4], [7, 16], [0, 18]]});
        op(state, "sketch_revolve", json!({"name": name, "angle": 360, "at": [at[0], at[1], off], "sketch": sk(pl, "XZ")}));
    }
}

fn modify_tab(state: &mut State, ui: &mut egui::Ui) {
    let target = selected_name(state);
    ui.label(
        RichText::new(match &target {
            Some(n) => format!("on {n}:"),
            None => "on the whole part (select a feature to modify only it):".to_string(),
        })
        .weak()
        .small(),
    );
    let t = target.map(Value::String).unwrap_or(Value::Null);
    let add = |state: &mut State, m: Value| {
        op(state, "modifier", json!({"feature": t, "modifier": m}));
    };
    if button(ui, "Fillet edges…", "Choose edges from a list of the part's edges and round them (exact kernel)") {
        super::pick::open(state, super::PickKind::Fillet);
    }
    if button(ui, "Chamfer edges…", "Choose edges and chamfer them (exact kernel)") {
        super::pick::open(state, super::PickKind::Chamfer);
    }
    if button(ui, "Shell, open faces…", "Hollow the part, choosing the faces to open (exact kernel)") {
        super::pick::open(state, super::PickKind::Shell);
    }
    if button(ui, "Draft faces…", "Taper side faces by an angle, choosing them from a list (exact kernel)") {
        super::pick::open(state, super::PickKind::Draft);
    }
    if button(ui, "Pattern on a face…", "Lay holes or bosses out in a row, a grid or a circle on a flat face") {
        super::pick::open(state, super::PickKind::PatternFace);
    }
    if button(ui, "Mirror about a face…", "Add the reflection of the part about a flat face") {
        super::pick::open(state, super::PickKind::MirrorFace);
    }
    ui.separator();
    if button(ui, "Fillet", "Round all edges by 1 mm") {
        add(state, json!({"kind": "Fillet", "radius": 1}));
    }
    if button(ui, "Shell", "Hollow the solid with a 2 mm wall") {
        add(state, json!({"kind": "Shell", "thickness": 2}));
    }
    if button(ui, "Open shell", "Hollow it with a 2 mm wall and open the top (an enclosure)") {
        add(state, json!({"kind": "ShellOpen", "thickness": 2, "open_top": true}));
    }
    ui.separator();
    if button(ui, "Linear pattern", "Three copies 20 mm apart along x") {
        add(state, json!({"kind": "LinearPattern", "count": 3, "step": [20, 0, 0]}));
    }
    if button(ui, "Grid pattern", "A 3 × 3 grid of copies 15 mm apart") {
        add(state, json!({"kind": "GridPattern", "counts": [3, 3], "step": [15, 15]}));
    }
    if button(ui, "Circular pattern", "Six copies around the vertical axis through the origin") {
        add(state, json!({"kind": "CircularPatternAngle", "count": 6, "total_deg": 360, "axis": "Z"}));
    }
    ui.separator();
    if button(ui, "Mirror YZ", "Keep the part and add its reflection across the YZ plane") {
        add(state, json!({"kind": "Mirror", "plane": "YZ"}));
    }
    if button(ui, "Mirror XZ", "Keep the part and add its reflection across the XZ plane") {
        add(state, json!({"kind": "Mirror", "plane": "XZ"}));
    }
    if button(ui, "Split", "Keep one side of the YZ plane") {
        add(state, json!({"kind": "Split", "plane": "YZ", "offset": 0, "keep_positive": true}));
    }
    if button(ui, "Scale", "Scale about the origin") {
        add(state, json!({"kind": "Scale", "factor": 1.0}));
    }
}

fn inspect_tab(state: &mut State, ui: &mut egui::Ui) {
    let summary = state.session.lock().cad_edit.as_ref().and_then(|e| e.summary.clone());
    let Some(s) = summary else {
        ui.label(RichText::new("The part has not been built yet.").weak());
        return;
    };
    let size = [s.bounds.1[0] - s.bounds.0[0], s.bounds.1[1] - s.bounds.0[1], s.bounds.1[2] - s.bounds.0[2]];
    ui.label(format!("volume {} mm³", super::num_text(s.volume)));
    ui.label(format!("surface {} mm²", super::num_text(s.area)));
    ui.label(format!("size {} × {} × {} mm", super::num_text(size[0]), super::num_text(size[1]), super::num_text(size[2])));
    ui.label(format!("center of volume ({}, {}, {})", super::num_text(s.centroid[0]), super::num_text(s.centroid[1]), super::num_text(s.centroid[2])));
    ui.separator();
    ui.label("density");
    ui.add(egui::DragValue::new(&mut state.cad.density).speed(0.05).range(0.01..=30.0).suffix(" g/cm³"));
    ui.label(format!("mass {} g", super::num_text(s.volume / 1000.0 * state.cad.density)));
    ui.separator();
    ui.label("section at z");
    ui.add(egui::DragValue::new(&mut state.cad.section_z).speed(0.1).suffix(" mm"));
    if ui.button(crate::icons::with("Measure")).on_hover_text("The area of the part's cross-section at that height").clicked() {
        let doc = state.session.lock().cad_edit.as_ref().map(|e| e.document_to_build());
        state.cad.section_area =
            doc.and_then(|d| d.resolved().ok()).map(|d| d.section_area_at_z(state.cad.section_z, (s.bounds.1[0] - s.bounds.0[0]).max(1.0) / 200.0));
    }
    if let Some(a) = state.cad.section_area {
        ui.label(format!("= {} mm²", super::num_text(a)));
    }
    ui.separator();
    // The distance and angle between two faces.
    ui.label("between");
    let names: Vec<String> = s.faces.iter().map(|f| f.name.clone()).collect();
    let (a, b, _) = &mut state.cad.measure;
    for (k, slot) in [a, b].into_iter().enumerate() {
        egui::ComboBox::from_id_salt(("cad measure", k))
            .width(110.0)
            .selected_text(if slot.is_empty() { "a face…".to_string() } else { slot.clone() })
            .show_ui(ui, |ui| {
                for n in &names {
                    ui.selectable_value(slot, n.clone(), n);
                }
            });
    }
    let (a, b) = (state.cad.measure.0.clone(), state.cad.measure.1.clone());
    if ui
        .add_enabled(!a.is_empty() && !b.is_empty() && a != b, egui::Button::new(crate::icons::with("Distance")))
        .on_hover_text("The distance and the angle between the two faces")
        .on_disabled_hover_text("Choose two different faces in the lists (a face's name shows when you right-click it in the view)")
        .clicked()
    {
        let doc = state.session.lock().cad_edit.as_ref().map(|e| e.document_to_build());
        state.cad.measure.2 = Some(match doc.map(|d| aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &d, 64, true)) {
            Some(Ok(ev)) => match aurafx_cad::measure_faces(&ev, &a, &b) {
                Some(m) if m.parallel => format!("parallel, {} mm apart", super::num_text(m.distance)),
                Some(m) => format!("{}° apart, centers {} mm apart", super::num_text(m.angle_deg), super::num_text(m.center_distance)),
                None => "those faces are not on the part as built".to_string(),
            },
            Some(Err(e)) => format!("{e:#}"),
            None => String::new(),
        });
    }
    if let Some(t) = &state.cad.measure.2 {
        ui.label(format!("= {t}"));
    }
}

/// File types a part can be placed from.
const PART_FILES: &[&str] = &["afxcad", "step", "stp", "igs", "iges", "3mf", "sc"];

/// The parts a menu offers: the templates, a blank part and a CAD file. `make` turns the choice
/// into the arguments of the operation (a template name, a blank part, or a file's path).
fn part_menu(state: &mut State, ui: &mut egui::Ui, run: &str, extra: Value) {
    let mut chosen: Option<Value> = None;
    for (title, _) in aurafx_cad::templates() {
        if ui.button(title).clicked() {
            ui.close();
            chosen = Some(json!({"template": title.split([' ', '(']).next().unwrap_or(title)}));
        }
    }
    ui.separator();
    if ui.button("Blank part").on_hover_text("An empty part to model in: double-click it under Components to give it features").clicked() {
        ui.close();
        chosen = Some(json!({}));
    }
    if ui.button("From a file…").on_hover_text("A CAD file: .afxcad, STEP, IGES, 3MF or .sc").clicked() {
        ui.close();
        if let Some(p) = crate::dialogs::FileDialog::new().add_filter("CAD files", PART_FILES).pick_file() {
            let name = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Part".into());
            chosen = Some(json!({"path": p.to_string_lossy(), "name": name}));
        }
    }
    if let Some(mut a) = chosen {
        if let (Some(o), Some(e)) = (a.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        op(state, run, a);
    }
}

/// Find the pairs of parts that overlap and keep the result for the Assemble tab.
pub fn check_fit(state: &mut State) {
    let (doc, size) = {
        let s = state.session.lock();
        (
            s.cad_edit.as_ref().map(|e| e.document_to_build()),
            s.cad_edit.as_ref().and_then(|e| e.summary.as_ref()).map(|m| (0..3).map(|k| m.bounds.1[k] - m.bounds.0[k]).fold(1.0, f64::max)),
        )
    };
    if let Some(doc) = doc {
        match aurafx_cad::kernel::assemble::component_interference(aurafx_cad::KernelChoice::Auto, &doc, (size.unwrap_or(100.0) / 120.0).max(0.2)) {
            Ok(v) => {
                state.status = if v.is_empty() { "Check fit: no two parts overlap".into() } else { format!("Check fit: {} pair(s) overlap", v.len()) };
                state.cad.fit = Some(v);
            }
            Err(e) => state.toast(format!("{e}"), true),
        }
    }
}

fn assemble_tab(state: &mut State, ui: &mut egui::Ui) {
    if let Some(c) = super::focused(state) {
        ui.label(RichText::new(format!("Editing the component {c} on its own. The assembly's tools are here again when you go back.")).weak());
        if button(ui, "Back to the assembly", "Show the whole assembly again (also the breadcrumb at the top of the model tree)") {
            super::focus(state, None);
        }
        return;
    }
    ui.scroll_menu(crate::icons::with("Insert part"), |ui| part_menu(state, ui, "component_add", json!({"at": [0, 0, 0]})))
        .response
        .on_hover_text("Place another part beside this one: a template, a blank part or a CAD file. It keeps its own features; move it in the model tree.");
    for (label, how, tip) in [
        ("Add part", "add", "Join another part to this one (a boolean union of the two solids)"),
        ("Cut with part", "subtract", "Remove another part's volume from this one"),
        ("Keep common part", "intersect", "Keep only where this part and another overlap"),
    ] {
        ui.scroll_menu(crate::icons::with(label), |ui| part_menu(state, ui, "combine", json!({"op": how}))).response.on_hover_text(tip);
    }
    ui.separator();
    let has = state.session.lock().cad_edit.as_ref().is_some_and(|e| e.document.components.len() + usize::from(!e.document.features.is_empty()) > 1);
    if ui
        .add_enabled(has, egui::Button::new(crate::icons::with("Check fit")))
        .on_hover_text("Find the parts that overlap, with the overlap volume (the parts of the assembly, two at a time)")
        .on_disabled_hover_text("Needs at least two parts: insert one first")
        .clicked()
    {
        check_fit(state);
    }
    if let Some(fit) = &state.cad.fit {
        if fit.is_empty() {
            ui.label(RichText::new("no overlap").color(crate::theme::OK).small());
        }
        for (a, b, v) in fit {
            ui.label(RichText::new(format!("{a} and {b} overlap by {} mm³", super::num_text(*v))).color(crate::theme::ERROR).small());
        }
    }
    ui.separator();
    if ui
        .add_enabled(has, egui::Button::new(crate::icons::with("Add mate…")))
        .on_hover_text("Tie two parts together by a face of each: click the two faces, then choose coincident, concentric, distance or angle")
        .on_disabled_hover_text("A mate is between two parts: insert a second one first")
        .clicked()
    {
        super::pick::open(state, super::PickKind::Mate);
    }
    let mates = {
        let s = state.session.lock();
        s.cad_edit.as_ref().map(|e| {
            (
                e.document.mates.len(),
                e.summary.as_ref().and_then(|m| m.mate_dof),
                e.summary.as_ref().map(|m| m.mates.iter().filter(|x| x.1.is_some()).count()).unwrap_or(0),
            )
        })
    };
    if let Some((n, dof, bad)) = mates.filter(|m| m.0 > 0) {
        let text = match (bad, dof) {
            (0, Some(0)) => format!("{n} mate(s): the mated parts are fully held"),
            (0, Some(d)) => format!("{n} mate(s): {d} way(s) left to move"),
            (0, None) => format!("{n} mate(s)"),
            (b, _) => format!("{n} mate(s), {b} not met (see Mates in the model tree)"),
        };
        ui.label(RichText::new(text).color(if bad > 0 { crate::theme::ERROR } else { ui.visuals().weak_text_color() }).small());
    }
    ui.label(RichText::new("Parts and mates are listed in the model tree: double-click a part to edit its own features.").weak().small());
}

fn files_tab(state: &mut State, ui: &mut egui::Ui) {
    ui.scroll_menu(crate::icons::with("New from template"), |ui| {
        for (title, _) in aurafx_cad::templates() {
            if ui.button(title).clicked() {
                ui.close();
                let word = title.split([' ', '(']).next().unwrap_or(title).to_string();
                op(state, "template", json!({"name": word}));
            }
        }
    })
    .response
    .on_hover_text("Replace the part by a starter part (Ctrl+Z in the environment takes it back)");
    if button(ui, "Open .afxcad…", "Replace the part by a CAD document file") {
        if let Some(p) = crate::dialogs::FileDialog::new().add_filter("auraFX CAD document", &["afxcad"]).pick_file() {
            match aurafx_cad::Document::load(&p) {
                Ok(d) => {
                    let _ = state.session.lock().cad_env_set_document(d);
                }
                Err(e) => state.toast(format!("{e}"), true),
            }
        }
    }
    if button(ui, "Save as .afxcad…", "Write the part being edited to a file") {
        save_as(state);
    }
    if button(ui, "Export…", "Write the part as STEP or IGES (for another CAD program), 3MF or STL (for a printer), OBJ or .sc") {
        export_as(state);
    }
}

/// Write the working copy in an exchange format chosen by the file's extension.
pub fn export_as(state: &mut State) {
    let (doc, kernel) = {
        let s = state.session.lock();
        match s.cad_edit.as_ref() {
            Some(e) => (e.active().clone(), s.cad_kernel(e.node)),
            None => return,
        }
    };
    let Some(p) = crate::dialogs::FileDialog::new()
        .add_filter("STEP", &["step", "stp"])
        .add_filter("IGES", &["igs", "iges"])
        .add_filter("3MF", &["3mf"])
        .add_filter("STL", &["stl"])
        .add_filter("Wavefront OBJ", &["obj"])
        .add_filter("SolidCraft design", &["sc"])
        .set_file_name("part.step")
        .save_file()
    else {
        return;
    };
    let p = if p.extension().is_none() { p.with_extension("step") } else { p };
    match aurafx_cad::io::export_file(&doc, &p, kernel) {
        Ok(r) => {
            let how = if r.exact {
                "exact geometry".to_string()
            } else {
                format!("as facets{}", if r.note.is_empty() { String::new() } else { format!(": {}", r.note) })
            };
            state.toast(format!("Exported {} ({how})", p.display()), false);
        }
        Err(e) => state.toast(format!("{e}"), true),
    }
}

/// Save the working copy as an `.afxcad` file chosen by the user.
pub fn save_as(state: &mut State) {
    let doc = state.session.lock().cad_edit.as_ref().map(|e| e.active().clone());
    let Some(doc) = doc else { return };
    if let Some(p) = crate::dialogs::FileDialog::new().add_filter("auraFX CAD document", &["afxcad"]).set_file_name("part.afxcad").save_file() {
        let p = if p.extension().is_none() { p.with_extension("afxcad") } else { p };
        match doc.save(&p) {
            Ok(()) => state.toast(format!("Saved {}", p.display()), false),
            Err(e) => state.toast(format!("{e}"), true),
        }
    }
}
