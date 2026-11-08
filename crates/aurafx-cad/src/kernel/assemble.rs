//! Assemblies: a document's `components` (each a part of its own, placed by a translation and an
//! XYZ rotation) built with the chosen kernel and put side by side in one [`Evaluated`]. Every
//! component keeps its bodies and faces apart (named `Component/Face`), so interference, mass
//! and the downstream check see each part separately.

use super::{evaluate_with, Evaluated, EvaluatedBody, EvaluatedFace, KernelChoice};
use crate::document::{Document, Placement};
use crate::{CadError, Id};
use aurafx_data::*;
use glam::{DMat3, DQuat, DVec3, EulerRot};
use std::sync::Arc;

/// The rotation of a placement (XYZ Euler angles in degrees).
pub fn rotation(pl: &Placement) -> DMat3 {
    DMat3::from_quat(DQuat::from_euler(EulerRot::XYZ, pl.rotate_deg[0].to_radians(), pl.rotate_deg[1].to_radians(), pl.rotate_deg[2].to_radians()))
}

/// A point of the part in assembly coordinates.
pub fn place_point(pl: &Placement, p: [f64; 3]) -> [f64; 3] {
    (rotation(pl) * DVec3::from_array(p) + DVec3::from_array(pl.translate)).to_array()
}

/// The ids of a component's features and bodies are offset so that two parts never share one:
/// component `k` (1-based) owns the range `k * STRIDE ..`.
pub const ID_STRIDE: u64 = 1_000_000;

struct Part {
    /// Prefix of the names ("" for the document's own part).
    name: String,
    placement: Placement,
    ev: Evaluated,
    /// Offset added to every id of this part.
    base: u64,
}

/// Evaluate the document's own features (when it has any) and every visible component.
pub(super) fn evaluate_assembly(choice: KernelChoice, doc: &Document, resolution: usize, project: bool) -> Result<Evaluated, CadError> {
    let mut parts: Vec<Part> = Vec::new();
    if doc.features.iter().any(|f| !f.suppressed) {
        let mut own = doc.clone();
        own.components.clear();
        parts.push(Part { name: String::new(), placement: Placement::default(), ev: evaluate_with(choice, &own, resolution, project)?, base: 0 });
    }
    for c in doc.components.iter().filter(|c| c.visible) {
        let ev = evaluate_with(choice, &c.document, resolution, project).map_err(|e| CadError::Edit(format!("component '{}': {e}", c.name)))?;
        parts.push(Part { name: c.name.clone(), placement: c.placement.clone(), ev, base: c.id.0 * ID_STRIDE });
    }
    if parts.is_empty() {
        return Err(CadError::Kernel { kernel: "assembly", reason: "the assembly has no visible part yet: add a component or a feature".into() });
    }
    Ok(merge(parts))
}

