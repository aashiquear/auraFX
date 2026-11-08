//! The B-rep kernel (stage C6): the document's features become SolidCraft *commands* (stable
//! dotted ids such as `solid.extrude`, so upstream versions stay cheap to take) run in process
//! on a headless session of the vendored engine, which builds exact solids. What comes back is
//! the exact bodies, tessellated, with the same face *labels* the implicit kernel gives
//! (`Base z+`, `Bore side`): every B-rep face is named by the feature that owns it and its
//! place on that feature, so names, ids and the downstream check mean the same on both kernels.
//!
//! The kernel builds what it can map exactly (primitives, extruded and revolved sketches,
//! holes, patterns by copies, rounding and hollowing the whole part) and says so when it
//! cannot: [`CadError::KernelRefused`] names the feature and the reason, and
//! [`evaluate_with`](super::evaluate_with) with `Auto` then falls back to the implicit kernel.

use super::implicit::Labeller;
use super::sdf::placement_rotation;
use super::*;
use crate::document::{BooleanOp, EdgeTreatment, EndCondition, Feature, HoleKind, Modifier, Shape};
use crate::path::Polyline;
use crate::sketch::{Entity, Sketch};
use glam::{DMat3, DVec2, DVec3};
use serde_json::{json, Value};
use solidcraft_engine::Session;
use std::collections::BTreeMap;

/// The B-rep kernel.
#[derive(Debug, Clone, Default)]
pub struct BrepKernel;

fn refuse<T>(what: impl Into<String>) -> Result<T, CadError> {
    Err(CadError::KernelRefused("brep", what.into()))
}

fn v3(v: DVec3) -> Value {
    json!([v.x, v.y, v.z])
}

impl Build {
    fn new() -> Self {
        Build { s: Session::default(), sketches: 0, any: false, main: None, tmp: None }
    }
}

/// Open an imported file's bytes as the design of the session.
fn open_import(b: &mut Build, format: &str, data: &str) -> Result<(), CadError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| CadError::Kernel { kernel: "brep", reason: format!("the imported data is not valid: {e}") })?;
    let ext = match format {
        "step" | "stp" => "step",
        "igs" | "iges" => "igs",
        "3mf" | "sc" | "stl" | "obj" => format,
        other => return refuse(format!("imported data of type '{other}'")),
    };
    let dir = std::env::temp_dir().join(format!("aurafx-import-{}-{}", std::process::id(), IMPORT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| CadError::Kernel { kernel: "brep", reason: e.to_string() })?;
    let path = dir.join(format!("import.{ext}"));
    std::fs::write(&path, &bytes).map_err(|e| CadError::Kernel { kernel: "brep", reason: e.to_string() })?;
    let r = b.run("doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    r.map(|_| ())
}

static IMPORT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A feature's placement: where its local origin is and how its axes turn.
#[derive(Clone, Copy)]
struct Pl {
    t: DVec3,
    r: DMat3,
}

impl Pl {
    fn at(&self, local: DVec3) -> DVec3 {
        self.t + self.r * local
    }
    fn axis(&self) -> DVec3 {
        self.r * DVec3::Z
    }
    fn is_upright(&self) -> bool {
        (self.axis() - DVec3::Z).length() < 1e-9 && (self.r * DVec3::X - DVec3::X).length() < 1e-9
    }
}

fn rot_about(axis: crate::document::Axis, deg: f64) -> DMat3 {
    let a = deg.to_radians();
    match axis {
        crate::document::Axis::X => DMat3::from_rotation_x(a),
        crate::document::Axis::Y => DMat3::from_rotation_y(a),
        crate::document::Axis::Z => DMat3::from_rotation_z(a),
    }
}

/// The placements a feature takes once its patterns are expanded into copies.
fn placements(f: &Feature) -> Result<(Vec<Pl>, Vec<Modifier>), CadError> {
    let r0 = placement_rotation(f.placement.rotate_deg);
    let t0 = DVec3::from_array(f.placement.translate);
    // Local offsets and orientations, then the placement.
    let mut local: Vec<(DVec3, DMat3)> = vec![(DVec3::ZERO, DMat3::IDENTITY)];
    let mut body_mods: Vec<Modifier> = Vec::new();
    let mut patterned = false;
    for m in f.modifiers.iter().filter(|m| !m.is_world()) {
        local = match m {
            Modifier::LinearPattern { count, step } => {
                patterned = true;
                let s = DVec3::from_array(*step);
                local.iter().flat_map(|(o, q)| (0..(*count).max(1)).map(move |k| (*o + s * k as f64, *q))).collect()
            }
            Modifier::GridPattern { counts, step } => {
                patterned = true;
                local
                    .iter()
                    .flat_map(|(o, q)| {
                        (0..counts[0].max(1))
                            .flat_map(move |i| (0..counts[1].max(1)).map(move |j| (*o + DVec3::new(step[0] * i as f64, step[1] * j as f64, 0.0), *q)))
                    })
                    .collect()
            }
            Modifier::CircularPattern { count, axis_z: true } => {
                patterned = true;
                let n = (*count).max(1);
                local
                    .iter()
                    .flat_map(|(o, q)| {
                        (0..n).map(move |k| {
                            let w = DMat3::from_rotation_z(std::f64::consts::TAU * k as f64 / n as f64);
                            (w * *o, w * *q)
                        })
                    })
                    .collect()
            }
            // A mirror of a shape centered on its own origin is the shape itself: nothing to add.
            Modifier::Mirror { .. } => local,
            // Rounding and hollowing act on the feature's own body, before it joins the part.
            Modifier::Fillet { .. } | Modifier::Shell { .. } | Modifier::ShellOpen { .. } => {
                body_mods.push(m.clone());
                local
            }
            other => return refuse(format!("'{}' has a {} modifier, which the B-rep kernel does not build", f.name, modifier_name(other))),
        };
    }
    if patterned && !body_mods.is_empty() {
        return refuse(format!("'{}' rounds or hollows a pattern of copies (do it on the whole part)", f.name));
    }
    let mut out: Vec<Pl> = local.into_iter().map(|(o, q)| Pl { t: t0 + r0 * o, r: r0 * q }).collect();
    for m in f.modifiers.iter().filter(|m| m.is_world()) {
        if let Modifier::CircularPatternAngle { count, total_deg, axis } = m {
            let n = (*count).max(1);
            let step = if (*total_deg - 360.0).abs() < 1e-9 { *total_deg / n as f64 } else { *total_deg / (n.max(2) - 1) as f64 };
            out = out
                .iter()
                .flat_map(|p| {
                    (0..n).map(move |k| {
                        let w = rot_about(*axis, step * k as f64);
                        Pl { t: w * p.t, r: w * p.r }
                    })
                })
                .collect();
        }
    }
    Ok((out, body_mods))
}

fn modifier_name(m: &Modifier) -> &'static str {
    match m {
        Modifier::Fillet { .. } => "fillet",
        Modifier::Shell { .. } => "shell",
        Modifier::ShellOpen { .. } => "open shell",
        Modifier::Split { .. } => "split",
        Modifier::Scale { .. } => "scale",
        Modifier::LinearPattern { .. } => "linear pattern",
        Modifier::GridPattern { .. } => "grid pattern",
        Modifier::CircularPattern { .. } | Modifier::CircularPatternAngle { .. } => "circular pattern",
        Modifier::Mirror { .. } => "mirror",
    }
}

