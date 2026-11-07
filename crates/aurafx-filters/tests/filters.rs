use aurafx_data::*;
use aurafx_filters::params::*;
use aurafx_filters::*;
use std::collections::HashMap;
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

#[test]
fn selection_filters() {
    let t = td("wavelet_tets.vtu");
    let sel = run("extract_selection", Some(&t), &[("ids", serde_json::json!([0, 1, 2, 10]))]);
    assert_eq!(sel.n_cells(), 4);
    assert!(sel.cell_data().unwrap().get("vtkOriginalCellIds").is_some());
    let fd = run("find_data", Some(&t), &[("expression", "RTData > 200".into())]);
    assert!(fd.n_cells() > 0 && fd.n_cells() < t.n_cells());
    let fp = run("find_data", Some(&t), &[("attribute", "Points".into()), ("expression", "RTData > 200".into())]);
    let r = fp.point_data().unwrap().get("RTData").unwrap().range(None);
    assert!(r.0 > 200.0, "{r:?}");
}

#[test]
fn geometry_batch() {
    let sph = run("sphere", None, &[("theta", 24.into()), ("phi", 24.into())]);
    let n = run("surface_normals", Some(&sph), &[]);
    let na = n.point_data().unwrap().get("Normals").unwrap();
    // Outward normals: dot(normal, position) > 0 for a sphere at the origin.
    let pts = n.points().unwrap();
    for i in 0..pts.len() {
        let p = pts.get(i);
        assert!(p[0] * na.get(i, 0) + p[1] * na.get(i, 1) + p[2] * na.get(i, 2) > 0.0);
    }
    // Shrink duplicates points; Clean merges them back.
    let t = td("wavelet_tets.vtu");
    let sh = run("shrink", Some(&t), &[("factor", 1.0.into())]);
    assert!(sh.n_points() > t.n_points());
    let cl = run("clean", Some(&sh), &[]);
    assert_eq!(cl.n_points(), t.n_points());
    // Two separate boxes -> two regions.
    let refl = run("reflect", Some(&run("box", None, &[("center", serde_json::json!([2, 0, 0]))])), &[]);
    let merged = run("merge_blocks", Some(&refl), &[]);
    let conn = run("connectivity", Some(&merged), &[]);
    assert_eq!(conn.field_data().get("NumberOfRegions").unwrap().get(0, 0), 2.0);
    let largest = run("connectivity", Some(&merged), &[("extract", "largest".into())]);
    assert_eq!(largest.n_cells(), 6);
    // Cell size of the wavelet image (1x1x1 voxels) sums to 1000.
    let w = run("wavelet", None, &[("half_size", 5.into())]);
    let cs = run("cell_size", Some(&w), &[]);
    let vol = cs.cell_data().unwrap().get("Volume").unwrap();
    let total: f64 = (0..vol.len()).map(|i| vol.get(i, 0)).sum();
    assert!((total - 1000.0).abs() < 1e-6);
    let q = run("mesh_quality", Some(&w), &[]);
    assert!((q.cell_data().unwrap().get("EdgeRatio").unwrap().range(None).1 - 1.0).abs() < 1e-9);
    assert!((q.cell_data().unwrap().get("MinAngle").unwrap().range(None).0 - 90.0).abs() < 1e-6);
    let tet = run("tetrahedralize", Some(&w), &[]);
    // 5 or 6 tetrahedra per hexahedron (cut to match its neighbours), same volume.
    assert!((5000..=6000).contains(&tet.n_cells()), "{}", tet.n_cells());
    let DataSet::Table(t) = run("integrate_variables", Some(&tet), &[]) else { panic!() };
    assert!((t.column("Volume").unwrap().get(0, 0) - 1000.0).abs() < 1e-6);
    let tri = run("triangulate", Some(&run("box", None, &[])), &[]);
    assert_eq!(tri.n_cells(), 12);
}

#[test]
fn stream_tracer_follows_rotation() {
    // Rigid rotation about z: v = (-y, x, 0). Streamlines are circles: radius stays constant.
    let w = run("wavelet", None, &[("half_size", 8.into())]);
    let c = run("calculator", Some(&w), &[("expression", "vec(-coordsY, coordsX, 0)".into()), ("result", "v".into())]);
    let st = run(
        "stream_tracer",
        Some(&c),
        &[
            ("vectors", "v".into()),
            ("seed_type", "Line".into()),
            ("point1", serde_json::json!([2, 0, 0])),
            ("point2", serde_json::json!([5, 0, 0])),
            ("seeds", 4.into()),
            ("direction", "Forward".into()),
            ("max_length", 1.0.into()),
        ],
    );
    let DataSet::PolyData(pd) = &st else { panic!() };
    assert_eq!(pd.lines.len(), 4);
    for l in 0..pd.lines.len() {
        let cell = pd.lines.cell(l);
        let r0 = {
            let p = pd.points.get(cell[0] as usize);
            (p[0] * p[0] + p[1] * p[1]).sqrt()
        };
        for &i in cell {
            let p = pd.points.get(i as usize);
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!((r - r0).abs() < 0.02 * r0, "line {l}: radius drifted {r0} -> {r}");
        }
        assert!(cell.len() > 20);
    }
    let tube = run("tube", Some(&st), &[("radius", 0.1.into())]);
    assert!(tube.n_cells() > 100);
}

#[test]
fn extract_subset_crops_and_samples() {
    use aurafx_data::DataSet;
    let img = aurafx_filters::sources::wavelet([-10, 10, -10, 10, -10, 10], 255.0, 0.5);
    let src = DataSet::ImageData(img.clone());
    let out = aurafx_filters::geometry::extract_subset(&src, [-5.0; 3], [5.0; 3], [2.0; 3], false).unwrap();
    let DataSet::ImageData(o) = &out else { panic!("image stays an image") };
    assert_eq!(o.dims(), [6, 6, 6]);
    assert_eq!(o.spacing, [2.0, 2.0, 2.0]);
    assert_eq!(o.origin, [-5.0, -5.0, -5.0]);
    // ParaView 6.1.1: Wavelet -> Extract Subset (VOI -5..5, rate 2): RTData 147.6765..268.4889, 125 cells.
    let (lo, hi) = o.point_data.get("RTData").unwrap().range(None);
    assert!((lo - 147.676_528_930_664).abs() < 1e-3 && (hi - 268.488_922_119_141).abs() < 1e-3, "{lo} {hi}");
    assert_eq!(out.n_cells(), 125);
    // Point (a, b, c) of the subset is input index (-5 + 2a, ...).
    let (a_in, a_out) = (img.point_data.get("RTData").unwrap(), o.point_data.get("RTData").unwrap());
    for (a, b, c) in [(0, 0, 0), (5, 2, 1), (3, 5, 4)] {
        let (i, j, k) = (5 - 5 + 2 * a, 5 - 5 + 2 * b, 5 - 5 + 2 * c); // -5 is index 5 of 21
        let src_idx = (i + 5) + 21 * ((j + 5) + 21 * (k + 5));
        assert_eq!(a_out.get(a + 6 * (b + 6 * c), 0), a_in.get(src_idx, 0));
    }
    // Rate 3 over 0..10 with the boundary kept: 0, 3, 6, 9, 10 -> uneven, so rectilinear.
    let r = aurafx_filters::geometry::extract_subset(&src, [0.0; 3], [10.0; 3], [3.0; 3], true).unwrap();
    let DataSet::RectilinearGrid(g) = &r else { panic!("uneven spacing gives a rectilinear grid") };
    assert_eq!(g.x, vec![0.0, 3.0, 6.0, 9.0, 10.0]);
    assert!(aurafx_filters::geometry::extract_subset(&src, [5.0; 3], [2.0; 3], [1.0; 3], false).is_err());
}

#[test]
fn quadratic_cells_render_curved() {
    // One quadratic tetra whose 0-1 mid node bulges out of the flat face.
    let mut pts = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let mid = |a: usize, b: usize, p: &Vec<f64>| [(p[3 * a] + p[3 * b]) / 2.0, (p[3 * a + 1] + p[3 * b + 1]) / 2.0, (p[3 * a + 2] + p[3 * b + 2]) / 2.0];
    let base = pts.clone();
    for (a, b) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)] {
        pts.extend_from_slice(&mid(a, b, &base));
    }
    pts[3 * 4 + 1] = -0.3; // node 4 (edge 0-1) moved to y = -0.3
    let mut cells = Cells::new();
    cells.push(CellType::QuadraticTetra, &(0..10).collect::<Vec<u32>>());
    let mut ug = UnstructuredGrid { points: Points::new_f64(pts), cells, ..Default::default() };
    ug.cell_data.insert(Array::scalars_f64("id", vec![7.0]));
    let s = aurafx_filters::surface::extract_surface(&DataSet::UnstructuredGrid(ug)).unwrap();
    assert_eq!(s.polys.len(), 16, "4 curved faces x 4 triangles");
    assert_eq!(s.points.len(), 10, "every node is used");
    let min_y = (0..s.points.len()).map(|i| s.points.get(i)[1]).fold(f64::INFINITY, f64::min);
    assert!((min_y + 0.3).abs() < 1e-12, "the bulge is part of the surface");
    assert_eq!(s.cell_data.get("id").unwrap().range(None), (7.0, 7.0));

    // A quadratic quad shell (8 nodes) becomes 6 triangles; a quadratic edge a 2-segment line.
    let mut cells = Cells::new();
    cells.push(CellType::QuadraticQuad, &[0, 1, 2, 3, 4, 5, 6, 7]);
    cells.push(CellType::QuadraticEdge, &[0, 1, 4]);
    let q = vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 2.0, 0.0, 0.0, 2.0, 0.0, 1.0, -0.2, 0.0, 2.0, 1.0, 0.0, 1.0, 2.0, 0.0, 0.0, 1.0, 0.0];
    let ug = UnstructuredGrid { points: Points::new_f64(q), cells, ..Default::default() };
    let s = aurafx_filters::surface::extract_surface(&DataSet::UnstructuredGrid(ug)).unwrap();
    assert_eq!(s.polys.len(), 6);
    assert_eq!(s.lines.len(), 1);
    assert_eq!(s.lines.cell(0).len(), 3);
    // Total area grows by the bulge but stays close to the flat 4.0.
    let area: f64 = (0..s.polys.len())
        .map(|i| {
            let c = s.polys.cell(i);
            let p: Vec<[f64; 3]> = c.iter().map(|&k| s.points.get(k as usize)).collect();
            let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1]], [p[2][0] - p[0][0], p[2][1] - p[0][1]]);
            0.5 * (u[0] * v[1] - u[1] * v[0])
        })
        .sum();
    assert!(area > 4.0 && area < 4.3, "consistent winding, area {area}");
}

fn multi(id: &str, inputs: &[&DataSet], set: &[(&str, serde_json::Value)]) -> Result<DataSet, String> {
    let def = find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    def.run_multi(inputs, &p).map_err(|e| format!("{e:#}"))
}

#[test]
fn append_filters_match_paraview() {
    use serde_json::json;
    // ParaView 6.1.1: Append Datasets(Wavelet -3..3, same shifted by 10 in x) -> 686 points,
    // 432 cells, RTData 37.3531..263.4803.
    let w = DataSet::ImageData(aurafx_filters::sources::wavelet([-3, 3, -3, 3, -3, 3], 255.0, 0.5));
    let mut shifted = w.clone();
    if let DataSet::ImageData(i) = &mut shifted {
        i.origin[0] += 10.0;
    }
    let a = multi("append_datasets", &[&w, &shifted], &[]).unwrap();
    assert!(matches!(a, DataSet::UnstructuredGrid(_)));
    assert_eq!((a.n_points(), a.n_cells()), (686, 432));
    let (lo, hi) = a.point_data().unwrap().get("RTData").unwrap().range(None);
    assert!((lo - 37.3531).abs() < 1e-3 && (hi - 263.4803).abs() < 1e-3, "{lo} {hi}");
    assert!((a.bounds().max[0] - 13.0).abs() < 1e-12);

    // Sphere + Cone (our sources tessellate differently from VTK's, so counts are the sums).
    // Arrays that only one input has (the sphere's Normals) are dropped, as in ParaView.
    let s = run("sphere", None, &[("theta", json!(8)), ("phi", json!(8))]);
    let c = run("cone", None, &[]);
    assert!(s.point_data().unwrap().get("Normals").is_some() && c.point_data().unwrap().get("Normals").is_none());
    let (np, nc) = (s.n_points() + c.n_points(), s.n_cells() + c.n_cells());
    let a = multi("append_datasets", &[&s, &c], &[]).unwrap();
    assert_eq!((a.n_points(), a.n_cells()), (np, nc));
    assert!(a.point_data().unwrap().get("Normals").is_none());
    let g = multi("append_geometry", &[&s, &c], &[]).unwrap();
    let DataSet::PolyData(pg) = &g else { panic!("append geometry gives polydata") };
    assert_eq!((g.n_points(), g.n_cells()), (np, nc));
    // Cone polygons index the cone's points, which come after the sphere's.
    let ns = s.n_points() as u32;
    assert!(pg.polys.iter().any(|(_, p)| p.iter().all(|&i| i >= ns)));
    assert!(multi("append_geometry", &[&s, &w], &[]).unwrap_err().contains("polygonal"));

    // Append Attributes(Wavelet, Elevation(Wavelet)): RTData, RTData_input_1, Elevation.
    let e = run("elevation", Some(&w), &[]);
    let at = multi("append_attributes", &[&w, &e], &[]).unwrap();
    let names: Vec<&str> = at.point_data().unwrap().names().collect();
    assert_eq!(names, ["RTData", "RTData_input_1", "Elevation"]);
    assert!(matches!(at, DataSet::ImageData(_)));
    assert!(multi("append_attributes", &[&w, &s], &[]).is_err());
}

#[test]
fn resample_with_dataset_matches_paraview() {
    use serde_json::json;
    // ParaView 6.1.1: Resample With Dataset(source Wavelet -10..10, destination Sphere r=12,
    // 16x16): 226 points, 104 valid, sum(RTData) 12917.279 (0 outside), max 149.4725.
    let w = DataSet::ImageData(aurafx_filters::sources::wavelet([-10, 10, -10, 10, -10, 10], 255.0, 0.5));
    let s = run("sphere", None, &[("radius", json!(12.0)), ("theta", json!(16)), ("phi", json!(16))]);
    let r = multi("resample_with_dataset", &[&w, &s], &[]).unwrap();
    assert!(matches!(r, DataSet::PolyData(_)));
    let pd = r.point_data().unwrap();
    let mask = pd.get("vtkValidPointMask").unwrap();
    assert_eq!(mask.scalar_type(), ScalarType::U8);
    let valid: f64 = (0..r.n_points()).map(|i| mask.get(i, 0)).sum();
    assert_eq!((r.n_points(), valid as usize), (226, 104));
    let rt = pd.get("RTData").unwrap();
    let sum: f64 = (0..r.n_points()).map(|i| rt.get(i, 0)).sum();
    let (_, hi) = rt.range(None);
    assert!((sum - 12917.279).abs() < 0.05, "sum {sum}");
    assert!((hi - 149.4725).abs() < 1e-3, "max {hi}");
    assert!(pd.get("Normals").is_none(), "destination arrays are not passed by default");
    let keep = multi("resample_with_dataset", &[&w, &s], &[("pass_point_arrays", json!(true))]).unwrap();
    assert!(keep.point_data().unwrap().get("Normals").is_some());
    // Cell data of the source becomes point data (ParaView: same 104 valid, sum 12773.920).
    let cd = run("point_data_to_cell_data", Some(&w), &[]);
    let r = multi("resample_with_dataset", &[&cd, &s], &[]).unwrap();
    let rt = r.point_data().unwrap().get("RTData").unwrap();
    let sum: f64 = (0..r.n_points()).map(|i| rt.get(i, 0)).sum();
    assert!((sum - 12773.92).abs() < 0.05, "cell sum {sum}");
    assert!(multi("resample_with_dataset", &[&w], &[]).unwrap_err().contains("destination"));
}

