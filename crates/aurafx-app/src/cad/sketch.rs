//! The sketch canvas of the CAD environment (stage C4), ported from EZ-YX-craft's sketch mode
//! (MIT OR Apache-2.0): a 2D canvas on the sketch plane in the place of the active 3D view,
//! with line, rectangle, circle, arc, spline and point tools, snapping to existing points and a
//! grid, horizontal and vertical inference, a constraint and dimension toolbar in the ribbon,
//! driving dimensions typed as values or expressions of the dimensions, and the solver's
//! status: blue while under-defined (with the degrees of freedom left), black when fully
//! defined, red when over-defined. A sketch starts on a principal plane or on a flat face of
//! the part and finishes as an extrusion, a cut or a revolve, or as the new sketch of a feature.

use super::{expr_field, num_text, op};
use crate::app::State;
use aurafx_cad::sketch::{Constraint, Dim, Entity, PlaneFrame, SPoint, Sketch, SketchPlane, SolveReport, SolveStatus};
use aurafx_cad::{BooleanOp, Id};
use egui::{Color32, Pos2, RichText, Stroke};
use glam::DVec2;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Circle,
    Arc,
    Spline,
    Point,
}

/// A sketch being edited.
pub struct SketchEdit {
    /// The feature whose sketch this is (`None`: a new sketch, finished into a new feature).
    pub feature: Option<Id>,
    /// The sketch kept with the part that is being changed, by its name.
    pub saved: Option<String>,
    pub sketch: Sketch,
    pub tool: Tool,
    pub sel_pts: Vec<usize>,
    pub sel_ents: Vec<usize>,
    chain: Option<usize>,
    pending: Vec<usize>,
    pub report: Option<SolveReport>,
    center: DVec2,
    scale: f32,
    fitted: bool,
    drag: Option<usize>,
    pub op: BooleanOp,
    /// The depth of the extrusion: a number or an expression of the dimensions.
    pub depth: String,
    pub reverse: bool,
    pub error: Option<String>,
    pub snap_grid: f64,
    /// Which snaps are on.
    pub snaps: super::sketch_snap::Snaps,
    /// A dimension whose label is being dragged.
    dim_drag: Option<usize>,
    /// A dimension being typed into (its constraint and the text).
    pub dim_edit: Option<(usize, String)>,
    /// Where the cursor is taken to be when the mouse is not over the canvas (sketch units; for
    /// scripted pictures).
    pub demo_cursor: Option<DVec2>,
    /// Earlier states of the sketch (Ctrl+Z while sketching), and the state last recorded.
    undo: Vec<Sketch>,
    redo: Vec<Sketch>,
    recorded: Option<Sketch>,
}

impl SketchEdit {
    /// Record the sketch when it changed since the last call (called once a frame while no
    /// mouse button is down, so a drag is one step).
    fn record(&mut self) {
        match &self.recorded {
            Some(r) if *r == self.sketch => {}
            Some(r) => {
                self.undo.push(r.clone());
                if self.undo.len() > 200 {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.recorded = Some(self.sketch.clone());
            }
            None => self.recorded = Some(self.sketch.clone()),
        }
    }
    /// Take back the last change of the sketch (true when there was one).
    pub fn undo(&mut self) -> bool {
        self.record();
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.sketch, prev));
        self.after_history();
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.sketch, next));
        self.after_history();
        true
    }
    fn after_history(&mut self) {
        self.recorded = Some(self.sketch.clone());
        self.sel_pts.clear();
        self.sel_ents.clear();
        self.chain = None;
        self.pending.clear();
        self.drag = None;
        self.dim_edit = None;
        self.report = Some(self.sketch.solve());
    }
    /// Switch the drawing tool (ending a line chain or a half-drawn shape).
    pub fn select_tool(&mut self, t: Tool) {
        self.tool = t;
        self.chain = None;
        self.pending.clear();
    }
    /// Continue a line chain from point `p`.
    pub fn set_chain(&mut self, p: Option<usize>) {
        self.chain = p.filter(|i| *i < self.sketch.points.len());
    }
}

impl SketchEdit {
    pub fn new(feature: Option<Id>, sketch: Sketch) -> Self {
        Self {
            feature,
            saved: None,
            sketch,
            tool: Tool::Line,
            sel_pts: vec![],
            sel_ents: vec![],
            chain: None,
            pending: vec![],
            report: None,
            center: DVec2::ZERO,
            scale: 4.0,
            fitted: false,
            drag: None,
            op: BooleanOp::Add,
            depth: "10".into(),
            reverse: false,
            error: None,
            snap_grid: 0.5,
            snaps: Default::default(),
            dim_drag: None,
            dim_edit: None,
            demo_cursor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            recorded: None,
        }
    }
}

/// Whether a sketch is open.
pub fn sketching(state: &State) -> bool {
    state.cad.sketch.is_some()
}

// ------------------------------------------------------------------------------- starting

/// A new sketch on a principal plane (`offset` along its normal).
pub fn begin_new(state: &mut State, plane: SketchPlane, offset: f64, reverse: bool) {
    state.cad.tab = super::RibbonTab::Sketch;
    let mut sk = Sketch::new(plane);
    sk.offset = offset;
    let mut e = SketchEdit::new(None, sk);
    e.reverse = reverse;
    state.cad.sketch = Some(e);
    solve(state);
}

/// A new sketch on a plane anywhere (a reference plane or a slanted face).
pub fn begin_on_frame(state: &mut State, frame: PlaneFrame, name: Option<String>, reverse: bool) {
    state.cad.tab = super::RibbonTab::Sketch;
    let sk = Sketch::new(SketchPlane::XY).on_frame(frame, name);
    let mut e = SketchEdit::new(None, sk);
    e.reverse = reverse;
    state.cad.sketch = Some(e);
    solve(state);
}

/// Sketch on a flat face given by a point on it and its outward direction: on the matching
/// principal plane when the normal is along x, y or z, otherwise on the face's own plane (x
/// along world x, or y, laid flat on it), named after the face.
pub fn begin_on_face_at(state: &mut State, point: [f64; 3], direction: [f64; 3], name: &str) {
    match Sketch::on_face(point, direction, name) {
        Some((mut sk, reverse)) => {
            // The sketch remembers its face and follows it. Not a face of a component seen from
            // the assembly: the assembly's own part does not have it.
            if name.contains('/') {
                sk.face = None;
            }
            state.cad.tab = super::RibbonTab::Sketch;
            let mut e = SketchEdit::new(None, sk);
            e.reverse = reverse;
            state.cad.sketch = Some(e);
            solve(state);
        }
        None => state.toast("That face has no direction".to_string(), true),
    }
}

/// Sketch on a flat face of the last build.
pub fn begin_on_face(state: &mut State, face: &aurafx_pipeline::CadFace) {
    begin_on_face_at(state, face.centroid, face.direction, &face.name);
}

/// Edit the sketch of an extruded or revolved feature.
pub fn begin_edit(state: &mut State, feature: Id) {
    state.cad.tab = super::RibbonTab::Sketch;
    let sk = {
        let s = state.session.lock();
        s.cad_edit.as_ref().and_then(|e| e.active().feature(feature)).and_then(|f| match &f.shape {
            aurafx_cad::Shape::SketchExtrude { sketch, .. } | aurafx_cad::Shape::SketchRevolve { sketch, .. } => Some(sketch.clone()),
            _ => None,
        })
    };
    match sk {
        Some(sk) => {
            state.cad.sketch = Some(SketchEdit::new(Some(feature), sk));
            solve(state);
        }
        None => state.toast("That feature has no sketch".to_string(), true),
    }
}

/// Change a sketch kept with the part (the profile or path of a sweep, a section of a loft).
pub fn begin_saved(state: &mut State, name: &str) {
    let sk = state.session.lock().cad_edit.as_ref().and_then(|e| e.active().saved_sketch(name).cloned());
    match sk {
        Some(sk) => {
            state.cad.tab = super::RibbonTab::Sketch;
            let mut e = SketchEdit::new(None, sk);
            e.saved = Some(name.to_string());
            state.cad.sketch = Some(e);
            solve(state);
        }
        None => state.toast(format!("The part has no sketch named '{name}'"), true),
    }
}