/// A sketch plane as SolidCraft takes it: an origin and two axes in world coordinates.
struct Frame {
    o: DVec3,
    x: DVec3,
    y: DVec3,
}

impl Frame {
    fn json(&self) -> Value {
        json!({"origin": v3(self.o), "x_dir": v3(self.x), "y_dir": v3(self.y)})
    }
}

struct Build {
    s: Session,
    sketches: usize,
    /// Whether a body exists yet (the first adding feature makes one).
    any: bool,
    /// The name of the part's main body, once there is one (imported solids live in a component of
    /// their own, so features that join or cut say which body they mean).
    main: Option<String>,
    /// While a feature is built on its own (to round or hollow it before it joins the part): the
    /// name of its temporary body.
    tmp: Option<String>,
}

impl Build {
    fn run(&mut self, id: &str, p: Value) -> Result<Value, CadError> {
        self.s.execute(id, &p).map_err(|e| CadError::Kernel { kernel: "brep", reason: format!("{id}: {e}") })
    }

    /// A command that makes a solid: into the temporary body while one is being built alone.
    fn solid(&mut self, id: &str, mut p: Value) -> Result<Value, CadError> {
        if let Some(t) = self.tmp.clone() {
            p["body_name"] = json!(t);
        } else if self.any && p.get("operation").and_then(|o| o.as_str()).is_some_and(|o| o != "new") {
            if let Some(m) = self.main_body() {
                p["targets"] = json!([m]);
            }
        }
        self.run(id, p)
    }

    /// The name of the first body that is not a feature in the making.
    fn main_body(&mut self) -> Option<String> {
        if self.main.is_none() {
            let st = self.s.world_state().ok()?;
            self.main = st.bodies.iter().find(|b| b.name != TMP).map(|b| b.name.clone());
        }
        self.main.clone()
    }

    fn operation(&mut self, op: BooleanOp) -> Option<&'static str> {
        if self.tmp.is_some() {
            return Some("new");
        }
        match (self.any, op) {
            (false, BooleanOp::Add) => Some("new"),
            (false, _) => None, // cutting from nothing is a no-op, as on the implicit kernel
            (true, BooleanOp::Add) => Some("join"),
            (true, BooleanOp::Cut) => Some("cut"),
            (true, BooleanOp::Intersect) => Some("intersect"),
        }
    }

    fn draw_sketch(&mut self, frame: &Frame, shape: &SketchShape) -> Result<String, CadError> {
        self.sketches += 1;
        let name = format!("AfxSketch{}", self.sketches);
        self.run("sketch.create", json!({"plane": frame.json(), "name": name}))?;
        match shape {
            SketchShape::Sketch(sk) => {
                let p = |i: usize| sk.point(i);
                let xy = |q: DVec2| json!([q.x, q.y]);
                for e in sk.entities.iter().filter(|e| !e.construction()) {
                    match e {
                        Entity::Line { a, b, .. } => {
                            self.run("sketch.line", json!({"points": [xy(p(*a)), xy(p(*b))]}))?;
                        }
                        Entity::Circle { center, radius, .. } => {
                            self.run("sketch.circle.center", json!({"center": xy(p(*center)), "radius": radius}))?;
                        }
                        Entity::Arc { center, start, end, .. } => {
                            self.run("sketch.arc.center_point", json!({"center": xy(p(*center)), "start": xy(p(*start)), "end": xy(p(*end))}))?;
                        }
                        Entity::Spline { .. } | Entity::Polyline { .. } => {
                            // A free curve goes in as the polyline of its outline.
                        }
                    }
                }
                for (i, path) in sk.entity_paths().iter().enumerate() {
                    let free =
                        sk.entities.iter().filter(|e| !e.construction()).nth(i).is_some_and(|e| matches!(e, Entity::Spline { .. } | Entity::Polyline { .. }));
                    if free {
                        self.polyline(path)?;
                    }
                }
            }
            SketchShape::Loops(loops) => {
                for l in loops.iter() {
                    self.polyline(l)?;
                }
            }
        }
        self.run("sketch.finish", json!({}))?;
        Ok(name)
    }

    fn polyline(&mut self, l: &Polyline) -> Result<(), CadError> {
        let pts: Vec<Value> = l.pts.iter().map(|q| json!([q.x, q.y])).collect();
        self.run("sketch.line", json!({"points": pts, "closed": l.closed}))?;
        Ok(())
    }
}

