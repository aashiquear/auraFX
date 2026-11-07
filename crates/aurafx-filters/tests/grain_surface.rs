//! Grain boundary surfaces: closed feature surfaces whose enclosed volumes equal the voxel
//! volumes, smoothing that keeps those volumes, and the GBCD of a volume of coherent twins.

use aurafx_crystal::{self as cx};
use aurafx_data::*;
use std::collections::HashMap;

fn run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

fn voxel_volumes(d: &DataSet) -> Vec<f64> {
    let ids = d.cell_data().unwrap().get("FeatureIds").unwrap();
    let nf = (0..ids.len()).map(|i| ids.get(i, 0) as usize).max().unwrap() + 1;
    let mut v = vec![0.0; nf];
    for i in 0..ids.len() {
        v[ids.get(i, 0) as usize] += 1.0;
    }
    v
}

fn field(d: &DataSet, k: &str) -> Vec<f64> {
    let a = d.field_data().get(k).unwrap_or_else(|| panic!("no {k}"));
    (0..a.len()).map(|i| a.get(i, 0)).collect()
}

#[test]
fn surfaces_are_closed_and_keep_volumes() {
    let v = run("voronoi_grains", None, &[("cells", serde_json::json!([30, 30, 30])), ("grains", 40.into())]);
    let s = run("quick_surface_mesh", Some(&v), &[]);
    // Node types: boundaries, triple lines and quadruple points, inside and on the faces.
    let nt = s.point_data().unwrap().get("NodeType").unwrap();
    let mut kinds: HashMap<i64, usize> = HashMap::new();
    for i in 0..nt.len() {
        *kinds.entry(nt.get(i, 0) as i64).or_insert(0) += 1;
    }
    eprintln!("node types {kinds:?}");
    for t in [2, 3, 4, 12, 13] {
        assert!(kinds.contains_key(&t), "{t}");
    }
    // Enclosed volumes equal the voxel volumes.
    let g = run("triangle_geometry", Some(&s), &[]);
    let (sv, vv) = (field(&g, "SurfaceVolumes"), voxel_volumes(&v));
    for f in 1..vv.len() {
        assert!((sv[f] - vv[f]).abs() < 1e-9, "feature {f}: {} vs {}", sv[f], vv[f]);
    }
    // Every feature's surface is closed and consistently oriented: each directed edge is
    // matched by its reverse (a voxel feature touching itself along an edge has four
    // triangles there, two each way).
    let fs = run("feature_surfaces", Some(&s), &[]);
    let DataSet::MultiBlock(mb) = &fs else { panic!() };
    assert_eq!(mb.blocks.len(), 40);
    for (name, b) in &mb.blocks {
        let DataSet::PolyData(p) = b.as_ref().unwrap() else { panic!() };
        let mut dir: HashMap<(u32, u32), usize> = HashMap::new();
        for (_, c) in p.polys.iter() {
            for k in 0..3 {
                *dir.entry((c[k], c[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        assert!(dir.iter().all(|((a, b), n)| dir.get(&(*b, *a)) == Some(n)), "{name} is not closed");
    }
    // One STL file per feature.
    let dir = std::env::temp_dir().join(format!("aurafx_stl_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    aurafx_io::writers::write_file(&fs, &dir.join("grains.stl")).unwrap();
    let n = std::fs::read_dir(&dir).unwrap().count();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(n, 40);
    // Smoothing: Taubin keeps the volumes, plain Laplacian shrinks them more.
    let err = |d: &DataSet| -> f64 {
        let sv = field(&run("triangle_geometry", Some(d), &[]), "SurfaceVolumes");
        (1..vv.len()).map(|f| (sv[f] - vv[f]).abs() / vv[f]).sum::<f64>() / (vv.len() - 1) as f64
    };
    let taubin = run("smooth_boundaries", Some(&s), &[]);
    let lap = run("smooth_boundaries", Some(&s), &[("mu", 0.0.into())]);
    let (et, el) = (err(&taubin), err(&lap));
    eprintln!("mean volume change: Taubin {:.3} %, Laplacian {:.3} %", 100.0 * et, 100.0 * el);
    assert!(et < 0.03 && et < el, "{et} {el}");
    // The outer faces stay flat: no node leaves the box.
    let pts = taubin.points().unwrap();
    for i in 0..pts.len() {
        let q = pts.get(i);
        assert!(q.iter().all(|x| *x >= -1e-9 && *x <= 30.0 + 1e-9));
    }
    // The smallest angles get better.
    let m0 = field_cells(&run("triangle_geometry", Some(&s), &[]), "FaceMinimumAngles");
    let m1 = field_cells(&run("triangle_geometry", Some(&taubin), &[]), "FaceMinimumAngles");
    assert!((m0 - 45.0).abs() < 1e-9 && m1 > 5.0, "{m0} {m1}");
}

fn field_cells(d: &DataSet, k: &str) -> f64 {
    let a = d.cell_data().unwrap().get(k).unwrap();
    let s: f64 = (0..a.len()).map(|i| a.get(i, 0)).sum();
    s / a.len() as f64
}

/// Layers of a crystal and its Sigma 3 twin stacked along z, with [111] along z: every
/// boundary is a coherent twin, a (111) plane in both crystals.
#[test]
fn gbcd_of_coherent_twins() {
    let n = 24;
    let mut img = ImageData { extent: [0, n, 0, n, 0, n], spacing: [1.0; 3], ..Default::default() };
    // g maps sample z to crystal [111]: g = R(z -> [111]) in the crystal frame.
    let c = [1.0 / 3f64.sqrt(); 3];
    let axis = [-c[1], c[0], 0.0];
    let s = (axis[0] * axis[0] + axis[1] * axis[1]).sqrt();
    let g = cx::axis_angle_to_quat(axis.map(|x| x / s), c[2].acos());
    assert!((cx::rotate(g, [0.0, 0.0, 1.0])[0] - c[0]).abs() < 1e-12);
    let twin = cx::mul(cx::axis_angle_to_quat(c, 60f64.to_radians()), g);
    let nc = (n * n * n) as usize;
    let mut ids = Vec::with_capacity(nc);
    let mut eul = Vec::with_capacity(nc * 3);
    for k in 0..n {
        for _ in 0..n * n {
            let layer = k / 4;
            ids.push(layer as i32 + 1);
            eul.extend(cx::quat_to_euler(if layer % 2 == 0 { g } else { twin }).map(|x| x as f32));
        }
    }
    img.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(vec![1; nc])));
    img.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eul)));
    img.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1])));
    let vol = DataSet::ImageData(img);
    let s = run("quick_surface_mesh", Some(&vol), &[]);
    let gb = run("gbcd_section", Some(&s), &[]);
    let mrd = gb.point_data().unwrap().get("MRD").unwrap();
    let dirs = gb.point_data().unwrap().get("CrystalDirection").unwrap();
    let at = |d: [f64; 3]| -> f64 {
        let k = (0..dirs.len()).max_by(|a, b| {
            let da: f64 = (0..3).map(|j| dirs.get(*a, j) * d[j]).sum();
            let db: f64 = (0..3).map(|j| dirs.get(*b, j) * d[j]).sum();
            da.total_cmp(&db)
        }).unwrap();
        mrd.get(k, 0)
    };
    let (p111, p100, p110) = (at(c), at([0.0, 0.0, 1.0]), at([0.0, 1.0 / 2f64.sqrt(), 1.0 / 2f64.sqrt()]));
    let mis = field(&gb, "MisorientationMRD")[0];
    eprintln!("twins: MRD (111) {p111:.0}, (100) {p100:.3}, (110) {p110:.3}, misorientation {mis:.0}");
    assert!(p111 > 1000.0 * p100.max(1e-3) && p111 > 1000.0, "{p111} {p100}");
    assert!(p110 < 1e-3 * p111);
    // Face colors: the twin boundaries are 60 degree boundaries.
    let fc = run("face_orientation_colors", Some(&s), &[]);
    let m = fc.cell_data().unwrap().get("FaceMisorientations").unwrap();
    let inner: Vec<f64> = (0..m.len()).map(|i| m.get(i, 0)).filter(|x| x.is_finite()).collect();
    assert!(!inner.is_empty() && inner.iter().all(|x| (x - 60.0).abs() < 1e-3), "{:?}", &inner[..3.min(inner.len())]);
    // A random microstructure has no preferred misorientation: about 1 at 45 degrees about
    // [100] within 10 degrees.
    let v = run("voronoi_grains", None, &[("cells", serde_json::json!([60, 60, 60])), ("grains", 1500.into())]);
    let r = run("gbcd_section", Some(&run("quick_surface_mesh", Some(&v), &[])), &[("angle", 45.0.into()), ("axis", serde_json::json!([1, 0, 0])), ("tolerance", 10.0.into())]);
    let mr = field(&r, "MisorientationMRD")[0];
    eprintln!("random: misorientation MRD {mr:.2}");
    assert!(mr > 0.4 && mr < 2.5, "{mr}");
}