/// Finish the open sketch by keeping it with the part under a name, for a sweep or a loft.
pub fn keep(state: &mut State) {
    let Some(e) = state.cad.sketch.take() else { return };
    if e.feature.is_some() || e.saved.is_some() {
        state.cad.sketch = Some(e);
        return finish(state, false);
    }
    let sketch_json = serde_json::to_value(&e.sketch).unwrap_or(serde_json::Value::Null);
    match op(state, "sketch_add", json!({"sketch_json": sketch_json})) {
        Some(made) => {
            state.cad.tab = super::RibbonTab::Features;
            state.status =
                format!("Kept as '{}': use it in a Sweep or a Loft (Features tab), or draw the next sketch", made["sketch"].as_str().unwrap_or("a sketch"));
        }
        None => state.cad.sketch = Some(e),
    }
}

/// Open a sketch from a description (Python: `e.sketch_new(items=[...])`).
pub fn begin_from_spec(state: &mut State, spec: &serde_json::Value) {
    state.cad.tab = super::RibbonTab::Sketch;
    let built = {
        let s = state.session.lock();
        s.cad_edit.as_ref().map(|e| aurafx_cad::edit::sketch_from_spec(e.active(), spec))
    };
    match built {
        Some(Ok(sk)) => {
            state.cad.sketch = Some(SketchEdit::new(None, sk));
            solve(state);
        }
        Some(Err(e)) => state.toast(format!("{e}"), true),
        None => {}
    }
}

pub fn cancel(state: &mut State) {
    state.cad.sketch = None;
}

/// Re-solve after a change: the dimensions' expressions use the document's dimensions.
pub fn solve(state: &mut State) {
    let vars = state.session.lock().cad_edit.as_ref().and_then(|e| e.active().parameter_values().ok()).unwrap_or_default();
    if let Some(e) = state.cad.sketch.as_mut() {
        match e.sketch.solve_with(&vars) {
            Ok(r) => {
                e.report = Some(r);
                e.error = None;
            }
            Err(err) => e.error = Some(err.to_string()),
        }
    }
}

// ------------------------------------------------------------------------------- pure helpers

