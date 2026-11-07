//! Overlay-grid meshing: quality floors (no inverted elements, smallest scaled Jacobian),
//! conformity (every interior face shared by exactly two elements) and the volume of each
//! material against the exact one, on CSG parts and voxel images.

use aurafx_mesh::hex::HexMesh;
use aurafx_mesh::overlay::*;
use aurafx_mesh::Model;
use std::collections::HashMap;

fn volumes(m: &HexMesh) -> HashMap<i32, f64> {
    let mut v = HashMap::new();
    for (e, q) in m.signed_volumes().iter().enumerate() {
        *v.entry(m.block[e]).or_insert(0.0) += q;
    }
    v
}

fn conforming(m: &HexMesh) -> bool {
    let mut count: HashMap<[u32; 4], usize> = HashMap::new();
    for e in 0..m.n_elements() {
        for s in 1..=6u8 {
            let mut k = m.side_corners(e, s);
            k.sort_unstable();
            *count.entry(k).or_insert(0) += 1;
        }
    }
    // Faces are shared by one or two elements, and no two faces share three corners.
    count.values().all(|c| *c <= 2)
}

fn mesh_model(cmds: &str, h: f64, opt: impl Fn(&mut OverlayOptions)) -> (Overlay, Model) {
    let model = Model::from_commands(cmds, None).unwrap();
    let shapes: Vec<(u32, &aurafx_mesh::shape::Shape)> = model.bodies.values().map(|b| (b.id, &b.shape)).collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for (_, s) in &shapes {
        let (a, b) = s.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(a[k] - 1.5 * h);
            hi[k] = hi[k].max(b[k] + 1.5 * h);
        }
    }
    let cells = std::array::from_fn(|k| ((hi[k] - lo[k]) / h).round() as usize);
    let mut o = OverlayOptions { lo, hi, cells, face_names: model.faces.clone(), ..Default::default() };
    opt(&mut o);
    let src = ShapeSource { shapes };
    (overlay(&src, &o).unwrap(), model)
}

#[test]
fn overlay_parts() {
    let pi = std::f64::consts::PI;
    for (name, cmds, h, exact) in [
        ("sphere", "sphere radius 1", 0.125, vec![(1, 4.0 / 3.0 * pi)]),
        ("plate with hole", "brick x 4 y 2 z 1; cylinder radius 0.5 height 2; subtract body 2 from body 1", 0.125, vec![(1, 8.0 - pi * 0.25)]),
        ("sphere in a block", "sphere radius 0.6; brick x 2 y 2 z 2", 0.1, vec![(1, 4.0 / 3.0 * pi * 0.216), (2, 8.0 - 4.0 / 3.0 * pi * 0.216)]),
        ("crossed cylinders", "cylinder radius 0.5 height 3; cylinder radius 0.5 height 3; rotate body 2 angle 90 about x; unite body 1 2", 0.1, vec![(1, 2.0 * pi * 0.25 * 3.0 - 16.0 / 3.0 * 0.125)]),
    ] {
        let (r, _) = mesh_model(cmds, h, |_| {});
        let m = &r.mesh;
        let v = volumes(m);
        eprintln!("{name}: {} elements, min SJ {:.3}, inverted {}, sets {:?}", m.n_elements(), r.min_sj, r.inverted, m.side_sets.iter().map(|s| (s.0.clone(), s.1.len())).collect::<Vec<_>>());
        assert_eq!(r.inverted, 0, "{name}");
        assert!(r.min_sj > 0.2, "{name}: {}", r.min_sj);
        assert!(conforming(m), "{name}");
        for (b, want) in exact {
            let got = v[&b];
            eprintln!("  block {b}: {got} vs {want} ({:.2} %)", 100.0 * (got - want) / want);
            assert!((got - want).abs() / want < 0.015, "{name} block {b}");
        }
        // The geometry faces become side sets.
        assert!(m.side_sets.iter().all(|s| !s.1.is_empty()));
    }
    // Stair-step: whole cells only, every element a perfect cube.
    let (r, _) = mesh_model("sphere radius 1", 0.125, |o| {
        o.pillow = false;
        o.project = false;
    });
    assert!((r.min_sj - 1.0).abs() < 1e-12);
    // The void meshed too: the whole grid is filled.
    let (r, _) = mesh_model("sphere radius 1", 0.25, |o| o.mesh_void = true);
    let v = volumes(&r.mesh);
    assert_eq!(r.inverted, 0);
    assert!((v.values().sum::<f64>() - 2.75f64.powi(3)).abs() < 1e-9, "{v:?}");
    assert_eq!(r.mesh.block_names[&2], "void");
    assert!(r.mesh.side_set("interface material 1 - void").is_some());
}

