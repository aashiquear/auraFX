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

#[test]
fn probe_and_plot_over_line() {
    let t = td("wavelet_tets.vtu");
    let w = run("wavelet", None, &[("half_size", 5.into())]);
    let line =
        run("plot_over_line", Some(&t), &[("point1", serde_json::json!([-5, 0, 0])), ("point2", serde_json::json!([5, 0, 0])), ("resolution", 10.into())]);
    let pd = line.point_data().unwrap();
    let mask = pd.get("vtkValidPointMask").unwrap();
    assert_eq!(mask.range(None), (1.0, 1.0));
    // On grid nodes the probe must reproduce the source values exactly.
    let rt = pd.get("RTData").unwrap();
    let wpd = w.point_data().unwrap().get("RTData").unwrap();
    for i in 0..=10usize {
        // wavelet index for (x=i-5, y=0, z=0): i + 11*(5 + 11*5)
        let expect = wpd.get(i + 11 * (5 + 11 * 5), 0);
        assert!((rt.get(i, 0) - expect).abs() < 1e-3, "{i}: {} vs {expect}", rt.get(i, 0));
    }
    let img = run("resample_to_image", Some(&t), &[("nx", 6.into()), ("ny", 6.into()), ("nz", 6.into())]);
    assert_eq!(img.n_points(), 216);
}

#[test]
fn analysis_filters() {
    // Box of size 2x3x4 -> volume 24 when tetrahedralized via a structured image.
    let mut img = aurafx_data::ImageData { extent: [0, 2, 0, 3, 0, 4], ..Default::default() };
    let n = img.n_points();
    let pts = img.points();
    // f = 2x + 3y - z -> gradient (2, 3, -1) exactly.
    let f: Vec<f64> = (0..n)
        .map(|i| {
            let p = pts.get(i);
            2.0 * p[0] + 3.0 * p[1] - p[2]
        })
        .collect();
    img.point_data.insert(aurafx_data::Array::scalars_f64("f", f));
    let ds = DataSet::ImageData(img);
    let it = run("integrate_variables", Some(&ds), &[]);
    let DataSet::Table(t) = &it else { panic!() };
    assert!((t.column("Volume").unwrap().get(0, 0) - 24.0).abs() < 1e-9);
    // integral of f over [0,2]x[0,3]x[0,4] = 2*(2*3*4)/2*2... compute: ∫2x = 2*(4/2)*3*4 = 48, ∫3y = 3*(9/2)*2*4 = 108, ∫-z = -(16/2)*2*3 = -48 -> 108
    assert!((t.column("f").unwrap().get(0, 0) - 108.0).abs() < 1e-6);
    let g = run("gradient", Some(&ds), &[("array", "f".into())]);
    let ga = g.point_data().unwrap().get("f_gradient").unwrap();
    for i in 0..ga.len() {
        assert!((ga.get(i, 0) - 2.0).abs() < 1e-9 && (ga.get(i, 1) - 3.0).abs() < 1e-9 && (ga.get(i, 2) + 1.0).abs() < 1e-9);
    }
    let h = run("histogram", Some(&ds), &[("array", "f".into()), ("bins", 10.into())]);
    let DataSet::Table(ht) = &h else { panic!() };
    let total: f64 = (0..10).map(|i| ht.column("f_count").unwrap().get(i, 0)).sum();
    assert_eq!(total as usize, n);
    let st = run("descriptive_statistics", Some(&ds), &[]);
    assert!(st.n_cells() >= 1);
}