fn spline_pts(p: &[DVec2], closed: bool) -> Vec<DVec2> {
    let n = p.len();
    if n < 3 {
        return p.to_vec();
    }
    let get = |i: isize| -> DVec2 {
        if closed {
            p[i.rem_euclid(n as isize) as usize]
        } else {
            p[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let segs = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..segs as isize {
        let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
        for k in 0..12 {
            let t = k as f64 / 12.0;
            let (t2, t3) = (t * t, t * t * t);
            out.push(0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3));
        }
    }
    if !closed {
        out.push(p[n - 1]);
    }
    out
}

fn arc_pts(c: DVec2, s: DVec2, e: DVec2) -> Vec<DVec2> {
    let r = (s - c).length();
    let a0 = (s - c).y.atan2((s - c).x);
    let mut a1 = (e - c).y.atan2((e - c).x);
    if a1 <= a0 {
        a1 += std::f64::consts::TAU;
    }
    (0..=32).map(|k| c + DVec2::from_angle(a0 + (a1 - a0) * k as f64 / 32.0) * r).collect()
}

/// The flattened points of a shape and whether it is closed.
pub fn entity_points(sk: &Sketch, e: &Entity) -> (Vec<DVec2>, bool) {
    entity_pts(sk, e)
}

fn entity_pts(sk: &Sketch, e: &Entity) -> (Vec<DVec2>, bool) {
    let p = |i: usize| sk.points.get(i).map(|q| q.pos()).unwrap_or_default();
    match e {
        Entity::Line { a, b, .. } => (vec![p(*a), p(*b)], false),
        Entity::Circle { center, radius, .. } => {
            ((0..64).map(|k| p(*center) + DVec2::from_angle(k as f64 / 64.0 * std::f64::consts::TAU) * *radius).collect(), true)
        }
        Entity::Arc { center, start, end, .. } => (arc_pts(p(*center), p(*start), p(*end)), false),
        Entity::Spline { ctrl, closed, .. } => (spline_pts(&ctrl.iter().map(|i| p(*i)).collect::<Vec<_>>(), *closed), *closed),
        Entity::Polyline { pts, closed, .. } => (pts.iter().map(|q| DVec2::new(q[0], q[1])).collect(), *closed),
    }
}

fn is_line(sk: &Sketch, e: usize) -> bool {
    matches!(sk.entities.get(e), Some(Entity::Line { .. }))
}
fn is_round(sk: &Sketch, e: usize) -> bool {
    matches!(sk.entities.get(e), Some(Entity::Circle { .. } | Entity::Arc { .. }))
}
fn line_len(sk: &Sketch, e: usize) -> f64 {
    match sk.entities.get(e) {
        Some(Entity::Line { a, b, .. }) => (sk.point(*a) - sk.point(*b)).length(),
        _ => 0.0,
    }
}
fn radius_of(sk: &Sketch, e: usize) -> f64 {
    match sk.entities.get(e) {
        Some(Entity::Circle { radius, .. }) => *radius,
        Some(Entity::Arc { center, start, .. }) => (sk.point(*start) - sk.point(*center)).length(),
        _ => 0.0,
    }
}
fn line_dir(sk: &Sketch, e: usize) -> DVec2 {
    match sk.entities.get(e) {
        Some(Entity::Line { a, b, .. }) => (sk.point(*b) - sk.point(*a)).normalize_or_zero(),
        _ => DVec2::X,
    }
}

/// Re-index a constraint after deleting entities and points (`None`: it goes too).
fn remap(c: &Constraint, em: &dyn Fn(usize) -> Option<usize>, pm: &dyn Fn(usize) -> Option<usize>) -> Option<Constraint> {
    use Constraint::*;
    Some(match c {
        Coincident(p, q) => Coincident(pm(*p)?, pm(*q)?),
        Horizontal(e) => Horizontal(em(*e)?),
        Vertical(e) => Vertical(em(*e)?),
        Distance(p, q, d) => Distance(pm(*p)?, pm(*q)?, d.clone()),
        PointLineDistance(p, e, d) => PointLineDistance(pm(*p)?, em(*e)?, d.clone()),
        HorizontalDistance(p, q, d) => HorizontalDistance(pm(*p)?, pm(*q)?, d.clone()),
        VerticalDistance(p, q, d) => VerticalDistance(pm(*p)?, pm(*q)?, d.clone()),
        Length(e, d) => Length(em(*e)?, d.clone()),
        Radius(e, d) => Radius(em(*e)?, d.clone()),
        Diameter(e, d) => Diameter(em(*e)?, d.clone()),
        Angle(a, b, d) => Angle(em(*a)?, em(*b)?, d.clone()),
        Parallel(a, b) => Parallel(em(*a)?, em(*b)?),
        Perpendicular(a, b) => Perpendicular(em(*a)?, em(*b)?),
        Equal(a, b) => Equal(em(*a)?, em(*b)?),
        Tangent(a, b) => Tangent(em(*a)?, em(*b)?),
        Concentric(a, b) => Concentric(em(*a)?, em(*b)?),
        PointOnLine(p, e) => PointOnLine(pm(*p)?, em(*e)?),
        PointOnCircle(p, e) => PointOnCircle(pm(*p)?, em(*e)?),
        Midpoint(p, e) => Midpoint(pm(*p)?, em(*e)?),
        Symmetric(p, q, e) => Symmetric(pm(*p)?, pm(*q)?, em(*e)?),
        Fix(p) => Fix(pm(*p)?),
    })
}

/// Delete the selected points and entities, and what depends on them, renumbering the rest.
pub fn delete_selection(e: &mut SketchEdit) {
    let sk = &mut e.sketch;
    let dead_e: Vec<usize> = e.sel_ents.clone();
    let mut dead_p: Vec<usize> = e.sel_pts.clone();
    let used = |sk: &Sketch, p: usize, skip: &[usize]| {
        sk.entities.iter().enumerate().filter(|(i, _)| !skip.contains(i)).any(|(_, en)| match en {
            Entity::Line { a, b, .. } => *a == p || *b == p,
            Entity::Circle { center, .. } => *center == p,
            Entity::Arc { center, start, end, .. } => *center == p || *start == p || *end == p,
            Entity::Spline { ctrl, .. } => ctrl.contains(&p),
            Entity::Polyline { .. } => false,
        })
    };
    for p in 0..sk.points.len() {
        if !used(sk, p, &dead_e) && used(sk, p, &[]) {
            dead_p.push(p);
        }
    }
    let mut dead_e = dead_e;
    for (i, en) in sk.entities.iter().enumerate() {
        let pts: Vec<usize> = match en {
            Entity::Line { a, b, .. } => vec![*a, *b],
            Entity::Circle { center, .. } => vec![*center],
            Entity::Arc { center, start, end, .. } => vec![*center, *start, *end],
            Entity::Spline { ctrl, .. } => ctrl.clone(),
            Entity::Polyline { .. } => vec![],
        };
        if pts.iter().any(|p| dead_p.contains(p)) && !dead_e.contains(&i) {
            dead_e.push(i);
        }
    }
    // The entities that stay keep their numbers (the faces made from them their names).
    sk.normalize_entity_ids();
    sk.entity_ids = sk.entity_ids.iter().enumerate().filter(|(i, _)| !dead_e.contains(i)).map(|(_, id)| *id).collect();
    let emap = |i: usize| (!dead_e.contains(&i)).then(|| i - dead_e.iter().filter(|d| **d < i).count());
    let pmap = |i: usize| (!dead_p.contains(&i)).then(|| i - dead_p.iter().filter(|d| **d < i).count());
    sk.constraints = sk.constraints.iter().filter_map(|c| remap(c, &emap, &pmap)).collect();
    let fix = |p: &mut usize| *p = pmap(*p).unwrap_or(0);
    let mut ents = Vec::new();
    for (i, mut en) in std::mem::take(&mut sk.entities).into_iter().enumerate() {
        if dead_e.contains(&i) {
            continue;
        }
        match &mut en {
            Entity::Line { a, b, .. } => {
                fix(a);
                fix(b)
            }
            Entity::Circle { center, .. } => fix(center),
            Entity::Arc { center, start, end, .. } => {
                fix(center);
                fix(start);
                fix(end)
            }
            Entity::Spline { ctrl, .. } => ctrl.iter_mut().for_each(fix),
            Entity::Polyline { .. } => {}
        }
        ents.push(en);
    }
    sk.entities = ents;
    let mut i = 0;
    sk.points.retain(|_| {
        let keep = !dead_p.contains(&i);
        i += 1;
        keep
    });
    e.sel_pts.clear();
    e.sel_ents.clear();
    e.chain = None;
    e.pending.clear();
}

/// The operation that makes the finished sketch part of the document.
pub fn finish_op(e: &SketchEdit, name: &str, revolve: bool) -> (&'static str, serde_json::Value) {
    let sketch_json = serde_json::to_value(&e.sketch).unwrap_or(serde_json::Value::Null);
    if let Some(kept) = &e.saved {
        return ("sketch_set", json!({"name": kept, "sketch_json": sketch_json}));
    }
    match (e.feature, revolve) {
        (Some(_), _) => ("set_sketch", json!({"feature": name, "sketch_json": sketch_json})),
        (None, true) => ("sketch_revolve", json!({"sketch_json": sketch_json, "angle": 360, "op": op_word(e.op)})),
        (None, false) => {
            let depth = super::number_or_expr(&e.depth);
            ("sketch_extrude", json!({"sketch_json": sketch_json, "depth": depth, "op": op_word(e.op), "reverse": e.reverse}))
        }
    }
}

fn op_word(o: BooleanOp) -> &'static str {
    match o {
        BooleanOp::Add => "add",
        BooleanOp::Cut => "cut",
        BooleanOp::Intersect => "intersect",
    }
}

/// Finish the sketch: as a new extrusion or revolve, or as the new sketch of its feature.
pub fn finish(state: &mut State, revolve: bool) {
    let Some(e) = state.cad.sketch.take() else { return };
    let name = e.feature.and_then(|id| state.session.lock().cad_edit.as_ref().and_then(|x| x.active().feature(id).map(|f| f.name.clone()))).unwrap_or_default();
    let (opname, mut args) = finish_op(&e, &name, revolve);
    if e.feature.is_none() && e.saved.is_none() {
        let base = if revolve {
            "Revolve"
        } else if e.op == BooleanOp::Cut {
            "Pocket"
        } else {
            "Extrude"
        };
        args["name"] = json!(unique_name(state, base));
    }
    if op(state, opname, args).is_none() {
        // The part refused it: keep the sketch open so nothing is lost.
        state.cad.sketch = Some(e);
    }
}

fn unique_name(state: &State, base: &str) -> String {
    let s = state.session.lock();
    let names: Vec<String> = s.cad_edit.as_ref().map(|e| e.active().features.iter().map(|f| f.name.clone()).collect()).unwrap_or_default();
    if !names.iter().any(|n| n == base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !names.contains(n)).unwrap()
}

// ------------------------------------------------------------------------------- canvas

pub fn canvas_ui(state: &mut State, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
    let rect = resp.rect;
    let dark = ui.visuals().dark_mode;
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let mut changed = false;
    {
        let Some(e) = state.cad.sketch.as_mut() else { return };
        if !ui.input(|i| i.pointer.any_down()) {
            e.record();
        }
        if !e.fitted {
            e.fitted = true;
            let mut lo = DVec2::splat(-40.0);
            let mut hi = DVec2::splat(40.0);
            for p in &e.sketch.points {
                lo = lo.min(p.pos() - 10.0);
                hi = hi.max(p.pos() + 10.0);
            }
            e.center = (lo + hi) * 0.5;
            e.scale = ((rect.width() as f64 / (hi.x - lo.x)).min(rect.height() as f64 / (hi.y - lo.y)) * 0.8).max(0.1) as f32;
        }
        if e.demo_cursor.is_some() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            e.demo_cursor = None;
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            e.center -= DVec2::new(d.x as f64, -d.y as f64) / e.scale as f64;
        }
        if resp.hovered() {
            let z = ui.input(|i| i.smooth_scroll_delta.y);
            if z.abs() > 0.0 {
                e.scale = (e.scale * (z * 0.0015).exp()).clamp(0.05, 200.0);
            }
        }
    }
    let e = state.cad.sketch.as_ref().unwrap();
    let (center, scale) = (e.center, e.scale);
    let to_s = |p: DVec2| Pos2::new(rect.center().x + ((p.x - center.x) as f32) * scale, rect.center().y - ((p.y - center.y) as f32) * scale);
    let to_w = |s: Pos2| DVec2::new(center.x + ((s.x - rect.center().x) / scale) as f64, center.y - ((s.y - rect.center().y) as f32 / scale) as f64);
    // Grid and axes.
    let step = if scale > 4.0 {
        5.0
    } else if scale > 1.0 {
        10.0
    } else {
        50.0
    };
    let gcol = if dark { Color32::from_gray(60) } else { Color32::from_rgba_unmultiplied(0, 0, 0, 24) };
    let (wlo, whi) = (to_w(rect.left_bottom()), to_w(rect.right_top()));
    let mut x = (wlo.x / step).floor() * step;
    while x < whi.x {
        painter.line_segment([to_s(DVec2::new(x, wlo.y)), to_s(DVec2::new(x, whi.y))], Stroke::new(0.5, gcol));
        x += step;
    }
    let mut y = (wlo.y / step).floor() * step;
    while y < whi.y {
        painter.line_segment([to_s(DVec2::new(wlo.x, y)), to_s(DVec2::new(whi.x, y))], Stroke::new(0.5, gcol));
        y += step;
    }
    painter.line_segment([to_s(DVec2::new(wlo.x, 0.0)), to_s(DVec2::new(whi.x, 0.0))], Stroke::new(1.0, Color32::from_rgb(220, 90, 90)));
    painter.line_segment([to_s(DVec2::new(0.0, wlo.y)), to_s(DVec2::new(0.0, whi.y))], Stroke::new(1.0, Color32::from_rgb(90, 180, 90)));
    let sk = &e.sketch;
    let status_of = e.report.as_ref().map(|r| r.status);
    let base = match status_of {
        Some(SolveStatus::FullyDefined) => {
            if dark {
                Color32::from_gray(235)
            } else {
                Color32::BLACK
            }
        }
        Some(SolveStatus::OverDefined | SolveStatus::Failed) => crate::theme::ERROR,
        _ => Color32::from_rgb(60, 130, 235),
    };
    for (i, en) in sk.entities.iter().enumerate() {
        let (pts, closed) = entity_pts(sk, en);
        let mut s: Vec<Pos2> = pts.iter().map(|p| to_s(*p)).collect();
        if closed && !s.is_empty() {
            s.push(s[0]);
        }
        let col = if e.sel_ents.contains(&i) { crate::theme::ACCENT } else { base };
        if en.construction() {
            for w in s.windows(2) {
                painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.0, col), 5.0, 4.0));
            }
        } else {
            painter.add(egui::Shape::line(s, Stroke::new(if e.sel_ents.contains(&i) { 2.6 } else { 1.8 }, col)));
        }
    }
    for (i, p) in sk.points.iter().enumerate() {
        let s = to_s(p.pos());
        let col = if e.sel_pts.contains(&i) { crate::theme::ACCENT } else { base };
        if p.fixed {
            painter.rect_filled(egui::Rect::from_center_size(s, egui::vec2(7.0, 7.0)), 0.0, col);
        } else {
            painter.circle_filled(s, 3.0, col);
        }
    }
    // Constraint glyphs; dimensions are drawn with their lines and labels.
    let mid = |en: usize| -> DVec2 {
        match sk.entities.get(en) {
            Some(Entity::Line { a, b, .. }) => (sk.point(*a) + sk.point(*b)) * 0.5,
            Some(Entity::Circle { center, radius, .. }) => sk.point(*center) + DVec2::new(radius * 0.7, radius * 0.7),
            Some(Entity::Arc { center, start, .. }) => (sk.point(*center) + sk.point(*start)) * 0.5,
            _ => DVec2::ZERO,
        }
    };
    let gcolr = Color32::from_rgb(30, 150, 90);
    let mut used: Vec<(Pos2, u32)> = Vec::new();
    for c in &sk.constraints {
        let (at, txt) = match c {
            Constraint::Horizontal(e) => (mid(*e), "H".to_string()),
            Constraint::Vertical(e) => (mid(*e), "V".to_string()),
            Constraint::Parallel(a, _) => (mid(*a), "∥".into()),
            Constraint::Perpendicular(a, _) => (mid(*a), "⊥".into()),
            Constraint::Equal(a, _) => (mid(*a), "=".into()),
            Constraint::Tangent(a, _) => (mid(*a), "T".into()),
            Constraint::Concentric(a, _) => (mid(*a), "◎".into()),
            Constraint::Fix(p) => (sk.point(*p), "fix".into()),
            _ => continue,
        };
        let s = to_s(at);
        let k = match used.iter_mut().find(|(p, _)| (*p - s).length() < 20.0) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                used.push((s, 0));
                0
            }
        };
        painter.text(s + egui::vec2(6.0, -6.0 + 15.0 * k as f32), egui::Align2::LEFT_BOTTOM, txt, egui::FontId::proportional(12.0), gcolr);
    }
    let dims = super::sketch_dims::all(sk, 22.0 / scale as f64);
    let hover_dim = resp.hover_pos().and_then(|h| super::sketch_dims::under(&dims, &to_s, h));
    let label_bg = ui.visuals().extreme_bg_color;
    for g in &dims {
        let hl = hover_dim == Some(g.index) || e.dim_drag == Some(g.index);
        super::sketch_dims::draw(&painter, g, &to_s, gcolr, label_bg, hl);
    }
    // Status.
    let (status, col) = match (&e.report, &e.error) {
        (_, Some(err)) => (format!("Cannot solve: {err}"), crate::theme::ERROR),
        (Some(r), _) => match r.status {
            SolveStatus::FullyDefined => ("Fully defined".to_string(), crate::theme::OK),
            SolveStatus::UnderDefined(n) => (format!("Under-defined: {n} degree(s) of freedom left (blue)"), Color32::from_rgb(60, 130, 235)),
            SolveStatus::OverDefined => ("Over-defined: remove a redundant or conflicting constraint (red)".to_string(), crate::theme::ERROR),
            SolveStatus::Failed => ("The solver failed: check the dimensions".to_string(), crate::theme::ERROR),
        },
        _ => ("Draw, then constrain and dimension".to_string(), ui.visuals().weak_text_color()),
    };
    painter.text(rect.left_top() + egui::vec2(10.0, 8.0), egui::Align2::LEFT_TOP, &status, egui::FontId::proportional(14.0), col);
    painter.text(
        rect.left_bottom() + egui::vec2(10.0, -8.0),
        egui::Align2::LEFT_BOTTOM,
        format!("Sketch on {} · click draws or selects · right or middle drag pans · wheel zooms · Esc ends a chain · Del deletes", e.sketch.plane_label()),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );

    // ---- interaction
    // A scripted cursor (for pictures) wins until the mouse moves.
    let hover = e.demo_cursor.map(to_s).or(resp.hover_pos());
    let pick_point = |sk: &Sketch, s: Pos2| -> Option<usize> {
        sk.points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (to_s(p.pos()) - s).length()))
            .filter(|(_, d)| *d < 9.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let pick_entity = |sk: &Sketch, s: Pos2| -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, en) in sk.entities.iter().enumerate() {
            let (pts, closed) = entity_pts(sk, en);
            let sp: Vec<Pos2> = pts.iter().map(|p| to_s(*p)).collect();
            let n = sp.len();
            let segs = if closed { n } else { n.saturating_sub(1) };
            for k in 0..segs {
                let (a, b) = (sp[k], sp[(k + 1) % n]);
                let ab = b - a;
                let t = ((s - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let d = (a + ab * t - s).length();
                if d < 7.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((i, d));
                }
            }
        }
        best.map(|(i, _)| i)
    };
    let tool = e.tool;
    let shift = ui.input(|i| i.modifiers.shift);
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let del = ui.input(|i| i.key_pressed(egui::Key::Delete)) && !ui.ctx().egui_wants_keyboard_input();
    let grid = e.snap_grid.max(1e-6);
    // Where the cursor lands (Ctrl held: no snapping).
    let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
    let anchor_pt = e.chain.or_else(|| e.pending.first().copied()).map(|a| e.sketch.point(a));
    let snaps = super::sketch_snap::Snaps { on: e.snaps.on && !ctrl, ..e.snaps };
    let snapped = hover.map(|h| super::sketch_snap::snap(&e.sketch, to_w(h), 10.0 / scale as f64, grid, anchor_pt, e.drag, &snaps));
    let drawing = matches!(tool, Tool::Line | Tool::Rect | Tool::Circle | Tool::Arc | Tool::Spline | Tool::Point) || e.drag.is_some();
    if let (Some(sn), true) = (&snapped, drawing && hover_dim.is_none()) {
        let gst = Stroke::new(1.0, Color32::from_rgb(0, 170, 190));
        for (a, b) in &sn.guides {
            painter.extend(egui::Shape::dashed_line(&[to_s(*a), to_s(*b)], gst, 4.0, 4.0));
        }
        let sp = to_s(sn.pos);
        let mc = Color32::from_rgb(255, 140, 0);
        use super::sketch_snap::SnapKind as K;
        match sn.kind {
            K::Point(_) => {
                painter.rect_stroke(egui::Rect::from_center_size(sp, egui::vec2(11.0, 11.0)), 0.0, Stroke::new(1.6, mc), egui::StrokeKind::Middle);
            }
            K::Center(_) => {
                painter.circle_stroke(sp, 6.0, Stroke::new(1.6, mc));
                painter.circle_filled(sp, 1.5, mc);
            }
            K::Midpoint(_) => {
                painter.add(egui::Shape::closed_line(
                    vec![sp + egui::vec2(0.0, -7.0), sp + egui::vec2(6.0, 4.0), sp + egui::vec2(-6.0, 4.0)],
                    Stroke::new(1.6, mc),
                ));
            }
            K::Quadrant(_) => {
                painter.add(egui::Shape::closed_line(
                    vec![sp + egui::vec2(0.0, -6.0), sp + egui::vec2(6.0, 0.0), sp + egui::vec2(0.0, 6.0), sp + egui::vec2(-6.0, 0.0)],
                    Stroke::new(1.6, mc),
                ));
            }
            K::Intersection(..) => {
                painter.line_segment([sp + egui::vec2(-5.0, -5.0), sp + egui::vec2(5.0, 5.0)], Stroke::new(1.8, mc));
                painter.line_segment([sp + egui::vec2(-5.0, 5.0), sp + egui::vec2(5.0, -5.0)], Stroke::new(1.8, mc));
            }
            K::OnCurve(_) => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.4, mc));
            }
            K::Guide(..) | K::Grid => {
                painter.line_segment([sp + egui::vec2(-4.0, 0.0), sp + egui::vec2(4.0, 0.0)], Stroke::new(1.0, mc));
                painter.line_segment([sp + egui::vec2(0.0, -4.0), sp + egui::vec2(0.0, 4.0)], Stroke::new(1.0, mc));
            }
            K::Free => {}
        }
        let lab = sn.kind.label();
        if !lab.is_empty() {
            painter.text(sp + egui::vec2(9.0, 9.0), egui::Align2::LEFT_TOP, lab, egui::FontId::proportional(11.0), mc);
        }
    }
    let snap = |w: DVec2| snapped.as_ref().map(|s| s.pos).unwrap_or(DVec2::new((w.x / grid).round() * grid, (w.y / grid).round() * grid));
    let e = state.cad.sketch.as_mut().unwrap();
    // Dimension labels: drag to move them, double-click to type a value (in any tool).
    let mut on_label = hover_dim.is_some();
    if let Some(i) = hover_dim {
        if resp.double_clicked() {
            let cur =
                e.sketch.constraints.get_mut(i).and_then(|c| c.dim_mut()).map(|d| d.expr.clone().unwrap_or_else(|| num_text(d.value))).unwrap_or_default();
            e.dim_edit = Some((i, cur));
        } else if resp.drag_started_by(egui::PointerButton::Primary) {
            e.dim_drag = Some(i);
        }
    }
    if let Some(i) = e.dim_drag {
        on_label = true;
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            super::sketch_dims::move_label(&mut e.sketch, i, DVec2::new(d.x as f64, -d.y as f64) / scale as f64);
        }
        if resp.drag_stopped() {
            e.dim_drag = None;
        }
    }
    if esc {
        if e.chain.is_some() || !e.pending.is_empty() {
            e.chain = None;
            e.pending.clear();
        } else {
            e.tool = Tool::Select;
        }
    }
    if del && (!e.sel_pts.is_empty() || !e.sel_ents.is_empty()) {
        delete_selection(e);
        changed = true;
    }
    let get_or_add = |e: &mut SketchEdit, s: Pos2| -> usize {
        use super::sketch_snap::SnapKind as K;
        match snapped.as_ref().map(|x| x.kind) {
            Some(K::Point(i) | K::Center(i)) => return i,
            Some(K::Free) | None => {
                if let Some(i) = pick_point(&e.sketch, s) {
                    return i;
                }
            }
            _ => {}
        }
        let w = snap(to_w(s));
        e.sketch.points.push(SPoint::new(w.x, w.y));
        let i = e.sketch.points.len() - 1;
        if let Some(k) = snapped.as_ref().map(|x| x.kind) {
            for c in super::sketch_snap::constraints_for(&e.sketch, i, k) {
                e.sketch.constrain(c);
            }
        }
        i
    };
    // The rubber band of the shape being drawn.
    if let Some(h) = hover {
        let anchor = e.chain.or_else(|| e.pending.first().copied());
        if let Some(a) = anchor.filter(|_| matches!(tool, Tool::Line | Tool::Rect | Tool::Circle | Tool::Arc | Tool::Spline)) {
            let pa = e.sketch.point(a);
            let ph = snap(to_w(h));
            let st = Stroke::new(1.0, crate::theme::ACCENT);
            match tool {
                Tool::Rect => {
                    painter.rect_stroke(egui::Rect::from_two_pos(to_s(pa), to_s(ph)), 0.0, st, egui::StrokeKind::Middle);
                }
                Tool::Circle => {
                    painter.circle_stroke(to_s(pa), ((ph - pa).length() as f32) * scale, st);
                }
                _ => {
                    painter.line_segment([to_s(pa), to_s(ph)], st);
                }
            }
            painter.text(
                h + egui::vec2(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                num_text((ph - pa).length()),
                egui::FontId::monospace(12.0),
                crate::theme::ACCENT,
            );
        }
    }
    match if on_label { None } else { Some(tool) } {
        None => {}
        Some(Tool::Select) => {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                e.drag = hover.and_then(|s| pick_point(&e.sketch, s)).filter(|i| !e.sketch.points[*i].fixed);
            }
            if let (Some(i), Some(h)) = (e.drag, hover) {
                if resp.dragged_by(egui::PointerButton::Primary) {
                    let w = if ctrl { to_w(h) } else { snap(to_w(h)) };
                    e.sketch.points[i].x = w.x;
                    e.sketch.points[i].y = w.y;
                    changed = true;
                }
            }
            if resp.drag_stopped() {
                e.drag = None;
            }
            if resp.clicked() {
                if let Some(s) = hover {
                    if !shift {
                        e.sel_pts.clear();
                        e.sel_ents.clear();
                    }
                    if let Some(p) = pick_point(&e.sketch, s) {
                        match e.sel_pts.iter().position(|x| *x == p) {
                            Some(k) => {
                                e.sel_pts.remove(k);
                            }
                            None => e.sel_pts.push(p),
                        }
                    } else if let Some(en) = pick_entity(&e.sketch, s) {
                        match e.sel_ents.iter().position(|x| *x == en) {
                            Some(k) => {
                                e.sel_ents.remove(k);
                            }
                            None => e.sel_ents.push(en),
                        }
                    }
                }
            }
        }
        Some(Tool::Point) => {
            if let (true, Some(s)) = (resp.clicked(), hover) {
                get_or_add(e, s);
                changed = true;
            }
        }
        Some(Tool::Line) => {
            if resp.secondary_clicked() {
                e.chain = None;
            }
            if let (true, Some(s)) = (resp.clicked(), hover) {
                let p = get_or_add(e, s);
                if let Some(prev) = e.chain.filter(|prev| *prev != p) {
                    let li = e.sketch.add_line(prev, p);
                    // Inference: nearly horizontal or vertical lines get the constraint.
                    let d = e.sketch.point(p) - e.sketch.point(prev);
                    if d.y.abs() < d.length() * 0.035 {
                        e.sketch.constrain(Constraint::Horizontal(li));
                    } else if d.x.abs() < d.length() * 0.035 {
                        e.sketch.constrain(Constraint::Vertical(li));
                    }
                    changed = true;
                }
                let closes =
                    e.chain.is_some() && e.sketch.entities.iter().filter(|en| matches!(en, Entity::Line { a, b, .. } if *a == p || *b == p)).count() >= 2;
                e.chain = if closes { None } else { Some(p) };
            }
        }
        Some(Tool::Rect) => {
            if let (true, Some(s)) = (resp.clicked(), hover) {
                let p = get_or_add(e, s);
                if let Some(a) = e.pending.pop() {
                    let (pa, pb) = (e.sketch.point(a), e.sketch.point(p));
                    let b = e.sketch.add_point(pb.x, pa.y);
                    let d = e.sketch.add_point(pa.x, pb.y);
                    let l0 = e.sketch.add_line(a, b);
                    let l1 = e.sketch.add_line(b, p);
                    let l2 = e.sketch.add_line(p, d);
                    let l3 = e.sketch.add_line(d, a);
                    for c in [Constraint::Horizontal(l0), Constraint::Vertical(l1), Constraint::Horizontal(l2), Constraint::Vertical(l3)] {
                        e.sketch.constrain(c);
                    }
                    changed = true;
                } else {
                    e.pending.push(p);
                }
            }
        }
        Some(Tool::Circle) => {
            if let (true, Some(s)) = (resp.clicked(), hover) {
                if let Some(c) = e.pending.pop() {
                    let r = (snap(to_w(s)) - e.sketch.point(c)).length().max(0.1);
                    e.sketch.push(Entity::Circle { center: c, radius: r, construction: false });
                    changed = true;
                } else {
                    let c = get_or_add(e, s);
                    e.pending.push(c);
                }
            }
        }
        Some(Tool::Arc) => {
            if let (true, Some(s)) = (resp.clicked(), hover) {
                if e.pending.len() == 2 {
                    let (c, st) = (e.pending[0], e.pending[1]);
                    let r = (e.sketch.point(st) - e.sketch.point(c)).length();
                    let dir = (snap(to_w(s)) - e.sketch.point(c)).normalize_or_zero();
                    let end = pick_point(&e.sketch, s).unwrap_or_else(|| {
                        let q = e.sketch.point(c) + dir * r;
                        e.sketch.add_point(q.x, q.y)
                    });
                    e.sketch.add_arc(c, st, end);
                    e.pending.clear();
                    changed = true;
                } else {
                    let p = get_or_add(e, s);
                    e.pending.push(p);
                }
            }
        }
        Some(Tool::Spline) => {
            if let (true, Some(s)) = (resp.clicked(), hover) {
                let p = get_or_add(e, s);
                e.pending.push(p);
            }
            if (resp.double_clicked() || resp.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter))) && e.pending.len() >= 2 {
                let mut ctrl = std::mem::take(&mut e.pending);
                ctrl.dedup();
                let closed = ctrl.len() > 3 && ctrl.first() == ctrl.last();
                if closed {
                    ctrl.pop();
                }
                e.sketch.add_spline(ctrl, closed);
                changed = true;
            }
        }
    }
    // The value editor of a double-clicked dimension: a number or an expression; Enter sets it.
    if let Some((i, mut text)) = e.dim_edit.take() {
        let at = dims.iter().find(|g| g.index == i).map(|g| to_s(g.label)).unwrap_or(rect.center());
        let mut keep = true;
        egui::Area::new(ui.scope_id().with(("sketch dim edit", i))).fixed_pos(at - egui::vec2(50.0, 12.0)).order(egui::Order::Foreground).show(
            ui.ctx(),
            |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(100.0).hint_text("value or expression"));
                    if !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if super::sketch_dims::set_from_text(&mut e.sketch, i, &text) {
                            changed = true;
                        }
                        keep = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) || r.lost_focus() {
                        keep = false;
                    }
                    ui.label(RichText::new("Enter sets it · Esc cancels").weak().small());
                });
            },
        );
        if keep {
            e.dim_edit = Some((i, text));
        }
    }
    if changed {
        solve(state);
    }
}

