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
