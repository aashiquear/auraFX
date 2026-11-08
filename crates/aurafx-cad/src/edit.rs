//! Editing a document with plain data: one operation name and a JSON object of arguments, the
//! same calls the Python `cad` object, the assistant and the PropertyManager make, so a script
//! and the interactive environment produce identical documents (and the same trace).
//!
//! Numbers in an operation may be written as expressions of the document's parameters
//! (`"width / 2"`): the field then becomes a binding, so changing the parameter moves the
//! geometry, and the stored number is the expression's value now.

use crate::document::*;
use crate::expr;
use crate::path::shapes;
use crate::sketch::{Constraint, Dim, Entity, Sketch, SketchPlane};
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
    Ok(Dim { value: x, expr: e })
}

fn vec3(v: Option<&Value>, default: [f64; 3]) -> [f64; 3] {
    v.and_then(|v| v.as_array())
        .filter(|a| a.len() == 3)
        .map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)])
        .unwrap_or(default)
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

/// A sketch from its description:
/// `{"plane": "XY", "offset": 0, "items": [{"rectangle": {"corner": [0, 0], "w": "width", "h": 20}},
/// {"circle": {"center": [5, 5], "r": 2}}, {"polygon": [[0, 0], [10, 0], [5, 8]]}]}`.
/// Rectangles and circles come fully dimensioned (with the expressions as dimensions), polygons
/// as free lines.
pub fn sketch_from_spec(doc: &Document, spec: &Value) -> Result<Sketch, CadError> {
    let mut s = Sketch::new(plane_of(spec.get("plane"))?);
    s.offset = spec.get("offset").and_then(|o| o.as_f64()).unwrap_or(0.0);
    let items = spec.get("items").and_then(|i| i.as_array()).ok_or_else(|| CadError::Edit("a sketch needs \"items\": [...]".into()))?;
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
            k => return bad(format!("unknown sketch item '{k}': rectangle, circle, polygon or regular")),
        }
    }
    Ok(s)
}

fn feature_summary(f: &Feature) -> Value {
    json!({
        "id": f.id.0, "name": f.name, "op": format!("{:?}", f.op).to_lowercase(),
        "shape": serde_json::to_value(&f.shape).ok().and_then(|v| v.get("kind").cloned()).unwrap_or(Value::Null),
        "suppressed": f.suppressed,
    })
}

/// Apply one operation to the document; returns what it made (a feature's id and name, ...).
pub fn apply_op(doc: &mut Document, op: &str, args: &Value) -> Result<Value, CadError> {
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
            Ok(json!({}))
        }
        "add" => {
            let kind = args.get("kind").and_then(|k| k.as_str()).ok_or_else(|| CadError::Edit("add needs a kind".into()))?;
            let mut f = Feature::new(args.get("name").and_then(|n| n.as_str()).unwrap_or(&capitalise(kind)), op_of(args)?, default_shape(kind)?);
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
            let sk = match args.get("sketch_json") {
                Some(j) => serde_json::from_value::<Sketch>(j.clone()).map_err(|e| CadError::Edit(format!("sketch_json: {e}")))?,
                None => sketch_from_spec(doc, args.get("sketch").ok_or_else(|| CadError::Edit(format!("{op} needs a sketch")))?)?,
            };
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
                    draft_deg: args.get("draft").and_then(|d| d.as_f64()).unwrap_or(0.0),
                    top_edge: edge("top")?,
                    bottom_edge: edge("bottom")?,
                    reverse: args.get("reverse").and_then(|r| r.as_bool()).unwrap_or(false),
                }, de.map(|e| ("/shape/depth", e)))
            } else {
                let (angle, ae) = num(doc, args.get("angle").unwrap_or(&json!(360)))?;
                (Shape::SketchRevolve { sketch: sk, angle_deg: angle }, ae.map(|e| ("/shape/angle_deg", e)))
            };
            let default_name = if op == "sketch_extrude" { "Extrude" } else { "Revolve" };
            let mut f = Feature::new(args.get("name").and_then(|n| n.as_str()).unwrap_or(default_name), op_of(args)?, shape.0);
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
            doc.features[i].name = args.get("to").and_then(|t| t.as_str()).ok_or_else(|| CadError::Edit("rename needs \"to\"".into()))?.to_string();
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
            // The general edit the PropertyManager uses for choices (an end condition, a hole
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
            let mut f = Feature::new(args.get("name").and_then(|n| n.as_str()).unwrap_or(default_name), BooleanOp::Add, shape);
            if let Some(e) = args.get("size").or(args.get("thickness")).and_then(|v| v.as_str()) {
                f.bindings.push(Binding { path: if op == "edge_blend" { "/shape/size".into() } else { "/shape/thickness".into() }, expr: e.into() });
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
            "unknown CAD operation '{other}' (new, template, parameter, add, sketch_extrude, sketch_revolve, modifier, suppress, remove, rename, move, bind, set, set_op, set_dim, set_json, set_sketch, edge_blend, shell_faces, remove_modifier, remove_finish, set_finish, configuration, activate)"
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