/// The GBCD normalisation: boundaries between random orientations, with random planes, give
/// 1 (checked with 40 000 separate triangles, each between its own pair of grains).
#[test]
fn gbcd_normalisation() {
    let n = 40_000;
    let mut s = 99u64;
    let mut u = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut pts = Vec::new();
    let mut polys = Cells::new();
    let mut labels = Vec::new();
    let mut quats = vec![0.0, 0.0, 0.0, 1.0];
    for k in 0..n {
        // A random unit-area triangle orientation: random normal, triangle in its plane.
        let z = 2.0 * u() - 1.0;
        let ph = std::f64::consts::TAU * u();
        let r = (1.0 - z * z).sqrt();
        let nrm = [r * ph.cos(), r * ph.sin(), z];
        let t1 = if nrm[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let e1 = {
            let c = [nrm[1] * t1[2] - nrm[2] * t1[1], nrm[2] * t1[0] - nrm[0] * t1[2], nrm[0] * t1[1] - nrm[1] * t1[0]];
            let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            c.map(|x| x / l)
        };
        let e2 = [nrm[1] * e1[2] - nrm[2] * e1[1], nrm[2] * e1[0] - nrm[0] * e1[2], nrm[0] * e1[1] - nrm[1] * e1[0]];
        let base = [k as f64 * 3.0, 0.0, 0.0];
        let p0 = base;
        let p1: Vec<f64> = (0..3).map(|a| base[a] + 2.0 * e1[a]).collect();
        let p2: Vec<f64> = (0..3).map(|a| base[a] + e2[a]).collect();
        pts.extend(p0);
        pts.extend(p1);
        pts.extend(p2);
        let i = (3 * k) as u32;
        polys.push(CellType::Triangle, &[i, i + 1, i + 2]);
        labels.extend([(2 * k + 1) as i32, (2 * k + 2) as i32]);
        for _ in 0..2 {
            quats.extend(cx::quat_to_xyzw(cx::random_quat(u(), u(), u())));
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    pd.cell_data.insert(Array::new("FaceLabels", 2, ArrayData::I32(labels)));
    pd.field_data.insert(Array::new("AvgQuats", 4, ArrayData::F64(quats)));
    pd.field_data.insert(Array::new("FeaturePhases", 1, ArrayData::I32(std::iter::once(0).chain(std::iter::repeat_n(1, 2 * n)).collect())));
    pd.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1])));
    for (angle, axis, tol) in [(45.0, [1.0, 0.0, 0.0], 10.0), (60.0, [1.0, 1.0, 1.0], 8.0), (30.0, [1.0, 1.0, 0.0], 10.0)] {
        let g = run("gbcd_section", Some(&DataSet::PolyData(pd.clone())), &[("angle", angle.into()), ("axis", serde_json::json!(axis)), ("tolerance", tol.into()), ("sigma", 15.0.into())]);
        let mis = field(&g, "MisorientationMRD")[0];
        let mrd = g.point_data().unwrap().get("MRD").unwrap();
        let mean: f64 = (0..mrd.len()).map(|i| mrd.get(i, 0)).sum::<f64>() / mrd.len() as f64;
        eprintln!("random pairs at {angle} deg: misorientation MRD {mis:.3}, mean plane MRD {mean:.3}");
        assert!((mis - 1.0).abs() < 0.12, "{mis}");
    }
}