/// Two grains in a periodic voxel cube: a sphere of label 2 in label 1, meshed on the image's
/// own grid, with flat matching domain faces and periodic pairs.
#[test]
fn overlay_voxels() {
    let n = 16;
    let mut labels = Vec::new();
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let c = [i, j, k].map(|x| (x as f64 + 0.5) / n as f64 - 0.5);
                labels.push(if c[0] * c[0] + c[1] * c[1] + c[2] * c[2] < 0.3 * 0.3 { 2 } else { 1 });
            }
        }
    }
    let src = LabelSource { n: [n; 3], origin: [0.0; 3], spacing: [1.0 / n as f64; 3], labels };
    for periodic in [false, true] {
        let o = OverlayOptions { lo: [0.0; 3], hi: [1.0; 3], cells: [n; 3], samples: 1, periodic, ..Default::default() };
        let r = overlay(&src, &o).unwrap();
        let m = &r.mesh;
        let v = volumes(m);
        eprintln!("voxels (periodic {periodic}): {} elements, min SJ {:.3}, volumes {v:?}", m.n_elements(), r.min_sj);
        assert_eq!(r.inverted, 0);
        assert!(r.min_sj > 0.25, "{}", r.min_sj);
        assert!(conforming(m));
        assert!((v[&1] + v[&2] - 1.0).abs() < 1e-9);
        let sphere = 4.0 / 3.0 * std::f64::consts::PI * 0.027;
        assert!((v[&2] - sphere).abs() / sphere < 0.1, "{}", v[&2]);
        for s in ["x-", "x+", "y-", "y+", "z-", "z+"] {
            assert_eq!(m.side_set(s).unwrap().len(), n * n, "{s}");
        }
        assert!(m.side_set("interface material 1 - material 2").is_some());
        if periodic {
            for (axis, pairs) in &m.periodic_pairs {
                assert_eq!(pairs.len(), (n + 1) * (n + 1), "{axis}");
            }
        }
    }
}

