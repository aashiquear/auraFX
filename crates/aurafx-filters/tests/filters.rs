use aurafx_data::*;
use aurafx_filters::params::*;
use aurafx_filters::*;
use std::path::PathBuf;

fn td(name: &str) -> DataSet {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name);
    match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    }
}

fn run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> DataSet {
    let def = find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

#[test]
fn wavelet_surface_and_slice() {
    let w = run("wavelet", None, &[("half_size", 5.into())]);
    assert_eq!(w.n_points(), 11 * 11 * 11);
    let s = run("extract_surface", Some(&w), &[]);
    assert_eq!(s.n_cells(), 6 * 100);
    assert_eq!(s.n_points(), 11 * 11 * 11 - 9 * 9 * 9);
    let sl = run("slice", Some(&w), &[("normal", serde_json::json!([0, 0, 1]))]);
    // A z=0 plane through 10x10 hexes, each giving 2 tets cut -> ~ 2 polygons per hex at least.
    assert!(sl.n_cells() >= 200, "{}", sl.n_cells());
    let b = sl.bounds();
    assert!((b.min[2]).abs() < 1e-9 && (b.max[2]).abs() < 1e-9);
    assert!(sl.point_data().unwrap().get("RTData").is_some());
}

#[test]
fn clip_contour_threshold_on_tets() {
    let t = td("wavelet_tets.vtu");
    let c = run("clip", Some(&t), &[("normal", serde_json::json!([1, 0, 0]))]);
    let b = c.bounds();
    assert!(b.min[0] >= -1e-9 && b.max[0] > 4.0, "{b:?}");
    assert!(c.n_cells() > 0);
    let ci = run("clip", Some(&t), &[("normal", serde_json::json!([1, 0, 0])), ("invert", true.into())]);
    assert!(ci.bounds().max[0] <= 1e-9);
    let iso = run("contour", Some(&t), &[("values", serde_json::json!([150.0]))]);
    assert!(iso.n_cells() > 100);
    let r = iso.point_data().unwrap().get("RTData").unwrap().range(None);
    assert!((r.0 - 150.0).abs() < 1e-3 && (r.1 - 150.0).abs() < 1e-3, "{r:?}");
    let th = run("threshold", Some(&t), &[("lower", 200.0.into()), ("upper", 300.0.into())]);
    assert!(th.n_cells() > 0 && th.n_cells() < t.n_cells());
    let r = th.point_data().unwrap().get("RTData").unwrap().range(None);
    assert!(r.0 >= 200.0 - 1e-6);
}

#[test]
fn calculator_and_warp() {
    let s = td("sphere.vtp");
    let c = run("calculator", Some(&s), &[("expression", "coordsX*2 + Elevation".into()), ("result", "e2".into())]);
    assert!(c.point_data().unwrap().get("e2").is_some());
    let v = run("calculator", Some(&s), &[("expression", "coords * 2".into()), ("result", "p2".into())]);
    assert_eq!(v.point_data().unwrap().get("p2").unwrap().ncomp, 3);
    let w = run("warp_by_vector", Some(&v), &[("vectors", "p2".into()), ("scale", 1.0.into())]);
    assert!(w.bounds().diagonal() > 2.5 * s.bounds().diagonal());
    let g = run("glyph", Some(&s), &[("shape", "Arrow".into()), ("orient", "Normals".into())]);
    assert!(g.n_cells() > 0);
}

#[test]
fn registry_search() {
    let r = search("iso");
    assert_eq!(r[0].id, "contour");
    assert!(search("cut").iter().any(|f| f.id == "slice"));
}