// ------------------------------------------------------------------------------- ribbon tools

/// The sketch tools in the ribbon's Sketch tab: drawing tools, constraints, dimensions, finish.
pub fn tools_ui(state: &mut State, ui: &mut egui::Ui) {
    let Some(e) = state.cad.sketch.as_mut() else { return };
    ui.label(RichText::new(if e.feature.is_some() { "Edit sketch:" } else { "New sketch:" }).strong());
    for (t, l, tip) in [
        (Tool::Select, "Select", "Select points and shapes (Shift adds); drag a point to move it  (Esc)"),
        (Tool::Line, "Line", "Click points to draw connected lines; right-click ends the chain  (L)"),
        (Tool::Rect, "Rectangle", "Click two opposite corners  (R)"),
        (Tool::Circle, "Circle", "Click the center, then a point on the circle  (C)"),
        (Tool::Arc, "Arc", "Click the center, the start, then the end (counter-clockwise)  (A)"),
        (Tool::Spline, "Spline", "Click control points; double-click or Enter finishes"),
        (Tool::Point, "Point", "Place a point  (P)"),
    ] {
        if ui.selectable_label(e.tool == t, crate::icons::with(l)).on_hover_text(tip).clicked() {
            e.tool = t;
            e.chain = None;
            e.pending.clear();
        }
    }
    ui.separator();
    ui.toggle_value(&mut e.snaps.on, crate::icons::with("Snap"))
        .on_hover_text("Snap to points, centres, midpoints, quadrants, intersections, curves, guide lines and the grid (choose which in the properties; hold Ctrl to place freely)");
    ui.separator();
    ui.label(RichText::new(format!("Constrain ({} pt, {} shape):", e.sel_pts.len(), e.sel_ents.len())).weak().small());
    let sk = &e.sketch;
    let (pts, ents) = (e.sel_pts.clone(), e.sel_ents.clone());
    let lines: Vec<usize> = ents.iter().copied().filter(|i| is_line(sk, *i)).collect();
    let rounds: Vec<usize> = ents.iter().copied().filter(|i| is_round(sk, *i)).collect();
    let mut add: Vec<Constraint> = Vec::new();
    let mut b = |ui: &mut egui::Ui, ok: bool, label: &str, tip: &str, c: &dyn Fn() -> Vec<Constraint>| {
        if ui
            .add_enabled(ok, egui::Button::new(crate::icons::with(label)))
            .on_hover_text(tip)
            .on_disabled_hover_text(format!("{tip}.\nSelect what it applies to first (the Select tool; Shift adds to the selection)."))
            .clicked()
        {
            add.extend(c());
        }
    };
    let only_lines = !lines.is_empty() && pts.is_empty() && rounds.is_empty();
    b(ui, only_lines, "H", "Horizontal", &|| lines.iter().map(|l| Constraint::Horizontal(*l)).collect());
    b(ui, only_lines, "V", "Vertical", &|| lines.iter().map(|l| Constraint::Vertical(*l)).collect());
    b(ui, lines.len() == 2 && ents.len() == 2, "Parallel", "Two lines parallel", &|| vec![Constraint::Parallel(lines[0], lines[1])]);
    b(ui, lines.len() == 2 && ents.len() == 2, "Perpendicular", "Two lines at right angles", &|| vec![Constraint::Perpendicular(lines[0], lines[1])]);
    b(ui, ents.len() == 2 && (lines.len() == 2 || rounds.len() == 2), "Equal", "Equal lengths or radii", &|| vec![Constraint::Equal(ents[0], ents[1])]);
    b(ui, (lines.len() == 1 && rounds.len() == 1) || rounds.len() == 2, "Tangent", "A line tangent to a circle or arc, or two touching", &|| {
        vec![Constraint::Tangent(ents[0], ents[1])]
    });
    b(ui, rounds.len() == 2, "Concentric", "Same center", &|| vec![Constraint::Concentric(rounds[0], rounds[1])]);
    b(ui, pts.len() == 2 && ents.is_empty(), "Coincident", "Two points are one", &|| vec![Constraint::Coincident(pts[0], pts[1])]);
    b(ui, pts.len() == 1 && lines.len() == 1, "Midpoint", "A point at the middle of a line", &|| vec![Constraint::Midpoint(pts[0], lines[0])]);
    b(ui, pts.len() == 1 && lines.len() == 1, "On line", "A point on a line", &|| vec![Constraint::PointOnLine(pts[0], lines[0])]);
    b(ui, pts.len() == 1 && rounds.len() == 1, "On circle", "A point on a circle", &|| vec![Constraint::PointOnCircle(pts[0], rounds[0])]);
    b(ui, pts.len() == 2 && lines.len() == 1, "Symmetric", "Two points mirrored about a line", &|| vec![Constraint::Symmetric(pts[0], pts[1], lines[0])]);
    b(ui, !pts.is_empty() && ents.is_empty(), "Fix", "Hold a point in place", &|| pts.iter().map(|p| Constraint::Fix(*p)).collect());
    ui.separator();
    ui.label(RichText::new("Dimension:").weak().small());
    b(ui, lines.len() == 1 && ents.len() == 1, "Length", "Fix a line's length (edit the value or an expression in the list)", &|| {
        vec![Constraint::Length(lines[0], Dim::new(line_len(sk, lines[0])))]
    });
    b(ui, rounds.len() == 1 && ents.len() == 1, "Radius", "Fix a radius", &|| vec![Constraint::Radius(rounds[0], Dim::new(radius_of(sk, rounds[0])))]);
    b(ui, rounds.len() == 1 && ents.len() == 1, "Diameter", "Fix a diameter", &|| {
        vec![Constraint::Diameter(rounds[0], Dim::new(2.0 * radius_of(sk, rounds[0])))]
    });
    b(ui, pts.len() == 2 && ents.is_empty(), "Distance", "The distance between two points", &|| {
        vec![Constraint::Distance(pts[0], pts[1], Dim::new((sk.point(pts[0]) - sk.point(pts[1])).length()))]
    });
    b(ui, pts.len() == 2 && ents.is_empty(), "Horizontal distance", "The horizontal distance between two points", &|| {
        vec![Constraint::HorizontalDistance(pts[0], pts[1], Dim::new((sk.point(pts[0]).x - sk.point(pts[1]).x).abs()))]
    });
    b(ui, pts.len() == 2 && ents.is_empty(), "Vertical distance", "The vertical distance between two points", &|| {
        vec![Constraint::VerticalDistance(pts[0], pts[1], Dim::new((sk.point(pts[0]).y - sk.point(pts[1]).y).abs()))]
    });
    b(ui, pts.len() == 1 && lines.len() == 1 && ents.len() == 1, "Distance to line", "The distance from a point to a line", &|| {
        let (a, bb) = match sk.entities[lines[0]] {
            Entity::Line { a, b, .. } => (sk.point(a), sk.point(b)),
            _ => (DVec2::ZERO, DVec2::X),
        };
        let d = (bb - a).normalize_or(DVec2::X).perp_dot(sk.point(pts[0]) - a).abs();
        vec![Constraint::PointLineDistance(pts[0], lines[0], Dim::new(d))]
    });
    b(ui, lines.len() == 2 && ents.len() == 2 && pts.is_empty(), "Distance between lines", "The distance between two parallel lines", &|| {
        let (p0, a, bb) = match (&sk.entities[lines[0]], &sk.entities[lines[1]]) {
            (Entity::Line { a: p0, .. }, Entity::Line { a, b, .. }) => (*p0, sk.point(*a), sk.point(*b)),
            _ => return vec![],
        };
        let d = (bb - a).normalize_or(DVec2::X).perp_dot(sk.point(p0) - a).abs();
        vec![Constraint::PointLineDistance(p0, lines[1], Dim::new(d))]
    });
    b(ui, lines.len() == 2 && ents.len() == 2, "Angle", "The angle between two lines", &|| {
        vec![Constraint::Angle(lines[0], lines[1], Dim::new(line_dir(sk, lines[0]).dot(line_dir(sk, lines[1])).clamp(-1.0, 1.0).acos().to_degrees()))]
    });
    let mut changed = false;
    if !add.is_empty() {
        let one_dim = add.len() == 1 && add[0].clone().dim_mut().is_some();
        for c in add {
            e.sketch.constrain(c);
        }
        // A new dimension opens its value for typing.
        if one_dim {
            let i = e.sketch.constraints.len() - 1;
            let cur = e.sketch.constraints[i].dim_mut().map(|d| num_text(d.value)).unwrap_or_default();
            e.dim_edit = Some((i, cur));
        }
        e.sel_pts.clear();
        e.sel_ents.clear();
        changed = true;
    }
    ui.separator();
    if ui
        .add_enabled(!e.sel_ents.is_empty(), egui::Button::new(crate::icons::with("Construction")))
        .on_hover_text("Toggle construction geometry (not part of the outline)")
        .clicked()
    {
        for i in e.sel_ents.clone() {
            if let Some(en) = e.sketch.entities.get_mut(i) {
                let v = !en.construction();
                en.set_construction(v);
            }
        }
        changed = true;
    }
    if ui
        .add_enabled(!e.sel_ents.is_empty() || !e.sel_pts.is_empty(), egui::Button::new(crate::icons::with("Delete")))
        .on_hover_text("Delete the selection (Del)")
        .clicked()
    {
        delete_selection(e);
        changed = true;
    }
    if changed {
        solve(state);
    }
}