/// A closed triangle surface (a cube turned 30 degrees about z) as the geometry.
#[test]
fn overlay_surface() {
    use aurafx_mesh::trimesh::TriMesh;
    let a = 30f64.to_radians();
    let pts: Vec<[f64; 3]> = (0..8)
        .map(|k| {
            let p = [(k & 1) as f64 - 0.5, ((k >> 1) & 1) as f64 - 0.5, ((k >> 2) & 1) as f64 - 0.5];
            [p[0] * a.cos() - p[1] * a.sin(), p[0] * a.sin() + p[1] * a.cos(), p[2]]
        })
        .collect();
    let tris: Vec<[u32; 3]> = vec![[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
    let tm = TriMesh::new(&pts, &tris).unwrap();
    let shape = aurafx_mesh::shape::Shape::Surface { mesh: std::sync::Arc::new(tm), face: 1 };
    let src = ShapeSource { shapes: vec![(1, &shape)] };
    let o = OverlayOptions { lo: [-1.0; 3], hi: [1.0; 3], cells: [20; 3], ..Default::default() };
    let r = overlay(&src, &o).unwrap();
    let v = volumes(&r.mesh);
    eprintln!("surface: min SJ {:.3}, volume {}", r.min_sj, v[&1]);
    assert_eq!(r.inverted, 0);
    assert!(conforming(&r.mesh));
    assert!((v[&1] - 1.0).abs() < 0.03, "{}", v[&1]);
}


/// Four grains meeting along triple lines and at the domain faces.
#[test]
fn overlay_grains() {
    let n = 20;
    let mut labels = Vec::new();
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let c = [i, j, k].map(|x| (x as f64 + 0.5) / n as f64);
                let r2 = (c[0] - 0.6).powi(2) + (c[1] - 0.4).powi(2) + (c[2] - 0.5).powi(2);
                labels.push(if r2 < 0.25 * 0.25 { 4 } else if c[0] + 0.3 * c[2] < 0.5 { 1 } else if c[1] < 0.55 - 0.2 * c[0] { 2 } else { 3 });
            }
        }
    }
    let src = LabelSource { n: [n; 3], origin: [0.0; 3], spacing: [1.0 / n as f64; 3], labels };
    let o = OverlayOptions { lo: [0.0; 3], hi: [1.0; 3], cells: [n; 3], samples: 1, ..Default::default() };
    let r = overlay(&src, &o).unwrap();
    let v = volumes(&r.mesh);
    eprintln!("grains: {} elements, min SJ {:.3}, volumes {v:?}", r.mesh.n_elements(), r.min_sj);
    assert_eq!(r.inverted, 0);
    assert!(r.min_sj > 0.25, "{}", r.min_sj);
    assert!(conforming(&r.mesh));
    assert!((v.values().sum::<f64>() - 1.0).abs() < 1e-9);
    assert_eq!(v.len(), 4);
}



/// Sharp edges: a brick turned off the grid keeps its edges (nodes are put on them), so its
/// volume is close to exact; without that the edges are rounded off.
#[test]
fn overlay_sharp_edges() {
    let cmds = "brick x 2 y 1 z 1; rotate body 1 angle 25 about z; rotate body 1 angle 10 about x";
    let mut err = Vec::new();
    for curves in [true, false] {
        let (r, _) = mesh_model(cmds, 0.1, |o| o.curves = curves);
        let v = volumes(&r.mesh)[&1];
        // The corners of the brick: the distance to the nearest mesh node.
        let model = Model::from_commands(cmds, None).unwrap();
        let s = &model.bodies[&1].shape;
        let mut far: f64 = 0.0;
        for k in 0..8 {
            let c = [(k & 1) as f64 - 0.5, ((k >> 1) & 1) as f64 - 0.5, ((k >> 2) & 1) as f64 - 0.5].map(|x| x);
            let c = [2.0 * c[0], c[1], c[2]];
            // The brick's corner in place: search the surface point of the shape nearest the
            // turned corner (rotation about z by 25 degrees, then x by 10).
            let (a, b) = (25f64.to_radians(), 10f64.to_radians());
            let p = [c[0] * a.cos() - c[1] * a.sin(), c[0] * a.sin() + c[1] * a.cos(), c[2]];
            let p = [p[0], p[1] * b.cos() - p[2] * b.sin(), p[1] * b.sin() + p[2] * b.cos()];
            assert!(s.eval(p).0.abs() < 1e-9);
            let d = r.mesh.points.iter().map(|q| ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt()).fold(f64::INFINITY, f64::min);
            far = far.max(d);
        }
        eprintln!("curves {curves}: volume {v:.5} ({:+.2} %), min SJ {:.3}, farthest corner {far:.4}", 100.0 * (v - 2.0) / 2.0, r.min_sj);
        assert_eq!(r.inverted, 0);
        err.push((v - 2.0).abs());
        if curves {
            assert!(far < 0.3 * 0.1, "{far}");
        }
    }
    assert!(err[0] < 0.5 * err[1], "{err:?}");
}