fn merge(parts: Vec<Part>) -> Evaluated {
    let label = |part: &str, name: &str| if part.is_empty() { name.to_string() } else { format!("{part}/{name}") };
    let mut pts: Vec<f64> = Vec::new();
    let mut polys = Cells::new();
    let (mut body_cell, mut face_cell) = (Vec::new(), Vec::new());
    let (mut face_names, mut body_ids, mut body_names, mut volumes, mut areas) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut bodies: Vec<EvaluatedBody> = Vec::new();
    let mut approximate: Vec<String> = Vec::new();
    let mut all_brep = true;
    for p in &parts {
        all_brep &= p.ev.kernel == "brep";
        approximate.extend(p.ev.approximate.iter().map(|a| if p.name.is_empty() { a.clone() } else { format!("{}: {a}", p.name) }));
        let r = rotation(&p.placement);
        let t = DVec3::from_array(p.placement.translate);
        let (lo_pt, face_base) = (pts.len() / 3, face_names.len() as i32);
        if let DataSet::PolyData(pd) = &p.ev.surface {
            for i in 0..pd.points.len() {
                pts.extend((r * DVec3::from_array(pd.points.get(i)) + t).to_array());
            }
            let faces = pd.cell_data.get("FaceId");
            let bodies_of = pd.cell_data.get("BodyId");
            for (ci, (_, c)) in pd.polys.iter().enumerate() {
                polys.push(CellType::Triangle, &c.iter().map(|i| i + lo_pt as u32).collect::<Vec<u32>>());
                face_cell.push(faces.map(|f| f.get(ci, 0) as i32).unwrap_or(0) + face_base);
                body_cell.push(bodies_of.map(|b| b.get(ci, 0) as i64 + p.base as i64).unwrap_or(p.base as i64) as i32);
            }
            if let Some(Array { data: ArrayData::String(names), .. }) = pd.field_data.get("FaceNames") {
                face_names.extend(names.iter().map(|n| label(&p.name, n)));
            }
        }
        for b in &p.ev.bodies {
            let corners = |lo: [f64; 3], hi: [f64; 3]| {
                let (mut a, mut z) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
                for i in 0..8 {
                    let c = DVec3::new(if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] });
                    let w = r * c + t;
                    a = a.min(w);
                    z = z.max(w);
                }
                (a.to_array(), z.to_array())
            };
            let id = Id(b.id.0 + p.base);
            body_ids.push(id.0 as i32);
            body_names.push(label(&p.name, &b.name));
            volumes.push(b.volume);
            areas.push(b.area);
            bodies.push(EvaluatedBody {
                id,
                name: label(&p.name, &b.name),
                faces: b
                    .faces
                    .iter()
                    .map(|f| EvaluatedFace {
                        id: (Id(f.id.0 .0 + p.base), f.id.1),
                        face_id: (f.face_id as i32 + face_base) as u32,
                        name: label(&p.name, &f.name),
                        kind: f.kind,
                        area: f.area,
                        centroid: (r * DVec3::from_array(f.centroid) + t).to_array(),
                        direction: (r * DVec3::from_array(f.direction)).to_array(),
                    })
                    .collect(),
                volume: b.volume,
                area: b.area,
                centroid: (r * DVec3::from_array(b.centroid) + t).to_array(),
                bounds: corners(b.bounds.0, b.bounds.1),
            });
        }
    }
    let mut field = Attributes::new();
    field.insert(Array::new("FaceNames", 1, ArrayData::String(face_names)));
    field.insert(Array::new("BodyIds", 1, ArrayData::I32(body_ids)));
    field.insert(Array::new("BodyNames", 1, ArrayData::String(body_names)));
    field.insert(Array::scalars_f64("Volume", volumes));
    field.insert(Array::scalars_f64("SurfaceArea", areas));
    let mut pd = PolyData { points: Points::new_f64(pts), polys, field_data: field, ..Default::default() };
    pd.cell_data.insert(Array::new("BodyId", 1, ArrayData::I32(body_cell)));
    pd.cell_data.insert(Array::new("FaceId", 1, ArrayData::I32(face_cell)));
    // The union of the parts: the smallest of their distances, each seen from its own frame.
    let fields: Option<Vec<(DMat3, DVec3, super::DistanceFn)>> =
        parts.iter().map(|p| p.ev.distance.clone().map(|d| (rotation(&p.placement).transpose(), DVec3::from_array(p.placement.translate), d))).collect();
    let distance: Option<super::DistanceFn> = fields.map(|fs| {
        let fs = Arc::new(fs);
        let f: super::DistanceFn =
            Arc::new(move |p: [f64; 3]| fs.iter().map(|(inv, t, d)| d((*inv * (DVec3::from_array(p) - *t)).to_array())).fold(f64::INFINITY, f64::min));
        f
    });
    Evaluated { bodies, surface: DataSet::PolyData(pd), distance, kernel: if all_brep { "brep" } else { "implicit" }, approximate }
}