enum SketchShape<'a> {
    Sketch(&'a Sketch),
    Loops(&'a [Polyline]),
}

fn extrude_args(sketch: &str, op: &str, distance: f64, direction: &str, taper: f64) -> Value {
    let mut a = json!({"sketch": sketch, "distance": distance, "direction": direction, "operation": op});
    if taper.abs() > 1e-12 {
        a["taper"] = json!(format!("{taper} deg"));
    }
    a
}

/// Build one feature (at one placement) into the session.
fn emit(b: &mut Build, f: &Feature, pl: Pl, name: &str) -> Result<(), CadError> {
    // Edge and shell features act on the part so far.
    match &f.shape {
        Shape::EdgeBlend { chamfer, size, edges } => {
            let Some(main) = b.main_body().filter(|_| b.any) else {
                return refuse(format!("'{}' rounds edges of nothing yet: put it after the feature it modifies", f.name));
            };
            let picks: Vec<Value> = edges.iter().map(|p| json!(p)).collect();
            if picks.is_empty() {
                return refuse(format!("'{}' has no edges picked", f.name));
            }
            let (id, key) = if *chamfer { ("solid.chamfer", "distance") } else { ("solid.fillet", "radius") };
            b.run(id, json!({"edges": picks, key: size.abs(), "tangent_chain": false, "body": main}))?;
            return Ok(());
        }
        Shape::ShellFaces { thickness, faces } => {
            let Some(main) = b.main_body().filter(|_| b.any) else {
                return refuse(format!("'{}' hollows nothing yet: put it after the feature it modifies", f.name));
            };
            b.run(
                "solid.shell",
                json!({"faces": faces.iter().map(|p| json!(p)).collect::<Vec<_>>(), "thickness": thickness.abs(), "direction": "inside", "body": main}),
            )?;
            return Ok(());
        }
        _ => {}
    }
    let Some(op) = b.operation(f.op) else { return Ok(()) };
    match &f.shape {
        Shape::Box { size, .. } => {
            let (sx, sy, sz) = (size[0], size[1], size[2]);
            if pl.is_upright() {
                b.solid("solid.box", json!({"length": sx, "width": sy, "height": sz, "center": v3(pl.t), "operation": op, "name": name}))?;
            } else {
                let frame = Frame { o: pl.at(DVec3::new(0.0, 0.0, -sz / 2.0)), x: pl.r * DVec3::X, y: pl.r * DVec3::Y };
                let sk = Sketch::polygon(&[[-sx / 2.0, -sy / 2.0], [sx / 2.0, -sy / 2.0], [sx / 2.0, sy / 2.0], [-sx / 2.0, sy / 2.0]]);
                let s = b.draw_sketch(&frame, &SketchShape::Sketch(&sk))?;
                b.solid("solid.extrude", {
                    let mut a = extrude_args(&s, op, sz, "positive", 0.0);
                    a["name"] = json!(name);
                    a
                })?;
            }
        }
        Shape::Sphere { radius } => {
            b.solid("solid.sphere", json!({"center": v3(pl.t), "radius": radius, "operation": op, "name": name}))?;
        }
        Shape::Cylinder { radius, height } => {
            b.solid("solid.cylinder", json!({"base": v3(pl.t), "axis": v3(pl.axis()), "radius": radius, "height": height, "operation": op, "name": name}))?;
        }
        Shape::Cone { r_bottom, r_top, height } => {
            b.solid(
                "solid.cone",
                json!({"base": v3(pl.t), "axis": v3(pl.axis()), "radius": r_bottom, "top_radius": r_top, "height": height, "operation": op, "name": name}),
            )?;
        }
        Shape::Torus { major, minor } => {
            if !pl.is_upright() {
                return refuse(format!("'{}' is a tilted torus", f.name));
            }
            b.solid("solid.torus", json!({"center": v3(pl.t), "major": major, "minor": minor, "operation": op, "name": name}))?;
        }
        Shape::Hole { kind, diameter, depth } => hole(b, f, pl, op, name, kind, *diameter, *depth)?,
        Shape::SketchExtrude { sketch, depth, end, draft_deg, top_edge, bottom_edge, reverse } => {
            let _ = (top_edge, bottom_edge);
            let (basis, org) = sketch.plane.frame(sketch.offset);
            let frame = Frame { o: pl.at(org), x: pl.r * basis.x_axis, y: pl.r * basis.y_axis };
            let s = b.draw_sketch(&frame, &SketchShape::Sketch(sketch))?;
            let d = depth.abs();
            let (dist, dir, taper) = match end {
                EndCondition::Blind => (d, if *reverse { "negative" } else { "positive" }, *draft_deg),
                EndCondition::MidPlane => (d, "symmetric", *draft_deg),
                EndCondition::ThroughAll => (2.0 * super::sdf::THROUGH_ALL, "symmetric", 0.0),
            };
            let mut a = extrude_args(&s, op, dist, dir, taper);
            a["name"] = json!(name);
            b.solid("solid.extrude", a)?;
        }
        Shape::SketchRevolve { sketch, angle_deg } => {
            let (basis, org) = sketch.plane.frame(sketch.offset);
            let frame = Frame { o: pl.at(org), x: pl.r * basis.x_axis, y: pl.r * basis.y_axis };
            let s = b.draw_sketch(&frame, &SketchShape::Sketch(sketch))?;
            b.solid(
                "solid.revolve",
                json!({"sketch": s, "axis": "y", "angle": format!("{} deg", angle_deg.clamp(0.1, 360.0)), "operation": op, "name": name}),
            )?;
        }
        Shape::Extrude { sketch, height, draft_deg } => {
            let frame = Frame { o: pl.t, x: pl.r * DVec3::X, y: pl.r * DVec3::Y };
            let s = b.draw_sketch(&frame, &SketchShape::Loops(sketch))?;
            let mut a = extrude_args(&s, op, *height, "positive", *draft_deg);
            a["name"] = json!(name);
            b.solid("solid.extrude", a)?;
        }
        Shape::Revolve { .. } => return refuse(format!("'{}' is a legacy revolved outline (draw it as a sketch)", f.name)),
        Shape::Brep { format, data, .. } => {
            if b.any {
                return refuse(format!("'{}' is an imported solid; it must be the first feature", f.name));
            }
            if !pl.is_upright() || pl.t.length() > 1e-12 {
                return refuse(format!("'{}' is an imported solid with a placement (an import keeps its own origin)", f.name));
            }
            open_import(b, format, data)?;
            b.main = None;
        }
        Shape::Thread { .. } => return refuse(format!("'{}' is a modelled thread", f.name)),
        Shape::Sweep { .. } | Shape::Loft { .. } => return refuse(format!("'{}' is a sweep or loft, which the B-rep kernel does not map yet", f.name)),
        Shape::Body { .. } => return refuse(format!("'{}' combines another part", f.name)),
        Shape::EdgeBlend { .. } | Shape::ShellFaces { .. } => unreachable!("handled before the match"),
    }
    b.any = true;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn hole(b: &mut Build, f: &Feature, pl: Pl, op: &str, name: &str, kind: &HoleKind, diameter: f64, depth: f64) -> Result<(), CadError> {
    let r = diameter.abs() * 0.5;
    let depth = depth.abs();
    let up = pl.axis();
    let cyl = |b: &mut Build, label: &str, radius: f64, z0: f64, z1: f64| {
        b.solid("solid.cylinder", json!({"base": v3(pl.at(DVec3::new(0.0, 0.0, z0))), "axis": v3(up), "radius": radius, "height": z1 - z0, "operation": op, "name": format!("{name} {label}")}))
    };
    let top = |radius: f64| radius + 1.0;
    match kind {
        HoleKind::Simple => {
            cyl(b, "bore", r, -depth, top(r))?;
        }
        HoleKind::Drilled => {
            let tip = r / 59f64.to_radians().tan();
            cyl(b, "bore", r, -depth, top(r))?;
            b.solid("solid.cone", json!({"base": v3(pl.at(DVec3::new(0.0, 0.0, -depth))), "axis": v3(-up), "radius": r, "top_radius": 0.0, "height": tip, "operation": op, "name": format!("{name} point")}))?;
        }
        HoleKind::Counterbore { diameter: cd, depth: cdepth } => {
            let rc = (cd * 0.5).max(r);
            cyl(b, "bore", r, -depth, top(rc))?;
            cyl(b, "counterbore", rc, -cdepth.abs(), top(rc))?;
        }
        HoleKind::Countersink { diameter: cd, angle_deg } => {
            let rc = (cd * 0.5).max(r);
            let tan = (angle_deg * 0.5).clamp(1.0, 89.0).to_radians().tan();
            let h = (rc - r) / tan;
            cyl(b, "bore", r, -depth, top(rc))?;
            // The sink cone keeps widening one millimetre above the face (only air there).
            b.solid("solid.cone", json!({"base": v3(pl.at(DVec3::new(0.0, 0.0, 1.0))), "axis": v3(-up), "radius": rc + tan, "top_radius": r, "height": h + 1.0, "operation": op, "name": format!("{name} sink")}))?;
        }
        HoleKind::Tapped { pitch, cosmetic } => {
            if !cosmetic {
                return refuse(format!("'{}' is a hole with a modelled thread (make the thread cosmetic)", f.name));
            }
            let minor = (r - 0.5413 * pitch.abs().max(1e-3)).max(r * 0.3);
            cyl(b, "bore", minor, -depth, top(minor))?;
        }
    }
    Ok(())
}

const TMP: &str = "AfxFeature";

/// The edges of a body (their midpoints) that satisfy `keep`.
fn edge_picks(b: &mut Build, body: &str, keep: &dyn Fn(&[DVec3]) -> bool) -> Result<Vec<Value>, CadError> {
    let st = b.s.world_state().map_err(|e| CadError::Kernel { kernel: "brep", reason: e.to_string() })?;
    let mut picks = Vec::new();
    for bd in st.bodies.iter().filter(|x| x.name == body) {
        for e in bd.mesh().edges.iter().filter(|e| e.len() >= 2) {
            let pts: Vec<DVec3> = e.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect();
            if keep(&pts) {
                picks.push(v3(edge_mid(&pts)));
            }
        }
    }
    Ok(picks)
}

/// A point along an edge's polyline, away from its ends (two edges meeting at a corner would be
/// ambiguous there): the middle of a straight edge, the point halfway along a curved one.
fn edge_mid(pts: &[DVec3]) -> DVec3 {
    if pts.len() <= 2 {
        return (pts[0] + pts[pts.len() - 1]) * 0.5;
    }
    let total: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut run = 0.0;
    for w in pts.windows(2) {
        let l = w[0].distance(w[1]);
        if run + l >= total * 0.5 {
            return w[0].lerp(w[1], (total * 0.5 - run) / l.max(1e-300));
        }
        run += l;
    }
    pts[pts.len() / 2]
}

fn needs_own_body(f: &Feature, body_mods: &[Modifier]) -> bool {
    !body_mods.is_empty()
        || matches!(&f.shape, Shape::Box { corner_radius, .. } if *corner_radius > 0.0)
        || matches!(&f.shape, Shape::SketchExtrude { top_edge, bottom_edge, .. } if !matches!(top_edge, EdgeTreatment::None) || !matches!(bottom_edge, EdgeTreatment::None))
}

/// One feature at one placement: built straight into the part, or, when it is rounded or hollowed
/// first, as a body of its own that is then joined, cut or intersected.
fn emit_feature(b: &mut Build, f: &Feature, pl: Pl, name: &str, body_mods: &[Modifier]) -> Result<(), CadError> {
    if !needs_own_body(f, body_mods) {
        return emit(b, f, pl, name);
    }
    if b.operation(f.op).is_none() {
        return Ok(());
    }
    let had_part = b.any;
    b.tmp = Some(TMP.into());
    let made = emit(b, f, pl, name);
    b.tmp = None;
    made?;
    let kerr = |e: String| CadError::Kernel { kernel: "brep", reason: e };
    // Corner radius of a box, and the treatments of an extrusion's caps.
    if let Shape::Box { corner_radius, .. } = &f.shape {
        if *corner_radius > 0.0 {
            let picks = edge_picks(b, TMP, &|_| true)?;
            b.run("solid.fillet", json!({"edges": picks, "radius": corner_radius, "tangent_chain": false, "body": TMP}))?;
        }
    }
    if let Shape::SketchExtrude { sketch, depth, end, reverse, top_edge, bottom_edge, .. } = &f.shape {
        let (basis, org) = sketch.plane.frame(sketch.offset);
        let (n, o) = (pl.r * (basis * DVec3::Z), pl.at(org));
        let d = depth.abs();
        let (z0, z1) = match end {
            EndCondition::Blind if *reverse => (-d, 0.0),
            EndCondition::Blind => (0.0, d),
            EndCondition::MidPlane => (-d / 2.0, d / 2.0),
            EndCondition::ThroughAll => (-super::sdf::THROUGH_ALL, super::sdf::THROUGH_ALL),
        };
        for (edge, z) in [(top_edge, z1), (bottom_edge, z0)] {
            if matches!(edge, EdgeTreatment::None) {
                continue;
            }
            let picks = edge_picks(b, TMP, &|pts| pts.iter().all(|p| (n.dot(*p - o) - z).abs() < 1e-6))?;
            if picks.is_empty() {
                continue;
            }
            match edge {
                EdgeTreatment::Fillet { radius } => b.run("solid.fillet", json!({"edges": picks, "radius": radius, "tangent_chain": false, "body": TMP}))?,
                EdgeTreatment::Chamfer { distance } => {
                    b.run("solid.chamfer", json!({"edges": picks, "distance": distance, "tangent_chain": false, "body": TMP}))?
                }
                EdgeTreatment::None => Value::Null,
            };
        }
    }
    for m in body_mods {
        match m {
            Modifier::Fillet { radius } => {
                let picks = edge_picks(b, TMP, &|_| true)?;
                b.run("solid.fillet", json!({"edges": picks, "radius": radius, "tangent_chain": false, "body": TMP}))?;
            }
            Modifier::Shell { thickness } => {
                b.run("solid.shell", json!({"faces": [], "thickness": thickness.abs(), "direction": "inside", "body": TMP}))?;
            }
            Modifier::ShellOpen { thickness, open_top } => {
                let pick = if *open_top { top_point(b, Some(TMP))? } else { None };
                b.run(
                    "solid.shell",
                    json!({"faces": pick.map(|p| vec![v3(p)]).unwrap_or_default(), "thickness": thickness.abs(), "direction": "inside", "body": TMP}),
                )?;
            }
            _ => {}
        }
    }
    if had_part {
        let st = b.s.world_state().map_err(|e| kerr(e.to_string()))?;
        let main = st.bodies.iter().find(|x| x.name != TMP).map(|x| x.name.clone()).ok_or_else(|| kerr("the part has no body to join".into()))?;
        let op = match f.op {
            BooleanOp::Add => "join",
            BooleanOp::Cut => "cut",
            BooleanOp::Intersect => "intersect",
        };
        b.run("solid.combine", json!({"target": main, "tools": [TMP], "operation": op}))?;
    } else {
        b.run("body.rename", json!({"body": TMP, "name": "Part"}))?;
    }
    b.any = true;
    Ok(())
}

/// Model-wide modifiers (the finish): rounding every edge, hollowing.
fn finish(b: &mut Build, doc: &Document) -> Result<(), CadError> {
    for m in &doc.finish {
        match m {
            Modifier::Fillet { radius } => {
                let st = b.s.world_state().map_err(|e| CadError::Kernel { kernel: "brep", reason: e.to_string() })?;
                let mut picks: Vec<Value> = Vec::new();
                for body in &st.bodies {
                    for e in body.mesh().edges.iter().filter(|e| e.len() >= 2) {
                        let pts: Vec<DVec3> = e.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect();
                        picks.push(v3(edge_mid(&pts)));
                    }
                }
                if picks.is_empty() {
                    return refuse("the part has no edges to round");
                }
                b.run("solid.fillet", json!({"edges": picks, "radius": radius, "tangent_chain": false}))?;
            }
            Modifier::Shell { thickness } => {
                b.run("solid.shell", json!({"faces": [], "thickness": thickness.abs(), "direction": "inside"}))?;
            }
            Modifier::ShellOpen { thickness, open_top } => {
                let pick = if *open_top { top_point(b, None)? } else { None };
                b.run("solid.shell", json!({"faces": pick.map(|p| vec![v3(p)]).unwrap_or_default(), "thickness": thickness.abs(), "direction": "inside"}))?;
            }
            other => return refuse(format!("the {} of the whole part", modifier_name(other))),
        }
    }
    Ok(())
}

/// A point on the largest upward-facing triangle at the top of the part.
fn top_point(b: &mut Build, only: Option<&str>) -> Result<Option<DVec3>, CadError> {
    let st = b.s.world_state().map_err(|e| CadError::Kernel { kernel: "brep", reason: e.to_string() })?;
    let mut best: Option<(f64, DVec3)> = None;
    let mut zmax = f64::NEG_INFINITY;
    for body in st.bodies.iter().filter(|x| only.is_none_or(|o| x.name == o)) {
        for p in &body.mesh().positions {
            zmax = zmax.max(p.z);
        }
    }
    for body in st.bodies.iter().filter(|x| only.is_none_or(|o| x.name == o)) {
        let m = body.mesh();
        for t in &m.triangles {
            let q: Vec<DVec3> = t.iter().map(|i| DVec3::new(m.positions[*i as usize].x, m.positions[*i as usize].y, m.positions[*i as usize].z)).collect();
            let n = (q[1] - q[0]).cross(q[2] - q[0]);
            let area = n.length() * 0.5;
            let c = (q[0] + q[1] + q[2]) / 3.0;
            if area > 0.0 && (n.z / n.length()) > 0.99 && (c.z - zmax).abs() < 1e-6 && best.is_none_or(|(a, _)| area > a) {
                best = Some((area, c));
            }
        }
    }
    Ok(best.map(|(_, c)| c))
}

fn unique_names(doc: &Document) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    doc.features
        .iter()
        .map(|f| {
            let n = seen.entry(f.name.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                f.name.clone()
            } else {
                format!("{} {}", f.name, n)
            }
        })
        .collect()
}

impl Kernel for BrepKernel {
    fn name(&self) -> &'static str {
        "brep"
    }

    fn evaluate(&self, document: &Document) -> Result<Evaluated, CadError> {
        let (mut b, doc) = build_part(document)?;
        extract(&mut b, &doc)
    }
}

