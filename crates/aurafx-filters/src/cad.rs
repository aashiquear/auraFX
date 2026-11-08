//! The CAD Model source: a parametric part (`aurafx-cad`'s document, kept as JSON in the node's
//! parameters) evaluated into a surface with named faces, or into its distance field. The
//! document's own parameters can be overridden from the node (`parameters`, a JSON object of
//! name to expression) so a dimension changes and the pipeline re-runs without any editing.

use anyhow::{anyhow, bail, Result};
use aurafx_cad::kernel::implicit::ImplicitKernel;
use aurafx_cad::{CadFile, Document, Evaluated, Kernel};
use aurafx_data::*;
use rayon::prelude::*;

pub const OUT_SURFACE: &str = "Surface";
pub const OUT_DISTANCE: &str = "Distance field";

/// The document a new CAD Model node starts with: a plate to cut into.
pub fn default_document() -> Document {
    let mut d = Document::new("Part");
    let _ = aurafx_cad::edit::apply_op(&mut d, "parameter", &serde_json::json!({"name": "width", "expr": "40 mm"}));
    let _ = aurafx_cad::edit::apply_op(&mut d, "parameter", &serde_json::json!({"name": "depth", "expr": "30 mm"}));
    let _ = aurafx_cad::edit::apply_op(&mut d, "parameter", &serde_json::json!({"name": "thickness", "expr": "8 mm"}));
    let _ = aurafx_cad::edit::apply_op(
        &mut d,
        "sketch_extrude",
        &serde_json::json!({"name": "Base", "depth": "thickness", "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": "width", "h": "depth"}}]}}),
    );
    d
}

pub fn default_document_json() -> String {
    CadFile::new(default_document()).to_json()
}

/// The document of a node's `document` parameter (an `.afxcad` file's text, or a bare document).
pub fn parse_document(text: &str) -> Result<Document> {
    if text.trim().is_empty() {
        return Ok(default_document());
    }
    Document::from_afxcad(text).map_err(|e| anyhow!("{e}"))
}

/// The parameter overrides of a node: a JSON object of name to expression (or number).
pub fn parse_overrides(text: &str) -> Result<Vec<(String, String)>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| anyhow!("the parameter overrides are not a JSON object: {e}"))?;
    let obj = v.as_object().ok_or_else(|| anyhow!("the parameter overrides must be a JSON object such as {{\"width\": \"50\"}}"))?;
    Ok(obj.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect())
}

/// The document with the node's overrides and configuration applied.
pub fn effective_document(document: &str, overrides: &str, configuration: &str) -> Result<Document> {
    let mut d = parse_document(document)?;
    for (name, expr) in parse_overrides(overrides)? {
        if !d.parameters.iter().any(|p| p.name == name) {
            let names: Vec<_> = d.parameters.iter().map(|p| p.name.as_str()).collect();
            bail!("the document has no parameter '{name}' (its parameters are: {})", names.join(", "));
        }
        d.set_parameter(&name, &expr);
    }
    if !configuration.trim().is_empty() {
        if !d.configurations.iter().any(|c| c.name == configuration) {
            bail!("the document has no configuration '{configuration}'");
        }
        d.active_configuration = Some(configuration.to_string());
    }
    Ok(d)
}

/// Evaluate a document on the implicit kernel.
pub fn evaluate(doc: &Document, resolution: usize, project: bool) -> Result<Evaluated> {
    let k = ImplicitKernel { resolution, project };
    k.evaluate(doc).map_err(|e| anyhow!("{e}"))
}

/// Evaluate with the chosen kernel (`Auto`: the exact B-rep kernel when it builds the part,
/// else the implicit one, with the reason in the result's notes).
pub fn evaluate_with(kernel: aurafx_cad::KernelChoice, doc: &Document, resolution: usize, project: bool) -> Result<Evaluated> {
    aurafx_cad::kernel::evaluate_with(kernel, doc, resolution, project).map_err(|e| anyhow!("{e}"))
}

