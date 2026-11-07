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

/// CAD Model: the surface of the part, or its signed distance on a grid.
pub fn cad_model(document: &str, overrides: &str, configuration: &str, resolution: usize, output: &str, project: bool) -> Result<DataSet> {
    let doc = effective_document(document, overrides, configuration)?;
    let ev = evaluate(&doc, resolution, project)?;
    if output != OUT_DISTANCE {
        return Ok(ev.surface);
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
        let ds = cad_model("", "", "", 48, OUT_SURFACE, true).unwrap();
        let pd = surface(&ds);
        let names = pd.field_data.get("FaceNames").unwrap();
        assert_eq!(names.len(), 6);
        let vol = pd.field_data.get("Volume").unwrap().get(0, 0);
        assert!((vol - 40.0 * 30.0 * 8.0).abs() / 9600.0 < 0.01, "{vol}");
    }

    #[test]
    fn overrides_and_configurations_change_the_part_and_mistakes_say_why() {
        let v = |o: &str, c: &str| {
            let ds = cad_model("", o, c, 48, OUT_SURFACE, true).unwrap();
            surface(&ds).field_data.get("Volume").unwrap().get(0, 0)
        };
        assert!((v(r#"{"width": "60 mm"}"#, "") - 60.0 * 30.0 * 8.0).abs() / 14400.0 < 0.01);
        let mut d = default_document();
        aurafx_cad::edit::apply_op(&mut d, "configuration", &serde_json::json!({"name": "thin", "overrides": {"thickness": 2}})).unwrap();
        let text = CadFile::new(d).to_json();
        let ds = cad_model(&text, "", "thin", 48, OUT_SURFACE, true).unwrap();
        let vol = surface(&ds).field_data.get("Volume").unwrap().get(0, 0);
        assert!((vol - 40.0 * 30.0 * 2.0).abs() / 2400.0 < 0.02, "{vol}");
        let e = cad_model("", r#"{"nope": 1}"#, "", 48, OUT_SURFACE, true).unwrap_err().to_string();
        assert!(e.contains("no parameter 'nope'") && e.contains("width"), "{e}");
        assert!(cad_model("{not json", "", "", 48, OUT_SURFACE, true).is_err());
    }

    #[test]
    fn distance_field_is_negative_inside_the_part() {
        let ds = cad_model("", "", "", 32, OUT_DISTANCE, true).unwrap();
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
        let surf = cad_model(&text, "", "", 64, OUT_SURFACE, true).unwrap();
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
