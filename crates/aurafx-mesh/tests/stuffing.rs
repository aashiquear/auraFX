//! Tetrahedral meshes of implicit and surface geometry: every element positive, conforming,
//! volumes summing to the shape's, quality floors, and local refinement keeping all that.

use aurafx_mesh::stuffing::{refine_marked, stuff};
use aurafx_mesh::tet::TetMesh;
use std::collections::HashMap;

fn conforming(m: &TetMesh) -> bool {
    let mut c: HashMap<[u32; 3], usize> = HashMap::new();
    for e in 0..m.n_elements() {
        for s in 1..=4u8 {
            let mut k = m.side_corners(e, s);
            k.sort_unstable();
            *c.entry(k).or_insert(0) += 1;
        }
    }
    c.values().all(|n| *n <= 2)
}

fn min_dihedral(m: &TetMesh) -> f64 {
    use aurafx_mesh::quality::{tet_metric, TetMetric};
    m.tets.iter().map(|t| tet_metric(TetMetric::MinimumDihedralAngle, &t.map(|k| m.points[k as usize]), 0.0)).fold(f64::INFINITY, f64::min)
}

#[test]
fn stuffing_shapes() {
    let pi = std::f64::consts::PI;
    let sphere = |p: [f64; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0;
    let torus = |p: [f64; 3]| {
        let q = (p[0] * p[0] + p[1] * p[1]).sqrt() - 1.0;
        (q * q + p[2] * p[2]).sqrt() - 0.35
    };
    let cases: Vec<(&str, Box<dyn Fn([f64; 3]) -> f64 + Sync>, f64, f64)> = vec![
        ("sphere", Box::new(sphere), 4.0 / 3.0 * pi, 1.2),
        ("torus", Box::new(torus), 2.0 * pi * pi * 0.35 * 0.35, 1.5),
    ];
    for (name, f, exact, r) in cases {
        for h in [0.2, 0.1] {
            let m = stuff(&*f, [-r - h; 3], [r + h; 3], h).unwrap();
            let v: f64 = m.volumes().iter().sum();
            let md = min_dihedral(&m);
            eprintln!("{name} h {h}: {} tets, volume {v:.4} vs {exact:.4} ({:+.2} %), min dihedral {md:.1}", m.n_elements(), 100.0 * (v - exact) / exact);
            assert!(m.volumes().iter().all(|x| *x > 0.0), "{name}");
            assert!(conforming(&m), "{name}");
            // The surface is flat between nodes: the error shrinks with h squared.
            assert!((v - exact).abs() / exact < if h > 0.15 { 0.04 } else { 0.012 }, "{name} {v}");
            assert!(md > 5.0, "{name} {md}");
        }
    }
    // Local refinement: the tetrahedra in the upper half split; still positive and conforming,
    // the volume the same up to the surface nodes moved onto the sphere.
    let m = stuff(&sphere, [-1.2; 3], [1.2; 3], 0.2).unwrap();
    let marked: Vec<bool> = m.tets.iter().map(|t| t.iter().map(|k| m.points[*k as usize][2]).sum::<f64>() > 0.0).collect();
    let project = |p: [f64; 3]| {
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        p.map(|x| x / l)
    };
    let r = refine_marked(&m, &marked, &project);
    let (v0, v1): (f64, f64) = (m.volumes().iter().sum(), r.volumes().iter().sum());
    eprintln!("refined: {} -> {} tets, volume {v0:.4} -> {v1:.4}, min dihedral {:.1}", m.n_elements(), r.n_elements(), min_dihedral(&r));
    assert!(r.volumes().iter().all(|x| *x > 0.0));
    assert!(conforming(&r));
    assert!(r.n_elements() > m.n_elements() * 3);
    assert!(v1 > v0 && (v1 - 4.0 / 3.0 * pi).abs() < (v0 - 4.0 / 3.0 * pi).abs());
}
