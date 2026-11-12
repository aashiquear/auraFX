//! Editing a document with plain data: one operation name and a JSON object of arguments, the
//! same calls the Python `cad` object, the assistant and the feature panel make, so a script
//! and the interactive environment produce identical documents (and the same trace).
//!
//! Numbers in an operation may be written as expressions of the document's parameters
//! (`"width / 2"`): the field then becomes a binding, so changing the parameter moves the
//! geometry, and the stored number is the expression's value now.

use crate::document::*;
use crate::expr;
use crate::path::shapes;
use crate::sketch::{Constraint, Dim, Entity, PlaneFrame, Sketch, SketchPlane};
use crate::{CadError, Id};
use glam::DVec2;
use serde_json::{json, Value};

fn bad<T>(m: impl Into<String>) -> Result<T, CadError> {
    Err(CadError::Edit(m.into()))
}

/// A number or an expression (text) in an argument.
fn num(doc: &Document, v: &Value) -> Result<(f64, Option<String>), CadError> {
    match v {
        Value::Number(n) => Ok((n.as_f64().unwrap_or(0.0), None)),
        Value::String(s) => {
            let vals = doc.parameter_values()?;
            let x = expr::eval(s, &|n| vals.get(n).copied()).map_err(|source| CadError::Expr { name: s.clone(), source })?;
            Ok((x, Some(s.clone())))
        }
        other => bad(format!("expected a number or an expression, got {other}")),
    }
}

fn dim(doc: &Document, v: &Value) -> Result<Dim, CadError> {
    let (x, e) = num(doc, v)?;
    Ok(Dim { value: x, expr: e, at: None })
}

fn vec3(v: Option<&Value>, default: [f64; 3]) -> [f64; 3] {
    v.and_then(|v| v.as_array())
        .filter(|a| a.len() == 3)
        .map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)])
        .unwrap_or(default)
}

/// A part given in an operation's arguments: a template, a document as JSON, a CAD file or a blank part.
fn part_from_args(args: &Value) -> Result<Document, CadError> {
    Ok(if let Some(t) = args.get("template").and_then(|t| t.as_str()) {
        let mut tmp = Document::new("Part");
        apply_op(&mut tmp, "template", &json!({ "name": t }))?;
        tmp
    } else if let Some(j) = args.get("document").filter(|j| j.is_object()) {
        serde_json::from_value(j.clone()).map_err(|e| CadError::Edit(format!("document: {e}")))?
    } else if let Some(p) = args.get("path").and_then(|p| p.as_str()) {
        crate::io::import_file(std::path::Path::new(p))?
    } else {
        Document::new(args.get("name").and_then(|n| n.as_str()).unwrap_or("Part"))
    })
}

/// The name of a new feature: the one asked for (refused when a feature already has it: faces are
/// named after features, so two with one name would make their faces ambiguous), or the default
/// made unique ("Box", "Box 2", ...).
fn new_name(doc: &Document, args: &Value, default: &str) -> Result<String, CadError> {
    let taken = |n: &str| doc.features.iter().any(|f| f.name == n);
    match args.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()) {
        Some(n) if taken(n) => bad(format!("a feature named '{n}' already exists: use another name, rename it, or start from nothing with clear()")),
        Some(n) => Ok(n.to_string()),
        None => Ok((1..).map(|k| if k == 1 { default.to_string() } else { format!("{default} {k}") }).find(|n| !taken(n)).unwrap()),
    }
}

fn find<'a>(doc: &'a Document, name: &Value) -> Result<usize, CadError> {
    match name {
        Value::Number(n) => {
            let id = Id(n.as_u64().unwrap_or(0));
            doc.features.iter().position(|f| f.id == id).ok_or_else(|| CadError::Edit(format!("no feature with id {id:?}")))
        }
        Value::String(s) => doc.features.iter().position(|f| &f.name == s).ok_or_else(|| {
            let names: Vec<_> = doc.features.iter().map(|f| f.name.as_str()).collect();
            CadError::Edit(format!("no feature named '{s}' (the features are: {})", names.join(", ")))
        }),
        _ => bad("a feature is named by its name or id"),
    }
}

fn op_of(args: &Value) -> Result<BooleanOp, CadError> {
    // A hole removes material unless told otherwise.
    let default = if args.get("kind").and_then(|k| k.as_str()) == Some("hole") { "cut" } else { "add" };
    match args.get("op").and_then(|o| o.as_str()).unwrap_or(default).to_lowercase().as_str() {
        "add" | "join" | "union" => Ok(BooleanOp::Add),
        "cut" | "subtract" | "remove" => Ok(BooleanOp::Cut),
        "intersect" => Ok(BooleanOp::Intersect),
        o => bad(format!("op is add, cut or intersect, not '{o}'")),
    }
}

fn placement(doc: &Document, args: &Value, f: &mut Feature) -> Result<(), CadError> {
    f.placement.translate = vec3(args.get("at"), [0.0; 3]);
    f.placement.rotate_deg = vec3(args.get("rotate"), [0.0; 3]);
    // Expressions in the position are bindings.
    for (key, path) in [("at", "/placement/translate"), ("rotate", "/placement/rotate_deg")] {
        if let Some(a) = args.get(key).and_then(|a| a.as_array()) {
            for (i, v) in a.iter().enumerate().take(3) {
                if let (x, Some(e)) = num(doc, v)? {
                    let slot = if key == "at" { &mut f.placement.translate } else { &mut f.placement.rotate_deg };
                    slot[i] = x;
                    f.bindings.push(Binding { path: format!("{path}/{i}"), expr: e });
                }
            }
        }
    }
    if let Some(b) = args.get("blend") {
        let (x, e) = num(doc, b)?;
        f.blend = x;
        if let Some(e) = e {
            f.bindings.push(Binding { path: "/blend".into(), expr: e });
        }
    }
    if args.get("blend_kind").and_then(|k| k.as_str()).is_some_and(|k| k.eq_ignore_ascii_case("chamfer")) {
        f.blend_kind = BlendKind::Chamfer;
    }
    Ok(())
}

fn default_shape(kind: &str) -> Result<Shape, CadError> {
    Ok(match kind.to_lowercase().as_str() {
        "box" | "brick" | "block" => Shape::Box { size: [10.0, 10.0, 10.0], corner_radius: 0.0 },
        "sphere" => Shape::Sphere { radius: 5.0 },
        "cylinder" => Shape::Cylinder { radius: 5.0, height: 10.0 },
        "cone" => Shape::Cone { r_bottom: 5.0, r_top: 0.0, height: 10.0 },
        "torus" => Shape::Torus { major: 10.0, minor: 2.0 },
        "hole" => Shape::Hole { kind: HoleKind::Simple, diameter: 5.0, depth: 10.0 },
        "thread" => Shape::Thread { major_diameter: 8.0, pitch: 1.25, length: 10.0, internal: false, depth: 0.0 },
        k => return bad(format!("unknown shape '{k}': box, sphere, cylinder, cone, torus, hole or thread (or sketch_extrude / sketch_revolve)")),
    })
}

/// Set the numeric fields named in `args` on the JSON of a shape; expressions become bindings.
fn fill_shape(doc: &Document, shape: &Shape, args: &Value, bindings: &mut Vec<Binding>) -> Result<Shape, CadError> {
    let mut v = serde_json::to_value(shape).map_err(|e| CadError::Edit(e.to_string()))?;
    let Some(obj) = args.as_object() else { return Ok(shape.clone()) };
    for (key, val) in obj {
        let Some(slot) = v.get_mut(key.as_str()) else { continue };
        let path = format!("/shape/{key}");
        match (&*slot, val) {
            (Value::Array(cur), Value::Array(new)) => {
                if cur.len() != new.len() {
                    return bad(format!("'{key}' takes {} values", cur.len()));
                }
                let mut out = Vec::new();
                for (i, x) in new.iter().enumerate() {
                    let (n, e) = num(doc, x)?;
                    if let Some(e) = e {
                        bindings.push(Binding { path: format!("{path}/{i}"), expr: e });
                    }
                    out.push(json!(n));
                }
                *slot = Value::Array(out);
            }
            (Value::Number(_), val @ (Value::Number(_) | Value::String(_))) => {
                let (n, e) = num(doc, val)?;
                if let Some(e) = e {
                    bindings.push(Binding { path, expr: e });
                }
                *slot = if slot.is_u64() { json!(n.round().max(0.0) as u64) } else { json!(n) };
            }
            (Value::Bool(_), Value::Bool(b)) => *slot = json!(b),
            _ => {}
        }
    }
    // Hole kinds: {"hole": "counterbore", "cb_diameter": 9, ...} are spelled out for scripts.
    if let (Shape::Hole { .. }, Some(kind)) = (shape, args.get("hole").and_then(|k| k.as_str())) {
        let g = |k: &str, d: f64| args.get(k).and_then(|x| x.as_f64()).unwrap_or(d);
        let hk = match kind.to_lowercase().as_str() {
            "simple" => HoleKind::Simple,
            "drilled" => HoleKind::Drilled,
            "counterbore" => HoleKind::Counterbore { diameter: g("cb_diameter", 2.0 * g("diameter", 5.0)), depth: g("cb_depth", 3.0) },
            "countersink" => HoleKind::Countersink { diameter: g("cs_diameter", 2.0 * g("diameter", 5.0)), angle_deg: g("cs_angle", 90.0) },
            "tapped" => HoleKind::Tapped { pitch: g("pitch", 1.25), cosmetic: args.get("cosmetic").and_then(|c| c.as_bool()).unwrap_or(false) },
            k => return bad(format!("hole is simple, drilled, counterbore, countersink or tapped, not '{k}'")),
        };
        v["hole"] = serde_json::to_value(hk).map_err(|e| CadError::Edit(e.to_string()))?;
    }
    serde_json::from_value(v).map_err(|e| CadError::Edit(e.to_string()))
}