fn integrals(ds: &DataSet) -> std::collections::BTreeMap<String, f64> {
    let DataSet::Table(t) = run("integrate_variables", Some(ds), &[]) else { panic!() };
    t.columns.iter().map(|c| (c.name.clone(), c.get(0, 0))).collect()
}

/// Curved Lagrange and Bezier cells (testdata/*.vtu from scripts/gen_higher_order.py) against
/// ParaView 6.1.1: Integrate Variables, Extract Surface at nonlinear subdivision levels 0-3,
/// Slice, Clip and Contour. ParaView triangulates surface quads, so only cell counts differ.
#[test]
fn higher_order_cells_match_paraview() {
    let close = |a: f64, b: f64, what: &str| assert!((a - b).abs() <= 2e-7 * b.abs().max(1.0), "{what}: {a} vs ParaView {b}");
    let plane = |z: f64| [("origin", serde_json::json!([0.0, 0.0, z])), ("normal", serde_json::json!([0.0, 0.0, 1.0]))];
    struct Ref {
        file: &'static str,
        volume: f64,
        field: (&'static str, f64),
        surf_points: [usize; 4],
        surf_area: [f64; 4],
        surf_field1: f64,
        slice_z: f64,
        slice: (f64, f64),
        clip: Option<f64>,
        contour: Option<(f64, f64)>,
    }
    let refs = [
        Ref {
            file: "lagrange_annulus.vtu",
            volume: 2.3494714599609368,
            field: ("T", 7.0919231106228064),
            surf_points: [20, 164, 650, 2594],
            surf_area: [11.274368659602779, 11.407968090214894, 11.420610595142348, 11.423774354648994],
            surf_field1: 36.36913782371417,
            slice_z: 0.3,
            slice: (2.3494714599609288, 6.6220288186306195),
            clip: Some(1.6446300228478956),
            contour: Some((2.5, 2.3408349845232475)),
        },
        Ref {
            file: "bezier_annulus.vtu",
            // ParaView integrates the control net here (3.0); auraFX the evaluated nodes,
            // which is what ParaView's own slice of the same cell gives (2.1213 x height 1).
            volume: 2.1213203435596424,
            field: ("T", 3.299831645537222),
            surf_points: [8, 26, 98, 386],
            surf_area: [9.242640687119286, 10.834841763594575, 11.27365495542861, 11.386759403789142],
            surf_field1: 17.253331804637813,
            slice_z: 0.3,
            slice: (2.1213203435596424, 3.299831645537222),
            clip: Some(1.484924234169723),
            contour: Some((1.5, 2.296100594190538)),
        },
        Ref {
            file: "lagrange_shapes.vtu",
            volume: 1.9979694745053096,
            field: ("u", 1.842509495288105),
            surf_points: [27, 127, 452, 1723],
            surf_area: [15.463546377497881, 16.068182533131267, 16.131036707822073, 16.146779479547273],
            surf_field1: 20.456019124324364,
            slice_z: -0.4,
            slice: (2.0075545843942626, 2.307321015617519),
            clip: None,
            contour: None,
        },
    ];
    for r in refs {
        let ds = td(r.file);
        let f = r.file;
        let it = integrals(&ds);
        close(it["Volume"], r.volume, &format!("{f} volume"));
        close(it[r.field.0], r.field.1, &format!("{f} integral of {}", r.field.0));
        for lvl in 0..4u32 {
            let s = DataSet::PolyData(surface::extract_surface_level(&ds, lvl).unwrap());
            assert_eq!(s.n_points(), r.surf_points[lvl as usize], "{f} surface level {lvl} points");
            let it = integrals(&s);
            close(it["Area"], r.surf_area[lvl as usize], &format!("{f} surface level {lvl} area"));
            if lvl == 1 {
                close(it[r.field.0], r.surf_field1, &format!("{f} surface integral of {}", r.field.0));
                assert!(s.point_data().unwrap().get("RationalWeights").is_none());
            }
        }
        let s = run("slice", Some(&ds), &plane(r.slice_z));
        let it = integrals(&s);
        close(it["Area"], r.slice.0, &format!("{f} slice area"));
        close(it[r.field.0], r.slice.1, &format!("{f} slice integral"));
        if let Some(v) = r.clip {
            close(integrals(&run("clip", Some(&ds), &plane(0.3)))["Volume"], v, &format!("{f} clip volume"));
        }
        if let Some((value, area)) = r.contour {
            let c = run("contour", Some(&ds), &[("array", "T".into()), ("values", serde_json::json!([value]))]);
            close(integrals(&c)["Area"], area, &format!("{f} contour area"));
        }
        // Tessellate: level 1 is the same linear cells; finer levels converge on the curved
        // volume (3*pi/4 for the exact Bezier quarter cylinder).
        let mut last = 0.0;
        for lvl in [1u32, 2, 3, 4] {
            let t = run("tessellate", Some(&ds), &[("level", lvl.into())]);
            assert!(!aurafx_filters::higher_order::has_curved(&t));
            let v = integrals(&t)["Volume"];
            if lvl == 1 {
                close(v, r.volume, &format!("{f} tessellate level 1"));
            }
            assert!(v > last, "{f}: tessellated volume grows with the level");
            last = v;
        }
        if f.starts_with("bezier") {
            assert!((last - 3.0 * std::f64::consts::PI / 4.0).abs() < 4e-3, "Bezier volume at level 4: {last}");
        }
    }
}

/// Volume enclosed by a closed surface (positive when its polygons face outward).
fn signed_volume(pd: &PolyData) -> f64 {
    let mut v = 0.0;
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        let p0 = pd.points.get(c[0] as usize);
        for k in 1..c.len() - 1 {
            let (a, b) = (pd.points.get(c[k] as usize), pd.points.get(c[k + 1] as usize));
            v += (p0[0] * (a[1] * b[2] - a[2] * b[1]) - p0[1] * (a[0] * b[2] - a[2] * b[0]) + p0[2] * (a[0] * b[1] - a[1] * b[0])) / 6.0;
        }
    }
    v
}

#[test]
fn curved_cells_face_outward_and_cell_type_source_matches_paraview() {
    // Each 3D curved cell of the shapes file on its own: its surface encloses its volume.
    let DataSet::UnstructuredGrid(shapes) = td("lagrange_shapes.vtu") else { panic!() };
    let one = |cid: usize| {
        let mut cells = Cells::new();
        cells.push(shapes.cells.types[cid], shapes.cells.cell(cid));
        let mut g = UnstructuredGrid { points: shapes.points.clone(), cells, point_data: shapes.point_data.clone(), ..Default::default() };
        g.cell_data = shapes.cell_data.gather(&[cid]);
        DataSet::UnstructuredGrid(g)
    };
    for cid in 0..shapes.cells.len() {
        if shapes.cells.types[cid].dimension() < 3 {
            continue;
        }
        let ds = one(cid);
        for lvl in 1..=3 {
            let v = integrals(&run("tessellate", Some(&ds), &[("level", lvl.into())]))["Volume"];
            let s = signed_volume(&surface::extract_surface_level(&ds, lvl).unwrap());
            assert!((s - v).abs() < 0.02 * v, "{:?} level {lvl}: surface encloses {s}, volume {v}", shapes.cells.types[cid]);
            // The tessellated (linear) cells are valid VTK cells: their surface faces outward too.
            let t = run("tessellate", Some(&ds), &[("level", lvl.into())]);
            let st = signed_volume(&surface::extract_surface(&t).unwrap());
            assert!((st - v).abs() < 0.02 * v, "{:?} tessellated level {lvl}: {st} vs {v}", shapes.cells.types[cid]);
        }
    }

    // Unstructured Cell Types with 2x2x2 blocks: point and cell counts from ParaView 6.1.1.
    for (name, order, points, cells) in [
        ("Hex", 1, 27, 8),
        ("Lagrange Hexahedron", 3, 343, 8),
        ("Lagrange Tetrahedron", 2, 189, 96),
        ("Lagrange Wedge", 2, 125, 16),
        ("Lagrange Triangle", 3, 49, 8),
        ("Lagrange Quadrilateral", 2, 25, 4),
        ("Lagrange Curve", 3, 7, 2),
        ("Tet", 1, 35, 96),
        ("Wedge", 1, 27, 16),
        ("Pyramid", 1, 35, 48),
        ("Quadratic Tet", 1, 189, 96),
        ("Bezier Hexahedron", 2, 125, 8),
    ] {
        let ds = run("unstructured_cell_types", None, &[("cell_type", name.into()), ("blocks", serde_json::json!([2, 2, 2])), ("order", order.into())]);
        assert_eq!((ds.n_points(), ds.n_cells()), (points, cells), "{name}");
        let dim = if name.contains("Curve") {
            1
        } else if name.contains("Triangle") || name.contains("Quadrilateral") {
            2
        } else {
            3
        };
        let poly = ds.point_data().unwrap().get("Polynomial").unwrap().range(None);
        assert_eq!(poly, (1.0, 1.0 + 2.0 * dim as f64), "{name} Polynomial range");
        let it = integrals(&ds);
        let measure = it.get("Volume").or(it.get("Area")).or(it.get("Length")).copied().unwrap();
        assert!((measure - 2f64.powi(dim as i32)).abs() < 1e-9, "{name}: measure {measure}");
        if dim == 3 {
            let s = signed_volume(&surface::extract_surface(&ds).unwrap());
            assert!((s - 8.0).abs() < 1e-9, "{name}: outward surface encloses {s}");
        }
    }
}

