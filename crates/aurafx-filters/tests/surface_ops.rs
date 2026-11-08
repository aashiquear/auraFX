//! Surface operations against closed forms.
use aurafx_data::*;
use aurafx_filters::surface_ops::*;
use std::f64::consts::PI;

fn run(id: &str, input: Option<&DataSet>, kv: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap();
    let mut p = def.default_params();
    for (k, v) in kv {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap()
}

fn area_volume(ds: &DataSet) -> (f64, f64) {
    let DataSet::PolyData(pd) = ds else { panic!("not a surface") };
    let (mut a, mut v) = (0.0, 0.0);
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        for k in 1..c.len() - 1 {
            let (p0, p1, p2) = (pd.points.get(c[0] as usize), pd.points.get(c[k] as usize), pd.points.get(c[k + 1] as usize));
            let (u, w) = ([p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]], [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]]);
            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            a += 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            v += (p0[0] * (p1[1] * p2[2] - p1[2] * p2[1]) - p0[1] * (p1[0] * p2[2] - p1[2] * p2[0]) + p0[2] * (p1[0] * p2[1] - p1[1] * p2[0])) / 6.0;
        }
    }
    (a, v)
}

fn sphere(r: f64, res: i64) -> DataSet {
    run("sphere", None, &[("radius", r.into()), ("theta", res.into()), ("phi", res.into())])
}

#[test]
fn reverse_sense_flips_the_enclosed_volume_and_the_normals() {
    let s = sphere(1.0, 24);
    let n = run("surface_normals", Some(&s), &[]);
    let (_, v0) = area_volume(&s);
    let r = run("reverse_sense", Some(&n), &[]);
    let (_, v1) = area_volume(&r);
    assert!((v0 + v1).abs() < 1e-9 * v0.abs(), "{v0} {v1}");
    let (DataSet::PolyData(a), DataSet::PolyData(b)) = (&n, &r) else { panic!() };
    let name = a.point_data.iter().find(|x| x.ncomp == 3 && x.name.to_lowercase().contains("normal")).map(|x| x.name.clone()).expect("normals");
    let (na, nb) = (a.point_data.get(&name).unwrap(), b.point_data.get(&name).unwrap());
    assert!((na.get(3, 1) + nb.get(3, 1)).abs() < 1e-12);
}

#[test]
fn linear_and_rotational_extrusions_have_the_closed_form_areas() {
    // A 3 mm line swept 2 mm: a 3 x 2 quad.
    let line = run("line", None, &[("point1", serde_json::json!([0.0, 0.0, 0.0])), ("point2", serde_json::json!([3.0, 0.0, 0.0]))]);
    let q = run("linear_extrusion", Some(&line), &[("vector", serde_json::json!([0.0, 2.0, 0.0]))]);
    assert!((area_volume(&q).0 - 6.0).abs() < 1e-12);
    // A unit square prism of height 2: surface 2 + 8.
    let plane = run("plane", None, &[("x_resolution", 1.into()), ("y_resolution", 1.into())]);
    let prism = run("linear_extrusion", Some(&plane), &[("vector", serde_json::json!([0.0, 0.0, 2.0]))]);
    let (a, v) = area_volume(&prism);
    let DataSet::PolyData(pl) = &plane else { panic!() };
    let side = (0..pl.points.len())
        .map(|i| pl.points.get(i))
        .fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
    let (w, h) = (side.1[0] - side.0[0], side.1[1] - side.0[1]);
    assert!((a - (2.0 * w * h + 2.0 * (w + h) * 2.0)).abs() < 1e-9 && (v.abs() - w * h * 2.0).abs() < 1e-9, "{a} {v}");
    // A line from radius 1 at height 0 to radius 0 at height 2, turned a full circle: a cone of area pi r l.
    let l2 = run("line", None, &[("point1", serde_json::json!([1.0, 0.0, 0.0])), ("point2", serde_json::json!([0.0, 0.0, 2.0]))]);
    let cone = run("rotational_extrusion", Some(&l2), &[("resolution", 360.into())]);
    let expect = PI * 1.0 * 5f64.sqrt();
    assert!((area_volume(&cone).0 - expect).abs() / expect < 1e-3, "{} vs {expect}", area_volume(&cone).0);
    // A screw: a quarter turn that also rises.
    let helix = run("rotational_extrusion", Some(&l2), &[("angle", 90.0.into()), ("translation", 1.0.into()), ("resolution", 9.into())]);
    let DataSet::PolyData(h) = &helix else { panic!() };
    assert_eq!(h.polys.len(), 90, "ten segments in nine steps");
}

#[test]
fn a_ribbon_is_as_wide_as_asked() {
    let line = run("line", None, &[("point1", serde_json::json!([0.0, 0.0, 0.0])), ("point2", serde_json::json!([4.0, 0.0, 0.0])), ("resolution", 4.into())]);
    let r = run("ribbon", Some(&line), &[("width", 0.5.into())]);
    assert!((area_volume(&r).0 - 4.0 * 0.5).abs() < 1e-9);
}