fn plane_of(v: Option<&Value>) -> Result<SketchPlane, CadError> {
    match v.and_then(|p| p.as_str()).unwrap_or("XY").to_uppercase().as_str() {
        "XY" | "TOP" => Ok(SketchPlane::XY),
        "XZ" | "FRONT" => Ok(SketchPlane::XZ),
        "YZ" | "RIGHT" => Ok(SketchPlane::YZ),
        p => bad(format!("plane is XY, XZ or YZ, not '{p}'")),
    }
}

fn vec3_of(v: Option<&Value>) -> Option<[f64; 3]> {
    let a = v?.as_array()?;
    (a.len() == 3).then(|| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)])
}

/// `{"origin": [x, y, z], "normal": [..], "x_axis": [..] (optional)}` as a plane frame.
fn frame_of(v: &Value) -> Result<PlaneFrame, CadError> {
    let o = vec3_of(v.get("origin")).unwrap_or([0.0; 3]);
    let n = vec3_of(v.get("normal")).ok_or_else(|| CadError::Edit("a plane needs \"normal\": [x, y, z]".into()))?;
    PlaneFrame::new(o, n, vec3_of(v.get("x_axis"))).ok_or_else(|| CadError::Edit("the plane's normal is zero".into()))
}

/// A reference plane from `plane` arguments: `{"name", "from": "XY"|"XZ"|"YZ"|<plane>, "offset",
/// "angle", "about": "x"|"y"}`, `{"points": [[..], [..], [..]]}` or `{"origin", "normal", "x_axis"}`.
pub fn ref_plane_of(doc: &Document, args: &Value) -> Result<RefPlane, CadError> {
    let num = |k: &str| args.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let (frame, how) = if let Some(pts) = args.get("points").and_then(|p| p.as_array()) {
        let p: Vec<[f64; 3]> = pts.iter().filter_map(|x| vec3_of(Some(x))).collect();
        if p.len() != 3 {
            return bad("\"points\" takes three points [x, y, z]");
        }
        (PlaneFrame::through(p[0], p[1], p[2]).ok_or_else(|| CadError::Edit("the three points are on one line".into()))?, "through 3 points".to_string())
    } else if args.get("normal").is_some() {
        (frame_of(args)?, "by origin and normal".to_string())
    } else {
        let from = args.get("from").and_then(|f| f.as_str()).unwrap_or("XY");
        let (off, ang) = (num("offset"), num("angle"));
        let about_x = args.get("about").and_then(|a| a.as_str()).map(|a| a.eq_ignore_ascii_case("x")).unwrap_or(true);
        let base = match doc.planes.iter().find(|p| p.name.eq_ignore_ascii_case(from)) {
            Some(rp) => {
                // Turn about the reference plane's own x or y axis, through its moved origin.
                let r = rp.frame.basis();
                let moved = rp.frame.offset(off);
                let axis = if about_x { r.x_axis } else { r.y_axis };
                let q = glam::DQuat::from_axis_angle(axis, ang.to_radians());
                PlaneFrame { origin: moved.origin, x_axis: (q * r.x_axis).to_array(), normal: (q * r.z_axis).to_array() }
            }
            None => PlaneFrame::from_principal(plane_of(Some(&json!(from)))?, off, ang, about_x),
        };
        let mut how = from.to_string();
        if off != 0.0 {
            how += &format!(" offset {off}");
        }
        if ang != 0.0 {
            how += &format!(" turned {ang}° about its {}", if about_x { "x" } else { "y" });
        }
        (base, how)
    };
    let name = match args.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()) {
        Some(n) => n.to_string(),
        None => (1..).map(|k| format!("Plane {k}")).find(|n| !doc.planes.iter().any(|p| &p.name == n)).unwrap(),
    };
    let how = args.get("how").and_then(|h| h.as_str()).map(str::to_string).unwrap_or(how);
    Ok(RefPlane { name, frame, how })
}

/// A sketch from its description:
/// `{"plane": "XY", "offset": 0, "items": [{"rectangle": {"corner": [0, 0], "w": "width", "h": 20}},
/// {"circle": {"center": [5, 5], "r": 2}}, {"polygon": [[0, 0], [10, 0], [5, 8]]}]}`.
/// Rectangles and circles come fully dimensioned (with the expressions as dimensions), polygons
/// as free lines.
pub fn sketch_from_spec(doc: &Document, spec: &Value) -> Result<Sketch, CadError> {
    sketch_from_spec_on(doc, spec).map(|s| s.0)
}

/// An index of a point or an entity in a sketch description, checked.
fn index_in(v: &Value, n: usize, what: &str, of: &str) -> Result<usize, CadError> {
    match v.as_u64().map(|i| i as usize) {
        Some(i) if i < n => Ok(i),
        Some(i) => bad(format!("{of} names {what} {i}, and the sketch has {n} (they count from 0)")),
        None => bad(format!("{of} takes the numbers of {what}s, not {v}")),
    }
}