/// Run every feature of the document (resolved) and its finishing modifiers in a headless session.
fn build_part(document: &Document) -> Result<(Build, Document), CadError> {
    let doc = document.resolved()?;
    let mut b = Build::new();
    let names = unique_names(&doc);
    for (idx, f) in doc.features.iter().enumerate().filter(|(_, f)| !f.suppressed) {
        if f.blend > 0.0 {
            return refuse(format!("'{}' blends with the body (round the junction with a fillet on the part)", f.name));
        }
        let (pls, body_mods) = placements(f)?;
        for (k, pl) in pls.into_iter().enumerate() {
            emit_feature(&mut b, f, pl, &format!("{}{}", names[idx], if k == 0 { String::new() } else { format!(" copy {k}") }), &body_mods)?;
        }
    }
    if !b.any {
        return Err(CadError::Kernel { kernel: "brep", reason: "the document has no solid yet: add a feature that adds material".into() });
    }
    finish(&mut b, &doc)?;
    Ok((b, doc))
}

/// One edge of the exact part, for picking: a point on it (`mid`), its length, and the names of
/// the faces on either side.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeInfo {
    pub index: usize,
    pub mid: [f64; 3],
    pub length: f64,
    pub faces: Vec<String>,
    pub straight: bool,
}

/// The edges of the part the B-rep kernel builds, with the names of their faces.
pub fn list_edges(document: &Document) -> Result<Vec<EdgeInfo>, CadError> {
    let (b, doc) = build_part(document)?;
    let labeller = Labeller::new(&doc);
    let st = b.s.world_state().map_err(|e| CadError::Kernel { kernel: "brep", reason: e.to_string() })?;
    let mut out = Vec::new();
    for body in st.bodies.iter() {
        let m = body.mesh();
        let pts: Vec<DVec3> = m.positions.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect();
        // The label of every B-rep face, by vote of its triangles' areas.
        let mut votes: BTreeMap<u32, BTreeMap<(usize, usize), f64>> = BTreeMap::new();
        for (ti, t) in m.triangles.iter().enumerate() {
            let (a, c, d) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let n = (c - a).cross(d - a);
            let area = 0.5 * n.length();
            if area <= 0.0 {
                continue;
            }
            let lab = labeller.label((a + c + d) / 3.0, n.normalize());
            *votes.entry(m.tri_face.get(ti).copied().unwrap_or(0)).or_default().entry(lab).or_insert(0.0) += area;
        }
        let face_name = |f: u32| -> Option<String> {
            let lab = votes.get(&f)?.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, _)| *k)?;
            Some(labeller.describe(lab.0, lab.1).0)
        };
        for (i, e) in m.edges.iter().enumerate().filter(|(_, e)| e.len() >= 2) {
            let poly: Vec<DVec3> = e.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect();
            let length: f64 = poly.windows(2).map(|w| w[0].distance(w[1])).sum();
            let mut faces: Vec<String> = m.edge_faces.get(i).map(|fs| fs.iter().filter_map(|f| face_name(*f)).collect()).unwrap_or_default();
            faces.sort();
            faces.dedup();
            let straight = poly.len() == 2 || (edge_mid(&poly) - (poly[0] + poly[poly.len() - 1]) * 0.5).length() < 1e-6 * length.max(1.0);
            out.push(EdgeInfo { index: out.len(), mid: edge_mid(&poly).to_array(), length, faces, straight });
        }
    }
    Ok(out)
}

