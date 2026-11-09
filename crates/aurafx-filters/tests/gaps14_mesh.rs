//! Section 14.3 checks through the registry: paving and sweeping, remeshing with sizing.
use aurafx_data::*;

fn run(id: &str, input: Option<&DataSet>, kv: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in kv {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

#[test]
fn a_plate_with_a_hole_is_paved_and_swept_into_good_hexahedra() {
    let face = run("face_mesh", None, &[("size", serde_json::json!(0.15))]);
    let DataSet::PolyData(pd) = &face else { panic!() };
    let quads = pd.polys.len();
    assert!(pd.polys.iter().all(|(t, _)| t == CellType::Quad));
    let q = pd.field_data.get("MinQuality").unwrap().get(0, 0);
    assert!(q > 0.3, "worst quad scaled Jacobian {q}");
    // Quads of about the size: area / size^2.
    let area = 8.0 - std::f64::consts::PI * 0.25;
    let expect = area / (0.15 * 0.15);
    assert!((quads as f64 / expect - 1.0).abs() < 0.35, "{quads} quads, about {expect} expected");
    let hex = run("sweep_quads", Some(&face), &[("distance", serde_json::json!(0.5)), ("layers", serde_json::json!(3))]);
    assert_eq!(hex.n_cells(), 3 * quads);
    let qual = run("mesh_quality", Some(&hex), &[]);
    let sj = qual.cell_data().unwrap().get("ScaledJacobian").unwrap();
    let worst = (0..sj.len()).map(|i| sj.get(i, 0)).fold(f64::INFINITY, f64::min);
    assert!(worst > 0.3, "worst hexahedron scaled Jacobian {worst}");
}

#[test]
fn refined_near_the_hole_and_by_pieces_per_outline() {
    let coarse = run("face_mesh", None, &[("size", serde_json::json!(0.3)), ("element", serde_json::json!("Triangles"))]);
    let fine = run("face_mesh", None, &[("size", serde_json::json!(0.3)), ("near", serde_json::json!(0.05)), ("element", serde_json::json!("Triangles"))]);
    assert!(fine.n_cells() > 2 * coarse.n_cells(), "{} vs {}", fine.n_cells(), coarse.n_cells());
    // 40 pieces on the outer rectangle, 24 on the hole: boundary nodes 40 + 24.
    let t = run("face_mesh", None, &[("size", serde_json::json!(0.3)), ("intervals", serde_json::json!("40 24")), ("element", serde_json::json!("Triangles"))]);
    let b = t.point_data().unwrap().get("BoundaryLoop").unwrap();
    let on = |k: f64| (0..b.len()).filter(|&i| b.get(i, 0) == k).count();
    assert_eq!((on(0.0), on(1.0)), (40, 24));
}

#[test]
fn a_sphere_surface_remeshed_by_size_curvature_and_field() {
    let sphere = run("sphere", None, &[("theta_resolution", serde_json::json!(64)), ("phi_resolution", serde_json::json!(32))]);
    let r = run("remesh_surface", Some(&sphere), &[("size", serde_json::json!(0.05))]);
    let DataSet::PolyData(pd) = &r else { panic!() };
    // Edges near 0.05 on a sphere of radius 0.5: about 2.3 area / (0.433 h^2) triangles.
    let expect = 4.0 * std::f64::consts::PI * 0.25 / (0.433 * 0.05 * 0.05);
    assert!((pd.polys.len() as f64 / expect - 1.0).abs() < 0.3, "{} vs {expect}", pd.polys.len());
    assert!(pd.field_data.get("MinQuality").unwrap().get(0, 0) > 0.3);
    // Quads.
    let q = run("remesh_surface", Some(&sphere), &[("size", serde_json::json!(0.05)), ("element", serde_json::json!("Quadrilaterals"))]);
    let DataSet::PolyData(qd) = &q else { panic!() };
    assert!(qd.polys.iter().all(|(t, _)| t == CellType::Quad));
    // A chord tolerance of 0.002 on radius 0.5: chords of 2 sqrt(2 R tol) = 0.089.
    let c = run("remesh_surface", Some(&sphere), &[("size", serde_json::json!(1.0)), ("tolerance", serde_json::json!(0.002))]);
    let n = c.n_cells() as f64;
    let expect_c = 4.0 * std::f64::consts::PI * 0.25 / (0.433 * 0.089 * 0.089);
    assert!((n / expect_c - 1.0).abs() < 0.4, "{n} vs {expect_c}");
}

#[test]
fn a_torus_surface_remeshed_into_quads_by_curvature_has_no_folded_quads() {
    let t = run("geometry", None, &[("commands", serde_json::json!("torus major 1 minor 0.35")), ("resolution", serde_json::json!(64))]);
    for (size, tol) in [(0.12, 0.0), (0.4, 0.004)] {
        let q = run(
            "remesh_surface",
            Some(&t),
            &[("size", serde_json::json!(size)), ("tolerance", serde_json::json!(tol)), ("element", serde_json::json!("Quadrilaterals"))],
        );
        let DataSet::PolyData(pd) = &q else { panic!() };
        let qual = pd.cell_data.get("Quality").unwrap();
        let bad = (0..qual.len()).filter(|&i| qual.get(i, 0) < 0.2).count();
        eprintln!("size {size} tol {tol}: {} quads, min {}, below 0.2: {bad}", pd.polys.len(), pd.field_data.get("MinQuality").unwrap().get(0, 0));
        assert!(pd.field_data.get("MinQuality").unwrap().get(0, 0) > 0.0);
    }
}