/// The points, entities and constraints of a sketch written one by one (what the Python sketch
/// object sends): `"points": [[x, y], [x, y, true]]` (true: fixed), `"entities": [{"line": [a, b]},
/// {"circle": [center, r]}, {"arc": [center, start, end]}, {"spline": [p, ...], "closed": false}]`
/// (each may say `"construction": true`) and `"constraints": [{"horizontal": [line]},
/// {"distance": [p, q], "value": "gap"}, ...]`; numbers are the points' and entities' places
/// in those lists.
fn raw_sketch(doc: &Document, spec: &Value, s: &mut Sketch) -> Result<(), CadError> {
    for p in spec.get("points").and_then(|p| p.as_array()).into_iter().flatten() {
        let a = p.as_array().filter(|a| a.len() >= 2).ok_or_else(|| CadError::Edit("a sketch point is [x, y] or [x, y, true] (fixed)".into()))?;
        let (x, y) = (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
        if a.get(2).and_then(|f| f.as_bool()).unwrap_or(false) {
            s.add_fixed_point(x, y);
        } else {
            s.add_point(x, y);
        }
    }
    let np = s.points.len();
    for e in spec.get("entities").and_then(|e| e.as_array()).into_iter().flatten() {
        let construction = e.get("construction").and_then(|c| c.as_bool()).unwrap_or(false);
        let pts = |key: &str, n: usize| -> Result<Vec<usize>, CadError> {
            let a =
                e.get(key).and_then(|a| a.as_array()).filter(|a| a.len() >= n).ok_or_else(|| CadError::Edit(format!("a {key} takes {n} point number(s)")))?;
            a.iter().take(if key == "spline" { a.len() } else { n }).map(|v| index_in(v, np, "point", &format!("a {key}"))).collect()
        };
        let ent = if e.get("line").is_some() {
            let p = pts("line", 2)?;
            Entity::Line { a: p[0], b: p[1], construction }
        } else if let Some(c) = e.get("circle").and_then(|c| c.as_array()).filter(|c| c.len() == 2) {
            Entity::Circle { center: index_in(&c[0], np, "point", "a circle")?, radius: num(doc, &c[1])?.0, construction }
        } else if e.get("arc").is_some() {
            let p = pts("arc", 3)?;
            Entity::Arc { center: p[0], start: p[1], end: p[2], construction }
        } else if e.get("spline").is_some() {
            Entity::Spline { ctrl: pts("spline", 2)?, closed: e.get("closed").and_then(|c| c.as_bool()).unwrap_or(false), construction }
        } else {
            return bad(format!("a sketch entity is a line, a circle, an arc or a spline, not {e}"));
        };
        s.push(ent);
    }
    Ok(())
}

fn raw_constraints(doc: &Document, spec: &Value, s: &mut Sketch) -> Result<(), CadError> {
    let (np, ne) = (s.points.len(), s.entities.len());
    for c in spec.get("constraints").and_then(|c| c.as_array()).into_iter().flatten() {
        let Some((kind, a)) = c.as_object().and_then(|o| o.iter().find(|(k, _)| k.as_str() != "value")) else {
            return bad("a constraint is {\"horizontal\": [line]} or {\"distance\": [p, q], \"value\": 40}");
        };
        let a = a.as_array().cloned().unwrap_or_default();
        let of = format!("the {kind} constraint");
        let need = |n: usize| if a.len() == n { Ok(()) } else { bad(format!("{of} takes {n} item(s), not {}", a.len())) };
        let p = |i: usize| index_in(&a[i], np, "point", &of);
        let e = |i: usize| index_in(&a[i], ne, "entity", &of);
        let value = || -> Result<Dim, CadError> {
            dim(doc, c.get("value").ok_or_else(|| CadError::Edit(format!("{of} needs a \"value\" (a number or an expression of the part's dimensions)")))?)
        };
        let made = match kind.as_str() {
            "coincident" => (need(2)?, Constraint::Coincident(p(0)?, p(1)?)).1,
            "horizontal" => (need(1)?, Constraint::Horizontal(e(0)?)).1,
            "vertical" => (need(1)?, Constraint::Vertical(e(0)?)).1,
            "parallel" => (need(2)?, Constraint::Parallel(e(0)?, e(1)?)).1,
            "perpendicular" => (need(2)?, Constraint::Perpendicular(e(0)?, e(1)?)).1,
            "equal" => (need(2)?, Constraint::Equal(e(0)?, e(1)?)).1,
            "tangent" => (need(2)?, Constraint::Tangent(e(0)?, e(1)?)).1,
            "concentric" => (need(2)?, Constraint::Concentric(e(0)?, e(1)?)).1,
            "point_on_line" => (need(2)?, Constraint::PointOnLine(p(0)?, e(1)?)).1,
            "point_on_circle" => (need(2)?, Constraint::PointOnCircle(p(0)?, e(1)?)).1,
            "midpoint" => (need(2)?, Constraint::Midpoint(p(0)?, e(1)?)).1,
            "symmetric" => (need(3)?, Constraint::Symmetric(p(0)?, p(1)?, e(2)?)).1,
            "fix" => (need(1)?, Constraint::Fix(p(0)?)).1,
            "distance" => (need(2)?, Constraint::Distance(p(0)?, p(1)?, value()?)).1,
            "hdistance" => (need(2)?, Constraint::HorizontalDistance(p(0)?, p(1)?, value()?)).1,
            "vdistance" => (need(2)?, Constraint::VerticalDistance(p(0)?, p(1)?, value()?)).1,
            "point_line_distance" => (need(2)?, Constraint::PointLineDistance(p(0)?, e(1)?, value()?)).1,
            "length" => (need(1)?, Constraint::Length(e(0)?, value()?)).1,
            "radius" => (need(1)?, Constraint::Radius(e(0)?, value()?)).1,
            "diameter" => (need(1)?, Constraint::Diameter(e(0)?, value()?)).1,
            "angle" => (need(2)?, Constraint::Angle(e(0)?, e(1)?, value()?)).1,
            k => return bad(format!("unknown constraint '{k}'")),
        };
        s.constrain(made);
    }
    Ok(())
}

/// [`sketch_from_spec`], and whether the face the sketch is on looks against the sketch plane's
/// normal (a boss on it then extrudes in reverse). A sketch on a face is `{"face": "Plate z+", ...}`.
pub fn sketch_from_spec_on(doc: &Document, spec: &Value) -> Result<(Sketch, bool), CadError> {
    // The plane: XY/XZ/YZ (Top/Front/Right), a reference plane's name, or {"origin", "normal", "x_axis"}.
    let pv = spec.get("plane");
    let mut against = false;
    let on_face = spec.get("face").and_then(|f| f.as_str()).filter(|f| !f.is_empty());
    let mut s = match pv {
        _ if on_face.is_some() => {
            let face = on_face.unwrap();
            let (p, n) = crate::swept::face_plane_before(doc, doc.features.len(), face)?;
            let (sk, rev) = Sketch::on_face(p, n, face).ok_or_else(|| CadError::Edit(format!("the face '{face}' has no direction")))?;
            against = rev;
            sk
        }
        Some(Value::Object(_)) => Sketch::new(SketchPlane::XY).on_frame(frame_of(pv.unwrap())?, None),
        Some(Value::String(n)) if doc.planes.iter().any(|p| p.name.eq_ignore_ascii_case(n)) => {
            let rp = doc.planes.iter().find(|p| p.name.eq_ignore_ascii_case(n)).unwrap();
            Sketch::new(SketchPlane::XY).on_frame(rp.frame, Some(rp.name.clone()))
        }
        _ => Sketch::new(plane_of(pv)?),
    };
    if on_face.is_none() {
        s.offset = spec.get("offset").and_then(|o| o.as_f64()).unwrap_or(0.0);
    }
    raw_sketch(doc, spec, &mut s)?;
    let none = Vec::new();
    let items = match spec.get("items").and_then(|i| i.as_array()) {
        Some(i) => i,
        None if spec.get("entities").is_some() => &none,
        None => return bad("a sketch needs \"items\": [...]"),
    };
    for item in items {
        let Some((kind, a)) = item.as_object().and_then(|o| o.iter().next()) else {
            return bad("each sketch item is {\"rectangle\": {...}}, {\"circle\": {...}} or {\"polygon\": [...]}");
        };
        match kind.as_str() {
            "rectangle" => {
                let c = a.get("corner").and_then(|c| c.as_array()).filter(|c| c.len() == 2);
                let (x0, y0) = c.map(|c| (c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))).unwrap_or((0.0, 0.0));
                let (wd, hd) = (dim(doc, a.get("w").unwrap_or(&json!(10)))?, dim(doc, a.get("h").unwrap_or(&json!(10)))?);
                let center = a.get("center").and_then(|c| c.as_array()).filter(|c| c.len() == 2);
                let (x0, y0) = match center {
                    Some(c) => (c[0].as_f64().unwrap_or(0.0) - wd.value / 2.0, c[1].as_f64().unwrap_or(0.0) - hd.value / 2.0),
                    None => (x0, y0),
                };
                // A rectangle given by its center stays centered when its dimensions change: the
                // center is fixed and is the midpoint of a diagonal. One given by a corner keeps that corner.
                let p = if let Some(c) = center {
                    let (cx, cy) = (c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0));
                    let p = [s.add_point(x0, y0), s.add_point(x0 + wd.value, y0), s.add_point(x0 + wd.value, y0 + hd.value), s.add_point(x0, y0 + hd.value)];
                    let cp = s.add_fixed_point(cx, cy);
                    let diag = s.add_construction_line(p[0], p[2]);
                    s.constrain(Constraint::Midpoint(cp, diag));
                    p
                } else {
                    [s.add_fixed_point(x0, y0), s.add_point(x0 + wd.value, y0), s.add_point(x0 + wd.value, y0 + hd.value), s.add_point(x0, y0 + hd.value)]
                };
                let l: Vec<usize> = (0..4).map(|i| s.add_line(p[i], p[(i + 1) % 4])).collect();
                s.constrain(Constraint::Horizontal(l[0]));
                s.constrain(Constraint::Vertical(l[1]));
                s.constrain(Constraint::Horizontal(l[2]));
                s.constrain(Constraint::Vertical(l[3]));
                s.constrain(Constraint::Length(l[0], wd));
                s.constrain(Constraint::Length(l[1], hd));
            }
            "circle" => {
                let c = a.get("center").and_then(|c| c.as_array()).filter(|c| c.len() == 2);
                let (cx, cy) = c.map(|c| (c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))).unwrap_or((0.0, 0.0));
                let r = dim(doc, a.get("r").or(a.get("radius")).unwrap_or(&json!(5)))?;
                let cp = s.add_fixed_point(cx, cy);
                let e = s.push(Entity::Circle { center: cp, radius: r.value, construction: false });
                s.constrain(Constraint::Radius(e, r));
            }
            "polygon" | "polyline" => {
                let pts: Vec<[f64; 2]> = a
                    .as_array()
                    .ok_or_else(|| CadError::Edit("polygon takes [[x, y], ...]".into()))?
                    .iter()
                    .map(|p| {
                        let p = p.as_array().filter(|p| p.len() == 2).ok_or_else(|| CadError::Edit("a polygon point is [x, y]".into()))?;
                        Ok([p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)])
                    })
                    .collect::<Result<_, CadError>>()?;
                if pts.len() < 3 {
                    return bad("a polygon needs at least three points");
                }
                let ids: Vec<usize> = pts.iter().map(|p| s.add_fixed_point(p[0], p[1])).collect();
                for i in 0..ids.len() {
                    s.add_line(ids[i], ids[(i + 1) % ids.len()]);
                }
            }
            "regular" => {
                // {"regular": {"center": [0,0], "r": 10, "sides": 6}}
                let c = a.get("center").and_then(|c| c.as_array()).filter(|c| c.len() == 2);
                let (cx, cy) = c.map(|c| (c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))).unwrap_or((0.0, 0.0));
                let r = num(doc, a.get("r").unwrap_or(&json!(10)))?.0;
                let sides = a.get("sides").and_then(|n| n.as_u64()).unwrap_or(6).max(3) as usize;
                let pl = shapes::polygon(DVec2::new(cx, cy), r, sides, 0.0);
                s.add_polyline(&pl);
            }
            "path" => {
                // {"path": [[0, 0], [40, 0], [40, 30]]}: lines joined end to end, left open (the path of a sweep).
                let pts: Vec<[f64; 2]> = a
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| p.as_array().filter(|p| p.len() == 2).map(|p| [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)]))
                            .collect()
                    })
                    .unwrap_or_default();
                if pts.len() < 2 {
                    return bad("a path takes two or more points [[x, y], ...]");
                }
                let ids: Vec<usize> = pts.iter().map(|p| s.add_fixed_point(p[0], p[1])).collect();
                for w in ids.windows(2) {
                    s.add_line(w[0], w[1]);
                }
            }
            k => return bad(format!("unknown sketch item '{k}': rectangle, circle, polygon, regular or path")),
        }
    }
    raw_constraints(doc, spec, &mut s)?;
    Ok((s, against))
}

/// The sketch an operation is given: a description (`sketch`) or a whole sketch (`sketch_json`,
/// what the sketch canvas makes); with it, whether its face looks against its plane's normal.
fn sketch_arg(doc: &Document, args: &Value, op: &str) -> Result<(Sketch, bool), CadError> {
    match args.get("sketch_json") {
        Some(j) => serde_json::from_value::<Sketch>(j.clone()).map(|s| (s, false)).map_err(|e| CadError::Edit(format!("sketch_json: {e}"))),
        None => sketch_from_spec_on(doc, args.get("sketch").ok_or_else(|| CadError::Edit(format!("{op} needs a sketch")))?),
    }
}

/// A name for a new sketch kept with the part: the one asked for (refused when taken), or
/// "Sketch 1", "Sketch 2", ...
fn sketch_name(doc: &Document, want: Option<&str>, base: &str) -> Result<String, CadError> {
    let taken = |n: &str| doc.sketches.iter().any(|s| s.name == n);
    match want.filter(|n| !n.trim().is_empty()) {
        Some(n) if taken(n) => bad(format!("the part already has a sketch named '{n}'")),
        Some(n) => Ok(n.trim().to_string()),
        None => Ok((1..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).unwrap()),
    }
}