/// Read a CAD file as the surface of its part (faces named: `FaceId`, `FaceNames`).
pub fn read_cad_file(path: &std::path::Path) -> Result<DataSet> {
    let doc = aurafx_cad::io::import_file(path).map_err(|e| anyhow!("{e}"))?;
    Ok(evaluate_with(aurafx_cad::KernelChoice::Auto, &doc, 64, true)?.surface)
}

/// Add the CAD file extensions to aurafx-io's readers (a static read; File > Open makes a CAD Model).
pub fn register_readers() {
    aurafx_io::register_reader(aurafx_io::PluginReader {
        extensions: ["step", "stp", "igs", "iges", "3mf", "sc", "afxcad"].iter().map(|s| s.to_string()).collect(),
        label: "CAD".into(),
        plugin: "built-in (aurafx-cad)".into(),
        read: std::sync::Arc::new(|p| read_cad_file(p)),
    });
}

/// CAD Model: the surface of the part, or its signed distance on a grid.
pub fn cad_model(document: &str, overrides: &str, configuration: &str, resolution: usize, output: &str, project: bool, kernel: &str) -> Result<DataSet> {
    let doc = effective_document(document, overrides, configuration)?;
    let choice = aurafx_cad::KernelChoice::parse(kernel).ok_or_else(|| anyhow!("kernel is Auto, Implicit or B-rep, not '{kernel}'"))?;
    let ev = evaluate_with(choice, &doc, resolution, project)?;
    if output != OUT_DISTANCE {
        let mut surface = ev.surface;
        if let DataSet::PolyData(pd) = &mut surface {
            // Which kernel built it, and what it could not build exactly.
            pd.field_data.insert(Array::new("Kernel", 1, ArrayData::String(vec![ev.kernel.to_string()])));
            if !ev.approximate.is_empty() {
                pd.field_data.insert(Array::new("KernelNotes", 1, ArrayData::String(ev.approximate.clone())));
            }
        }
        return Ok(surface);
    }
    let dist = ev.distance.clone().ok_or_else(|| anyhow!("this kernel has no distance field"))?;
    let (lo, hi) = (ev.bodies.iter().fold([f64::INFINITY; 3], |a, b| std::array::from_fn(|k| a[k].min(b.bounds.0[k]))), ev.bodies.iter().fold([f64::NEG_INFINITY; 3], |a, b| std::array::from_fn(|k| a[k].max(b.bounds.1[k]))));
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max).max(1e-12);
    let h = size / resolution.max(2) as f64;
    let n: [i64; 3] = std::array::from_fn(|k| ((hi[k] - lo[k]) / h).ceil() as i64 + 4);
    let mut img = ImageData { extent: [0, n[0], 0, n[1], 0, n[2]], origin: std::array::from_fn(|k| lo[k] - 2.0 * h), spacing: [h; 3], ..Default::default() };
    let (nx, ny, nz) = ((n[0] + 1) as usize, (n[1] + 1) as usize, (n[2] + 1) as usize);
    let origin = img.origin;
    let d: Vec<f64> = (0..nx * ny * nz)
        .into_par_iter()
        .map(|i| {
            let (x, y, z) = (i % nx, (i / nx) % ny, i / (nx * ny));
            dist([origin[0] + x as f64 * h, origin[1] + y as f64 * h, origin[2] + z as f64 * h])
        })
        .collect();
    img.point_data.insert(Array::scalars_f64("Distance", d));
    img.field_data = match &ev.surface {
        DataSet::PolyData(p) => p.field_data.clone(),
        _ => Attributes::new(),
    };
    Ok(DataSet::ImageData(img))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(ds: &DataSet) -> &PolyData {
        match ds {
            DataSet::PolyData(p) => p,
            _ => panic!("not a surface"),
        }
    }

    #[test]
    fn the_default_part_is_a_plate_with_named_faces() {
        let ds = cad_model("", "", "", 48, OUT_SURFACE, true, "Auto").unwrap();
        let pd = surface(&ds);
        let names = pd.field_data.get("FaceNames").unwrap();
        assert_eq!(names.len(), 6);
        let vol = pd.field_data.get("Volume").unwrap().get(0, 0);
        assert!((vol - 40.0 * 30.0 * 8.0).abs() / 9600.0 < 0.01, "{vol}");
    }

    #[test]
    fn overrides_and_configurations_change_the_part_and_mistakes_say_why() {
        let v = |o: &str, c: &str| {
            let ds = cad_model("", o, c, 48, OUT_SURFACE, true, "Auto").unwrap();
            surface(&ds).field_data.get("Volume").unwrap().get(0, 0)
        };
        assert!((v(r#"{"width": "60 mm"}"#, "") - 60.0 * 30.0 * 8.0).abs() / 14400.0 < 0.01);
        let mut d = default_document();
        aurafx_cad::edit::apply_op(&mut d, "configuration", &serde_json::json!({"name": "thin", "overrides": {"thickness": 2}})).unwrap();
        let text = CadFile::new(d).to_json();
        let ds = cad_model(&text, "", "thin", 48, OUT_SURFACE, true, "Auto").unwrap();
        let vol = surface(&ds).field_data.get("Volume").unwrap().get(0, 0);
        assert!((vol - 40.0 * 30.0 * 2.0).abs() / 2400.0 < 0.02, "{vol}");
        let e = cad_model("", r#"{"nope": 1}"#, "", 48, OUT_SURFACE, true, "Auto").unwrap_err().to_string();
        assert!(e.contains("no parameter 'nope'") && e.contains("width"), "{e}");
        assert!(cad_model("{not json", "", "", 48, OUT_SURFACE, true, "Auto").is_err());
    }

    #[test]
    fn the_kernel_is_chosen_and_the_result_says_which_built_it() {
        let kernel = |k: &str| match cad_model("", "", "", 40, OUT_SURFACE, true, k).unwrap() {
            DataSet::PolyData(p) => p.field_data.get("Kernel").map(|a| match &a.data { ArrayData::String(v) => v[0].clone(), _ => String::new() }).unwrap(),
            _ => panic!(),
        };
        assert_eq!(kernel("Auto"), "brep");
        assert_eq!(kernel("Implicit"), "implicit");
        assert_eq!(kernel("B-rep"), "brep");
        assert!(cad_model("", "", "", 40, OUT_SURFACE, true, "nope").unwrap_err().to_string().contains("Auto, Implicit or B-rep"));
    }

    #[test]
    fn distance_field_is_negative_inside_the_part() {
        let ds = cad_model("", "", "", 32, OUT_DISTANCE, true, "Auto").unwrap();
        let DataSet::ImageData(img) = &ds else { panic!() };
        let d = img.point_data.get("Distance").unwrap();
        let (neg, pos) = (0..d.len()).fold((0, 0), |(n, p), i| if d.get(i, 0) < 0.0 { (n + 1, p) } else { (n, p + 1) });
        assert!(neg > 100 && pos > 100, "{neg} {pos}");
    }

    #[test]
    fn a_template_meshed_by_the_overlay_mesher_passes_mesh_quality() {
        use crate::mesh_gen::{mesh_quality, overlay_mesh, OverlayParams, OVERLAY_SMOOTH};
        let doc = aurafx_cad::templates::flange();
        let text = CadFile::new(doc).to_json();
        let surf = cad_model(&text, "", "", 64, OUT_SURFACE, true, "Auto").unwrap();
        let mesh = overlay_mesh(
            &surf,
            &OverlayParams { array: String::new(), cell: 0.0, mode: OVERLAY_SMOOTH.into(), void: false, periodic: false, min_cells: 8, smoothing: 8, element: "HEX8".into(), sharp: true },
        )
        .unwrap();
        let q = mesh_quality(&mesh, true).unwrap();
        let DataSet::Table(t) = &q else { panic!("summary is a table") };
        let _ = t;
        let full = mesh_quality(&mesh, false).unwrap();
        let sj = full.cell_data().unwrap().get("ScaledJacobian").unwrap();
        let min = (0..sj.len()).map(|i| sj.get(i, 0)).filter(|v| !v.is_nan()).fold(f64::INFINITY, f64::min);
        assert!(min > 0.0, "an inverted or degenerate element: min scaled Jacobian {min}");
        assert!(sj.len() > 500, "{} elements", sj.len());
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;

    #[test]
    fn a_step_file_is_imported_into_geometry_commands_and_meshed_with_its_faces_as_side_sets() {
        let step = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cad/plate_with_hole.step");
        if !step.exists() {
            return;
        }
        let g = crate::registry::find("geometry").unwrap();
        let mut p = g.default_params();
        p.set_json("commands", &format!("import '{}'\nmesh body 1 size 3 scheme overlay", step.display()).into());
        p.set_json("output", &"Hex mesh".into());
        let mesh = g.run(None, &p).unwrap();
        let m = aurafx_mesh::hex::HexMesh::from_dataset(&mesh).unwrap();
        assert!(m.n_elements() > 100, "{}", m.n_elements());
        assert!(m.side_sets.iter().any(|(n, v)| n.contains("face") && !v.is_empty()), "{:?}", m.side_sets.iter().map(|s| s.0.clone()).collect::<Vec<_>>());
        // The surface output keeps the CAD file's faces too.
        p.set_json("output", &"Surface".into());
        let DataSet::PolyData(pd) = g.run(None, &p).unwrap() else { panic!() };
        let names = match &pd.field_data.get("FaceNames").unwrap().data {
            ArrayData::String(v) => v.clone(),
            _ => panic!(),
        };
        assert!(names.len() >= 8 && names[1].contains("plate_with_hole"), "{names:?}");
    }
}

#[cfg(test)]
mod mesh_tests {
    use super::*;
    use crate::mesh_gen::{overlay_mesh, OverlayParams, OVERLAY_SMOOTH};

    #[test]
    fn the_faces_of_a_cad_part_become_side_sets_of_its_mesh() {
        let mut d = aurafx_cad::Document::new("plate");
        aurafx_cad::edit::apply_op(&mut d, "sketch_extrude", &serde_json::json!({"name": "Plate", "depth": 10, "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": 40, "h": 30}}]}})).unwrap();
        aurafx_cad::edit::apply_op(&mut d, "add", &serde_json::json!({"kind": "hole", "name": "Bore", "diameter": 10, "depth": 10, "at": [0, 0, 10]})).unwrap();
        let text = CadFile::new(d).to_json();
        let surf = cad_model(&text, "", "", 64, OUT_SURFACE, true, "Auto").unwrap();
        let mesh = overlay_mesh(
            &surf,
            &OverlayParams { array: String::new(), cell: 2.0, mode: OVERLAY_SMOOTH.into(), void: false, periodic: false, min_cells: 8, smoothing: 8, element: "HEX8".into(), sharp: true },
        )
        .unwrap();
        let m = aurafx_mesh::hex::HexMesh::from_dataset(&mesh).unwrap();
        let names: Vec<&str> = m.side_sets.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Bore side") && names.contains(&"Plate top") && names.contains(&"Plate bottom"), "{names:?}");
        let bore = m.side_set("Bore side").unwrap();
        assert!(bore.len() > 20, "the bore's wall has elements: {}", bore.len());
        // Extract Set shows it as a surface.
        let wall = crate::mesh_gen::extract_set(&mesh, "Bore side").unwrap();
        assert!(wall.n_cells() > 20, "{}", wall.n_cells());
    }
}