/// The tessellation of an imported shape as a closed surface (for the implicit kernel's field).
pub fn tessellate_shape(shape: &Shape) -> Option<aurafx_mesh::TriMesh> {
    let Shape::Brep { format, data, .. } = shape else { return None };
    let mut b = Build::new();
    open_import(&mut b, format, data).ok()?;
    let st = b.s.world_state().ok()?;
    let (mut pts, mut tris): (Vec<[f64; 3]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new());
    for body in &st.bodies {
        let m = body.mesh();
        let base = pts.len() as u32;
        pts.extend(m.positions.iter().map(|p| [p.x, p.y, p.z]));
        tris.extend(m.triangles.iter().map(|t| t.map(|i| i + base)));
    }
    aurafx_mesh::TriMesh::new(&pts, &tris).ok()
}

/// Build the document on the B-rep kernel and write it with the engine's own writer: STEP
/// (`step`), 3MF (`3mf`), STL (`stl`), OBJ (`obj`) by the path's extension, or a SolidCraft design
/// (`sc`). Returns the number of bytes written.
pub fn export_exact(document: &Document, path: &std::path::Path) -> Result<u64, CadError> {
    let (mut b, _doc) = build_part(document)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let r = match ext.as_str() {
        "sc" => b.run("file.save_as", json!({"path": path.to_string_lossy()}))?,
        "step" | "stp" => b.run("file.export", json!({"path": path.to_string_lossy(), "format": "step"}))?,
        "3mf" => b.run("file.export", json!({"path": path.to_string_lossy(), "format": "3mf"}))?,
        "stl" | "obj" => b.run("file.export", json!({"path": path.to_string_lossy(), "format": ext}))?,
        other => return refuse(format!("writing '.{other}' files")),
    };
    Ok(r.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0))
}