#[test]
fn higher_order_files_round_trip() {
    // Degrees and rational weights survive writing (VTK XML and legacy) and reading back.
    let dir = std::env::temp_dir().join(format!("aurafx_ho_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["lagrange_shapes.vtu", "bezier_annulus.vtu"] {
        let ds = td(name);
        let want = integrals(&DataSet::PolyData(surface::extract_surface_level(&ds, 2).unwrap()))["Area"];
        for ext in ["vtu", "vtk"] {
            let path = dir.join(format!("{name}.{ext}"));
            aurafx_io::writers::write_file(&ds, &path).unwrap();
            let back = match aurafx_io::read_file(&path).unwrap() {
                aurafx_io::Loaded::Static(d) => (*d).clone(),
                _ => panic!(),
            };
            let got = integrals(&DataSet::PolyData(surface::extract_surface_level(&back, 2).unwrap()))["Area"];
            assert!((got - want).abs() < 1e-9 * want, "{name} via .{ext}: {got} vs {want}");
        }
    }
    // Files older than version 2.2 list the last two vertical hexahedron edges swapped.
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/lagrange_annulus.vtu")).unwrap();
    let old = dir.join("old.vtu");
    std::fs::write(&old, src.replace("version=\"2.2\"", "version=\"1.0\"")).unwrap();
    let (DataSet::UnstructuredGrid(a), aurafx_io::Loaded::Static(b)) = (td("lagrange_annulus.vtu"), aurafx_io::read_file(&old).unwrap()) else { panic!() };
    let DataSet::UnstructuredGrid(b) = &*b else { panic!() };
    let (ca, cb) = (a.cells.cell(0), b.cells.cell(0));
    // Order 3: vertical edges start at 8 + 4*2 + 4*2 = 24, two nodes each.
    assert_eq!(&ca[..28], &cb[..28]);
    assert_eq!((&ca[28..30], &ca[30..32]), (&cb[30..32], &cb[28..30]));
    assert_eq!(&ca[32..], &cb[32..]);
    if std::env::var_os("AURAFX_KEEP_TEST_FILES").is_some() {
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Quadratic cells (20-node hex, 15-node wedge, 8-node quad, quadratic-linear quad and wedge,
/// cubic line; testdata/quadratic_shapes.vtu) against ParaView 6.1.1.
#[test]
fn quadratic_cells_match_paraview() {
    let close = |a: f64, b: f64, what: &str| assert!((a - b).abs() <= 2e-7 * b.abs().max(1.0), "{what}: {a} vs ParaView {b}");
    let ds = td("quadratic_shapes.vtu");
    for (lvl, (points, area)) in
        [(34, 30.168865222284417), (77, 33.15411810680545), (261, 33.755654836998275), (977, 33.904836820852786)].into_iter().enumerate()
    {
        let s = DataSet::PolyData(surface::extract_surface_level(&ds, lvl as u32).unwrap());
        assert_eq!(s.n_points(), points, "surface level {lvl} points");
        close(integrals(&s)["Area"], area, &format!("surface level {lvl} area"));
    }
    let plane = [("origin", serde_json::json!([0.0, 0.0, -0.25])), ("normal", serde_json::json!([0.0, 0.0, 1.0]))];
    let it = integrals(&run("slice", Some(&ds), &plane));
    close(it["Area"], 4.776043107505829, "slice area");
    close(it["u"], 15.983134023592822, "slice integral");
    let it = integrals(&run("contour", Some(&ds), &[("array", "u".into()), ("values", serde_json::json!([3.0]))]));
    close(it["Area"], 4.191241161378176, "contour area");
    close(it["u"], 12.573723484134526, "contour integral");
    // One 20-node hexahedron. (ParaView's volumes of the other cells are signed or wrong: the
    // 15-node wedge here is inverted for VTK, the quadratic-linear wedge gives 0.0696 for the
    // same shape, and the cubic line 1.48 for a 2.25-long arc; auraFX gives positive, consistent values.)
    let DataSet::UnstructuredGrid(g) = &ds else { panic!() };
    let one = |cid: usize| {
        let mut cells = Cells::new();
        cells.push(g.cells.types[cid], g.cells.cell(cid));
        DataSet::UnstructuredGrid(UnstructuredGrid { points: g.points.clone(), cells, point_data: g.point_data.clone(), ..Default::default() })
    };
    let hex = one(0);
    close(integrals(&hex)["Volume"], 1.8313626454302376, "hex20 volume");
    close(integrals(&run("clip", Some(&hex), &plane))["Volume"], 0.4578406613575594, "hex20 clip");
    let it = integrals(&ds);
    assert!((it["Volume"] - (2.0 * 1.8313626454302376 + 2.0 * 0.5566589083226766)).abs() < 1e-9, "{it:?}");
    assert_eq!(g.cells.types[6], CellType::CubicLine);
    let arc = integrals(&run("tessellate", Some(&one(6)), &[("level", 5.into())]))["Length"];
    assert!((arc - 2.25).abs() < 2e-3, "cubic line follows its arc: {arc}");
}

/// Filters run in parallel chunks: the same result on one thread as on all of them (and,
/// for cutting filters, the same as the old serial code: points in order of first use).
#[test]
fn parallel_filters_do_not_depend_on_thread_count() {
    let img = DataSet::ImageData(sources::wavelet([-20, 20, -20, 20, -20, 20], 255.0, 0.5));
    let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
    assert!(ug.n_cells() > 3 * interp::CHUNK, "several chunks");
    let cases: Vec<(&str, Vec<(&str, serde_json::Value)>)> = vec![
        ("slice", vec![("normal", serde_json::json!([1.0, 1.0, 1.0]))]),
        ("contour", vec![("values", serde_json::json!([120.0, 200.0]))]),
        ("clip", vec![("normal", serde_json::json!([1.0, 1.0, 0.0]))]),
        ("threshold", vec![("lower", serde_json::json!(150.0)), ("upper", serde_json::json!(300.0))]),
        ("extract_surface", vec![]),
        ("gradient", vec![("array", "RTData".into())]),
        ("integrate_variables", vec![]),
    ];
    let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    for (id, set) in &cases {
        for input in [&img, &ug] {
            let many = run(id, Some(input), set);
            let single = one.install(|| run(id, Some(input), set));
            assert!(many == single, "{id} on {} differs between 1 and {} threads", input.type_name(), rayon::current_num_threads());
        }
    }
    // Stream Tracer traces seeds in parallel; the lines come out in seed order.
    let g = run("gradient", Some(&img), &[("array", "RTData".into())]);
    let set = [("vectors", serde_json::json!("RTData_gradient")), ("seeds", 40.into())];
    let many = run("stream_tracer", Some(&g), &set);
    let single = one.install(|| run("stream_tracer", Some(&g), &set));
    assert!(many.n_cells() > 10 && many == single, "stream tracer differs between 1 and many threads");

    // Extract Surface's parallel path agrees with the general one (a mesh with a mixed-in
    // polyhedron-free curved-free cell set vs. the same mesh through the general path).
    let fast = surface::extract_surface(&ug).unwrap();
    let general = surface::extract_surface(&DataSet::UnstructuredGrid({
        let DataSet::UnstructuredGrid(mut g) = ug.clone() else { unreachable!() };
        // A curved cell elsewhere sends the grid down the general path.
        let base = g.points.len() as u32;
        let mut p = g.points.data.to_f64_vec();
        p.extend_from_slice(&[100.0, 0.0, 0.0, 101.0, 0.0, 0.0, 100.5, 0.0, 0.0]);
        g.points = Points::new_f64(p);
        g.cells.push(CellType::QuadraticEdge, &[base, base + 1, base + 2]);
        for a in g.point_data.arrays.values_mut() {
            let mut v = a.data.to_f64_vec();
            v.extend(std::iter::repeat_n(0.0, 3 * a.ncomp));
            a.data = ArrayData::from_f64(a.scalar_type(), &v);
        }
        for a in g.cell_data.arrays.values_mut() {
            let mut v = a.data.to_f64_vec();
            v.extend(std::iter::repeat_n(0.0, a.ncomp));
            a.data = ArrayData::from_f64(a.scalar_type(), &v);
        }
        g
    }))
    .unwrap();
    // The quadratic edge's 3 points come first in the general output; compare by coordinates.
    let coords = |pd: &PolyData| pd.polys.connectivity.iter().map(|&i| pd.points.get(i as usize)).collect::<Vec<_>>();
    assert_eq!(general.polys.offsets, fast.polys.offsets);
    assert_eq!(coords(&general), coords(&fast));
    assert_eq!(general.points.len(), fast.points.len() + 3);
}

/// Hexahedra, wedges and pyramids are split into tetrahedra that agree across shared faces,
/// so contours are watertight and Tetrahedralize leaves no inner faces.
#[test]
fn cell_splits_conform_across_faces() {
    use std::collections::HashMap;
    let img = DataSet::ImageData(sources::wavelet([-10, 10, -10, 10, -10, 10], 255.0, 0.5));
    // Contour: every edge inside the domain is shared by two polygons.
    let DataSet::PolyData(c) = run("contour", Some(&img), &[("values", serde_json::json!([150.0]))]) else { panic!() };
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for i in 0..c.polys.len() {
        let p = c.polys.cell(i);
        for k in 0..p.len() {
            let (a, b) = (p[k], p[(k + 1) % p.len()]);
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let bb = img.bounds();
    let inner_open = edges
        .iter()
        .filter(|(_, &n)| n == 1)
        .filter(|((a, b), _)| {
            [a, b]
                .iter()
                .all(|&&q| (0..3).all(|k| (c.points.get(q as usize)[k] - bb.min[k]).abs() > 1e-9 && (c.points.get(q as usize)[k] - bb.max[k]).abs() > 1e-9))
        })
        .count();
    assert!(c.polys.len() > 1000);
    assert_eq!(inner_open, 0, "the isosurface has cracks");
    // Tetrahedralize: the surface is just the box, two triangles per boundary square.
    let t = run("tetrahedralize", Some(&img), &[]);
    let s = surface::extract_surface(&t).unwrap();
    assert_eq!(s.polys.len(), 6 * 20 * 20 * 2);
    // Random corner orders: volumes add up, tetrahedra are positive, and two cells sharing a
    // face cut it the same way.
    let mut seed = 12345u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as u32
    };
    let corners = |ty: CellType| -> Vec<[f64; 3]> {
        match ty {
            CellType::Hexahedron => vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
            CellType::Wedge => vec![[0., 0., 0.], [0., 1., 0.], [1., 0., 0.], [0., 0., 1.], [0., 1., 1.], [1., 0., 1.]],
            _ => vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0.5, 0.5, 1.]],
        }
    };
    let vol = |p: [[f64; 3]; 4]| {
        let e = |k: usize| [p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]];
        let (a, b, c) = (e(1), e(2), e(3));
        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
    };
    for (ty, want) in [(CellType::Hexahedron, 1.0), (CellType::Wedge, 0.5), (CellType::Pyramid, 1.0 / 3.0)] {
        let p = corners(ty);
        for _ in 0..500 {
            let ids: Vec<u32> = (0..p.len()).map(|_| rnd()).collect();
            let tets = simplex::split_by(ty, |k| ids[k]);
            let total: f64 = tets
                .iter()
                .map(|t| {
                    let v = vol(t.map(|k| p[k]));
                    assert!(v > 0.0, "{ty:?}: negative tetrahedron {t:?}");
                    v
                })
                .sum();
            assert!((total - want).abs() < 1e-12, "{ty:?}: volume {total}");
            // The triangles each quad face is cut into go through its smallest corner.
            for q in ty.faces().iter().filter(|q| q.len() == 4) {
                let m = *q.iter().min_by_key(|&&k| ids[k]).unwrap();
                for t in tets {
                    let on: Vec<usize> = t.iter().copied().filter(|k| q.contains(k)).collect();
                    if on.len() == 3 {
                        assert!(on.contains(&m), "{ty:?}: face {q:?} not cut through its smallest corner");
                    }
                }
            }
        }
    }
}

/// Sum of each component, first tuple and last tuple of a point array.
fn summary(d: &DataSet, name: &str) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let a = d.point_data().unwrap().get(name).unwrap_or_else(|| panic!("no {name}"));
    let (n, c) = (a.len(), a.ncomp);
    let v: Vec<f64> = (0..n * c).map(|i| a.data.get_f64(i)).collect();
    let sums = (0..c).map(|k| (0..n).map(|i| v[i * c + k]).sum()).collect();
    (sums, v[..c].to_vec(), v[(n - 1) * c..].to_vec())
}

fn close_all(tag: &str, got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(got.len(), want.len(), "{tag}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= tol * (1.0 + w.abs()), "{tag}: got {got:?}, ParaView {want:?}");
    }
}

/// Texture Map to Plane / Cylinder / Sphere, Perlin Noise and Spatio Temporal Harmonics
/// against ParaView 6.1.1 (sums, first and last tuples of the arrays it makes).
#[test]
fn texture_maps_noise_and_harmonics_match_paraview() {
    use serde_json::json;
    type Case<'a> = (&'a str, &'a str, Vec<(&'a str, serde_json::Value)>, [f64; 2], [f64; 2], [f64; 2]);
    let sphere = td("sphere.vtp");
    let contour = td("contour.vtp");
    let manual_plane = || vec![("automatic", json!(false)), ("origin", json!([-1, -1, 0])), ("point1", json!([1, -1, 0.2])), ("point2", json!([-1, 1, 0.5]))];
    let manual_cyl = || vec![("automatic", json!(false)), ("point1", json!([0, 0, -1])), ("point2", json!([0.2, 0.1, 1])), ("prevent_seam", json!(false))];
    let sphere_cases: Vec<Case> = vec![
        ("plane auto", "texture_map_to_plane", vec![], [265.000061, 265.000122], [0.5, 0.5], [0.565917, 0.482338]),
        ("plane manual", "texture_map_to_plane", manual_plane(), [262.376221, 249.411835], [0.544554, 0.588235], [0.511118, 0.337452]),
        ("cylinder auto", "texture_map_to_cylinder", vec![], [264.999969, 265.0], [0.5, 1.0], [0.083333, 0.004657]),
        ("cylinder seam", "texture_map_to_cylinder", vec![("prevent_seam", json!(false))], [253.499847, 265.0], [0.25, 1.0], [0.958333, 0.004657]),
        ("cylinder manual", "texture_map_to_cylinder", manual_cyl(), [278.310944, 261.728455], [0.574188, 0.987654], [0.957426, 0.010225]),
        ("sphere", "texture_map_to_sphere", vec![], [264.499969, 265.0], [0.0, 0.0], [0.083333, 0.956522]),
        ("sphere seam", "texture_map_to_sphere", vec![("prevent_seam", json!(false))], [253.249847, 265.0], [0.0, 0.0], [0.958333, 0.956522]),
    ];
    let contour_cases: Vec<Case> = vec![
        ("plane auto", "texture_map_to_plane", vec![], [244.030579, 230.792053], [0.399588, 0.20079], [0.599725, 0.685263]),
        ("plane manual", "texture_map_to_plane", manual_plane(), [293.961884, 212.162369], [-0.201944, -1.421091], [1.237624, 1.917968]),
        ("cylinder auto", "texture_map_to_cylinder", vec![], [234.701614, 230.999725], [0.6484, 0.092073], [0.40828, 1.0]),
        ("cylinder seam", "texture_map_to_cylinder", vec![("prevent_seam", json!(false))], [225.219498, 230.999725], [0.6758, 0.092073], [0.20414, 1.0]),
        ("cylinder manual", "texture_map_to_cylinder", manual_cyl(), [231.840714, 234.381546], [0.725582, -1.644086], [0.235952, 3.057424]),
        ("sphere", "texture_map_to_sphere", vec![], [232.267807, 231.000015], [0.629777, 0.786266], [0.382639, 0.12019]),
        ("sphere seam", "texture_map_to_sphere", vec![("prevent_seam", json!(false))], [225.270767, 231.000015], [0.685112, 0.786266], [0.19132, 0.12019]),
    ];
    for (data, cases) in [(&sphere, sphere_cases), (&contour, contour_cases)] {
        for (tag, id, set, sums, first, last) in cases {
            let out = run(id, Some(data), &set);
            let (s, f, l) = summary(&out, "Texture Coordinates");
            close_all(&format!("{tag} sums"), &s, &sums, 2e-5);
            close_all(&format!("{tag} first"), &f, &first, 2e-5);
            close_all(&format!("{tag} last"), &l, &last, 2e-5);
            assert_eq!(out.point_data().unwrap().active(AttributeKind::TCoords).map(|a| a.name.as_str()), Some("Texture Coordinates"));
        }
    }

    let wavelet = td("wavelet.vti");
    let p = run("perlin_noise", Some(&wavelet), &[("frequency", json!([0.15, 0.2, 0.25])), ("phase", json!([0.3, 0.1, 0.7])), ("amplitude", json!(2.5))]);
    let (s, f, l) = summary(&p, "PerlinNoise");
    close_all("perlin", &[s[0], f[0], l[0]], &[622.142334, -0.283101, -0.111462], 1e-5);
    let p = run("perlin_noise", Some(&sphere), &[]);
    let (s, f, l) = summary(&p, "PerlinNoise");
    close_all("perlin default", &[s[0], f[0], l[0]], &[60.971581, -0.859183, 0.818144], 1e-5);

    let h = run("spatio_temporal_harmonics", Some(&wavelet), &[]);
    let (s, f, l) = summary(&h, "SpatioTemporalHarmonics");
    close_all("harmonics filter", &[s[0], f[0], l[0]], &[-241.929211, -2.000078, -1.999893], 1e-6);
    assert!(h.point_data().unwrap().get("RTData").is_some());

    // The source at its first time step and at its fourth (0.9425).
    let def = find("spatio_temporal_harmonics_grid").unwrap();
    let params = def.default_params();
    let times = (def.temporal.unwrap().output_times)(&params, &[]);
    assert_eq!(times.len(), 20);
    let g = def.run(None, &params).unwrap();
    assert_eq!(g.n_points(), 21 * 21 * 21);
    let (s, f, l) = summary(&g, "SpatioTemporalHarmonics");
    close_all("harmonics source t0", &[s[0], f[0], l[0]], &[881.490474, 2.000171, 1.9998], 1e-6);
    let v = aurafx_filters::texture::harmonics_grid([-10, 10, -10, 10, -10, 10], &aurafx_filters::texture::DEFAULT_HARMONICS, times[3]).unwrap();
    let (s, f, l) = summary(&v, "SpatioTemporalHarmonics");
    close_all("harmonics source t3", &[s[0], f[0], l[0]], &[358.824287, 1.621371, 1.621274], 1e-6);
}

/// AMR: ParaView's AMR Gaussian Pulse source, and filters skipping the coarse cells finer
/// boxes cover (vtkGhostType REFINEDCELL), against ParaView 6.1.1.
#[test]
fn amr_gaussian_pulse_matches_paraview() {
    use serde_json::json;
    let cell_sum = |d: &DataSet, name: &str| -> f64 {
        match d {
            DataSet::MultiBlock(mb) => mb.leaves().iter().map(|(_, l)| l.cell_data().and_then(|c| c.get(name)).map(|a| (0..a.len()).map(|i| a.get(i, 0)).sum::<f64>()).unwrap_or(0.0)).sum(),
            d => d.cell_data().and_then(|c| c.get(name)).map(|a| (0..a.len()).map(|i| a.get(i, 0)).sum()).unwrap_or(0.0),
        }
    };
    let close = |a: f64, b: f64, tag: &str| assert!((a - b).abs() <= 1e-9 * (1.0 + b.abs()), "{tag}: {a} vs ParaView {b}");
    let col = |d: &DataSet, name: &str| -> f64 {
        let DataSet::Table(t) = d else { panic!("not a table") };
        t.column(name).unwrap_or_else(|| panic!("no column {name}")).get(0, 0)
    };
    let amr = run("amr_gaussian_pulse", None, &[]);
    assert!(aurafx_data::amr::is_amr(&amr));
    assert_eq!((amr.n_cells(), amr.n_points()), (605, 986));
    close(cell_sum(&amr, "Gaussian-Pulse"), 9.989747547467575, "pulse sum");
    // 60 of the 125 root cells are covered by the finer boxes.
    let DataSet::MultiBlock(mb) = &amr else { panic!() };
    let root = &mb.leaves()[0].1;
    let ghosts = root.cell_data().unwrap().get("vtkGhostType").unwrap();
    assert_eq!((0..ghosts.len()).filter(|&i| ghosts.get(i, 0) == 8.0).count(), 60);
    let two_d = run("amr_gaussian_pulse", None, &[("dimension", json!("2D"))]);
    assert_eq!(two_d.n_cells(), 73);
    close(cell_sum(&two_d, "Gaussian-Pulse"), 6.329186193165329, "2D pulse sum");

    // Integrals count each place once: the volume is the root box's.
    let iv = run("integrate_variables", Some(&amr), &[]);
    close(col(&iv, "Volume"), 15.625, "volume");
    close(col(&iv, "Gaussian-Pulse (cell)"), 0.14695209681902666, "pulse integral");
    // Slices through both levels: the area of the domain, the integral as ParaView's.
    for (origin, normal, want) in [([0.0, 0.0, 1.1], [0.0, 0.0, 1.0], 0.001999357733375088), ([-0.3, 0.0, 0.0], [1.0, 0.0, 0.0], 0.1035181663512717)] {
        let sl = run("slice", Some(&amr), &[("origin", json!(origin)), ("normal", json!(normal))]);
        let iv = run("integrate_variables", Some(&sl), &[]);
        close(col(&iv, "Area"), 6.25, "slice area");
        close(col(&iv, "Gaussian-Pulse (cell)"), want, "slice integral");
    }
    let th = run("threshold", Some(&amr), &[("array", json!("Gaussian-Pulse")), ("lower", json!(0.01)), ("upper", json!(1.0))]);
    assert_eq!(th.n_cells(), 80);
    close(cell_sum(&th, "Gaussian-Pulse"), 8.711454370986036, "threshold sum");
    // The surface: visible cells' outer faces per block (ParaView: 622 faces, area 62.5,
    // including the faces where coarse and fine boxes meet), with no faces of covered cells.
    let surf = aurafx_filters::surface::extract_surface(&amr).unwrap();
    assert_eq!(surf.polys.len(), 622);
    let sv = run("extract_surface", Some(&amr), &[]);
    close(col(&run("integrate_variables", Some(&sv), &[]), "Gaussian-Pulse (cell)"), 0.6484506420945171, "surface integral");
    let area: f64 = (0..surf.polys.len())
        .map(|c| {
            let q = surf.polys.cell(c);
            let (a, b, d) = (surf.points.get(q[0] as usize), surf.points.get(q[1] as usize), surf.points.get(q[3] as usize));
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
            ((u[1] * v[2] - u[2] * v[1]).powi(2) + (u[2] * v[0] - u[0] * v[2]).powi(2) + (u[0] * v[1] - u[1] * v[0]).powi(2)).sqrt()
        })
        .sum();
    close(area, 62.5, "surface area");
    // Resampling takes the finest cell at each sample (no sample sits on a cell face).
    let img = run("resample_to_image", Some(&amr), &[("nx", json!(14)), ("ny", json!(14)), ("nz", json!(14))]);
    let g = img.point_data().unwrap().get("Gaussian-Pulse").unwrap();
    close((0..g.len()).map(|i| g.get(i, 0)).sum(), 26.763096558880346, "resampled sum");
    // Cell to point data averages within each box (covered cells included), as ParaView.
    let pts = run("cell_data_to_point_data", Some(&amr), &[]);
    let psum: f64 = match &pts {
        DataSet::MultiBlock(mb) => mb.leaves().iter().map(|(_, l)| l.point_data().and_then(|p| p.get("Gaussian-Pulse")).map(|a| (0..a.len()).map(|i| a.get(i, 0)).sum::<f64>()).unwrap_or(0.0)).sum(),
        _ => panic!(),
    };
    close(psum, 17.109011619129443, "cell to point sum");
    // Level ids.
    let ids = run("overlapping_amr_level_ids", Some(&amr), &[]);
    close(cell_sum(&ids, "LevelIdScalars"), 480.0, "level ids"); // 2 fine boxes × 240 cells at level 1
}