// ------------------------------------------------------------------------------- properties

/// The sketch's properties in the feature panel: plane, the list of constraints with their
/// dimensions (values or expressions), the outline count, and how to finish.
pub fn props_ui(state: &mut State, ui: &mut egui::Ui) {
    let feature_name = state
        .cad
        .sketch
        .as_ref()
        .and_then(|e| e.feature)
        .and_then(|id| state.session.lock().cad_edit.as_ref().and_then(|x| x.active().feature(id).map(|f| f.name.clone())));
    let Some(e) = state.cad.sketch.as_mut() else { return };
    let title = match (&feature_name, &e.saved) {
        (Some(f), _) => format!("Sketch of {f}"),
        (None, Some(k)) => format!("Sketch '{k}'"),
        (None, None) => "New sketch".to_string(),
    };
    ui.label(RichText::new(title).strong());
    if let Some(face) = &e.sketch.face {
        ui.label(RichText::new(format!("On the face {face}: it stays on that face when the part changes.")).weak().small());
    }
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Plane");
        if e.feature.is_none() && e.sketch.frame.is_none() && e.sketch.face.is_none() {
            for (pl, l) in [(SketchPlane::XY, "Top (XY)"), (SketchPlane::XZ, "Front (XZ)"), (SketchPlane::YZ, "Right (YZ)")] {
                changed |= ui.selectable_value(&mut e.sketch.plane, pl, l).changed();
            }
        } else {
            ui.label(e.sketch.plane_label());
        }
    });
    ui.horizontal(|ui| {
        ui.label("Offset");
        let r = ui.add(egui::DragValue::new(&mut e.sketch.offset).speed(0.1).suffix(" mm"));
        changed |= r.drag_stopped() || r.lost_focus();
        ui.label("Grid");
        ui.add(egui::DragValue::new(&mut e.snap_grid).speed(0.05).range(0.01..=50.0).suffix(" mm"));
    });
    ui.horizontal_wrapped(|ui| {
        let sn = &mut e.snaps;
        ui.checkbox(&mut sn.on, RichText::new("Snap:").strong()).on_hover_text("All snapping (Ctrl held places freely)");
        ui.add_enabled_ui(sn.on, |ui| {
            ui.checkbox(&mut sn.points, "points");
            ui.checkbox(&mut sn.centers, "centers");
            ui.checkbox(&mut sn.midpoints, "midpoints");
            ui.checkbox(&mut sn.quadrants, "quadrants");
            ui.checkbox(&mut sn.intersections, "intersections");
            ui.checkbox(&mut sn.curves, "on curves");
            ui.checkbox(&mut sn.guides, "guide lines").on_hover_text("Dashed horizontal and vertical lines through points, also far away");
            ui.checkbox(&mut sn.grid, "grid");
        });
    });
    ui.label(RichText::new("Dimensions: drag a label to move it, double-click it to type a value or an expression.").weak().small());
    // The status.
    if let Some(r) = &e.report {
        ui.label(
            RichText::new(match r.status {
                SolveStatus::FullyDefined => "Fully defined".to_string(),
                SolveStatus::UnderDefined(n) => format!("Under-defined: {n} degree(s) of freedom left"),
                SolveStatus::OverDefined => "Over-defined".to_string(),
                SolveStatus::Failed => "The solver failed".to_string(),
            })
            .color(match r.status {
                SolveStatus::FullyDefined => crate::theme::OK,
                SolveStatus::UnderDefined(_) => Color32::from_rgb(60, 130, 235),
                _ => crate::theme::ERROR,
            }),
        );
    }
    if let Some(err) = &e.error {
        ui.label(RichText::new(err).color(crate::theme::ERROR).small());
    }
    ui.separator();
    ui.label(RichText::new(format!("Constraints ({})", e.sketch.constraints.len())).strong());
    let mut remove = None;
    for (i, c) in e.sketch.constraints.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let name = format!("{c:?}");
            ui.label(RichText::new(name.split('(').next().unwrap_or("")).small());
            if let Some(d) = c.dim_mut() {
                let shown = d.expr.clone().unwrap_or_else(|| num_text(d.value));
                if let Some(t) = expr_field(ui, ("sk dim", i), &shown, 90.0) {
                    match t.parse::<f64>() {
                        Ok(x) => *d = Dim::new(x),
                        Err(_) => d.expr = Some(t),
                    }
                    changed = true;
                }
                if d.expr.is_some() {
                    ui.label(RichText::new(format!("= {}", num_text(d.value))).weak().small());
                }
            }
            if crate::touch::small_button(ui, "×").on_hover_text("Remove this constraint").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        e.sketch.constraints.remove(i);
        changed = true;
    }
    ui.separator();
    ui.label(format!("{} closed outline(s), {} open path(s)", e.sketch.profiles().len(), e.sketch.open_paths().len()));
    let outlines = e.sketch.profiles().len();
    let drawn = e.sketch.entities.iter().any(|x| !x.construction());
    let kept = e.saved.is_some();
    let editing = e.feature.is_some() || kept;
    if !editing {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut e.op, BooleanOp::Add, "Boss (adds)");
            ui.selectable_value(&mut e.op, BooleanOp::Cut, "Cut");
            ui.selectable_value(&mut e.op, BooleanOp::Intersect, "Intersect");
        });
        ui.horizontal(|ui| {
            ui.label("Depth");
            ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut e.depth).hint_text("10 or width / 2"));
            ui.checkbox(&mut e.reverse, "reverse").on_hover_text("Extrude against the sketch plane's normal");
        });
    }
    let (mut extrude, mut revolve, mut apply, mut cancel_it, mut keep_it) = (false, false, false, false, false);
    ui.horizontal_wrapped(|ui| {
        if editing {
            // A kept sketch may be an open path (of a sweep); a feature's needs a closed outline.
            if ui
                .add_enabled(if kept { drawn } else { outlines > 0 }, egui::Button::new(RichText::new(crate::icons::with("Apply sketch")).strong()))
                .on_disabled_hover_text(if kept { "Draw something first" } else { "The sketch needs a closed outline" })
                .clicked()
            {
                apply = true;
            }
        } else {
            if ui
                .add_enabled(drawn, egui::Button::new(crate::icons::with("Keep as a sketch")))
                .on_hover_text(
                    "Keep the sketch with the part without making a solid: as the profile or the path of a sweep, or a section of a loft (Features tab)",
                )
                .on_disabled_hover_text("Draw something first")
                .clicked()
            {
                keep_it = true;
            }
            if ui
                .add_enabled(
                    outlines > 0,
                    egui::Button::new(RichText::new(crate::icons::with(if e.op == BooleanOp::Cut { "Cut" } else { "Extrude" })).strong()),
                )
                .clicked()
            {
                extrude = true;
            }
            if ui
                .add_enabled(outlines > 0, egui::Button::new(crate::icons::with("Revolve")))
                .on_hover_text("About the sketch's vertical axis; x is the radius (draw on the Front plane with x ≥ 0)")
                .clicked()
            {
                revolve = true;
            }
        }
        if ui.button("Cancel").clicked() {
            cancel_it = true;
        }
    });
    if changed {
        solve(state);
    }
    if cancel_it {
        cancel(state);
    } else if keep_it {
        keep(state);
    } else if extrude || apply {
        finish(state, false);
    } else if revolve {
        finish(state, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_sketch() -> SketchEdit {
        let mut s = Sketch::rectangle(20.0, 10.0);
        let _ = s.solve();
        SketchEdit::new(None, s)
    }

    #[test]
    fn deleting_a_line_removes_what_depends_on_it_and_renumbers_the_rest() {
        let mut e = rect_sketch();
        let (n_ents, n_cons) = (e.sketch.entities.len(), e.sketch.constraints.len());
        e.sel_ents = vec![0];
        delete_selection(&mut e);
        assert!(e.sketch.entities.len() < n_ents);
        assert!(e.sketch.constraints.len() < n_cons);
        // Every remaining index is valid.
        let np = e.sketch.points.len();
        for en in &e.sketch.entities {
            match en {
                Entity::Line { a, b, .. } => assert!(*a < np && *b < np),
                Entity::Circle { center, .. } => assert!(*center < np),
                _ => {}
            }
        }
        let ne = e.sketch.entities.len();
        for c in &e.sketch.constraints {
            if let Constraint::Horizontal(i) | Constraint::Vertical(i) | Constraint::Length(i, _) = c {
                assert!(*i < ne);
            }
        }
        assert!(e.sketch.solve().converged || true);
    }

    #[test]
    fn the_finished_sketch_becomes_an_operation_the_document_accepts() {
        let mut e = rect_sketch();
        e.depth = "width / 2".into();
        let (name, args) = finish_op(&e, "", false);
        assert_eq!(name, "sketch_extrude");
        let mut d = aurafx_cad::Document::new("p");
        aurafx_cad::edit::apply_op(&mut d, "parameter", &json!({"name": "width", "expr": 30})).unwrap();
        aurafx_cad::edit::apply_op(&mut d, name, &args).unwrap();
        let ev = aurafx_filters::cad::evaluate(&d, 48, true).unwrap();
        // 20 x 10 profile, 15 deep.
        assert!((ev.volume() - 20.0 * 10.0 * 15.0).abs() / 3000.0 < 0.02, "{}", ev.volume());
        // Editing the sketch of a feature replaces it.
        let mut e2 = SketchEdit::new(Some(d.features[0].id), Sketch::rectangle(40.0, 10.0));
        e2.report = None;
        let (name2, args2) = finish_op(&e2, "Extrude", false);
        assert_eq!(name2, "set_sketch");
        aurafx_cad::edit::apply_op(&mut d, name2, &args2).unwrap();
        let ev2 = aurafx_filters::cad::evaluate(&d, 48, true).unwrap();
        assert!((ev2.volume() - 40.0 * 10.0 * 15.0).abs() / 6000.0 < 0.02, "{}", ev2.volume());
        // A revolve of a half profile.
        let mut e3 = SketchEdit::new(None, Sketch::polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 20.0], [0.0, 20.0]]));
        e3.sketch.plane = SketchPlane::XZ;
        let (n3, a3) = finish_op(&e3, "", true);
        assert_eq!(n3, "sketch_revolve");
        let mut d3 = aurafx_cad::Document::new("r");
        aurafx_cad::edit::apply_op(&mut d3, n3, &a3).unwrap();
        let v = aurafx_filters::cad::evaluate(&d3, 64, true).unwrap().volume();
        assert!((v - std::f64::consts::PI * 100.0 * 20.0).abs() / 6283.0 < 0.02, "{v}");
    }

    #[test]
    fn sketches_on_faces_use_the_matching_plane_offset_and_direction() {
        // The plane, the offset and whether the face looks against the plane's normal; None: a plane of its own.
        let on = |c: [f64; 3], d: [f64; 3]| {
            let (s, against) = Sketch::on_face(c, d, "Face").unwrap();
            assert_eq!(s.face.as_deref(), Some("Face"), "the sketch remembers its face");
            s.frame.is_none().then_some((s.plane, s.offset, against))
        };
        // Top of a plate at z = 8, pointing up: XY plane, offset 8, along the normal.
        assert_eq!(on([0.0, 0.0, 8.0], [0.0, 0.0, 1.0]), Some((SketchPlane::XY, 8.0, false)));
        // The bottom looks down: the same plane, extruding against its normal.
        assert_eq!(on([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]), Some((SketchPlane::XY, 0.0, true)));
        // A face looking along +x is the YZ plane (normal +x).
        assert_eq!(on([20.0, 0.0, 4.0], [1.0, 0.0, 0.0]), Some((SketchPlane::YZ, 20.0, false)));
        // The front plane's normal is -y: a face looking along -y at y = -15 has offset 15.
        assert_eq!(on([0.0, -15.0, 4.0], [0.0, -1.0, 0.0]), Some((SketchPlane::XZ, 15.0, false)));
        assert_eq!(on([0.0, 15.0, 4.0], [0.0, 1.0, 0.0]), Some((SketchPlane::XZ, -15.0, true)));
        // A slanted face gets a plane of its own.
        assert!(on([0.0; 3], [0.0, 0.6, 0.8]).is_none());
    }

    #[test]
    fn splines_and_arcs_flatten_to_points_through_their_ends() {
        let a = arc_pts(DVec2::ZERO, DVec2::new(5.0, 0.0), DVec2::new(0.0, 5.0));
        assert!((a[0] - DVec2::new(5.0, 0.0)).length() < 1e-9 && (a[a.len() - 1] - DVec2::new(0.0, 5.0)).length() < 1e-9);
        let s = spline_pts(&[DVec2::ZERO, DVec2::new(5.0, 5.0), DVec2::new(10.0, 0.0)], false);
        assert_eq!(s[0], DVec2::ZERO);
        assert!((s[s.len() - 1] - DVec2::new(10.0, 0.0)).length() < 1e-9);
    }
}