/// Read the exact bodies back: tessellation, labelled faces, volumes, a distance function.
fn extract(b: &mut Build, doc: &Document) -> Result<Evaluated, CadError> {
    let kerr = |e: String| CadError::Kernel { kernel: "brep", reason: e };
    let st = b.s.world_state().map_err(|e| kerr(e.to_string()))?;
    // Exact volumes and areas per body.
    let measured = b.s.execute("inspect.measure", &json!({})).ok();
    let exact = |name: &str| -> Option<(f64, f64)> {
        let m = measured.as_ref()?;
        let list = m.get("bodies").and_then(|v| v.as_array())?;
        let e = list.iter().find(|x| x.get("name").and_then(|n| n.as_str()) == Some(name))?;
        Some((e.get("volume_mm3").and_then(|v| v.as_f64())?, e.get("area_mm2").and_then(|v| v.as_f64())?))
    };
    let labeller = Labeller::new(doc);
    let mut meshes: Vec<BodyMesh> = Vec::new();
    let mut bodies: Vec<EvaluatedBody> = Vec::new();
    let mut face_names: Vec<String> = Vec::new();
    let (mut volumes, mut areas) = (Vec::new(), Vec::new());
    let mut all_pts: Vec<[f64; 3]> = Vec::new();
    let mut all_tris: Vec<[u32; 3]> = Vec::new();
    for (bi, body) in st.bodies.iter().enumerate() {
        let m = body.mesh();
        let pts: Vec<DVec3> = m.positions.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect();
        let tris: Vec<[u32; 3]> = m.triangles.clone();
        // Vote the label of every B-rep face from its triangles, weighted by area.
        let mut votes: BTreeMap<u32, BTreeMap<(usize, usize), f64>> = BTreeMap::new();
        let mut tri_label: Vec<(usize, usize)> = Vec::with_capacity(tris.len());
        let mut tri_info: Vec<(f64, DVec3, DVec3)> = Vec::with_capacity(tris.len());
        for (ti, t) in tris.iter().enumerate() {
            let (a, c, d) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let n = (c - a).cross(d - a);
            let area = 0.5 * n.length();
            let cen = (a + c + d) / 3.0;
            let nn = if n.length() > 0.0 { n.normalize() } else { DVec3::Z };
            let face = m.tri_face.get(ti).copied().unwrap_or(0);
            let lab = labeller.label_in(cen, nn, face);
            *votes.entry(face).or_default().entry(lab).or_insert(0.0) += area;
            tri_label.push(lab);
            tri_info.push((area, cen, nn));
        }
        let winner: BTreeMap<u32, (usize, usize)> =
            votes.iter().map(|(f, v)| (*f, v.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, _)| *k).unwrap_or((0, 0)))).collect();
        let labels: Vec<(usize, usize)> =
            (0..tris.len()).map(|ti| winner.get(&m.tri_face.get(ti).copied().unwrap_or(0)).copied().unwrap_or(tri_label[ti])).collect();
        // Global face numbers in (feature, local face) order, across bodies.
        let mut keys: Vec<(usize, usize)> = labels.clone();
        keys.sort_unstable();
        keys.dedup();
        let base_id = face_names.len() as u32;
        let key_id: BTreeMap<(usize, usize), u32> = keys.iter().enumerate().map(|(i, k)| (*k, base_id + i as u32)).collect();
        let mut faces: Vec<EvaluatedFace> = keys
            .iter()
            .map(|&(fi, li)| {
                let (name, kind, fid) = labeller.describe(fi, li);
                EvaluatedFace { id: (fid, li as u32), face_id: key_id[&(fi, li)], name, kind, area: 0.0, centroid: [0.0; 3], direction: [0.0; 3] }
            })
            .collect();
        let mut acc = vec![(0.0, DVec3::ZERO, DVec3::ZERO); faces.len()];
        let (mut vol, mut area_sum, mut cen_v) = (0.0, 0.0, DVec3::ZERO);
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for ((t, lab), (area, cen, nn)) in tris.iter().zip(&labels).zip(&tri_info) {
            let (a, c, d) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let v6 = a.dot(c.cross(d));
            vol += v6 / 6.0;
            cen_v += (a + c + d) * v6 / 24.0;
            area_sum += area;
            let idx = (key_id[lab] - base_id) as usize;
            acc[idx].0 += area;
            acc[idx].1 += *cen * *area;
            acc[idx].2 += *nn * *area;
            for p in [a, c, d] {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        for (fc, (ar, wc, wn)) in faces.iter_mut().zip(&acc) {
            fc.area = *ar;
            if *ar > 0.0 {
                fc.centroid = (*wc / *ar).to_array();
            }
            if wn.length() > 0.0 {
                fc.direction = wn.normalize().to_array();
            }
        }
        let (ev, ea) = exact(&body.name).unwrap_or((vol, area_sum));
        for f in &faces {
            face_names.push(f.name.clone());
        }
        let base = all_pts.len() as u32;
        all_pts.extend(pts.iter().map(|p| p.to_array()));
        all_tris.extend(tris.iter().map(|t| t.map(|i| i + base)));
        meshes.push(BodyMesh {
            id: Id(bi as u64 + 1),
            name: body.name.clone(),
            points: pts.iter().map(|p| p.to_array()).collect(),
            tris: tris.clone(),
            face: labels.iter().map(|l| key_id[l]).collect(),
        });
        volumes.push(ev);
        areas.push(ea);
        bodies.push(EvaluatedBody {
            id: Id(bi as u64 + 1),
            name: body.name.clone(),
            faces,
            volume: ev,
            area: ea,
            centroid: if vol.abs() > 1e-12 { (cen_v / vol).to_array() } else { [0.0; 3] },
            bounds: (lo.to_array(), hi.to_array()),
        });
    }
    if bodies.is_empty() {
        return Err(kerr("the B-rep session has no body".into()));
    }
    let surface = surface_dataset(&meshes, &face_names, &volumes, &areas);
    let distance: Option<DistanceFn> = aurafx_mesh::TriMesh::new(&all_pts, &all_tris).ok().map(|m| {
        let m = std::sync::Arc::new(m);
        let f: DistanceFn = std::sync::Arc::new(move |p| m.distance(p));
        f
    });
    Ok(Evaluated { bodies, surface, distance, kernel: "brep", approximate: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::apply_op;
    use crate::kernel::implicit::ImplicitKernel;
    use std::f64::consts::PI;

    fn eval(d: &Document) -> Evaluated {
        BrepKernel.evaluate(d).unwrap_or_else(|e| panic!("{e}"))
    }
    fn close(v: f64, e: f64, tol: f64) -> bool {
        (v - e).abs() / e.abs() < tol
    }
    fn one(shape: Shape) -> Document {
        Document::new("part").add(Feature::new("F", BooleanOp::Add, shape))
    }

    #[test]
    fn primitives_are_exact() {
        let b = eval(&one(Shape::Box { size: [40.0, 20.0, 10.0], corner_radius: 0.0 }));
        assert!(close(b.volume(), 8000.0, 1e-4), "box {}", b.volume());
        assert_eq!(b.kernel, "brep");
        let names: Vec<_> = b.faces().map(|(_, f)| f.name.clone()).collect();
        assert_eq!(names.len(), 6, "{names:?}");
        assert!(names.contains(&"F z+".to_string()));
        let c = eval(&one(Shape::Cylinder { radius: 10.0, height: 30.0 }));
        assert!(close(c.volume(), PI * 100.0 * 30.0, 1e-3), "cylinder {}", c.volume());
        assert_eq!(c.bodies[0].faces.len(), 3);
        let s = eval(&one(Shape::Sphere { radius: 12.0 }));
        assert!(close(s.volume(), 4.0 / 3.0 * PI * 1728.0, 1e-3), "sphere {}", s.volume());
        let k = eval(&one(Shape::Cone { r_bottom: 10.0, r_top: 4.0, height: 12.0 }));
        let expect = PI * 12.0 / 3.0 * (100.0 + 40.0 + 16.0);
        assert!(close(k.volume(), expect, 1e-3), "cone {}", k.volume());
        let t = eval(&one(Shape::Torus { major: 20.0, minor: 5.0 }));
        assert!(close(t.volume(), 2.0 * PI * PI * 20.0 * 25.0, 1e-3), "torus {}", t.volume());
    }

    #[test]
    fn a_plate_with_a_hole_has_the_same_named_faces_and_exact_volume() {
        let mut d = Document::new("plate");
        apply_op(
            &mut d,
            "sketch_extrude",
            &json!({"name": "Plate", "depth": 10, "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": 40, "h": 30}}]}}),
        )
        .unwrap();
        apply_op(&mut d, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 10, "at": [5, 3, 10]})).unwrap();
        let b = eval(&d);
        let expect = 40.0 * 30.0 * 10.0 - PI * 16.0 * 10.0;
        assert!(close(b.volume(), expect, 1e-4), "{} vs {expect}", b.volume());
        let i = ImplicitKernel::new(96).evaluate(&d).unwrap();
        // Both kernels name a face the same way; the implicit one adds sliver faces where a cutter's
        // flat end lies in the plate's own face, which the exact kernel does not have.
        let nb: Vec<String> = b.faces().map(|(_, f)| f.name.clone()).collect();
        let ni: Vec<String> = i.faces().map(|(_, f)| f.name.clone()).collect();
        assert!(nb.iter().all(|n| ni.contains(n)), "{nb:?} not all in {ni:?}");
        assert_eq!(nb.len(), 7, "{nb:?}");
        let wall = b.face_by_name("Bore side").unwrap();
        assert!(close(wall.area, 2.0 * PI * 4.0 * 10.0, 5e-3), "{}", wall.area);
        assert!(close(i.volume(), b.volume(), 0.01));
        // The exact kernel also gives a distance: inside the plate is negative, outside positive.
        let dist = b.distance.as_ref().unwrap();
        assert!(dist([-15.0, -10.0, 5.0]) < 0.0 && dist([50.0, 0.0, 5.0]) > 0.0);
    }

    #[test]
    fn revolves_arcs_patterns_and_rounding_agree_with_the_implicit_kernel() {
        // A revolved profile with an arc.
        let mut d = Document::new("knob");
        apply_op(
            &mut d,
            "sketch_revolve",
            &json!({"name": "Turned", "angle": 360, "sketch": {"plane": "XZ", "items": [{"polygon": [[0, 0], [12, 0], [12, 4], [7, 16], [0, 18]]}]}}),
        )
        .unwrap();
        let (b, i) = (eval(&d), ImplicitKernel::new(96).evaluate(&d).unwrap());
        assert!(close(b.volume(), i.volume(), 0.015), "revolve {} vs {}", b.volume(), i.volume());
        // A bolt circle of holes (copies), a through-all cut.
        let mut p = Document::new("flange");
        apply_op(&mut p, "add", &json!({"kind": "cylinder", "name": "Disc", "radius": 40, "height": 8})).unwrap();
        apply_op(&mut p, "add", &json!({"kind": "hole", "name": "Bolt", "diameter": 6, "depth": 8, "at": [30, 0, 8]})).unwrap();
        apply_op(&mut p, "modifier", &json!({"feature": "Bolt", "modifier": {"kind": "CircularPatternAngle", "count": 6, "total_deg": 360, "axis": "Z"}}))
            .unwrap();
        let (b, i) = (eval(&p), ImplicitKernel::new(128).evaluate(&p).unwrap());
        let expect = PI * 1600.0 * 8.0 - 6.0 * PI * 9.0 * 8.0;
        assert!(close(b.volume(), expect, 1e-3), "{} vs {expect}", b.volume());
        assert!(close(i.volume(), b.volume(), 0.01));
        // Rounding the whole part: the volume falls by the edge material.
        let mut r = Document::new("block");
        apply_op(&mut r, "add", &json!({"kind": "box", "name": "Block", "size": [20, 20, 20]})).unwrap();
        let before = eval(&r).volume();
        apply_op(&mut r, "modifier", &json!({"modifier": {"kind": "Fillet", "radius": 2}})).unwrap();
        let after = eval(&r).volume();
        assert!(after < before - 100.0 && after > before * 0.95, "{before} -> {after}");
    }

    #[test]
    fn what_it_cannot_build_is_refused_with_the_reason_and_auto_falls_back() {
        let mut d = Document::new("x");
        apply_op(&mut d, "add", &json!({"kind": "thread", "name": "Rod", "major_diameter": 8, "pitch": 1.25, "length": 10})).unwrap();
        let e = BrepKernel.evaluate(&d).unwrap_err();
        assert!(matches!(e, CadError::KernelRefused("brep", _)) && e.to_string().contains("modelled thread"), "{e}");
        let ev = crate::kernel::evaluate_with(KernelChoice::Auto, &d, 48, true).unwrap();
        assert_eq!(ev.kernel, "implicit");
        assert!(ev.approximate.iter().any(|a| a.contains("modelled thread")), "{:?}", ev.approximate);
        assert_eq!(crate::kernel::evaluate_with(KernelChoice::Brep, &d, 48, true).unwrap_err().to_string().contains("refused"), true);
        // An empty document is an error either way.
        assert!(BrepKernel.evaluate(&Document::new("empty")).is_err());
    }

    #[test]
    fn the_templates_give_the_same_volumes_on_both_kernels_where_the_exact_one_builds_them() {
        let mut built = 0;
        for (name, doc) in crate::templates::templates() {
            let imp = ImplicitKernel::new(96).evaluate(&doc).unwrap_or_else(|e| panic!("{name}: {e}"));
            match BrepKernel.evaluate(&doc) {
                Ok(b) => {
                    built += 1;
                    let tol = 0.02;
                    assert!(close(b.volume(), imp.volume(), tol), "{name}: brep {} vs implicit {}", b.volume(), imp.volume());
                    eprintln!("template {name}: both kernels, volume {:.1}", b.volume());
                }
                Err(e) => eprintln!("template {name}: refused ({e})"),
            }
        }
        assert!(built >= 1, "at least one template builds exactly");
    }

    #[test]
    fn picked_edges_are_rounded_exactly_and_the_implicit_kernel_says_it_approximates() {
        let mut d = Document::new("block");
        apply_op(&mut d, "add", &json!({"kind": "box", "name": "Block", "size": [20, 20, 20]})).unwrap();
        let before = eval(&d).volume();
        let edges = list_edges(&d).unwrap();
        assert_eq!(edges.len(), 12, "a box has twelve edges");
        assert!(edges.iter().all(|e| (e.length - 20.0).abs() < 1e-6 && e.straight && e.faces.len() == 2), "{edges:?}");
        // The top four edges: those whose faces include "Block z+".
        let top: Vec<[f64; 3]> = edges.iter().filter(|e| e.faces.contains(&"Block z+".to_string())).map(|e| e.mid).collect();
        assert_eq!(top.len(), 4);
        apply_op(&mut d, "edge_blend", &json!({"size": 2, "edges": top})).unwrap();
        let after = eval(&d);
        // Four edges of length 20 each lose (1 - pi/4) r^2 = 0.858 mm^2 of section.
        let lost = 4.0 * 20.0 * (1.0 - PI / 4.0) * 4.0;
        // (Where two rounded edges meet at a corner the pieces overlap a little: within 6 mm3.)
        assert!((before - after.volume() - lost).abs() < 6.0 && after.volume() < before - 50.0, "{before} -> {} (expected about -{lost})", after.volume());
        assert_eq!(after.kernel, "brep");
        // A chamfer of the same size loses a triangle: 4 * 20 * 2.
        let mut c = Document::new("block");
        apply_op(&mut c, "add", &json!({"kind": "box", "name": "Block", "size": [20, 20, 20]})).unwrap();
        let top2: Vec<[f64; 3]> = list_edges(&c).unwrap().iter().filter(|e| e.faces.contains(&"Block z+".to_string())).map(|e| e.mid).collect();
        apply_op(&mut c, "edge_blend", &json!({"chamfer": true, "size": 2, "edges": top2})).unwrap();
        assert!((before - eval(&c).volume() - 4.0 * 20.0 * 2.0).abs() < 12.0, "{}", eval(&c).volume());
        // The implicit kernel rounds every edge instead and flags it.
        let imp = ImplicitKernel::new(64).evaluate(&d).unwrap();
        assert!(imp.approximate.iter().any(|a| a.contains("stay sharp")), "{:?}", imp.approximate);
        assert!((imp.volume() - before).abs() / before < 0.01, "the edges stay sharp: {}", imp.volume());
    }

    #[test]
    fn a_shell_with_a_picked_open_face_is_exact() {
        let mut d = Document::new("box");
        apply_op(&mut d, "add", &json!({"kind": "box", "name": "Box", "size": [30, 20, 10]})).unwrap();
        let ev = eval(&d);
        let top = ev.face_pick("Box z+").unwrap();
        assert!((top[2] - 5.0).abs() < 1e-6, "{top:?}");
        apply_op(&mut d, "shell_faces", &json!({"thickness": 2, "faces": [top]})).unwrap();
        let shelled = eval(&d).volume();
        // An open box with 2 mm walls: outer minus the cavity (26 x 16 x 8).
        let expect = 30.0 * 20.0 * 10.0 - 26.0 * 16.0 * 8.0;
        assert!((shelled - expect).abs() / expect < 0.01, "{shelled} vs {expect}");
    }

    #[test]
    fn mass_interference_and_face_measures_are_analytic() {
        let boxed = |name: &str, size: [f64; 3]| {
            let mut d = Document::new(name);
            apply_op(&mut d, "add", &json!({"kind": "box", "name": "B", "size": size})).unwrap();
            d
        };
        let a = eval(&boxed("a", [20.0, 20.0, 20.0]));
        let m = a.mass_properties(7.85);
        assert!(close(m.volume_mm3, 8000.0, 1e-4) && close(m.mass_g, 8.0 * 7.85, 1e-4), "{m:?}");
        assert!(m.centroid.iter().all(|c| c.abs() < 1e-6), "{:?}", m.centroid);
        // Two 20 mm cubes, the second moved by (12, 6, 0): overlap 8 x 14 x 20.
        let pl = |x: f64, y: f64, z: f64| crate::document::Placement { translate: [x, y, z], rotate_deg: [0.0; 3] };
        let v = crate::interference_built(&a, &pl(0.0, 0.0, 0.0), &a, &pl(12.0, 6.0, 0.0), 0.4);
        assert!(close(v, 8.0 * 14.0 * 20.0, 0.03), "{v}");
        assert_eq!(crate::interference_built(&a, &pl(0.0, 0.0, 0.0), &a, &pl(50.0, 0.0, 0.0), 0.5), 0.0);
        // The distance between the two flat ends of a 30 mm brick, and the angle between a top and a side.
        let long = eval(&boxed("l", [30.0, 10.0, 10.0]));
        let m1 = crate::measure_faces(&long, "B x-", "B x+").unwrap();
        assert!(m1.parallel && (m1.distance - 30.0).abs() < 1e-6, "{m1:?}");
        let m2 = crate::measure_faces(&long, "B z+", "B x+").unwrap();
        assert!(!m2.parallel && (m2.angle_deg - 90.0).abs() < 1e-3, "{m2:?}");
    }
}