/// AMR Contour on an AMReX plotfile whose isosurface is a sphere crossing the boundary
/// between the levels: closed (every edge shared by two triangles), a sphere's topology, and
/// the sphere's area and centre.
#[test]
fn amr_contour_is_closed_across_levels() {
    use serde_json::json;
    let amr = td("amr/plt_3d");
    let r = (f64::ln(1.0 / 0.3) / 4.0).sqrt(); // density = exp(-4 r²)
    let edges = |pd: &PolyData| -> HashMap<(u32, u32), usize> {
        let mut m = HashMap::new();
        for i in 0..pd.polys.len() {
            let c = pd.polys.cell(i);
            for k in 0..c.len() {
                let (a, b) = (c[k], c[(k + 1) % c.len()]);
                *m.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        m
    };
    let c = run("amr_contour", Some(&amr), &[("array", json!("density")), ("values", json!([0.3]))]);
    let DataSet::PolyData(pd) = &c else { panic!() };
    let e = edges(pd);
    let open = e.values().filter(|&&n| n != 2).count();
    assert_eq!(open, 0, "{open} edges not shared by exactly two triangles");
    let used: std::collections::HashSet<u32> = (0..pd.polys.len()).flat_map(|i| pd.polys.cell(i).to_vec()).collect();
    let euler = used.len() as i64 - e.len() as i64 + pd.polys.len() as i64;
    assert_eq!(euler, 2, "a sphere");
    let (mut area, mut centroid) = (0.0, [0.0f64; 3]);
    for i in 0..pd.polys.len() {
        let t = pd.polys.cell(i);
        let p = |k: usize| pd.points.get(t[k] as usize);
        for k in 1..t.len() - 1 {
            let (a, b, cc) = (p(0), p(k), p(k + 1));
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let da = 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            area += da;
            for q in 0..3 {
                centroid[q] += da * (a[q] + b[q] + cc[q]) / 3.0;
            }
        }
    }
    let want_a = 4.0 * std::f64::consts::PI * r * r;
    let centroid = centroid.map(|c| c / area);
    eprintln!("AMR contour: {} triangles, area {area} (sphere {want_a}), centre {centroid:?}", pd.polys.len());
    assert!((area / want_a - 1.0).abs() < 0.01, "area {area} vs {want_a}");
    assert!(centroid[0].abs() < 0.01 && centroid[1].abs() < 0.01 && (centroid[2] - 1.0).abs() < 0.01, "centred at (0, 0, 1)");

    // The plain Contour of a cell array of AMR data takes the same route (box by box, with
    // cell data averaged to points, it left 346 open edges along the level boundary).
    let plain = run("contour", Some(&amr), &[("array", json!(["CELLS", "density"])), ("values", json!([0.3]))]);
    assert_eq!(plain.n_cells(), pd.polys.len());

    // 2-D: an isoline that closes on itself (every point on two segments).
    // (z = 0 there, so density = exp(-4 (x² + y² + 1)): 0.01 is a circle of radius 0.39.)
    let c2 = run("amr_contour", Some(&td("amr/plt_2d/Header")), &[("array", json!("density")), ("values", json!([0.01]))]);
    let DataSet::PolyData(l) = &c2 else { panic!() };
    assert!(l.lines.len() > 10, "{} segments", l.lines.len());
    let mut deg: HashMap<u32, usize> = HashMap::new();
    for i in 0..l.lines.len() {
        let s = l.lines.cell(i);
        *deg.entry(s[0]).or_default() += 1;
        *deg.entry(s[s.len() - 1]).or_default() += 1;
    }
    assert!(deg.values().all(|&d| d == 2), "closed isoline");
    // The dual grid itself.
    let dg = run("amr_dual_grid", Some(&amr), &[]);
    assert!(dg.point_data().unwrap().get("velocity").is_some());
}

/// Hyper tree grids against ParaView 6.1.1: the descriptor source, files written by ParaView
/// (testdata/htg), To Unstructured Grid (leaves, their order and cell data), Depth Limiter,
/// Generate Fields, Evaluate Coarse and Cell Centers.
#[test]
fn hyper_tree_grids_match_paraview() {
    use serde_json::json;
    let sum = |d: &DataSet, name: &str, cells: bool| -> f64 {
        let a = if cells { d.cell_data() } else { d.point_data() }.and_then(|x| x.get(name)).unwrap_or_else(|| panic!("no {name}"));
        (0..a.len()).map(|i| (0..a.ncomp).map(|c| a.get(i, c)).sum::<f64>()).sum()
    };
    let close = |a: f64, b: f64, tag: &str| assert!((a - b).abs() <= 1e-9 * (1.0 + b.abs()), "{tag}: {a} vs ParaView {b}");
    let leaves = |d: &DataSet| match d {
        DataSet::HyperTreeGrid(h) => h.n_leaves(),
        _ => panic!("not a hyper tree grid"),
    };
    // (file, cells, visible leaves, Depth sum, leaf Depth sum, bounds max, depth-1 leaves, their Depth sum)
    let cases = [
        ("htg/tree3d.htg", 76, 68, 104.0, 99.0, [4.5, 2.0, 1.4], 33, 24.0),
        ("htg/tree2d.htg", 72, 65, 90.0, 87.0, [3.0, 6.0, 0.0], 41, 36.0),
        ("htg/tree3d_mask.htg", 76, 61, 104.0, 90.0, [4.5, 2.0, 1.4], 0, 0.0),
    ];
    for (file, cells, nleaves, dsum, lsum, bmax, d1, d1sum) in cases {
        let h = td(file);
        assert_eq!((h.n_cells(), leaves(&h)), (cells, nleaves), "{file}");
        close(sum(&h, "Depth", true), dsum, file);
        let ug = run("hyper_tree_grid_to_unstructured_grid", Some(&h), &[]);
        assert_eq!(ug.n_cells(), nleaves);
        close(sum(&ug, "Depth", true), lsum, file);
        assert_eq!(ug.bounds().max, bmax, "{file}");
        if d1 > 0 {
            let lim = run("hyper_tree_grid_depth_limiter", Some(&h), &[("depth", json!(1))]);
            let lug = run("hyper_tree_grid_to_unstructured_grid", Some(&lim), &[]);
            assert_eq!(lug.n_cells(), d1, "{file} depth 1");
            close(sum(&lug, "Depth", true), d1sum, file);
        }
        // Cell Centers on the leaves (ParaView: one point per visible leaf, Depth kept).
        let cc = run("cell_centers", Some(&h), &[]);
        assert_eq!(cc.n_points(), nleaves, "{file} centers");
    }
    // 2-D leaves are pixels; their surface is quads with corners around the edge (area 18).
    let surf = run("extract_surface", Some(&td("htg/tree2d.htg")), &[]);
    let DataSet::PolyData(sp) = &surf else { panic!() };
    assert!(sp.polys.types.iter().all(|t| *t == CellType::Quad));
    let area: f64 = (0..sp.polys.len())
        .map(|c| {
            let q = sp.polys.cell(c);
            let p: Vec<[f64; 3]> = q.iter().map(|&i| sp.points.get(i as usize)).collect();
            0.5 * ((p[0][0] * p[1][1] - p[1][0] * p[0][1]) + (p[1][0] * p[2][1] - p[2][0] * p[1][1]) + (p[2][0] * p[3][1] - p[3][0] * p[2][1]) + (p[3][0] * p[0][1] - p[0][0] * p[3][1])).abs()
        })
        .sum();
    assert!((area - 18.0).abs() < 1e-9, "area {area}");
    // The leaves in ParaView's order: their cell indices (OriginalIds) on the masked file.
    let ug = run("hyper_tree_grid_to_unstructured_grid", Some(&td("htg/tree3d_mask.htg")), &[]);
    let ids: Vec<i64> = (0..61).map(|i| ug.cell_data().unwrap().get("OriginalIds").unwrap().get(i, 0) as i64).collect();
    assert_eq!(
        ids,
        vec![
            2, 3, 9, 10, 14, 15, 16, 5, 6, 7, 8, 17, 27, 28, 29, 30, 31, 32, 33, 34, 20, 21, 22, 23, 24, 35, 36, 37, 38, 39, 40, 42, 26, 44, 45, 55, 56, 57, 58, 59, 60, 61, 62, 48, 49, 50,
            51, 52, 53, 63, 64, 65, 66, 67, 68, 69, 70, 72, 73, 74, 75
        ]
    );
    // Generate Fields: (file, CellSize, ValidCell, CellCenter sums, TotalVisibleVolume).
    for (file, size, valid, center, total) in
        [("htg/tree3d.htg", 16.40625, 68.0, 236.2, 12.6), ("htg/tree2d.htg", 26.666666667, 65.0, 315.0, 18.0), ("htg/tree3d_mask.htg", 16.40625, 61.0, 218.325, 10.303125)]
    {
        let g = run("hyper_tree_grid_generate_fields", Some(&td(file)), &[]);
        assert!((sum(&g, "CellSize", true) - size).abs() < 1e-8, "{file} CellSize {}", sum(&g, "CellSize", true));
        close(sum(&g, "ValidCell", true), valid, file);
        close(sum(&g, "CellCenter", true), center, file);
        close(g.field_data().get("TotalVisibleVolume").unwrap().get(0, 0), total, file);
    }
    // Evaluate Coarse on the masked file (default 0.5), Depth sums.
    let m = td("htg/tree3d_mask.htg");
    for (op, want) in [("Min", 106.0), ("Max", 115.0), ("Sum", 261.0), ("Average", 111.375), ("Unmasked Average", 111.375), ("Elder Child", 113.0), ("Splatting Average", 126.0)] {
        let e = run("hyper_tree_grid_evaluate_coarse", Some(&m), &[("operator", json!(op)), ("default", json!(0.5))]);
        close(sum(&e, "Depth", true), want, op);
    }
    // The descriptor source builds the same grids as the files ParaView wrote from it.
    let d3 = "R.R. ..R. .... | ...R .... R... ..R. R... ...R | .... .... .... .... .... .... .... .... .... ....";
    let s = run("hyper_tree_grid", None, &[("dimensions", json!([4, 3, 3])), ("max_depth", json!(4)), ("descriptor", json!(d3)), ("grid_scale", json!([1.5, 1.0, 0.7]))]);
    assert_eq!((s.n_cells(), leaves(&s)), (76, 68));
    close(sum(&s, "Depth", true), 104.0, "source");
    let sug = run("hyper_tree_grid_to_unstructured_grid", Some(&s), &[]);
    let fug = run("hyper_tree_grid_to_unstructured_grid", Some(&td("htg/tree3d.htg")), &[]);
    let pts = |d: &DataSet| d.points().unwrap();
    assert_eq!((0..pts(&sug).len()).map(|i| pts(&sug).get(i)).collect::<Vec<_>>(), (0..pts(&fug).len()).map(|i| pts(&fug).get(i)).collect::<Vec<_>>());
    let d2 = "R..R.R..R | .R....... ......... R........ ..R...... | ......... ......... .........";
    let s2 = run("hyper_tree_grid", None, &[("dimensions", json!([4, 4, 1])), ("branch_factor", json!("3")), ("max_depth", json!(3)), ("descriptor", json!(d2)), ("grid_scale", json!([1.0, 2.0, 1.0]))]);
    assert_eq!((s2.n_cells(), leaves(&s2)), (72, 65));
    // The random source with VTK's random sequence: ParaView's defaults and one other setting.
    for (set, cells, nleaves, dsum, leaf_sum, first_ids) in [
        (vec![], 65354, 57191, 305288.0, 275290.0, vec![0i64, 14571, 14587, 14588, 14589, 14595, 14596, 14603, 14604, 14605, 14606, 14607, 14608, 14609, 14610]),
        (
            vec![("seed", json!(7)), ("max_depth", json!(3)), ("split_fraction", json!(0.4)), ("dimensions", json!([4, 3, 1]))],
            34,
            27,
            52.0,
            46.0,
            vec![1, 2, 5, 6, 7, 8, 4, 10, 17, 18, 19, 25, 26, 27, 28],
        ),
    ] {
        let r = run("random_hyper_tree_grid", None, &set);
        assert_eq!((r.n_cells(), leaves(&r)), (cells, nleaves));
        close(sum(&r, "Depth", true), dsum, "random depth");
        let ug = run("hyper_tree_grid_to_unstructured_grid", Some(&r), &[]);
        close(sum(&ug, "Depth", true), leaf_sum, "random leaves depth");
        let ids: Vec<i64> = (0..15).map(|i| ug.cell_data().unwrap().get("OriginalIds").unwrap().get(i, 0) as i64).collect();
        assert_eq!(ids, first_ids);
    }
}


/// Molecules: the CML reader and Compute Molecule Bonds against ParaView 6.1.1 (its CML
/// reader and bond perceiver on testdata/molecules/test.cml).
#[test]
fn molecule_bonds_match_paraview() {
    use serde_json::json;
    let m = td("molecules/test.cml");
    let DataSet::PolyData(pd) = &m else { panic!() };
    let z: Vec<i64> = (0..20).map(|i| pd.point_data.get("atomic_number").unwrap().get(i, 0) as i64).collect();
    assert_eq!(z, vec![6, 6, 6, 6, 6, 6, 17, 1, 1, 1, 7, 1, 1, 16, 1, 35, 8, 1, 1, 11]);
    assert_eq!(pd.lines.len(), 17);
    let orders: Vec<f64> = (20..37).map(|i| pd.cell_data.get("bond_order").unwrap().get(i, 0)).collect();
    assert_eq!(orders, vec![2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
    let bonds = |d: &DataSet| -> Vec<(u32, u32)> {
        let DataSet::PolyData(p) = d else { panic!() };
        let mut v: Vec<(u32, u32)> = (0..p.lines.len()).map(|c| (p.lines.cell(c)[0].min(p.lines.cell(c)[1]), p.lines.cell(c)[0].max(p.lines.cell(c)[1]))).collect();
        v.sort();
        v
    };
    let file_bonds = bonds(&m);
    // The atoms alone: bonds perceived from distances only, as ParaView's filter computes them.
    let mut atoms = pd.clone();
    atoms.lines = Cells::new();
    atoms.cell_data = Attributes::new();
    let atoms = DataSet::PolyData(atoms);
    let mut with_extra = file_bonds.clone();
    with_extra.extend([(11, 12), (16, 19)]);
    with_extra.sort();
    for (set, want) in [(vec![], with_extra.clone()), (vec![("tolerance", json!(0.1))], file_bonds.clone()), (vec![("absolute", json!(false)), ("tolerance", json!(1.1))], file_bonds.clone())] {
        assert_eq!(bonds(&run("compute_molecule_bonds", Some(&atoms), &set)), want, "{set:?}");
    }
    // On the file's molecule the file's bonds stay and only the new ones are added (with order 1).
    let b = run("compute_molecule_bonds", Some(&m), &[]);
    assert_eq!(bonds(&b), with_extra);
    assert_eq!(b.cell_data().unwrap().get("bond_order").unwrap().len(), 20 + 19);
}

/// Cell grids against ParaView 6.1.1 (testdata/cellgrid: VTK's DG test files; reference in
/// paraview_reference.json from scripts/cellgrid_reference.py): To Unstructured Grid (every
/// corner point and value, HGRAD/HCURL/HDIV), Centers, Elevation and the Cell Grid source.
#[test]
fn cell_grids_match_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cellgrid/paraview_reference.json")).unwrap()).unwrap();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + b.abs());
    for (file, want) in reference.as_object().unwrap() {
        let g = td(&format!("cellgrid/{file}"));
        assert!(matches!(g, DataSet::CellGrid(_)), "{file}");
        let ug = run("cell_grid_to_unstructured_grid", Some(&g), &[]);
        assert_eq!(ug.n_cells() as u64, want["ug"]["cells"].as_u64().unwrap(), "{file} cells");
        let pts = ug.points().unwrap();
        let wpts = want["ug"]["points"].as_array().unwrap();
        // ParaView also lists the higher-order nodes of quadratic cells; compare at the corners.
        let mut matched = 0;
        for (wi, wp) in wpts.iter().enumerate() {
            let wp: Vec<f64> = wp.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            let Some(i) = (0..pts.len()).find(|&i| (0..3).all(|k| close(pts.get(i)[k], wp[k]))) else { continue };
            matched += 1;
            for (name, vals) in want["ug"]["arrays"].as_object().unwrap() {
                let a = ug.point_data().unwrap().get(name).unwrap_or_else(|| panic!("{file}: no {name}"));
                let wv: Vec<f64> = vals[wi].as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
                for (c, &w) in wv.iter().enumerate() {
                    // ParaView keeps some files' values in single precision.
                    if w.is_finite() {
                        assert!((a.get(i, c) - w).abs() <= 1e-6 * (1.0 + w.abs()), "{file} {name} at {wp:?}: {} vs ParaView {w}", a.get(i, c));
                    }
                }
            }
        }
        assert_eq!(matched, pts.len(), "{file}: every corner point is one of ParaView's");
        // Centers (ParaView's are garbage on the mixed-type file).
        if file != "dgMixed.dg" {
            let c = run("cell_grid_centers", Some(&g), &[]);
            let cp = c.points().unwrap();
            let wc = want["centers"]["points"].as_array().unwrap();
            assert_eq!(cp.len(), wc.len(), "{file} centers");
            for (i, w) in wc.iter().enumerate() {
                let w: Vec<f64> = w.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
                assert!((0..3).all(|k| (cp.get(i)[k] - w[k]).abs() < 1e-9), "{file} center {i}: {:?} vs {w:?}", cp.get(i));
                for (name, vals) in want["centers"]["arrays"].as_object().unwrap() {
                    let a = c.point_data().unwrap().get(name).unwrap();
                    for (k, w) in vals[i].as_array().unwrap().iter().enumerate() {
                        let w = w.as_f64().unwrap();
                        assert!((a.get(i, k) - w).abs() < 1e-6 * (1.0 + w.abs()), "{file} center {i} {name}: {} vs {w}", a.get(i, k));
                    }
                }
            }
        }
        // Elevation along z: the z coordinate at every corner.
        let e = run("cell_grid_elevation", Some(&g), &[]);
        let eu = run("cell_grid_to_unstructured_grid", Some(&e), &[]);
        let ea = eu.point_data().unwrap().get("Elevation").unwrap();
        let ep = eu.points().unwrap();
        for i in 0..ep.len() {
            assert!((ea.get(i, 0) - ep.get(i)[2]).abs() < 1e-12, "{file} elevation");
        }
    }
    // The source's 3-D cells (ParaView: sums of the corner values).
    for (ty, hcurl, hdiv) in [("Hexahedron", [2.0, 0.0, 0.0], [0.0, -4.0, 0.0]), ("Tetrahedron", [4.0, 2.0, 2.0], [1.0, -3.0, 1.0])] {
        let s = run("cell_grid", None, &[("cell_type", json!(ty))]);
        let u = run("cell_grid_to_unstructured_grid", Some(&s), &[]);
        let sum = |name: &str, c: usize| -> f64 {
            let a = u.point_data().unwrap().get(name).unwrap();
            (0..a.len()).map(|i| a.get(i, c)).sum()
        };
        for c in 0..3 {
            assert!(close(sum("hcurl", c), hcurl[c]), "{ty} hcurl {c}: {}", sum("hcurl", c));
            assert!(close(sum("hdiv", c), hdiv[c]), "{ty} hdiv {c}: {}", sum("hdiv", c));
        }
        assert!(close(sum("hgrad", 0), 1.0));
    }
}

/// Hyper tree grids written to `.htg` and read back: the same visible leaves, in the same
/// order, with the same cell data (files from ParaView, both sources with and without a mask,
/// a 2-D branch factor 3 grid, and a grid whose cells are numbered level by level).
#[test]
fn hyper_tree_grid_files_round_trip() {
    use serde_json::json;
    let dir = std::env::temp_dir().join(format!("aurafx_htg_rt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut grids = vec![
        ("tree3d", td("htg/tree3d.htg")),
        ("tree2d", td("htg/tree2d.htg")),
        ("tree3d_mask", td("htg/tree3d_mask.htg")),
        ("random", run("random_hyper_tree_grid", None, &[("seed", json!(7)), ("max_depth", json!(4)), ("split_fraction", json!(0.4))])),
        ("descriptor", run("hyper_tree_grid", None, &[("dimensions", json!([4, 3, 1])), ("max_depth", json!(3)), ("descriptor", json!("R..R.. | .R.. R... | ........"))])),
    ];
    // The descriptor source with a mask against ParaView 6.1.1: (cells, visible leaves, Depth sum).
    for (k, (dims, depth, desc, mask, want)) in [
        ([3, 3, 1], 2, "R..R | ........", "1101 | 11110111", (12, 8, 7.0)),
        ([3, 3, 1], 3, "R.R. | .R...... | ....", "1111 | 01111111 | 1101", (16, 11, 12.0)),
        ([3, 3, 3], 2, "R....R.. | ................", "10111111 | 1111111111111110", (24, 20, 15.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let g = run("hyper_tree_grid", None, &[("dimensions", json!(dims)), ("max_depth", json!(depth)), ("descriptor", json!(desc)), ("use_mask", json!(true)), ("mask", json!(mask))]);
        let DataSet::HyperTreeGrid(h) = &g else { panic!() };
        let u = run("hyper_tree_grid_to_unstructured_grid", Some(&g), &[]);
        let d = u.cell_data().unwrap().get("Depth").unwrap();
        assert_eq!((h.n_vertices(), h.n_leaves(), (0..d.len()).map(|i| d.get(i, 0)).sum::<f64>()), want, "masked source {k}");
        grids.push((["mask0", "mask1", "mask2"][k], g));
    }
    for (name, g) in grids {
        let DataSet::HyperTreeGrid(h) = &g else { panic!("{name}") };
        let path = dir.join(format!("{name}.htg"));
        aurafx_io::writers::write_file(&g, &path).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let back = match aurafx_io::read_file(&path).unwrap() {
            aurafx_io::Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        };
        let DataSet::HyperTreeGrid(b) = &back else { panic!("{name}: read back as {}", back.type_name()) };
        assert_eq!((b.n_vertices(), b.n_leaves(), b.max_depth(), b.branch_factor, b.coords.clone()), (h.n_vertices(), h.n_leaves(), h.max_depth(), h.branch_factor, h.coords.clone()), "{name}");
        // What ParaView 6.1.1 reads from our files: (cells, visible leaves, leaf Depth sum).
        let pv = match name {
            "descriptor" => Some((22, 18, 22.0)),
            "random" => Some((8226, 7204, 26635.0)),
            _ => None,
        };
        let (m0, _) = h.leaves_mesh();
        let (m1, _) = b.leaves_mesh();
        if let Some(want) = pv {
            let d = m1.cell_data.get("Depth").unwrap();
            assert_eq!((b.n_vertices(), b.n_leaves(), (0..d.len()).map(|i| d.get(i, 0)).sum::<f64>()), want, "{name} as ParaView reads it");
        }
        assert_eq!(m0.points, m1.points, "{name} points");
        assert_eq!(m0.cells, m1.cells, "{name} cells");
        for a in h.cell_data.iter() {
            let x0 = m0.cell_data.get(&a.name).unwrap();
            let x1 = m1.cell_data.get(&a.name).unwrap_or_else(|| panic!("{name}: lost {}", a.name));
            assert_eq!(x0.data, x1.data, "{name} {}", a.name);
        }
    }
    // Keep the files for a check in ParaView: AURAFX_KEEP_HTG=folder.
    if let Ok(keep) = std::env::var("AURAFX_KEEP_HTG") {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            std::fs::copy(e.path(), std::path::Path::new(&keep).join(e.file_name())).unwrap();
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Hyper Tree Grid To Dual Grid against ParaView 6.1.1 (testdata/htg/dual_reference.json from
/// scripts/htg_dual_reference.py): every dual cell in order, its type, the places of its
/// points and the Depth there, on ParaView's files and on sources in 1-D, 2-D and 3-D, with
/// and without masks.
#[test]
fn hyper_tree_grid_dual_matches_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/htg/dual_reference.json")).unwrap()).unwrap();
    let source = |dims: [i64; 3], depth: i64, desc: &str, mask: Option<&str>| {
        let mut p = vec![("dimensions", json!(dims)), ("max_depth", json!(depth)), ("descriptor", json!(desc))];
        if let Some(m) = mask {
            p.push(("use_mask", json!(true)));
            p.push(("mask", json!(m)));
        }
        run("hyper_tree_grid", None, &p)
    };
    let inputs: Vec<(&str, DataSet)> = vec![
        ("tree3d.htg", td("htg/tree3d.htg")),
        ("tree2d.htg", td("htg/tree2d.htg")),
        ("tree3d_mask.htg", td("htg/tree3d_mask.htg")),
        ("line", source([6, 1, 1], 3, "R.R.R | .R.... | ..", None)),
        ("plane", source([4, 3, 1], 3, "R..R.. | .R.. R... | ........", None)),
        ("plane_mask", source([3, 3, 1], 3, "R.R. | .R...... | ....", Some("1111 | 01111111 | 1101"))),
        ("plane_mask2", source([4, 4, 1], 2, "R...R.... | ........", Some("111011111 | 11111011"))),
        ("box_mask", source([3, 3, 3], 2, "R....R.. | ................", Some("10111111 | 1111111111111110"))),
        ("box_mask2", source([3, 3, 3], 3, "R......R | .......R ........ | ........", Some("11101111 | 11111111 11011111 | 11111110"))),
        ("random_mask2d", run("random_hyper_tree_grid", None, &[("dimensions", json!([5, 5, 1])), ("seed", json!(3)), ("max_depth", json!(3)), ("split_fraction", json!(0.5)), ("masked_fraction", json!(0.4))])),
        ("random_mask3d", run("random_hyper_tree_grid", None, &[("dimensions", json!([3, 3, 3])), ("seed", json!(5)), ("max_depth", json!(3)), ("split_fraction", json!(0.5)), ("masked_fraction", json!(0.4))])),
    ];
    for (name, g) in inputs {
        let want = reference[name]["cells"].as_array().unwrap();
        let d = run("hyper_tree_grid_to_dual_grid", Some(&g), &[]);
        let DataSet::UnstructuredGrid(u) = &d else { panic!("{name}") };
        if u.cells.len() != want.len() {
            for c in 0..u.cells.len() {
                let p: Vec<[f64; 3]> = u.cells.cell(c).iter().map(|&i| u.points.get(i as usize)).collect();
                eprintln!("ours {c}: {p:?} depth {:?}", u.cells.cell(c).iter().map(|&i| u.point_data.get("Depth").unwrap().get(i as usize, 0)).collect::<Vec<_>>());
            }
            for (c, w) in want.iter().enumerate() {
                eprintln!("pv {c}: {} depth {}", w["points"], w["depth"]);
            }
        }
        assert_eq!(u.cells.len(), want.len(), "{name}: dual cells");
        let depth = u.point_data.get("Depth").unwrap_or_else(|| panic!("{name}: no Depth"));
        for (c, w) in want.iter().enumerate() {
            let (ty, ids) = (u.cells.cell_type(c), u.cells.cell(c));
            assert_eq!(ty.vtk_id() as u64, w["type"].as_u64().unwrap(), "{name} cell {c} type");
            let wp = w["points"].as_array().unwrap();
            assert_eq!(ids.len(), wp.len(), "{name} cell {c}");
            for (k, (&id, p)) in ids.iter().zip(wp).enumerate() {
                let p: Vec<f64> = p.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
                let q = u.points.get(id as usize);
                // ParaView keeps the dual points in single precision.
                assert!((0..3).all(|a| (q[a] - p[a]).abs() < 1e-6 * (1.0 + p[a].abs())), "{name} cell {c} point {k}: {q:?} vs ParaView {p:?}");
                assert_eq!(depth.get(id as usize, 0), w["depth"][k].as_f64().unwrap(), "{name} cell {c} point {k} Depth");
            }
        }
    }
}

/// The Random Hyper Tree Grid source's Masked fraction against ParaView 6.1.1: (cells, visible
/// leaves, their Depth sum, masked cells) for 3-D and 2-D grids and a fully masked one.
#[test]
fn random_hyper_tree_grid_mask_matches_paraview() {
    use serde_json::json;
    for (dims, seed, depth, split, frac, want) in [
        ([6, 6, 3], 0, 5, 0.5, 0.3, (65354, 18320, 87348.0, 43117)),
        ([7, 5, 1], 3, 4, 0.45, 0.5, (408, 61, 152.0, 323)),
        ([4, 4, 4], 11, 3, 0.6, 0.7, (4019, 602, 1701.0, 3317)),
        ([5, 5, 1], 2, 3, 0.5, 1.0, (152, 0, 0.0, 16)),
    ] {
        let g = run(
            "random_hyper_tree_grid",
            None,
            &[("dimensions", json!(dims)), ("seed", json!(seed)), ("max_depth", json!(depth)), ("split_fraction", json!(split)), ("masked_fraction", json!(frac))],
        );
        let DataSet::HyperTreeGrid(h) = &g else { panic!() };
        let u = run("hyper_tree_grid_to_unstructured_grid", Some(&g), &[]);
        let dsum = u.cell_data().and_then(|c| c.get("Depth")).map(|d| (0..d.len()).map(|i| d.get(i, 0)).sum::<f64>()).unwrap_or(0.0);
        let masked = h.mask.as_ref().map(|m| m.iter().filter(|b| **b).count()).unwrap_or(0);
        assert_eq!((h.n_vertices(), h.n_leaves(), dsum, masked), want, "{dims:?} seed {seed} masked {frac}");
    }
}

/// Resample AMR against ParaView 6.1.1 (testdata/amr/resample_reference.json from
/// scripts/resample_amr_reference.py, demand-driven mode off): the grid (origin, spacing,
/// dimensions, also for a region reaching past the data) and every value, at the points and at
/// the cell centers, on the AMR Gaussian pulse (3-D and 2-D) and a FLASH file.
#[test]
fn resample_amr_matches_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/amr/resample_reference.json")).unwrap()).unwrap();
    for (key, want) in reference.as_object().unwrap() {
        let input = match key.as_str() {
            "pulse2d" => run("amr_gaussian_pulse", None, &[("dimension", json!("2D"))]),
            "flash3d" => td("amr/flash3d_hdf5_plt_cnt_0007"),
            _ => run("amr_gaussian_pulse", None, &[]),
        };
        let mut params = vec![("samples", want["samples"].clone()), ("to_nodes", json!(want["nodes"].as_i64() == Some(1)))];
        if !want["region"].is_null() {
            params.push(("whole_domain", json!(false)));
            params.push(("min", want["region"][0].clone()));
            params.push(("max", want["region"][1].clone()));
        }
        let out = run("resample_amr", Some(&input), &params);
        let DataSet::ImageData(img) = &out else { panic!("{key}") };
        let f3 = |v: &Value| -> [f64; 3] { std::array::from_fn(|k| v[k].as_f64().unwrap()) };
        let close3 = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() <= 1e-12 * (1.0 + b[k].abs()));
        assert!(close3(img.origin, f3(&want["origin"])), "{key} origin {:?}", img.origin);
        assert!(close3(img.spacing, f3(&want["spacing"])), "{key} spacing {:?}", img.spacing);
        assert_eq!(img.dims().map(|d| d as f64), f3(&want["dims"]), "{key} dims");
        let attrs = if want["nodes"].as_i64() == Some(1) { &img.point_data } else { &img.cell_data };
        if want["nodes"].as_i64() == Some(0) {
            // ParaView's cell-center mode copies values from the wrong cells (its first cells
            // inside a level-1 box all get the centroid (0.125, 0.125, 0.125)); here each
            // cell's donor (the pulse's Centroid array) must be the cell holding its center:
            // within half a level-0 cell, and half a level-1 cell inside the level-1 boxes.
            let c = attrs.get("Centroid").unwrap();
            let n = img.dims().map(|d| d - 1);
            for k in 0..n[2] {
                for j in 0..n[1] {
                    for i in 0..n[0] {
                        let q = [img.origin[0] + (i as f64 + 0.5) * img.spacing[0], img.origin[1] + (j as f64 + 0.5) * img.spacing[1], img.origin[2] + (k as f64 + 0.5) * img.spacing[2]];
                        let id = i + n[0] * (j + n[1] * k);
                        let fine = (q[0] <= -1.0 && q[1] <= -0.5) || (q[0] >= -0.5 && q[1] >= -1.0);
                        let half = if fine { 0.125 } else { 0.25 };
                        assert!((0..3).all(|a| (c.get(id, a) - q[a]).abs() <= half + 1e-12), "{key} cell {id}: centroid {:?} for center {q:?}", (0..3).map(|a| c.get(id, a)).collect::<Vec<_>>());
                    }
                }
            }
            continue;
        }
        for (name, vals) in want["arrays"].as_object().unwrap() {
            if name == "vtkGhostType" {
                continue;
            }
            let a = attrs.get(name).unwrap_or_else(|| panic!("{key}: no {name}"));
            let vals: Vec<f64> = vals.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            assert_eq!(a.len() * a.ncomp, vals.len(), "{key} {name} size");
            for (i, w) in vals.iter().enumerate() {
                let g = a.get(i / a.ncomp, i % a.ncomp);
                assert!((g - w).abs() <= 1e-12 * (1.0 + w.abs()), "{key} {name}[{i}]: {g} vs ParaView {w}");
            }
        }
    }
}

/// Hyper Tree Grid Geometry and Feature Edges against ParaView 6.1.1
/// (testdata/htg/surface_reference.json from scripts/htg_surface_reference.py): every output
/// cell in ParaView's order, its points and the Depth it carries, on ParaView's files and on
/// 1-D, 2-D and 3-D sources with and without masks.
#[test]
fn hyper_tree_grid_surfaces_match_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/htg/surface_reference.json")).unwrap()).unwrap();
    let source = |dims: [i64; 3], depth: i64, desc: &str, mask: Option<&str>| {
        let mut p = vec![("dimensions", json!(dims)), ("max_depth", json!(depth)), ("descriptor", json!(desc))];
        if let Some(m) = mask {
            p.push(("use_mask", json!(true)));
            p.push(("mask", json!(m)));
        }
        run("hyper_tree_grid", None, &p)
    };
    let random = |dims: [i64; 3], seed: i64, depth: i64, split: f64, frac: f64| {
        run("random_hyper_tree_grid", None, &[("dimensions", json!(dims)), ("seed", json!(seed)), ("max_depth", json!(depth)), ("split_fraction", json!(split)), ("masked_fraction", json!(frac))])
    };
    let inputs: Vec<(&str, DataSet)> = vec![
        ("tree3d.htg", td("htg/tree3d.htg")),
        ("tree2d.htg", td("htg/tree2d.htg")),
        ("tree3d_mask.htg", td("htg/tree3d_mask.htg")),
        ("line", source([6, 1, 1], 3, "R.R.R | .R.... | ..", None)),
        ("plane_mask", source([3, 3, 1], 3, "R.R. | .R...... | ....", Some("1111 | 01111111 | 1101"))),
        ("box_mask2", source([3, 3, 3], 3, "R......R | .......R ........ | ........", Some("11101111 | 11111111 11011111 | 11111110"))),
        ("random_mask3d", random([3, 3, 3], 5, 3, 0.5, 0.4)),
        ("random_mask2d", random([5, 5, 1], 3, 3, 0.5, 0.4)),
    ];
    for (name, g) in inputs {
        for (filter, id) in [("geometry", "hyper_tree_grid_geometry"), ("edges", "hyper_tree_grid_feature_edges")] {
            let key = format!("{name}#{filter}");
            let want = &reference[&key];
            let DataSet::PolyData(p) = run(id, Some(&g), &[]) else { panic!("{key}") };
            let cells = if p.polys.is_empty() { &p.lines } else { &p.polys };
            let wc = want["cells"].as_array().unwrap();
            assert_eq!(cells.len(), wc.len(), "{key}: cells");
            assert_eq!(p.points.len() as u64, want["points"].as_u64().unwrap(), "{key}: points");
            let depth = p.cell_data.get("Depth").unwrap_or_else(|| panic!("{key}: no Depth"));
            for (c, w) in wc.iter().enumerate() {
                let ids = cells.cell(c);
                let wp = w[0].as_array().unwrap();
                assert_eq!(ids.len(), wp.len(), "{key} cell {c}");
                for (k, (&id, q)) in ids.iter().zip(wp).enumerate() {
                    let q: Vec<f64> = q.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
                    let g = p.points.get(id as usize);
                    assert!((0..3).all(|a| (g[a] - q[a]).abs() < 1e-6 * (1.0 + q[a].abs())), "{key} cell {c} point {k}: {g:?} vs ParaView {q:?}");
                }
                assert_eq!(depth.get(c, 0), w[1].as_f64().unwrap(), "{key} cell {c} Depth");
            }
        }
    }
}

/// Transform composes its rotations as ParaView's (vtkPVTransform: R = Rz Rx Ry), checked on a
/// point against ParaView 6.1.1 (translate (0.5, -1, 2), rotate (30, 40, 50), scale (2, 3, 0.5)).
#[test]
fn transform_matches_paraview() {
    let f = aurafx_filters::transform::affine([0.5, -1.0, 2.0], [30.0, 40.0, 50.0], [2.0, 3.0, 0.5]);
    let q = f([1.0, 2.0, 3.0]);
    let want = [-1.9281978607177734, 4.296149253845215, 4.88178014755249]; // float32 in ParaView
    assert!((0..3).all(|k| (q[k] - want[k]).abs() < 1e-6), "{q:?}");
}

/// Cell Grid Transform, Cell Grid Warp and Convert To Cell Grid against ParaView 6.1.1
/// (testdata/cellgrid/filters_reference.json from scripts/cellgrid_filters_reference.py), each
/// seen through Cell Grid To Unstructured Grid: every corner point and value (the HCURL and HDIV
/// fields follow the transformed shape). The .dg writer: written grids read back the same.
#[test]
fn cell_grid_filters_match_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cellgrid/filters_reference.json")).unwrap()).unwrap();
    let check = |key: &str, g: &DataSet| {
        let want = &reference[key];
        let ug = run("cell_grid_to_unstructured_grid", Some(g), &[]);
        assert_eq!(ug.n_cells() as u64, want["cells"].as_u64().unwrap(), "{key} cells");
        let pts = ug.points().unwrap();
        let wp = want["points"].as_array().unwrap();
        assert_eq!(pts.len(), wp.len(), "{key} points");
        for (wi, w) in wp.iter().enumerate() {
            let w: Vec<f64> = w.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            let i = (0..pts.len()).find(|&i| (0..3).all(|k| (pts.get(i)[k] - w[k]).abs() <= 1e-5 * (1.0 + w[k].abs()))).unwrap_or_else(|| panic!("{key}: no point at {w:?}"));
            for (name, vals) in want["arrays"].as_object().unwrap() {
                let a = ug.point_data().unwrap().get(name).unwrap_or_else(|| panic!("{key}: no {name}"));
                for (c, v) in vals[wi].as_array().unwrap().iter().enumerate() {
                    let v = v.as_f64().unwrap_or(f64::NAN);
                    if v.is_finite() {
                        assert!((a.get(i, c) - v).abs() <= 1e-5 * (1.0 + v.abs()), "{key} {name}[{c}] at {w:?}: {} vs ParaView {v}", a.get(i, c));
                    }
                }
            }
        }
    };
    let hex = td("cellgrid/dgHexahedra.dg");
    let t = run("cell_grid_transform", Some(&hex), &[("translate", json!([0.5, -1, 2])), ("rotate", json!([30, 40, 50])), ("scale", json!([2, 3, 0.5]))]);
    check("transform", &t);
    let src = run("cell_grid", None, &[("cell_type", json!("Hexahedron"))]);
    let w = run("cell_grid_warp", Some(&src), &[("attribute", json!("hgrad")), ("scale", json!(0.5))]);
    check("warp", &w);
    let c = run("convert_to_cell_grid", Some(&td("cellgrid/convert_input.vtu")), &[]);
    check("convert", &c);
    // The writer: each grid written and read back gives the same cells and values.
    let dir = std::env::temp_dir().join(format!("aurafx_dg_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, g) in [("t", &t), ("w", &w), ("c", &c), ("mixed", &td("cellgrid/dgMixed.dg"))] {
        let p = dir.join(format!("{name}.dg"));
        aurafx_io::writers::write_file(g, &p).unwrap();
        let back = match aurafx_io::read_file(&p).unwrap() {
            aurafx_io::Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        };
        let (a, b) = (run("cell_grid_to_unstructured_grid", Some(g), &[]), run("cell_grid_to_unstructured_grid", Some(&back), &[]));
        let (DataSet::UnstructuredGrid(a), DataSet::UnstructuredGrid(b)) = (&a, &b) else { panic!() };
        assert_eq!(a.points, b.points, "{name} points");
        assert_eq!(a.cells, b.cells, "{name} cells");
        for x in a.point_data.iter() {
            assert_eq!(x.data.to_f64_vec(), b.point_data.get(&x.name).unwrap().data.to_f64_vec(), "{name} {}", x.name);
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cell_grid_probe_matches_paraview() {
    use serde_json::{json, Value};
    // ParaView 6.1.1's vtkCellGridPointProbe (scripts/cellgrid_probe_reference.py) on VTK's DG
    // test files: a lattice over each grid's bounds plus the cell centers. ParaView finds the
    // same cells; its Newton step fails on 2-D cells (a singular Jacobian: NaN parameters
    // unless the point is the parametric origin) and reads uninitialized memory for edges,
    // so there auraFX's coordinates are checked against the points instead.
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cellgrid/probe_reference.json")).unwrap()).unwrap();
    let nums = |v: &Value| -> Vec<f64> { v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect() };
    let mut checked = 0;
    for (file, r) in reference.as_object().unwrap() {
        let grid = td(&format!("cellgrid/{file}.dg"));
        let pts: Vec<f64> = r["points"].as_array().unwrap().iter().flat_map(|p| nums(p)).collect();
        let probe_pts = DataSet::PolyData(PolyData::from_points(Points::new_f64(pts)));
        let DataSet::CellGrid(g) = &grid else { panic!("{file}: not a cell grid") };
        let flat = g.cell_types.values().all(|t| t.shape.dimension() < 3);
        for (att, want) in r["probes"].as_object().unwrap() {
            // The cell grid first or second: the same result.
            let out = multi("cell_grid_probe", &[&probe_pts, &grid], &[("attribute", json!(att))]).unwrap();
            let pd = out.point_data().unwrap();
            let op = out.points().unwrap();
            let cell = pd.get("ContainingCellID").unwrap();
            let par = pd.get("ParametricCoordinates").unwrap();
            let val = (!att.is_empty()).then(|| pd.get(att).unwrap());
            let n = op.len();
            if file == "dgEdges" {
                // ParaView's classification of edges is not repeatable; check ours alone.
                for i in 0..n {
                    if let Some(v) = val.filter(|_| att == "shape" && par.get(i, 0).is_finite()) {
                        (0..3).for_each(|k| assert!((v.get(i, k) - op.get(i)[k]).abs() < 1e-5, "{file}: edge value {i}"));
                    }
                }
                continue;
            }
            let wp = want["points"].as_array().unwrap();
            assert_eq!(n, wp.len(), "{file} {att:?}: probe hits");
            let mut used = vec![false; n];
            for (wi, w) in wp.iter().enumerate() {
                let w = nums(w);
                let wc = want["cell"][wi].as_u64().unwrap();
                let wpar = nums(&want["params"][wi]);
                let same = |i: usize| !used[i] && cell.get(i, 0) as u64 == wc && (0..3).all(|k| (op.get(i)[k] - w[k]).abs() <= 1e-6 * (1.0 + w[k].abs()));
                let close = |i: usize| (0..3).all(|k| (par.get(i, k) - wpar[k]).abs() < 1e-6);
                let i = (0..n).find(|&i| same(i) && (wpar[0].is_nan() || close(i))).unwrap_or_else(|| panic!("{file} {att:?}: no hit at {w:?} in cell {wc}"));
                used[i] = true;
                if wpar[0].is_finite() {
                    if let Some(v) = val {
                        for (c, x) in nums(&want["values"][wi]).iter().enumerate() {
                            assert!((v.get(i, c) - x).abs() <= 1e-6 * (1.0 + x.abs()), "{file} {att}[{c}] at {w:?}: {} vs ParaView {x}", v.get(i, c));
                        }
                    }
                } else if par.get(i, 0).is_finite() {
                    // Only where ParaView's Newton step cannot work (2-D cells).
                    assert!(flat, "{file} {att:?}: parameters at {w:?} where ParaView has none");
                    if att == "shape" {
                        (0..3).for_each(|k| assert!((val.unwrap().get(i, k) - w[k]).abs() < 1e-5, "{file}: shape at {w:?}"));
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 1000, "{checked}");
    // ParaView crashes probing the higher-order files (heap corruption); there the cells'
    // parametric centers must be found in their cells with the shape giving back the point.
    for file in ["dgQuadraticQuadrilaterals", "dgPyramid19"] {
        let grid = td(&format!("cellgrid/{file}.dg"));
        let centers = run("cell_grid_centers", Some(&grid), &[]);
        let out = multi("cell_grid_probe", &[&grid, &centers], &[("attribute", json!("shape"))]).unwrap();
        let pd = out.point_data().unwrap();
        let (op, cell, shape) = (out.points().unwrap(), pd.get("ContainingCellID").unwrap(), pd.get("shape").unwrap());
        let cp = centers.points().unwrap();
        for c in 0..cp.len() {
            let i = (0..op.len()).find(|&i| cell.get(i, 0) as usize == c && (0..3).all(|k| (op.get(i)[k] - cp.get(c)[k]).abs() < 1e-5)).unwrap_or_else(|| panic!("{file}: center {c} not found"));
            (0..3).for_each(|k| assert!((shape.get(i, k) - cp.get(c)[k]).abs() < 1e-5, "{file}: center {c}"));
        }
    }
}

/// Cell Grid Compute Sides against ParaView 6.1.1 (testdata/cellgrid/sides_reference.json from
/// scripts/cellgrid_sides_reference.py): on the ten DG files, for eight settings and the default
/// run twice, the cell-type entries (hidden cells, side lists, side types, offsets), the (cell,
/// side) pairs of every side list (ParaView lists them in hash-table order; compared as sets)
/// and the cells of To Unstructured Grid (type and corner points; ParaView also lists the other
/// nodes of quadratic cells). The continuous fields drawn
/// on the sides equal the input's at the same points (ParaView's are taken from the wrong
/// cells), and the result survives the .dg writer.
#[test]
fn cell_grid_compute_sides_matches_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cellgrid/sides_reference.json")).unwrap()).unwrap();
    let include = |f: u64| match f {
        37 => "Next lowest dimension",
        32 => "Surfaces of inputs",
        20 => "Edges of inputs",
        11 => "Vertices of inputs",
        63 => "All sides",
        _ => panic!("{f}"),
    };
    let strategy = |s: u64| ["Odd count", "Every side", "Boundary"][s as usize];
    let sides = |g: &DataSet, case: &str| {
        let c = reference["cases"][case].as_array().unwrap();
        run(
            "cell_grid_compute_sides",
            Some(g),
            &[
                ("include", json!(include(c[0].as_u64().unwrap()))),
                ("omit_renderable", json!(c[1].as_u64().unwrap() == 1)),
                ("preserve_renderable", json!(c[2].as_u64().unwrap() == 1)),
                ("strategy", json!(strategy(c[3].as_u64().unwrap()))),
            ],
        )
    };
    let mut checked = 0;
    for (file, cases) in reference["files"].as_object().unwrap() {
        let input = td(&format!("cellgrid/{file}.dg"));
        for (case, want) in cases.as_object().unwrap() {
            let out = if case == "twice" { sides(&sides(&input, "default"), "edges_keep") } else { sides(&input, case) };
            let DataSet::CellGrid(g) = &out else { panic!() };
            let key = format!("{file} {case}");
            // The cell-type entries.
            let types = want["cell-types"].as_array().unwrap();
            assert_eq!(types.len(), g.cell_types.len(), "{key} cell types");
            for ct in types {
                let t = &g.cell_types[ct["type"].as_str().unwrap()];
                let spec = &ct["cell-spec"];
                assert_eq!(t.blanked, spec["blanked"].as_bool().unwrap_or(false), "{key} blanked");
                assert_eq!(t.offset, spec["offset"].as_i64().unwrap_or(0), "{key} offset");
                let specs = ct["side-specs"].as_array().cloned().unwrap_or_default();
                assert_eq!(specs.len(), t.sides.len(), "{key} side lists");
                for (s, w) in t.sides.iter().zip(&specs) {
                    assert_eq!(json!([s.connectivity.0, s.connectivity.1]), w["connectivity"], "{key}");
                    assert_eq!(s.shape.name(), w["shape"].as_str().unwrap(), "{key}");
                    assert_eq!(s.side_type, w["side-type"].as_i64().unwrap_or(-1), "{key} side type");
                    assert_eq!(s.offset, w["offset"].as_i64().unwrap_or(0), "{key} {} offset", s.connectivity.0);
                    assert_eq!(s.blanked, w["blanked"].as_bool().unwrap_or(false), "{key} {} blanked", s.connectivity.0);
                    assert_eq!(s.selection_type, w["selection-type"].as_i64().unwrap_or(-1), "{key}");
                }
            }
            // The (cell, side) pairs.
            for (group, pairs) in want["lists"].as_object().unwrap() {
                let a = g.groups[group].iter().find(|a| a.name == "conn").unwrap();
                let mut ours: Vec<(i64, i64)> = (0..a.len()).map(|i| (a.get(i, 0) as i64, a.get(i, 1) as i64)).collect();
                let mut theirs: Vec<(i64, i64)> = pairs.as_array().unwrap().iter().map(|p| (p[0].as_i64().unwrap(), p[1].as_i64().unwrap())).collect();
                ours.sort();
                theirs.sort();
                assert_eq!(ours, theirs, "{key} {group}");
                checked += ours.len();
            }
            // To Unstructured Grid: each cell's type and corners (in any order of cells).
            if let Some(cells) = want["cells"].as_array() {
                let ug = g.to_unstructured();
                assert_eq!(ug.cells.len(), cells.len(), "{key} cells");
                let pts = &ug.points;
                let mut ours: Vec<(u8, Vec<[f64; 3]>)> = (0..ug.cells.len()).map(|i| (ug.cells.cell_type(i) as u8, ug.cells.cell(i).iter().map(|&p| pts.get(p as usize)).collect())).collect();
                for c in cells {
                    let ty = c[0].as_u64().unwrap() as u8;
                    let corners: Vec<[f64; 3]> = c[1].as_array().unwrap().iter().map(|p| std::array::from_fn(|k| p[k].as_f64().unwrap())).collect();
                    // ParaView lists quadratic cells' other nodes after the corners.
                    let same = |o: &(u8, Vec<[f64; 3]>)| o.0 == ty && o.1.len() <= corners.len() && o.1.iter().zip(&corners).all(|(a, b)| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9));
                    let i = ours.iter().position(same).unwrap_or_else(|| panic!("{key}: no cell {ty} {corners:?}"));
                    ours.swap_remove(i);
                }
            }
        }
        // The continuous fields on the default sides equal the input's at the same points.
        let DataSet::CellGrid(gi) = &input else { panic!() };
        let DataSet::CellGrid(go) = &sides(&input, "default") else { panic!() };
        let (a, b) = (gi.to_unstructured(), go.to_unstructured());
        for att in gi.attributes.iter().filter(|a| !a.shape && a.info.values().all(|i| i.sharing.is_some() && i.function_space == "HGRAD")) {
            let (va, vb) = (a.point_data.get(&att.name).unwrap(), b.point_data.get(&att.name).unwrap());
            for j in 0..b.points.len() {
                let x = b.points.get(j);
                let i = (0..a.points.len()).find(|&i| a.points.get(i) == x).unwrap();
                for c in 0..va.ncomp {
                    assert!((va.get(i, c) - vb.get(j, c)).abs() < 1e-12, "{file} {} at {x:?}", att.name);
                }
            }
        }
        // The writer keeps the side lists.
        let p = std::env::temp_dir().join(format!("aurafx_sides_{}_{file}.dg", std::process::id()));
        aurafx_io::writers::write_file(&DataSet::CellGrid(go.clone()), &p).unwrap();
        let back = match aurafx_io::read_file(&p).unwrap() {
            aurafx_io::Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        };
        std::fs::remove_file(&p).ok();
        let DataSet::CellGrid(back) = back else { panic!() };
        assert_eq!(back.cell_types, go.cell_types, "{file} written");
        assert_eq!(back.to_unstructured().cells, go.to_unstructured().cells, "{file} written cells");
    }
    assert!(checked > 1000, "{checked} sides checked");
}

/// Hyper Tree Grid Geometry with material interfaces against ParaView 6.1.1
/// (testdata/htg/interface_*.htg and interface_reference.json from
/// scripts/htg_interface_reference.py): grids in 1-D, 2-D (masked, branch factor 3) and 3-D
/// (masked) whose cells crossing a spherical shell carry one- or two-plane interfaces; with
/// Fill Material on and off, every output cell in order: its type, the cell it comes from and
/// its points (the same point numbering, so points are shared exactly as in VTK).
#[test]
fn hyper_tree_grid_interfaces_match_paraview() {
    use serde_json::{json, Value};
    let reference: Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/htg/interface_reference.json")).unwrap()).unwrap();
    let mut checked = 0;
    for (key, want) in reference.as_object().unwrap() {
        let (name, fill) = key.split_once('#').unwrap();
        let grid = td(&format!("htg/interface_{name}.htg"));
        let DataSet::HyperTreeGrid(h) = &grid else { panic!("{name}") };
        assert!(h.interface.is_some(), "{name}: interface arrays read");
        let out = run("hyper_tree_grid_geometry", Some(&grid), &[("fill_material", json!(fill == "1"))]);
        let DataSet::PolyData(pd) = &out else { panic!() };
        let cells = if pd.lines.is_empty() { &pd.polys } else { &pd.lines };
        let wpts = want["points"].as_array().unwrap();
        let wcells = want["cells"].as_array().unwrap();
        assert_eq!(pd.points.len(), wpts.len(), "{key} points");
        assert_eq!(cells.len(), wcells.len(), "{key} cells");
        let gid = pd.cell_data.get("gid").unwrap();
        for (i, w) in wcells.iter().enumerate() {
            let ids: Vec<u32> = w[0].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect();
            assert_eq!(cells.cell(i), &ids[..], "{key} cell {i} points");
            assert_eq!(gid.get(i, 0) as i64, w[1].as_i64().unwrap(), "{key} cell {i} source");
            assert_eq!(cells.cell_type(i) as u64, w[2].as_u64().unwrap(), "{key} cell {i} type");
            checked += 1;
        }
        for (j, w) in wpts.iter().enumerate() {
            let p = pd.points.get(j);
            for k in 0..3 {
                let x = w[k].as_f64().unwrap();
                // ParaView's output points are single precision.
                assert!((p[k] - x).abs() <= 1e-6 * (1.0 + x.abs()), "{key} point {j}: {p:?} vs {w}");
            }
        }
        // In 3-D Fill Material changes nothing.
        if h.dimension() == 3 {
            let other = run("hyper_tree_grid_geometry", Some(&grid), &[("fill_material", json!(false))]);
            assert_eq!(other, out, "{key}");
        }
        // Saving keeps the interface.
        let path = std::env::temp_dir().join(format!("aurafx_iface_{}_{name}.htg", std::process::id()));
        aurafx_io::writers::write_file(&grid, &path).unwrap();
        let back = match aurafx_io::read_file(&path).unwrap() {
            aurafx_io::Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        };
        std::fs::remove_file(&path).ok();
        assert_eq!(run("hyper_tree_grid_geometry", Some(&back), &[("fill_material", json!(fill == "1"))]), out, "{key} written");
    }
    assert!(checked > 3000, "{checked}");
}

/// Orientation colors, conversions and frame rotations on the EBSD test maps
/// (testdata/ebsd from scripts/gen_ebsd.py).
#[test]
fn grains_orientation_filters() {
    use aurafx_crystal::{self as cx, Laue};
    let exp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ebsd/ebsd_expected.json")).unwrap()).unwrap();
    // Nickel: one color per grain (each grain turns by at most 2 degrees across the map).
    let ni = td("ebsd/ni_map.ang");
    let c = run("orientation_colors", Some(&ni), &[]);
    let rgb = c.cell_data().unwrap().get("IPFColor").unwrap();
    assert_eq!((rgb.ncomp, rgb.scalar_type()), (3, ScalarType::U8));
    let grain: Vec<usize> = exp["ni_map.ang"]["grain"].as_array().unwrap().iter().map(|g| g.as_u64().unwrap() as usize).collect();
    let eul = ni.cell_data().unwrap().get("EulerAngles").unwrap();
    let mut first: HashMap<usize, usize> = HashMap::new();
    for (k, g) in grain.iter().enumerate() {
        let f = *first.entry(*g).or_insert(k);
        let q = |i: usize| cx::euler_to_quat([eul.get(i, 0), eul.get(i, 1), eul.get(i, 2)]);
        assert!(cx::disorientation_angle(q(f), q(k), Laue::CubicHigh).to_degrees() < 2.01);
        let want = cx::ipf_color(q(k), Laue::CubicHigh, [0.0, 0.0, 1.0]);
        assert_eq!([rgb.get(k, 0) as u8, rgb.get(k, 1) as u8, rgb.get(k, 2) as u8], want);
    }
    // Two phases: points not indexed are black, the titanium grains use the hexagonal key.
    let two = td("ebsd/two_phase.ang");
    let c = run("orientation_colors", Some(&two), &[("direction", serde_json::json!([1, 0, 0]))]);
    let rgb = c.cell_data().unwrap().get("IPFColor").unwrap();
    let ph = two.cell_data().unwrap().get("Phases").unwrap();
    let eul = two.cell_data().unwrap().get("EulerAngles").unwrap();
    let (mut black, mut hex) = (0, 0);
    for k in 0..ph.len() {
        let got = [rgb.get(k, 0) as u8, rgb.get(k, 1) as u8, rgb.get(k, 2) as u8];
        match ph.get(k, 0) as i32 {
            0 => {
                assert_eq!(got, [0, 0, 0]);
                black += 1;
            }
            p => {
                let l = if p == 2 { Laue::HexagonalHigh } else { Laue::CubicHigh };
                hex += (p == 2) as usize;
                assert_eq!(got, cx::ipf_color(cx::euler_to_quat([eul.get(k, 0), eul.get(k, 1), eul.get(k, 2)]), l, [1.0, 0.0, 0.0]));
            }
        }
    }
    assert!(black > 20 && hex > 100, "{black} {hex}");
    // Euler -> quaternions (fundamental zone) -> Euler describes the same orientations.
    let q = run("convert_orientations", Some(&ni), &[("output_type", "Quaternions".into()), ("reduce", true.into())]);
    let back = run("convert_orientations", Some(&q), &[("orientations", "Quats".into()), ("input_type", "Quaternions".into()), ("output_type", "Euler angles".into()), ("name", "E2".into())]);
    let (e1, e2) = (back.cell_data().unwrap().get("EulerAngles").unwrap(), back.cell_data().unwrap().get("E2").unwrap());
    let qa = back.cell_data().unwrap().get("Quats").unwrap();
    for k in (0..e1.len()).step_by(37) {
        let a = cx::euler_to_quat([e1.get(k, 0), e1.get(k, 1), e1.get(k, 2)]);
        let b = cx::euler_to_quat([e2.get(k, 0), e2.get(k, 1), e2.get(k, 2)]);
        assert!(cx::disorientation_angle(a, b, Laue::CubicHigh) < 1e-5);
        // Reduced: no larger than the largest cubic fundamental-zone rotation (62.8 degrees).
        assert!(2.0 * qa.get(k, 3).acos().to_degrees() <= 62.81);
    }
    // Rotating the sample frame by 90 degrees about z and back restores the orientations; a
    // 90-degree turn of the crystal frame about a cube axis changes nothing for a cubic crystal.
    let r = run("rotate_orientations", Some(&ni), &[("angle", 90.0.into())]);
    let r2 = run("rotate_orientations", Some(&r), &[("angle", (-90.0).into())]);
    let rc = run("rotate_orientations", Some(&ni), &[("frame", "Crystal".into()), ("axis", serde_json::json!([1, 0, 0]))]);
    let (a0, a1, a2, a3) = (eul_of(&ni), eul_of(&r), eul_of(&r2), eul_of(&rc));
    for k in (0..a0.len()).step_by(53) {
        assert!(cx::disorientation_angle(a0[k], a2[k], Laue::Triclinic) < 1e-5);
        assert!((cx::disorientation_angle(a0[k], a1[k], Laue::Triclinic).to_degrees() - 90.0).abs() < 1e-3);
        assert!(cx::disorientation_angle(a0[k], a3[k], Laue::CubicHigh) < 1e-5);
        // The sample direction x of the rotated frame is the old y: the old y color is the new x color.
        let old_y = cx::ipf_color(a0[k], Laue::CubicHigh, [0.0, 1.0, 0.0]);
        assert_eq!(cx::ipf_color(a1[k], Laue::CubicHigh, [1.0, 0.0, 0.0]), old_y);
    }
    // The color key: red, green and blue corners.
    let key = run("ipf_color_key", None, &[("resolution", 10.into())]);
    let kc = key.point_data().unwrap().get("IPFColor").unwrap();
    let px = |i: usize| [kc.get(i, 0) as u8, kc.get(i, 1) as u8, kc.get(i, 2) as u8];
    assert_eq!(px(0), [255, 0, 0]);
    assert_eq!(px(11 * 10), [0, 255, 0]);
    assert_eq!(px(11 * 11 - 1), [0, 0, 255]);
}

fn eul_of(d: &DataSet) -> Vec<aurafx_crystal::Quat> {
    let e = d.cell_data().unwrap().get("EulerAngles").unwrap();
    (0..e.len()).map(|k| aurafx_crystal::euler_to_quat([e.get(k, 0), e.get(k, 1), e.get(k, 2)])).collect()
}

/// Orientation Colors against the IPF colors DREAM.3D computed for a window of a real EBSD
/// volume (testdata/dream3d/am_bench_subset.dream3d: AM Bench 2022 IN718, cubic).
#[test]
fn ipf_colors_match_dream3d() {
    let d = td("dream3d/am_bench_subset.dream3d");
    for (dir, name) in [([0, 0, 1], "IPFColorZ"), ([1, 0, 0], "IPFColorX")] {
        let c = run("orientation_colors", Some(&d), &[("direction", serde_json::json!(dir))]);
        let (ours, theirs) = (c.cell_data().unwrap().get("IPFColor").unwrap(), d.cell_data().unwrap().get(name).unwrap());
        let mut worst = 0;
        for k in 0..ours.len() {
            for ch in 0..3 {
                worst = worst.max((ours.get(k, ch) as i32 - theirs.get(k, ch) as i32).abs());
            }
        }
        assert!(worst <= 1, "{name}: largest difference {worst}");
    }
}

/// Align Sections recovers the shifts of the TSL stack (testdata/ebsd/ni_stack.h5ebsd: slices
/// cut from one map at known windows); Crop and Change Resolution on a real `.dream3d` window.
#[test]
fn grains_volume_filters() {
    let st = td("ebsd/ni_stack.h5ebsd");
    // Slice z starts at window column c0, row r0 of the map: moving it by (c0 - c0', r0 - r0')
    // against the slice below lines it up.
    let want = [[0, 0], [2, -1], [5, 0], [4, 2]];
    for method in ["Misorientation", "Given shifts"] {
        let a = run("align_sections", Some(&st), &[("method", method.into()), ("shifts", serde_json::json!([2, -1, 3, 1, -1, 2]))]);
        let s = a.field_data().get("SectionShifts").unwrap();
        let got: Vec<[i64; 2]> = (0..4).map(|k| [s.get(k, 0) as i64, s.get(k, 1) as i64]).collect();
        assert_eq!(got, want.to_vec(), "{method}");
        // Every covered cell of an aligned slice now holds the orientation of slice 0 there.
        let e = a.cell_data().unwrap().get("EulerAngles").unwrap();
        for z in 1..4 {
            for j in 5..45 {
                for i in 6..64 {
                    let (c, c0) = (z * 3500 + j * 70 + i, j * 70 + i);
                    assert!((e.get(c, 0) - e.get(c0, 0)).abs() < 1e-6 || i < 6, "slice {z} cell ({i}, {j})");
                }
            }
        }
    }
    // Crop to a box in sample coordinates: features renumbered, feature rows kept.
    let d = td("dream3d/am_bench_subset.dream3d");
    let DataSet::ImageData(img) = &d else { panic!() };
    let (o, sp) = (img.origin, img.spacing);
    let lo = [o[0] + 10.0 * sp[0], o[1] + 5.0 * sp[1], o[2] - 1.0];
    let hi = [o[0] + 29.9 * sp[0], o[1] + 24.9 * sp[1], o[2] + 2.9 * sp[2]];
    let c = run("crop_volume", Some(&d), &[("min", serde_json::json!(lo)), ("max", serde_json::json!(hi))]);
    let DataSet::ImageData(ci) = &c else { panic!() };
    assert_eq!(ci.extent, [10, 30, 5, 25, 0, 3]);
    let (fid0, fid) = (img.cell_data.get("FeatureIds").unwrap(), ci.cell_data.get("FeatureIds").unwrap());
    let nf = (0..fid.len()).map(|k| fid.get(k, 0) as usize).max().unwrap();
    assert_eq!(ci.field_data.get("AvgQuats").unwrap().len(), nf + 1);
    let mut seen = vec![false; nf + 1];
    for k in 0..fid.len() {
        seen[fid.get(k, 0) as usize] = true;
    }
    assert!(seen[1..].iter().all(|s| *s), "ids are compact");
    // The feature table row follows its feature: the average orientation of a cell's feature
    // is the same before and after.
    let (q0, q1) = (img.field_data.get("AvgQuats").unwrap(), ci.field_data.get("AvgQuats").unwrap());
    for (j, i, z) in [(5usize, 10usize, 0usize), (12, 20, 1), (24, 29, 2)] {
        let k0 = (z * 40 + j) * 48 + i;
        let k1 = (z * 20 + (j - 5)) * 20 + (i - 10);
        let (a, b) = (fid0.get(k0, 0) as usize, fid.get(k1, 0) as usize);
        for comp in 0..4 {
            assert_eq!(q0.get(a, comp), q1.get(b, comp));
        }
    }
    // Change Resolution: cells twice as large in x and y, values from the old cell at each centre.
    let r = run("change_resolution", Some(&d), &[("spacing", serde_json::json!([2.0 * sp[0], 2.0 * sp[1], sp[2]]))]);
    let DataSet::ImageData(ri) = &r else { panic!() };
    assert_eq!(ri.extent, [0, 24, 0, 20, 0, 6]);
    assert!((0..3).all(|k| (ri.origin[k] - o[k]).abs() < 1e-9));
    let (e0, e1) = (img.cell_data.get("EulerAngles").unwrap(), ri.cell_data.get("EulerAngles").unwrap());
    for (i, j, z) in [(0usize, 0usize, 0usize), (7, 3, 2), (23, 19, 5)] {
        let k1 = (z * 20 + j) * 24 + i;
        let k0 = (z * 40 + 2 * j + 1) * 48 + 2 * i + 1;
        assert_eq!(e1.get(k1, 1), e0.get(k0, 1));
    }
    // A flat 2-D map stays flat.
    let ni = td("ebsd/ni_map.ang");
    let c = run("crop_volume", Some(&ni), &[("min", serde_json::json!([5.0, 5.0, -1.0])), ("max", serde_json::json!([20.0, 10.0, 1.0]))]);
    let DataSet::ImageData(ci) = &c else { panic!() };
    assert_eq!((ci.extent, c.n_cells()), ([10, 41, 10, 21, 0, 0], 31 * 11));
    let r = run("change_resolution", Some(&ni), &[("spacing", serde_json::json!([1.0, 1.0, 1.0]))]);
    let DataSet::ImageData(ri) = &r else { panic!() };
    assert_eq!((ri.extent, r.n_cells()), ([0, 40, 0, 30, 0, 0], 1200));
}

/// The Geometry source: surfaces with the closed-form volume and area, named faces, an STL
/// round trip and the Signed Distance Field of the written file.
#[test]
fn geometry_source_and_distance_fields() {
    use std::f64::consts::PI;
    let cmds = "brick 2; cylinder radius 0.5 height 4; subtract body 2 from body 1; sphere radius 0.6; move body 3 x 3";
    let g = run("geometry", None, &[("commands", cmds.into()), ("resolution", 96.into())]);
    let fd = g.field_data();
    let (v, a) = (fd.get("Volume").unwrap(), fd.get("SurfaceArea").unwrap());
    let rel = |got: f64, want: f64| ((got - want) / want).abs();
    assert!(rel(v.get(0, 0), 8.0 - PI * 0.5) < 0.005, "brick V {}", v.get(0, 0));
    assert!(rel(a.get(0, 0), 24.0 - PI * 0.5 + 2.0 * PI) < 0.01, "brick A {}", a.get(0, 0));
    assert!(rel(v.get(1, 0), 4.0 / 3.0 * PI * 0.216) < 0.005, "sphere V {}", v.get(1, 0));
    assert!(rel(a.get(1, 0), 4.0 * PI * 0.36) < 0.005, "sphere A {}", a.get(1, 0));
    // Faces: the top of the brick, the hole and the sphere all appear, with their names.
    let names = match &fd.get("FaceNames").unwrap().data {
        ArrayData::String(s) => s.clone(),
        _ => panic!(),
    };
    let fid = g.cell_data().unwrap().get("FaceId").unwrap();
    let seen: std::collections::BTreeSet<String> = (0..fid.len()).map(|k| names[fid.get(k, 0) as usize].clone()).collect();
    for n in ["brick 1 z+", "brick 1 x-", "cylinder 2 side", "sphere 3 surface"] {
        assert!(seen.contains(n), "{n} missing from {seen:?}");
    }
    // The cylinder's caps lie outside the brick: never drawn.
    assert!(!seen.contains("cylinder 2 top"));
    // STL round trip: write the sphere, read it back as a body, same volume.
    let dir = std::env::temp_dir().join(format!("aurafx_geom_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ball = run("geometry", None, &[("commands", "sphere radius 1".into()), ("resolution", 64.into())]);
    let stl = dir.join("ball.stl");
    aurafx_io::writers::write_file(&ball, &stl).unwrap();
    let back = run("geometry", None, &[("commands", format!("import '{}'", stl.display()).into()), ("resolution", 64.into())]);
    let (v0, v1) = (ball.field_data().get("Volume").unwrap().get(0, 0), back.field_data().get("Volume").unwrap().get(0, 0));
    assert!(rel(v1, v0) < 0.005, "{v0} {v1}");
    assert!(rel(v0, 4.0 / 3.0 * PI) < 0.005);
    // Signed Distance Field of the read surface: -1 at the centre, 0.5 half a unit outside.
    let s = td_path(&stl);
    let sdf = run("signed_distance", Some(&s), &[("resolution", 20.into()), ("margin", 0.25.into())]);
    let DataSet::ImageData(img) = &sdf else { panic!() };
    let d = img.point_data.get("Distance").unwrap();
    let e = img.extent;
    let (nx, ny) = ((e[1] + 1) as usize, (e[3] + 1) as usize);
    let at = |p: [f64; 3]| {
        let ijk: [usize; 3] = std::array::from_fn(|k| ((p[k] - img.origin[k]) / img.spacing[k]).round() as usize);
        let q: [f64; 3] = std::array::from_fn(|k| img.origin[k] + ijk[k] as f64 * img.spacing[k]);
        (d.get((ijk[2] * ny + ijk[1]) * nx + ijk[0], 0), (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() - 1.0)
    };
    for p in [[0.0, 0.0, 0.0], [1.4, 0.0, 0.0], [0.0, -0.5, 0.3], [0.9, 0.9, 0.9]] {
        let (got, want) = at(p);
        assert!((got - want).abs() < 0.01, "{p:?}: {got} vs {want}");
    }
    // The distance-field output of the source.
    let df = run("geometry", None, &[("commands", "sphere radius 1".into()), ("output", "Distance field".into()), ("resolution", 16.into())]);
    assert!(df.point_data().unwrap().get("Distance").is_some());
    std::fs::remove_dir_all(&dir).ok();
}

fn td_path(p: &std::path::Path) -> DataSet {
    match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    }
}

/// Box Mesh, Inclusion Mesh, Elevate and Mesh Quality's hexahedron metrics.
#[test]
fn structured_hex_meshes() {
    let b = run("box_mesh", None, &[("size", serde_json::json!([2, 1, 1])), ("cells", serde_json::json!([4, 2, 2])), ("grading", serde_json::json!([3, 1, 1]))]);
    assert_eq!((b.n_points(), b.n_cells()), (5 * 3 * 3, 16));
    assert_eq!(b.field_data().get("SideSet x-").unwrap().len(), 4);
    let q = run("mesh_quality", Some(&b), &[]);
    let sj = q.cell_data().unwrap().get("ScaledJacobian").unwrap();
    assert!((0..sj.len()).all(|i| (sj.get(i, 0) - 1.0).abs() < 1e-12));
    let e = run("elevate", Some(&b), &[("element", "HEX20".into())]);
    assert_eq!(e.n_points(), 45 + (4 * 3 * 3 + 5 * 2 * 3 + 5 * 3 * 2));
    // Spheres in a cube: the elements fill it, the scaled Jacobian stays at 1/sqrt(3) or
    // above, interface nodes lie on the spheres.
    let m = run("inclusion_mesh", None, &[("count", 6.into()), ("radius", 0.08.into()), ("cells", 20.into()), ("seed", 3.into())]);
    let placed = m.field_data().get("InclusionsPlaced").unwrap().get(0, 0);
    assert!(placed >= 4.0, "{placed}");
    let v = run("cell_size", Some(&m), &[]);
    let vol = v.cell_data().unwrap().get("Volume").unwrap();
    let total: f64 = (0..vol.len()).map(|i| vol.get(i, 0)).sum();
    assert!((total - 1.0).abs() < 1e-9, "{total}");
    let q = run("mesh_quality", Some(&m), &[]);
    let sj = q.cell_data().unwrap().get("ScaledJacobian").unwrap();
    let min = (0..sj.len()).map(|i| sj.get(i, 0)).fold(f64::INFINITY, f64::min);
    assert!(min > 0.577, "{min}");
    let t = run("mesh_quality", Some(&m), &[("output", "Summary table".into())]);
    let DataSet::Table(t) = t else { panic!() };
    let names = match &t.columns.get("Metric").unwrap().data {
        ArrayData::String(s) => s.clone(),
        _ => panic!(),
    };
    let row = names.iter().position(|n| n == "ScaledJacobian").unwrap();
    assert!((t.columns.get("Min").unwrap().get(row, 0) - min).abs() < 1e-12);
    assert_eq!(t.columns.get("CellsOutside").unwrap().get(row, 0), 0.0);
    // HEX27 with the curved interface: same elements, more nodes.
    let m27 = run("inclusion_mesh", None, &[("count", 6.into()), ("radius", 0.08.into()), ("cells", 20.into()), ("seed", 3.into()), ("element", "HEX27".into())]);
    assert_eq!(m27.n_cells(), m.n_cells());
    assert!(m27.n_points() > 6 * m.n_points());
    // Extract Set: the interface faces as a surface.
    let s = run("extract_set", Some(&m), &[("set", "interface".into())]);
    assert_eq!(s.n_cells(), m.field_data().get("SideSet interface").unwrap().len());
    assert!(s.cell_data().unwrap().get("particle_id").is_some());
    // Plate with a hole (the benchmark's default plate), a dog-bone, a sweep and the
    // Geometry source's hex mesh.
    let pl = run("plate_mesh", None, &[]);
    assert_eq!(pl.n_cells(), 860);
    assert!(pl.field_data().get("SideSet hole").is_some_and(|a| a.len() == 24));
    let db = run("dogbone_mesh", None, &[("stations", serde_json::json!([0, 0.37, 4, 0.34, 8, 0.36]))]);
    assert_eq!(db.field_data().get("ElementBlockNames").unwrap().len(), 3);
    let bottom = run("extract_set", Some(&pl), &[("set", "z-".into())]);
    let thick = run("sweep_quads", Some(&bottom), &[("distance", 4.0.into()), ("layers", 3.into())]);
    assert_eq!(thick.n_cells(), 3 * 860);
    let g = run("geometry", None, &[("commands", "brick x 2 y 1 z 1; cylinder radius 0.3 height 1; move body 2 x 3; mesh body 1 2 size 0.25".into()), ("output", "Hex mesh".into())]);
    assert!(g.field_data().get("SideSet brick 1 x+").is_some() && g.field_data().get("SideSet cylinder 2 side").is_some());
    let dense = run("inclusion_mesh", None, &[("count", 30.into()), ("radius", 0.07.into()), ("cells", 24.into()), ("packing", "Collective".into())]);
    let sparse = run("inclusion_mesh", None, &[("count", 30.into()), ("radius", 0.07.into()), ("cells", 24.into())]);
    assert!(dense.field_data().get("InclusionsPlaced").unwrap().get(0, 0) >= sparse.field_data().get("InclusionsPlaced").unwrap().get(0, 0));
}