#[test]
fn subdivision_keeps_flat_shapes_and_loop_rounds_toward_the_limit() {
    let s = sphere(1.0, 12);
    let (a0, v0) = area_volume(&s);
    let lin = run("subdivide", Some(&s), &[("levels", 2.into())]);
    let (a1, v1) = area_volume(&lin);
    assert!((a1 - a0).abs() < 1e-9 * a0 && (v1 - v0).abs() < 1e-9 * v0.abs(), "linear subdivision does not move the surface");
    let DataSet::PolyData(l) = &lin else { panic!() };
    let DataSet::PolyData(o) = &s else { panic!() };
    let tris = |p: &PolyData| (0..p.polys.len()).map(|i| p.polys.cell(i).len() - 2).sum::<usize>();
    assert_eq!(l.polys.len(), tris(o) * 16);
    // Loop subdivision of an octahedron rounds it toward a smooth blob: the corners pull in and the
    // radial spread (shortest to longest distance from the center) closes up.
    let mut o = PolyData {
        points: Points::new_f64(vec![1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0]),
        ..Default::default()
    };
    for t in [[0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4], [2, 0, 5], [1, 2, 5], [3, 1, 5], [0, 3, 5]] {
        o.polys.push(CellType::Triangle, &t);
    }
    let oct = DataSet::PolyData(o);
    let spread = |d: &DataSet| {
        let DataSet::PolyData(p) = d else { panic!() };
        let r: Vec<f64> = (0..p.points.len()).map(|i| p.points.get(i)).map(|q| (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt()).collect();
        r.iter().copied().fold(f64::INFINITY, f64::min) / r.iter().copied().fold(0.0, f64::max)
    };
    let (_, vo) = area_volume(&oct);
    let lp = run("subdivide", Some(&oct), &[("scheme", "Loop".into()), ("levels", 3.into())]);
    let (_, vl) = area_volume(&lp);
    assert!(vl > 0.0 && vl < vo, "Loop shrinks the corners: {vo} -> {vl}");
    assert!(spread(&lp) > 0.85 && spread(&lp) < 1.0, "the blob is round but not a point cloud: {}", spread(&lp));
}

#[test]
fn curvature_of_a_sphere_is_one_over_r_squared_and_one_over_r() {
    let s = sphere(2.0, 64);
    let g = run("curvature", Some(&s), &[]);
    let m = run("curvature", Some(&s), &[("kind", "Mean".into())]);
    let (DataSet::PolyData(g), DataSet::PolyData(m)) = (&g, &m) else { panic!() };
    let (ga, ma) = (g.point_data.get("Gauss_Curvature").unwrap(), m.point_data.get("Mean_Curvature").unwrap());
    // Away from the poles, where the sphere's triangles are most regular.
    let mut ok = 0;
    for i in 0..g.points.len() {
        let p = g.points.get(i);
        if p[2].abs() < 1.2 {
            assert!((ga.get(i, 0) - 0.25).abs() < 0.25 * 0.04, "K {} at {p:?}", ga.get(i, 0));
            assert!((ma.get(i, 0) - 0.5).abs() < 0.5 * 0.04, "H {} at {p:?}", ma.get(i, 0));
            ok += 1;
        }
    }
    assert!(ok > 500);
}

#[test]
fn delaunay_of_a_grid_is_the_square_and_alpha_cuts_a_hole() {
    let n = 7;
    let mut p = Vec::new();
    for j in 0..n {
        for i in 0..n {
            // A tiny deterministic wobble avoids four co-circular points everywhere.
            p.extend([i as f64 + 0.001 * ((i * 7 + j * 3) % 5) as f64, j as f64 + 0.001 * ((i * 5 + j * 11) % 7) as f64, 0.0]);
        }
    }
    let cloud = DataSet::PolyData(PolyData::from_points(Points::new_f64(p)));
    let t = run("delaunay_2d", Some(&cloud), &[]);
    let (a, _) = area_volume(&t);
    assert!((a - 36.0).abs() < 0.1, "the hull of the grid: {a}");
    let DataSet::PolyData(pd) = &t else { panic!() };
    assert!(
        pd.polys.len() >= 2 * (n - 1) * (n - 1) && pd.polys.len() <= 2 * (n - 1) * (n - 1) + 14,
        "two triangles per cell (plus a few slivers on the wobbly boundary): {}",
        pd.polys.len()
    );
    let cut = run("delaunay_2d", Some(&cloud), &[("alpha", 0.4.into())]);
    let DataSet::PolyData(c) = &cut else { panic!() };
    assert!(c.polys.len() < pd.polys.len(), "alpha drops the long triangles");
    let _ = (reverse_sense, ribbon);
}

#[test]
fn decimating_a_sphere_by_half_keeps_its_volume_and_area() {
    let s = sphere(1.0, 48);
    let tris = |d: &DataSet| {
        let DataSet::PolyData(p) = d else { panic!() };
        (0..p.polys.len()).map(|i| p.polys.cell(i).len() - 2).sum::<usize>()
    };
    let (a0, v0) = area_volume(&s);
    let d = run("decimate", Some(&s), &[("reduction", 0.5.into())]);
    let (a1, v1) = area_volume(&d);
    let ratio = tris(&d) as f64 / tris(&s) as f64;
    assert!((ratio - 0.5).abs() < 0.05, "half the triangles remain: {ratio}");
    assert!((v1 - v0).abs() / v0.abs() < 0.02, "volume {v0} -> {v1}");
    assert!((a1 - a0).abs() / a0 < 0.03, "area {a0} -> {a1}");
    let heavy = run("decimate", Some(&s), &[("reduction", 0.9.into())]);
    assert!(tris(&heavy) < tris(&s) / 5 && area_volume(&heavy).1.abs() > 0.8 * v0.abs());
}