/// A sketch a sweep or a loft names: the name of one kept with the part, or a description
/// (kept under a name made from the feature's).
fn sketch_ref(doc: &mut Document, v: &Value, feature: &str, role: &str) -> Result<String, CadError> {
    match v {
        Value::String(n) if doc.saved_sketch(n).is_some() => Ok(n.clone()),
        Value::String(n) => {
            let have: Vec<&str> = doc.sketches.iter().map(|s| s.name.as_str()).collect();
            bad(format!(
                "the part has no sketch named '{n}' (its sketches are: {}): keep one with sketch_add first",
                if have.is_empty() { "none".to_string() } else { have.join(", ") }
            ))
        }
        Value::Object(_) => {
            let sk = if v.get("entities").is_some() || v.get("items").is_some() || v.get("plane").is_some() {
                sketch_from_spec(doc, v)?
            } else {
                serde_json::from_value::<Sketch>(v.clone()).map_err(|e| CadError::Edit(format!("the {role} sketch: {e}")))?
            };
            let name = (1..)
                .map(|k| if k == 1 { format!("{feature} {role}") } else { format!("{feature} {role} {k}") })
                .find(|n| doc.saved_sketch(n).is_none())
                .unwrap();
            doc.sketches.push(SavedSketch { name: name.clone(), sketch: sk });
            Ok(name)
        }
        other => bad(format!("the {role} of a sweep or loft is the name of a sketch or a sketch, not {other}")),
    }
}

/// The features that use the sketch kept as `name`.
fn sketch_users(doc: &Document, name: &str) -> Vec<String> {
    doc.features
        .iter()
        .filter(|f| match &f.shape {
            Shape::SketchSweep { profile, path, .. } => profile == name || path == name,
            Shape::SketchLoft { sections } => sections.iter().any(|s| s == name),
            _ => false,
        })
        .map(|f| f.name.clone())
        .collect()
}

/// One side of a mate from an operation's argument: "Component/Face", a face of the assembly's
/// own part ("Face"), a principal plane ("XY") or {"component": .., "face": ..}.
fn mate_ref(doc: &Document, v: Option<&Value>) -> Result<MateRef, CadError> {
    match v {
        Some(Value::Object(o)) => Ok(MateRef {
            component: o.get("component").and_then(|c| c.as_str()).unwrap_or("").to_string(),
            face: o.get("face").and_then(|c| c.as_str()).unwrap_or("").to_string(),
        }),
        Some(Value::String(t)) => {
            // The longest component name the text starts with (names may hold a slash themselves).
            let best = doc
                .components
                .iter()
                .filter(|c| t.strip_prefix(c.name.as_str()).is_some_and(|r| r.is_empty() || r.starts_with('/')))
                .max_by_key(|c| c.name.len());
            Ok(match best {
                Some(c) => MateRef { component: c.name.clone(), face: t[c.name.len()..].trim_start_matches('/').to_string() },
                // "Part/Face" of a part the assembly does not have is told apart from a face of its own.
                None => match t.split_once('/') {
                    Some((c, f)) => MateRef { component: c.to_string(), face: f.to_string() },
                    None => MateRef { component: String::new(), face: t.clone() },
                },
            })
        }
        _ => bad("a mate names two faces: \"Component/Face\" each (cad.faces() lists them)"),
    }
}