/// The overlap volume (mm³) of every pair of visible parts of an assembly, the document's own
/// part (called `(part)`) included: only the pairs that overlap are listed, largest first.
pub fn component_interference(choice: KernelChoice, doc: &Document, resolution: f64) -> Result<Vec<(String, String, f64)>, CadError> {
    let mut parts: Vec<(String, Placement, Evaluated)> = Vec::new();
    if doc.features.iter().any(|f| !f.suppressed) {
        let mut own = doc.clone();
        own.components.clear();
        parts.push(("(part)".into(), Placement::default(), evaluate_with(choice, &own, 64, true)?));
    }
    for c in doc.components.iter().filter(|c| c.visible) {
        parts.push((c.name.clone(), c.placement.clone(), evaluate_with(choice, &c.document, 64, true)?));
    }
    let mut out = Vec::new();
    for i in 0..parts.len() {
        for j in i + 1..parts.len() {
            let v = crate::analysis::interference_built(&parts[i].2, &parts[i].1, &parts[j].2, &parts[j].1, resolution);
            if v > 1e-9 {
                out.push((parts[i].0.clone(), parts[j].0.clone(), v));
            }
        }
    }
    out.sort_by(|a, b| b.2.total_cmp(&a.2));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::apply_op;
    use serde_json::json;

    fn plate() -> Document {
        let mut d = Document::new("Plate");
        apply_op(&mut d, "add", &json!({"kind": "box", "name": "Slab", "size": [40, 40, 10], "at": [0, 0, 5]})).unwrap();
        d
    }

    #[test]
    fn an_assembly_keeps_its_parts_apart_places_them_and_names_their_faces() {
        let mut a = Document::new("Assembly");
        apply_op(&mut a, "component_add", &json!({"name": "Base", "document": serde_json::to_value(plate()).unwrap()})).unwrap();
        apply_op(&mut a, "component_add", &json!({"name": "Cap", "document": serde_json::to_value(plate()).unwrap(), "at": [100, 0, 0], "rotate": [0, 0, 90]}))
            .unwrap();
        for choice in [KernelChoice::Brep, KernelChoice::Implicit] {
            let ev = evaluate_with(choice, &a, 64, true).unwrap();
            assert_eq!(ev.bodies.len(), 2, "{choice:?}");
            assert!((ev.volume() - 32000.0).abs() < 0.02 * 32000.0, "{choice:?}: {}", ev.volume());
            // The second part sits at x = 100: its faces are found there and carry its name.
            let cap = ev.face_by_name("Cap/Slab z+").expect("faces are named Component/Face");
            assert!((cap.centroid[0] - 100.0).abs() < 1.0 && (cap.centroid[2] - 10.0).abs() < 0.5, "{choice:?}: {:?}", cap.centroid);
            assert!(ev.face_by_name("Base/Slab z+").is_some());
            // Face ids are unique over the whole assembly.
            let mut ids: Vec<u32> = ev.faces().map(|(_, f)| f.face_id).collect();
            let n = ids.len();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), n);
            let DataSet::PolyData(pd) = &ev.surface else { panic!() };
            assert_eq!(pd.cell_data.get("FaceId").unwrap().len(), pd.polys.len());
            // The distance field is the union: inside each part, outside between them.
            let d = ev.distance.as_ref().unwrap();
            assert!(d([0.0, 0.0, 5.0]) < 0.0 && d([100.0, 0.0, 5.0]) < 0.0 && d([50.0, 0.0, 5.0]) > 5.0, "{choice:?}");
        }
    }

    #[test]
    fn overlapping_parts_are_found_and_measured() {
        let mut a = Document::new("Assembly");
        apply_op(&mut a, "component_add", &json!({"name": "A", "document": serde_json::to_value(plate()).unwrap()})).unwrap();
        apply_op(&mut a, "component_add", &json!({"name": "B", "document": serde_json::to_value(plate()).unwrap(), "at": [30, 0, 0]})).unwrap();
        apply_op(&mut a, "component_add", &json!({"name": "C", "document": serde_json::to_value(plate()).unwrap(), "at": [200, 0, 0]})).unwrap();
        // A and B overlap over x in 10..20: a 10 x 40 x 10 slab.
        let r = component_interference(KernelChoice::Auto, &a, 0.5).unwrap();
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!((r[0].0.as_str(), r[0].1.as_str()), ("A", "B"));
        assert!((r[0].2 - 4000.0).abs() < 80.0, "{r:?}");
        // Move B away and nothing overlaps.
        apply_op(&mut a, "component_move", &json!({"component": "B", "at": [60, 0, 0]})).unwrap();
        assert!(component_interference(KernelChoice::Auto, &a, 0.5).unwrap().is_empty());
        // The part of a component is edited in place.
        apply_op(&mut a, "component_op", &json!({"component": "B", "inner_op": "parameter", "args": {"name": "w", "expr": "12"}})).unwrap();
        assert_eq!(a.components[1].document.parameters.len(), 1);
        apply_op(&mut a, "component_visible", &json!({"component": "C", "visible": false})).unwrap();
        assert_eq!(evaluate_with(KernelChoice::Auto, &a, 64, true).unwrap().bodies.len(), 2);
        apply_op(&mut a, "component_remove", &json!({"component": "A"})).unwrap();
        assert_eq!(a.components.len(), 2);
        assert!(apply_op(&mut a, "component_move", &json!({"component": "Nope"})).unwrap_err().to_string().contains("the parts are"));
    }

    #[test]
    fn an_assembly_survives_the_file_round_trip() {
        let mut a = Document::new("Assembly");
        apply_op(&mut a, "component_add", &json!({"name": "Flange", "template": "flange", "at": [5, 6, 7], "rotate": [0, 90, 0]})).unwrap();
        let f = crate::file::CadFile::new(a.clone());
        let back = crate::file::CadFile::from_json(&f.to_json()).unwrap();
        assert_eq!(back.document, a);
        assert_eq!(back.document.components[0].placement.translate, [5.0, 6.0, 7.0]);
    }
}