fn find_mate(doc: &Document, v: Option<&Value>) -> Result<usize, CadError> {
    let name = v.and_then(|n| n.as_str()).unwrap_or("");
    doc.mates.iter().position(|m| m.name == name).ok_or_else(|| {
        CadError::Edit(format!("no mate named '{name}' (the mates are: {})", doc.mates.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")))
    })
}

fn feature_summary(f: &Feature) -> Value {
    json!({
        "id": f.id.0, "name": f.name, "op": format!("{:?}", f.op).to_lowercase(),
        "shape": serde_json::to_value(&f.shape).ok().and_then(|v| v.get("kind").cloned()).unwrap_or(Value::Null),
        "suppressed": f.suppressed,
    })
}

/// Apply one operation to the document; returns what it made (a feature's id and name, ...).
/// One operation on a document. All or nothing: when the operation is refused the document is
/// exactly as it was (operations may change it before they find the problem).
pub fn apply_op(doc: &mut Document, op: &str, args: &Value) -> Result<Value, CadError> {
    let before = doc.clone();
    match apply_op_inner(doc, op, args) {
        Ok(v) => {
            doc.normalize_ids();
            Ok(v)
        }
        Err(e) => {
            *doc = before;
            Err(e)
        }
    }
}

/// Drop the bindings of modifier `k` of a feature and renumber those of the modifiers after it
/// (bindings address modifiers by position).
fn unbind_modifier(f: &mut Feature, k: usize) {
    let mut kept = Vec::new();
    for mut b in std::mem::take(&mut f.bindings) {
        if let Some(rest) = b.path.strip_prefix("/modifiers/") {
            let (idx, tail) = rest.split_once('/').unwrap_or((rest, ""));
            match idx.parse::<usize>() {
                Ok(i) if i == k => continue,
                Ok(i) if i > k => b.path = if tail.is_empty() { format!("/modifiers/{}", i - 1) } else { format!("/modifiers/{}/{tail}", i - 1) },
                _ => {}
            }
        }
        kept.push(b);
    }
    f.bindings = kept;
}

fn apply_op_inner(doc: &mut Document, op: &str, args: &Value) -> Result<Value, CadError> {
    match op {
        "new" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("Part");
            *doc = Document::new(name);
            Ok(json!({}))
        }
        "template" => {
            let want = args.get("name").and_then(|n| n.as_str()).unwrap_or("").to_lowercase();
            let all = crate::templates::templates();
            let (title, found) = all
                .iter()
                .find(|(t, _)| t.to_lowercase().starts_with(&want) || t.to_lowercase().contains(&want))
                .ok_or_else(|| CadError::Edit(format!("no template '{want}'; there are: {}", all.iter().map(|(t, _)| *t).collect::<Vec<_>>().join("; "))))?;
            *doc = found.clone();
            Ok(json!({ "template": title }))
        }
        "rename_document" => {
            doc.name = args.get("name").and_then(|n| n.as_str()).unwrap_or("Part").to_string();
            Ok(json!({}))
        }
        "parameter" => {
            let name = args.get("name").and_then(|n| n.as_str()).ok_or_else(|| CadError::Edit("parameter needs a name".into()))?;
            let e = match args.get("expr").or(args.get("value")) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => return bad("parameter needs an expression or a number"),
            };
            if !name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return bad(format!("'{name}' is not a usable parameter name (letters, digits and _; not starting with a digit)"));
            }
            doc.set_parameter(name, &e);
            if let Some(c) = args.get("comment").and_then(|c| c.as_str()) {
                if let Some(p) = doc.parameters.iter_mut().find(|p| p.name == name) {
                    p.comment = c.to_string();
                }
            }
            doc.parameter_values()?; // refuse a cycle or an unknown name at once
            Ok(json!({ "name": name, "expr": e }))
        }
        "remove_parameter" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let before = doc.parameters.len();
            doc.parameters.retain(|p| p.name != name);
            if doc.parameters.len() == before {
                return bad(format!("no parameter '{name}'"));
            }
            // Still used by another dimension, a feature or a sketch: say so instead of leaving
            // a part that no longer builds.
            if let Err(e) = doc.resolved() {
                return bad(format!("'{name}' is still used ({e}): change what uses it first"));
            }
            Ok(json!({}))
        }
        "add" => {
            let kind = args.get("kind").and_then(|k| k.as_str()).ok_or_else(|| CadError::Edit("add needs a kind".into()))?;
            let name = new_name(doc, args, &capitalise(kind))?;
            let mut f = Feature::new(name, op_of(args)?, default_shape(kind)?);
            let mut bindings = Vec::new();
            f.shape = fill_shape(doc, &f.shape, args, &mut bindings)?;
            f.bindings = bindings;
            placement(doc, args, &mut f)?;
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "sketch_extrude" | "sketch_revolve" => {
            // A sketch is given as a description (`sketch`) or as a whole sketch (`sketch_json`, what
            // the sketch canvas makes).
            let (sk, against) = sketch_arg(doc, args, op)?;
            // On a face a boss grows out of the part and a cut goes into it, unless told otherwise.
            let on_face = args.get("sketch_json").is_none() && sk.face.is_some();
            let outward = against != (op_of(args)? == BooleanOp::Cut);
            let shape = if op == "sketch_extrude" {
                let (depth, de) = num(doc, args.get("depth").unwrap_or(&json!(10)))?;
                let end = match args.get("end").and_then(|e| e.as_str()).unwrap_or("blind").to_lowercase().as_str() {
                    "blind" => EndCondition::Blind,
                    "midplane" | "mid_plane" | "symmetric" => EndCondition::MidPlane,
                    "through" | "through_all" | "throughall" => EndCondition::ThroughAll,
                    e => return bad(format!("end is blind, midplane or through_all, not '{e}'")),
                };
                let edge = |k: &str| -> Result<EdgeTreatment, CadError> {
                    Ok(match args.get(k) {
                        Some(v) if v.get("fillet").is_some() => EdgeTreatment::Fillet { radius: num(doc, &v["fillet"])?.0 },
                        Some(v) if v.get("chamfer").is_some() => EdgeTreatment::Chamfer { distance: num(doc, &v["chamfer"])?.0 },
                        _ => EdgeTreatment::None,
                    })
                };
                (Shape::SketchExtrude {
                    sketch: sk,
                    depth,
                    end,
                    draft_deg: match args.get("draft") {
                        Some(d) => num(doc, d)?.0,
                        None => 0.0,
                    },
                    top_edge: edge("top")?,
                    bottom_edge: edge("bottom")?,
                    reverse: args.get("reverse").and_then(|r| r.as_bool()).unwrap_or(on_face && outward),
                }, de.map(|e| ("/shape/depth", e)))
            } else {
                let (angle, ae) = num(doc, args.get("angle").unwrap_or(&json!(360)))?;
                (Shape::SketchRevolve { sketch: sk, angle_deg: angle }, ae.map(|e| ("/shape/angle_deg", e)))
            };
            let default_name = if op == "sketch_extrude" { "Extrude" } else { "Revolve" };
            let name = new_name(doc, args, default_name)?;
            let mut f = Feature::new(name, op_of(args)?, shape.0);
            if let Some((p, e)) = shape.1 {
                f.bindings.push(Binding { path: p.into(), expr: e });
            }
            placement(doc, args, &mut f)?;
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "modifier" => {
            let m: Modifier = serde_json::from_value(args.get("modifier").cloned().unwrap_or(Value::Null))
                .map_err(|e| CadError::Edit(format!("modifier: {e} (for example {{\"kind\": \"Fillet\", \"radius\": 2}})")))?;
            match args.get("feature") {
                None | Some(Value::Null) => doc.finish.push(m),
                Some(Value::String(s)) if s == "finish" || s == "part" => doc.finish.push(m),
                Some(f) => {
                    let i = find(doc, f)?;
                    doc.features[i].modifiers.push(m);
                }
            }
            Ok(json!({}))
        }
        "clear_modifiers" => {
            match args.get("feature") {
                None | Some(Value::Null) => doc.finish.clear(),
                Some(f) => {
                    let i = find(doc, f)?;
                    doc.features[i].modifiers.clear();
                    doc.features[i].bindings.retain(|b| !b.path.starts_with("/modifiers/"));
                }
            }
            Ok(json!({}))
        }
        "suppress" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            doc.features[i].suppressed = args.get("suppressed").and_then(|s| s.as_bool()).unwrap_or(true);
            Ok(json!({}))
        }
        "remove" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let f = doc.features.remove(i);
            Ok(json!({ "removed": f.name }))
        }
        "rename" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let to = args.get("to").and_then(|t| t.as_str()).ok_or_else(|| CadError::Edit("rename needs \"to\"".into()))?.trim().to_string();
            if to.is_empty() {
                return bad("a feature needs a name");
            }
            if doc.features.iter().enumerate().any(|(k, f)| k != i && f.name == to) {
                return bad(format!("a feature named '{to}' already exists (faces are named after their feature)"));
            }
            doc.features[i].name = to;
            Ok(json!({}))
        }
        "move" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let to = (args.get("to").and_then(|t| t.as_u64()).unwrap_or(0) as usize).min(doc.features.len() - 1);
            let f = doc.features.remove(i);
            doc.features.insert(to, f);
            Ok(json!({}))
        }
        "bind" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let path = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| CadError::Edit("bind needs a JSON pointer \"path\" such as /shape/size/0".into()))?;
            let e = args.get("expr").and_then(|p| p.as_str()).ok_or_else(|| CadError::Edit("bind needs an \"expr\"".into()))?;
            let f = &mut doc.features[i];
            f.bindings.retain(|b| b.path != path);
            f.bindings.push(Binding { path: path.into(), expr: e.into() });
            doc.resolved()?; // the path and the expression are checked now
            Ok(json!({}))
        }
        "set" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let path = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| CadError::Edit("set needs a JSON pointer \"path\"".into()))?;
            let (x, _) = num(doc, args.get("value").unwrap_or(&Value::Null))?;
            let mut v = serde_json::to_value(&doc.features[i]).map_err(|e| CadError::Edit(e.to_string()))?;
            let slot = v.pointer_mut(path).ok_or_else(|| CadError::Edit(format!("no field {path} on '{}'", doc.features[i].name)))?;
            if !slot.is_number() {
                return bad(format!("{path} is not a number"));
            }
            *slot = if slot.is_u64() { json!(x.round().max(0.0) as u64) } else { json!(x) };
            doc.features[i] = serde_json::from_value(v).map_err(|e| CadError::Edit(e.to_string()))?;
            doc.features[i].bindings.retain(|b| b.path != path);
            Ok(json!({}))
        }
        "set_json" => {
            // The general edit the feature panel uses for choices (an end condition, a hole
            // type): any value at a JSON pointer inside the feature, checked by reading it back.
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let path = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| CadError::Edit("set_json needs a JSON pointer \"path\"".into()))?;
            let mut v = serde_json::to_value(&doc.features[i]).map_err(|e| CadError::Edit(e.to_string()))?;
            let slot = v.pointer_mut(path).ok_or_else(|| CadError::Edit(format!("no field {path} on '{}'", doc.features[i].name)))?;
            *slot = args.get("json").cloned().unwrap_or(Value::Null);
            let f: Feature = serde_json::from_value(v).map_err(|e| CadError::Edit(format!("{path}: {e}")))?;
            doc.features[i] = f;
            doc.features[i].bindings.retain(|b| !b.path.starts_with(path));
            Ok(json!({}))
        }
        "set_sketch" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let sk: Sketch = serde_json::from_value(args.get("sketch_json").cloned().unwrap_or(Value::Null)).map_err(|e| CadError::Edit(format!("sketch_json: {e}")))?;
            let name = doc.features[i].name.clone();
            *doc.features[i].shape.sketch_mut().ok_or_else(|| CadError::Edit(format!("'{name}' has no sketch")))? = sk;
            doc.resolved()?;
            Ok(json!({}))
        }
        "sketch_face" => {
            // Put a feature's sketch on a face of the part (it then follows that face), or with
            // "face": null let go of the face it is on and stay where it is.
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let name = doc.features[i].name.clone();
            if doc.features[i].shape.sketch().is_none() {
                return bad(format!("'{name}' has no sketch"));
            }
            match args.get("face").and_then(|f| f.as_str()).filter(|f| !f.is_empty()) {
                None => {
                    if let Some(sk) = doc.features[i].shape.sketch_mut() {
                        sk.face = None;
                    }
                }
                Some(face) => {
                    let (p, n) = crate::swept::face_plane_before(doc, i, face)?;
                    let sk = doc.features[i].shape.sketch_mut().unwrap();
                    sk.follow_face(p, n);
                    sk.face = Some(face.to_string());
                    sk.plane_name = Some(face.to_string());
                }
            }
            doc.resolved()?;
            Ok(json!({ "feature": name }))
        }
        "sketch_add" => {
            // Keep a sketch with the part under a name: the profile or the path of a sweep, a section of a loft.
            let (sk, _) = sketch_arg(doc, args, op)?;
            let name = sketch_name(doc, args.get("name").and_then(|n| n.as_str()), "Sketch")?;
            doc.sketches.push(SavedSketch { name: name.clone(), sketch: sk });
            doc.resolved()?;
            Ok(json!({ "sketch": name }))
        }
        "sketch_set" | "sketch_remove" | "sketch_rename" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let i = doc.sketches.iter().position(|s| s.name == name).ok_or_else(|| {
                CadError::Edit(format!("the part has no sketch named '{name}' (its sketches are: {})", doc.sketches.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))
            })?;
            match op {
                "sketch_set" => {
                    doc.sketches[i].sketch = sketch_arg(doc, args, op)?.0;
                    doc.resolved()?; // the features that use it must still build
                }
                "sketch_remove" => {
                    let users = sketch_users(doc, name);
                    if !users.is_empty() {
                        return bad(format!("the sketch '{name}' is used by {}: remove or re-point that feature first", users.join(", ")));
                    }
                    doc.sketches.remove(i);
                }
                _ => {
                    let Some(to) = args.get("to").and_then(|t| t.as_str()).map(str::trim).filter(|t| !t.is_empty()) else {
                        return bad("sketch_rename needs \"to\"");
                    };
                    let to = sketch_name(doc, Some(to), "Sketch")?;
                    doc.sketches[i].name = to.clone();
                    for f in &mut doc.features {
                        match &mut f.shape {
                            Shape::SketchSweep { profile, path, .. } => {
                                for r in [profile, path] {
                                    if r == name {
                                        *r = to.clone();
                                    }
                                }
                            }
                            Shape::SketchLoft { sections } => sections.iter_mut().filter(|s| *s == name).for_each(|s| *s = to.clone()),
                            _ => {}
                        }
                    }
                }
            }
            Ok(json!({}))
        }
        "sweep" | "loft" => {
            // A sweep of one sketch's outline along another's path, or a loft through the outlines
            // of sketches on parallel planes. The sketches are named (kept with sketch_add) or given.
            let name = new_name(doc, args, if op == "sweep" { "Sweep" } else { "Loft" })?;
            let shape = if op == "sweep" {
                let profile = sketch_ref(doc, args.get("profile").unwrap_or(&Value::Null), &name, "profile")?;
                let path = sketch_ref(doc, args.get("path").unwrap_or(&Value::Null), &name, "path")?;
                Shape::SketchSweep { profile, path, twist_deg: 0.0 }
            } else {
                let list = args.get("sections").and_then(|s| s.as_array()).cloned().unwrap_or_default();
                let mut sections = Vec::new();
                for v in &list {
                    sections.push(sketch_ref(doc, v, &name, "section")?);
                }
                Shape::SketchLoft { sections }
            };
            let mut f = Feature::new(name, op_of(args)?, shape);
            if let Some(t) = args.get("twist") {
                let (x, e) = num(doc, t)?;
                if let Shape::SketchSweep { twist_deg, .. } = &mut f.shape {
                    *twist_deg = x;
                }
                if let Some(e) = e {
                    f.bindings.push(Binding { path: "/shape/twist_deg".into(), expr: e });
                }
            }
            placement(doc, args, &mut f)?;
            // Checked now: the sketches must have what the feature needs.
            let mut probe = f.clone();
            crate::swept::lower(&doc.resolved()?, &mut probe)?;
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "sweep_set" | "loft_set" => {
            // Choose other sketches for a sweep (profile, path) or a loft (sections).
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let fname = doc.features[i].name.clone();
            let mut shape = doc.features[i].shape.clone();
            match &mut shape {
                Shape::SketchSweep { profile, path, .. } if op == "sweep_set" => {
                    if let Some(v) = args.get("profile") {
                        *profile = sketch_ref(doc, v, &fname, "profile")?;
                    }
                    if let Some(v) = args.get("path") {
                        *path = sketch_ref(doc, v, &fname, "path")?;
                    }
                }
                Shape::SketchLoft { sections } if op == "loft_set" => {
                    let list = args.get("sections").and_then(|s| s.as_array()).cloned().ok_or_else(|| CadError::Edit("loft_set needs \"sections\": [names]".into()))?;
                    sections.clear();
                    for v in &list {
                        sections.push(sketch_ref(doc, v, &fname, "section")?);
                    }
                }
                _ => return bad(format!("'{fname}' is not a {}", if op == "sweep_set" { "sweep" } else { "loft" })),
            }
            doc.features[i].shape = shape;
            let mut probe = doc.features[i].clone();
            crate::swept::lower(&doc.resolved()?, &mut probe)?;
            Ok(json!({}))
        }
        "mate" => {
            // A relation between two parts of the assembly, by a face of each.
            let kind_text = args.get("kind").and_then(|k| k.as_str()).unwrap_or("coincident");
            let kind = MateKind::parse(kind_text).ok_or_else(|| CadError::Edit(format!("a mate is coincident, concentric, distance, angle or fixed, not '{kind_text}'")))?;
            let a = mate_ref(doc, args.get("a"))?;
            let b = if kind == MateKind::Fixed { MateRef::default() } else { mate_ref(doc, args.get("b"))? };
            let known = |r: &MateRef| r.component.is_empty() || doc.components.iter().any(|c| c.name == r.component);
            for r in [&a, &b] {
                if !known(r) {
                    return bad(format!("the assembly has no part '{}' (its parts are: {})", r.component, doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")));
                }
            }
            if kind == MateKind::Fixed && a.component.is_empty() {
                return bad("a fixed mate names the part that stays in place: {\"kind\": \"fixed\", \"a\": \"Base\"}");
            }
            let (value, expr) = match args.get("value") {
                Some(v) if kind.has_value() => num(doc, v)?,
                _ => (0.0, None),
            };
            let base = match kind {
                MateKind::Fixed => format!("Fixed {}", a.component),
                k => {
                    let mut c = k.label().chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                }
            };
            let taken = |n: &str| doc.mates.iter().any(|m| m.name == n);
            let name = match args.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()) {
                Some(n) if taken(n) => return bad(format!("a mate named '{n}' already exists")),
                Some(n) => n.trim().to_string(),
                None => (1..).map(|k| if k == 1 && kind == MateKind::Fixed { base.clone() } else { format!("{base} {k}") }).find(|n| !taken(n)).unwrap(),
            };
            let flip = args.get("flip").and_then(|f| f.as_bool()).unwrap_or(false);
            doc.mates.push(Mate { name: name.clone(), kind, a, b, value, expr, flip, suppressed: false });
            // The faces must be there and of the right kind; what the mates leave free is reported.
            let (_, solved) = crate::kernel::assemble::solve_mates(crate::kernel::KernelChoice::Auto, doc, 48)?;
            let solved = solved.ok_or_else(|| CadError::Edit("the mate could not be checked".into()))?;
            if let Some(e) = solved.status.iter().find(|s| s.name == name).and_then(|s| s.error.clone()) {
                return bad(format!("mate '{name}': {e}"));
            }
            Ok(json!({ "mate": name, "dof": solved.dof, "status": solved.describe() }))
        }
        "mate_set" => {
            let i = find_mate(doc, args.get("mate"))?;
            if let Some(v) = args.get("value") {
                if !doc.mates[i].kind.has_value() {
                    return bad(format!("the mate '{}' is {}: it has no value to set", doc.mates[i].name, doc.mates[i].kind.label()));
                }
                let (x, e) = num(doc, v)?;
                doc.mates[i].value = x;
                doc.mates[i].expr = e;
            }
            if let Some(f) = args.get("flip").and_then(|f| f.as_bool()) {
                doc.mates[i].flip = f;
            }
            if let Some(s) = args.get("suppressed").and_then(|f| f.as_bool()) {
                doc.mates[i].suppressed = s;
            }
            if let Some(n) = args.get("to").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty()) {
                if doc.mates.iter().enumerate().any(|(k, m)| k != i && m.name == n) {
                    return bad(format!("a mate named '{n}' already exists"));
                }
                doc.mates[i].name = n.to_string();
            }
            Ok(json!({ "mate": doc.mates[i].name, "value": doc.mates[i].value }))
        }
        "mate_remove" => {
            let i = find_mate(doc, args.get("mate"))?;
            let m = doc.mates.remove(i);
            Ok(json!({ "removed": m.name }))
        }
        "edge_blend" | "shell_faces" => {
            // Round or chamfer picked edges, or hollow with picked open faces: the part so far.
            let picks = |k: &str| -> Result<Vec<[f64; 3]>, CadError> {
                args.get(k)
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().map(|p| Ok(vec3(Some(p), [f64::NAN; 3])).and_then(|v: [f64; 3]| if v[0].is_nan() { bad("a pick is a point [x, y, z] on the edge or face") } else { Ok(v) })).collect::<Result<Vec<_>, CadError>>())
                    .unwrap_or_else(|| Ok(Vec::new()))
            };
            let (shape, default_name) = if op == "edge_blend" {
                let chamfer = args.get("chamfer").and_then(|c| c.as_bool()).unwrap_or(false);
                let edges = picks("edges")?;
                if edges.is_empty() {
                    return bad("pick at least one edge (a point on each; cad.edges() lists them)");
                }
                (Shape::EdgeBlend { chamfer, size: num(doc, args.get("size").unwrap_or(&json!(1)))?.0, edges }, if chamfer { "Chamfer" } else { "Fillet" })
            } else {
                (Shape::ShellFaces { thickness: num(doc, args.get("thickness").unwrap_or(&json!(2)))?.0, faces: picks("faces")? }, "Shell")
            };
            let mut f = Feature::new(new_name(doc, args, default_name)?, BooleanOp::Add, shape);
            if let Some(e) = args.get("size").or(args.get("thickness")).and_then(|v| v.as_str()) {
                f.bindings.push(Binding { path: if op == "edge_blend" { "/shape/size".into() } else { "/shape/thickness".into() }, expr: e.into() });
            }
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "component_add" => {
            // A part placed beside the others: from a template, a document given as JSON, a CAD file
            // or a blank part to model in (`component_op` edits it afterwards).
            let part = part_from_args(args)?;
            let base = args.get("name").and_then(|n| n.as_str()).map(str::to_string).unwrap_or_else(|| if part.name.is_empty() { "Part".into() } else { part.name.clone() });
            let name = (1..).map(|k| if k == 1 { base.clone() } else { format!("{base} {k}") }).find(|n| !doc.components.iter().any(|c| &c.name == n)).unwrap();
            // Never reused, also after the last part is removed (face ids derive from it).
            doc.next_id = doc.next_id.max(doc.components.iter().map(|c| c.id.0).max().unwrap_or(0)) + 1;
            let id = Id(doc.next_id);
            let placement = Placement { translate: vec3(args.get("at"), [0.0; 3]), rotate_deg: vec3(args.get("rotate"), [0.0; 3]) };
            doc.components.push(crate::document::ComponentRef { id, name: name.clone(), document: Box::new(part), placement, visible: true });
            Ok(json!({ "component": name, "id": id.0 }))
        }
        "component_move" | "component_remove" | "component_visible" | "component_rename" | "component_op" => {
            let want = args.get("component").cloned().unwrap_or(Value::Null);
            let i = match &want {
                Value::String(n) => doc.components.iter().position(|c| &c.name == n),
                Value::Number(k) => k.as_u64().and_then(|k| doc.components.iter().position(|c| c.id.0 == k)),
                _ => None,
            }
            .ok_or_else(|| CadError::Edit(format!("no component {want}; the parts are: {}", doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))))?;
            match op {
                "component_move" => {
                    let c = &mut doc.components[i];
                    if args.get("at").is_some() {
                        c.placement.translate = vec3(args.get("at"), c.placement.translate);
                    }
                    if args.get("rotate").is_some() {
                        c.placement.rotate_deg = vec3(args.get("rotate"), c.placement.rotate_deg);
                    }
                    if let Some(by) = args.get("by") {
                        let d = vec3(Some(by), [0.0; 3]);
                        for k in 0..3 {
                            c.placement.translate[k] += d[k];
                        }
                    }
                    Ok(json!({ "at": c.placement.translate, "rotate": c.placement.rotate_deg }))
                }
                "component_remove" => {
                    let gone = doc.components.remove(i);
                    // Its mates go with it.
                    let before = doc.mates.len();
                    doc.mates.retain(|m| m.a.component != gone.name && m.b.component != gone.name);
                    Ok(json!({ "mates_removed": before - doc.mates.len() }))
                }
                "component_visible" => {
                    doc.components[i].visible = args.get("visible").and_then(|v| v.as_bool()).unwrap_or(true);
                    Ok(json!({}))
                }
                "component_rename" => {
                    let n = args.get("to").and_then(|n| n.as_str()).ok_or_else(|| CadError::Edit("component_rename needs \"to\"".into()))?;
                    if doc.components.iter().enumerate().any(|(k, c)| k != i && c.name == n) {
                        return bad(format!("a component is already called '{n}'"));
                    }
                    let old = std::mem::replace(&mut doc.components[i].name, n.to_string());
                    for m in &mut doc.mates {
                        for r in [&mut m.a, &mut m.b] {
                            if r.component == old {
                                r.component = n.to_string();
                            }
                        }
                    }
                    Ok(json!({}))
                }
                _ => {
                    // Any operation of the part API, applied to the component's own part.
                    let inner = args.get("inner_op").and_then(|o| o.as_str()).ok_or_else(|| CadError::Edit("component_op needs \"inner_op\" (an operation such as add or parameter)".into()))?;
                    if inner.starts_with("component_") {
                        return bad("a component's own components are edited by opening it (component_op does not nest)");
                    }
                    let rest = args.get("args").cloned().unwrap_or_else(|| json!({}));
                    apply_op(&mut doc.components[i].document, inner, &rest)
                }
            }
        }
        "face_pattern" => {
            // Holes (or bosses) laid out on a flat face: in a row, a grid or a circle about its center.
            let (center, normal) = if let Some(face) = args.get("face").and_then(|f| f.as_str()) {
                let ev = crate::kernel::evaluate_with(crate::kernel::KernelChoice::Auto, doc, 48, false)?;
                let f = ev.face_by_name(face).ok_or_else(|| CadError::Edit(format!("no face named '{face}' (cad.faces() lists them)")))?;
                if f.kind != crate::kernel::FaceKind::Plane {
                    return bad(format!("'{face}' is a {} face; a pattern on a face needs a flat one", f.kind.label()));
                }
                (f.centroid, f.direction)
            } else {
                (vec3(args.get("point"), [f64::NAN; 3]), vec3(args.get("normal"), [0.0, 0.0, 1.0]))
            };
            if center[0].is_nan() {
                return bad("face_pattern needs a face name (\"face\") or a point and normal on it");
            }
            let n = glam::DVec3::from_array(normal).normalize_or_zero();
            if n == glam::DVec3::ZERO {
                return bad("the face has no direction");
            }
            let q = glam::DQuat::from_rotation_arc(glam::DVec3::Z, n);
            let (rx, ry, rz) = q.to_euler(glam::EulerRot::XYZ);
            // The first copy's offset from the face's center along the face's own x and y (two numbers).
            let off: Vec<f64> = args.get("offset").and_then(|o| o.as_array()).map(|a| a.iter().filter_map(|v| v.as_f64()).collect()).unwrap_or_default();
            if off.len() > 3 {
                return bad("offset is [along x, along y] on the face");
            }
            let at = [off.first().copied().unwrap_or(0.0), off.get(1).copied().unwrap_or(0.0)];
            let origin = glam::DVec3::from_array(center) + q * glam::DVec3::new(at[0], at[1], 0.0);
            let boss = args.get("kind").and_then(|k| k.as_str()) == Some("boss");
            let size = |k: &str, d: f64| -> Result<f64, CadError> { Ok(num(doc, args.get(k).unwrap_or(&json!(d)))?.0) };
            let (shape, op_kind, default_name) = if boss {
                (Shape::Cylinder { radius: size("diameter", 6.0)? / 2.0, height: size("height", 8.0)? }, BooleanOp::Add, "Boss")
            } else {
                (Shape::Hole { kind: HoleKind::Simple, diameter: size("diameter", 6.0)?, depth: size("depth", 10.0)? }, BooleanOp::Cut, "Hole")
            };
            let mut f = Feature::new(new_name(doc, args, default_name)?, op_kind, shape);
            f.placement = Placement { translate: origin.to_array(), rotate_deg: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()] };
            let count = args.get("count").and_then(|c| c.as_u64()).unwrap_or(4).max(1) as usize;
            match args.get("layout").and_then(|l| l.as_str()).unwrap_or("linear") {
                "linear" => f.modifiers.push(Modifier::LinearPattern { count, step: [size("spacing", 12.0)?, 0.0, 0.0] }),
                "grid" => {
                    let counts = args.get("counts").and_then(|c| c.as_array()).map(|a| [a.first().and_then(|v| v.as_u64()).unwrap_or(2) as usize, a.get(1).and_then(|v| v.as_u64()).unwrap_or(2) as usize]).unwrap_or([2, 2]);
                    let sp = args.get("spacing").and_then(|s| s.as_array()).map(|a| [a.first().and_then(|v| v.as_f64()).unwrap_or(12.0), a.get(1).and_then(|v| v.as_f64()).unwrap_or(12.0)]).unwrap_or([12.0, 12.0]);
                    f.modifiers.push(Modifier::GridPattern { counts, step: sp });
                }
                "circular" => {
                    f.modifiers.push(Modifier::Offset { by: [size("radius", 15.0)?, 0.0, 0.0] });
                    f.modifiers.push(Modifier::CircularPattern { count, axis_z: true });
                }
                other => return bad(format!("unknown layout '{other}' (linear, grid, circular)")),
            }
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "combine" => {
            // Another part (a template, a document, a CAD file) added to, cut from or intersected with the
            // part so far, placed by `at` / `rotate`.
            let part = part_from_args(args)?;
            if part.features.iter().all(|f| f.suppressed) {
                return bad(if part.components.is_empty() {
                    "the part to combine has no solid: give a template, a document or a file"
                } else {
                    "the part to combine is an assembly: combine one of its parts"
                });
            }
            let op_kind = op_of(&json!({ "op": args.get("op").cloned().unwrap_or(json!("add")), "kind": "" }))?;
            let name = new_name(doc, args, &format!("Combine {}", part.name))?;
            let mut f = Feature::new(name, op_kind, Shape::Body { part: Box::new(part) });
            f.placement = Placement { translate: vec3(args.get("at"), [0.0; 3]), rotate_deg: vec3(args.get("rotate"), [0.0; 3]) };
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "mirror_plane" => {
            // Reflect the part so far about a plane (origin and normal; a flat face gives both).
            let origin = vec3(args.get("origin"), [0.0; 3]);
            let normal = vec3(args.get("normal"), [f64::NAN; 3]);
            if normal[0].is_nan() || normal.iter().all(|c| *c == 0.0) {
                return bad("mirror_plane needs a normal [x, y, z] (and an origin on the plane)");
            }
            let f = Feature::new(new_name(doc, args, "Mirror")?, BooleanOp::Add, Shape::MirrorPlane { origin, normal });
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "draft" => {
            // Taper picked side faces by an angle about a neutral plane (the B-rep kernel builds it).
            let faces: Vec<[f64; 3]> = args.get("faces").and_then(|a| a.as_array()).map(|a| a.iter().map(|p| vec3(Some(p), [0.0; 3])).collect()).unwrap_or_default();
            if faces.is_empty() {
                return bad("pick at least one face to draft (a point on each; cad.faces() lists them)");
            }
            let (angle, _) = num(doc, args.get("angle").unwrap_or(&json!(3)))?;
            let neutral_origin = vec3(args.get("neutral_origin"), [0.0; 3]);
            let neutral_normal = vec3(args.get("neutral_normal"), [0.0, 0.0, 1.0]);
            let mut f = Feature::new(new_name(doc, args, "Draft")?, BooleanOp::Add, Shape::Draft { angle_deg: angle, faces, neutral_origin, neutral_normal });
            if let Some(e) = args.get("angle").and_then(|v| v.as_str()) {
                f.bindings.push(Binding { path: "/shape/angle_deg".into(), expr: e.into() });
            }
            let id = doc.push(f);
            Ok(feature_summary(doc.feature(id).unwrap()))
        }
        "set_op" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            doc.features[i].op = op_of(&json!({ "op": args.get("to").cloned().unwrap_or(Value::Null), "kind": "" }))?;
            Ok(json!({}))
        }
        "remove_modifier" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let k = args.get("index").and_then(|k| k.as_u64()).unwrap_or(u64::MAX) as usize;
            if k >= doc.features[i].modifiers.len() {
                return bad(format!("'{}' has {} modifiers", doc.features[i].name, doc.features[i].modifiers.len()));
            }
            doc.features[i].modifiers.remove(k);
            unbind_modifier(&mut doc.features[i], k);
            Ok(json!({}))
        }
        "remove_finish" => {
            let k = args.get("index").and_then(|k| k.as_u64()).unwrap_or(u64::MAX) as usize;
            if k >= doc.finish.len() {
                return bad(format!("the part has {} finishing modifiers", doc.finish.len()));
            }
            doc.finish.remove(k);
            Ok(json!({}))
        }
        "set_finish" => {
            let k = args.get("index").and_then(|k| k.as_u64()).unwrap_or(u64::MAX) as usize;
            let field = args.get("field").and_then(|f| f.as_str()).ok_or_else(|| CadError::Edit("set_finish needs a \"field\"".into()))?;
            let (x, _) = num(doc, args.get("value").unwrap_or(&Value::Null))?;
            let m = doc.finish.get(k).ok_or_else(|| CadError::Edit(format!("the part has {} finishing modifiers", doc.finish.len())))?;
            let mut v = serde_json::to_value(m).map_err(|e| CadError::Edit(e.to_string()))?;
            let slot = v.get_mut(field).ok_or_else(|| CadError::Edit(format!("no field '{field}' on that modifier")))?;
            *slot = if slot.is_u64() { json!(x.round().max(0.0) as u64) } else { json!(x) };
            doc.finish[k] = serde_json::from_value(v).map_err(|e| CadError::Edit(e.to_string()))?;
            Ok(json!({}))
        }
        "set_dim" => {
            let i = find(doc, args.get("feature").unwrap_or(&Value::Null))?;
            let c = args.get("constraint").and_then(|c| c.as_u64()).unwrap_or(u64::MAX) as usize;
            let d = dim(doc, args.get("value").unwrap_or(&Value::Null))?;
            let name = doc.features[i].name.clone();
            let sk = doc.features[i].shape.sketch_mut().ok_or_else(|| CadError::Edit(format!("'{name}' has no sketch")))?;
            let slot = sk.constraints.get_mut(c).and_then(|k| k.dim_mut()).ok_or_else(|| CadError::Edit(format!("constraint {c} of the sketch of '{name}' is not a dimension")))?;
            *slot = d;
            doc.resolved()?; // the sketch must still solve
            Ok(json!({}))
        }
        "configuration" => {
            let name = args.get("name").and_then(|n| n.as_str()).ok_or_else(|| CadError::Edit("a configuration needs a name".into()))?;
            let overrides: Vec<Parameter> = args
                .get("overrides")
                .and_then(|o| o.as_object())
                .map(|o| o.iter().map(|(k, v)| Parameter::new(k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect())
                .unwrap_or_default();
            doc.configurations.retain(|c| c.name != name);
            doc.configurations.push(Configuration { name: name.into(), overrides });
            Ok(json!({}))
        }
        "remove_configuration" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let n0 = doc.configurations.len();
            doc.configurations.retain(|c| c.name != name);
            if doc.configurations.len() == n0 {
                return bad(format!("no configuration '{name}'"));
            }
            if doc.active_configuration.as_deref() == Some(name) {
                doc.active_configuration = None;
            }
            Ok(json!({}))
        }
        "plane" => {
            let rp = ref_plane_of(doc, args)?;
            if ["XY", "XZ", "YZ", "TOP", "FRONT", "RIGHT"].contains(&rp.name.to_uppercase().as_str()) {
                return bad(format!("'{}' is the name of a principal plane", rp.name));
            }
            let name = rp.name.clone();
            match doc.planes.iter_mut().find(|p| p.name == name) {
                Some(p) => *p = rp,
                None => doc.planes.push(rp),
            }
            Ok(json!({"name": name}))
        }
        "remove_plane" => {
            let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let n0 = doc.planes.len();
            doc.planes.retain(|p| p.name != name);
            if doc.planes.len() == n0 {
                return bad(format!("no plane '{name}'"));
            }
            Ok(json!({}))
        }
        "activate" => {
            let name = args.get("name").and_then(|n| n.as_str()).map(str::to_string);
            if let Some(n) = &name {
                if !doc.configurations.iter().any(|c| &c.name == n) {
                    return bad(format!("no configuration '{n}'"));
                }
            }
            doc.active_configuration = name;
            Ok(json!({}))
        }
        other => bad(format!(
            "unknown CAD operation '{other}' (new, template, parameter, add, sketch_extrude, sketch_revolve, sketch_add, sketch_set, sketch_remove, sketch_rename, sketch_face, sweep, loft, sweep_set, loft_set, mate, mate_set, mate_remove, modifier, suppress, remove, rename, move, bind, mirror_plane, draft, face_pattern, combine, component_add, component_move, component_remove, component_visible, component_rename, component_op, set, set_op, set_dim, set_json, set_sketch, edge_blend, shell_faces, remove_modifier, remove_finish, set_finish, configuration, remove_configuration, activate, plane, remove_plane)"
        )),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

impl Document {
    /// The features as JSON summaries (id, name, op, shape, suppressed), in timeline order.
    pub fn feature_list(&self) -> Vec<Value> {
        self.features.iter().map(feature_summary).collect()
    }
    /// The resolved parameters in order of definition: `(name, expression, value or error)`.
    pub fn parameter_table(&self) -> Vec<(String, String, Result<f64, String>)> {
        let vals = self.parameter_values();
        self.parameters
            .iter()
            .map(|p| {
                let v = match &vals {
                    Ok(v) => v.get(&p.name).copied().ok_or_else(|| "no value".to_string()),
                    Err(e) => Err(e.to_string()),
                };
                (p.name.clone(), p.expr.clone(), v)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::implicit::ImplicitKernel;
    use crate::kernel::Kernel;

    fn run(doc: &mut Document, op: &str, args: Value) -> Value {
        apply_op(doc, op, &args).unwrap_or_else(|e| panic!("{op}: {e}"))
    }

    #[test]
    fn a_plate_with_a_hole_is_built_by_operations_and_follows_its_parameters() {
        let mut d = Document::new("plate");
        run(&mut d, "parameter", json!({"name": "width", "expr": "40 mm"}));
        run(&mut d, "parameter", json!({"name": "d", "expr": "width / 8"}));
        run(
            &mut d,
            "sketch_extrude",
            json!({"name": "Plate", "depth": 10, "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": "width", "h": 30}}]}}),
        );
        run(&mut d, "add", json!({"kind": "hole", "name": "Bore", "op": "cut", "diameter": "d", "depth": 30, "at": [0, 0, 12]}));
        let k = ImplicitKernel::new(96);
        let e = k.evaluate(&d).unwrap();
        let expect = 40.0 * 30.0 * 10.0 - std::f64::consts::PI * 2.5 * 2.5 * 10.0; // d = width / 8 = 5
        assert!((e.volume() - expect).abs() / expect < 0.02, "{} vs {expect}", e.volume());
        assert!(e.face_by_name("Bore side").is_some(), "{:?}", e.faces().map(|(_, f)| &f.name).collect::<Vec<_>>());
        // The whole model follows the parameter.
        run(&mut d, "parameter", json!({"name": "width", "expr": 60}));
        let e2 = k.evaluate(&d).unwrap();
        let expect2 = 60.0 * 30.0 * 10.0 - std::f64::consts::PI * (60.0f64 / 16.0).powi(2) * 10.0; // d = 7.5
        assert!((e2.volume() - expect2).abs() / expect2 < 0.02, "{} vs {expect2}", e2.volume());
    }

    #[test]
    fn mistakes_get_messages_that_say_what_to_do() {
        let mut d = Document::new("x");
        run(&mut d, "add", json!({"kind": "box", "name": "A"}));
        let m = |r: Result<Value, CadError>| r.unwrap_err().to_string();
        assert!(m(apply_op(&mut d, "add", &json!({"kind": "cube2"}))).contains("unknown shape"));
        assert!(m(apply_op(&mut d, "suppress", &json!({"feature": "B"}))).contains("features are: A"));
        assert!(m(apply_op(&mut d, "parameter", &json!({"name": "a b", "expr": "1"}))).contains("usable parameter name"));
        assert!(m(apply_op(&mut d, "parameter", &json!({"name": "p", "expr": "q + 1"}))).to_lowercase().contains("unknown"));
        assert!(m(apply_op(&mut d, "frobnicate", &json!({}))).contains("unknown CAD operation"));
        assert!(m(apply_op(&mut d, "modifier", &json!({"modifier": {"kind": "Nope"}}))).contains("Fillet"));
    }

    #[test]
    fn suppress_reorder_rename_remove_configurations() {
        let mut d = Document::new("x");
        run(&mut d, "parameter", json!({"name": "s", "expr": 10}));
        run(&mut d, "add", json!({"kind": "box", "name": "A", "size": ["s", "s", "s"]}));
        run(&mut d, "add", json!({"kind": "sphere", "name": "B", "radius": 3, "at": [0, 0, 10]}));
        run(&mut d, "rename", json!({"feature": "B", "to": "Ball"}));
        run(&mut d, "move", json!({"feature": "Ball", "to": 0}));
        assert_eq!(d.features[0].name, "Ball");
        run(&mut d, "suppress", json!({"feature": "Ball"}));
        assert!(d.features[0].suppressed);
        run(&mut d, "configuration", json!({"name": "big", "overrides": {"s": 20}}));
        run(&mut d, "activate", json!({"name": "big"}));
        let r = d.resolved().unwrap();
        assert!(matches!(r.features[1].shape, Shape::Box { size, .. } if size == [20.0; 3]));
        run(&mut d, "remove", json!({"feature": "Ball"}));
        assert_eq!(d.features.len(), 1);
        // Ids are not reused after a removal.
        let id = run(&mut d, "add", json!({"kind": "cone"}))["id"].as_u64().unwrap();
        assert!(id > 2, "{id}");
    }

    #[test]
    fn templates_by_name_and_feature_list() {
        let mut d = Document::new("x");
        run(&mut d, "template", json!({"name": "flange"}));
        assert!(d.features.len() >= 3);
        assert!(d.feature_list().iter().all(|f| f["id"].as_u64().unwrap() > 0));
        assert!(apply_op(&mut d, "template", &json!({"name": "zzz"})).unwrap_err().to_string().contains("there are"));
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;

    #[test]
    fn the_properties_operations_edit_what_the_manager_shows() {
        let mut d = crate::templates::bracket();
        // A sketch dimension, by number and by expression.
        let f = d.features.iter().find(|f| matches!(f.shape, Shape::SketchExtrude { .. })).unwrap().name.clone();
        let (c, before) = {
            let Shape::SketchExtrude { sketch, .. } = &d.features.iter().find(|x| x.name == f).unwrap().shape else { unreachable!() };
            let c = sketch.constraints.iter().position(|k| k.dim().is_some()).unwrap();
            (c, sketch.constraints[c].dim().unwrap().value)
        };
        apply_op(&mut d, "set_dim", &json!({"feature": f, "constraint": c, "value": before + 5.0})).unwrap();
        let Shape::SketchExtrude { sketch, .. } = &d.features.iter().find(|x| x.name == f).unwrap().shape else { unreachable!() };
        assert_eq!(sketch.constraints[c].dim().unwrap().value, before + 5.0);
        apply_op(&mut d, "parameter", &json!({"name": "k", "expr": 3})).unwrap();
        apply_op(&mut d, "set_dim", &json!({"feature": f, "constraint": c, "value": "k * 4"})).unwrap();
        assert_eq!(
            d.resolved().unwrap().features.iter().find(|x| x.name == f).and_then(|x| match &x.shape {
                Shape::SketchExtrude { sketch, .. } => sketch.constraints[c].dim().map(|d| d.value),
                _ => None,
            }),
            Some(12.0)
        );
        assert!(apply_op(&mut d, "set_dim", &json!({"feature": f, "constraint": 9999, "value": 1})).is_err());
        // Operation, modifiers, finish.
        let mut d = Document::new("x");
        apply_op(&mut d, "add", &json!({"kind": "box", "name": "A"})).unwrap();
        apply_op(&mut d, "set_op", &json!({"feature": "A", "to": "cut"})).unwrap();
        assert_eq!(d.features[0].op, BooleanOp::Cut);
        apply_op(&mut d, "modifier", &json!({"feature": "A", "modifier": {"kind": "Fillet", "radius": 1}})).unwrap();
        apply_op(&mut d, "remove_modifier", &json!({"feature": "A", "index": 0})).unwrap();
        assert!(d.features[0].modifiers.is_empty());
        apply_op(&mut d, "modifier", &json!({"modifier": {"kind": "Fillet", "radius": 1}})).unwrap();
        apply_op(&mut d, "set_finish", &json!({"index": 0, "field": "radius", "value": 2.5})).unwrap();
        assert_eq!(d.finish[0], Modifier::Fillet { radius: 2.5 });
        apply_op(&mut d, "remove_finish", &json!({"index": 0})).unwrap();
        assert!(d.finish.is_empty());
        assert!(apply_op(&mut d, "remove_finish", &json!({"index": 0})).unwrap_err().to_string().contains("0 finishing"));
    }
}
