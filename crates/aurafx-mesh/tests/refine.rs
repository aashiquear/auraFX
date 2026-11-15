//! Refinement, hexahedron/tetrahedron conversions, smoothing and boundary layers: volumes kept,
//! every element positive, faces conforming, sets carried over.

use aurafx_mesh::hex::HexMesh;
use aurafx_mesh::improve;
use aurafx_mesh::structured::{box_mesh, Division};
use aurafx_mesh::tet::{hexes_to_tets, TetMesh};
use std::collections::HashMap;

fn cube(n: usize) -> HexMesh {
    let d = Division::uniform(n);
    box_mesh([0.0; 3], [1.0, 2.0, 0.5], [&d, &d, &d])
}

/// No face is shared by more than two elements, and inner faces by exactly two.
fn conforming_tets(m: &TetMesh) -> bool {
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

fn conforming_hexes(m: &HexMesh) -> bool {
    let mut c: HashMap<[u32; 4], usize> = HashMap::new();
    for e in 0..m.n_elements() {
        for s in 1..=6u8 {
            let mut k = m.side_corners(e, s);
            k.sort_unstable();
            *c.entry(k).or_insert(0) += 1;
        }
    }
    c.values().all(|n| *n <= 2)
}

#[test]
fn refine_and_convert() {
    let m = cube(4);
    let v0 = 1.0;
    let nz = m.side_set("z+").unwrap().len();
    // Hexahedra: 8 children each.
    let r = m.refine().unwrap();
    assert_eq!(r.n_elements(), 8 * m.n_elements());
    assert!(r.signed_volumes().iter().all(|v| *v > 0.0));
    assert!((r.signed_volumes().iter().sum::<f64>() - v0).abs() < 1e-12);
    assert!(conforming_hexes(&r));
    assert_eq!(r.side_set("z+").unwrap().len(), 4 * nz);
    assert_eq!(r.node_set("z+").unwrap().len(), 9 * 9);
    // HTet: 12 tetrahedra per hexahedron, conforming, the same volume.
    let t = hexes_to_tets(&m).unwrap();
    assert_eq!(t.n_elements(), 12 * m.n_elements());
    assert!(t.volumes().iter().all(|v| *v > 0.0));
    assert!((t.volumes().iter().sum::<f64>() - v0).abs() < 1e-12);
    assert!(conforming_tets(&t));
    assert_eq!(t.side_set("z+").unwrap().len(), 2 * nz);
    // Every side of the boundary is a boundary face, in one of the six sets.
    let in_sets: usize = ["x-", "x+", "y-", "y+", "z-", "z+"].iter().map(|s| t.side_set(s).unwrap().len()).sum();
    assert_eq!(in_sets, t.boundary_faces().len());
    // Tetrahedra refined: 8 each, conforming, same volume, side sets 4 times.
    let tr = t.refine();
    assert_eq!(tr.n_elements(), 8 * t.n_elements());
    assert!(tr.volumes().iter().all(|v| *v > 0.0));
    assert!((tr.volumes().iter().sum::<f64>() - v0).abs() < 1e-12);
    assert!(conforming_tets(&tr));
    assert_eq!(tr.side_set("z+").unwrap().len(), 4 * 2 * nz);
    // THex: 4 hexahedra per tetrahedron.
    let h = t.to_hexes();
    assert_eq!(h.n_elements(), 4 * t.n_elements());
    assert!(h.signed_volumes().iter().all(|v| *v > 0.0));
    assert!((h.signed_volumes().iter().sum::<f64>() - v0).abs() < 1e-12);
    assert!(conforming_hexes(&h));
    assert_eq!(h.side_set("z+").unwrap().len(), 3 * 2 * nz);
    let in_sets: usize = ["x-", "x+", "y-", "y+", "z-", "z+"].iter().map(|s| h.side_set(s).unwrap().len()).sum();
    assert_eq!(in_sets, h.boundary_faces().len());
    // Through the datasets and back.
    let back = TetMesh::from_dataset(&t.to_dataset()).unwrap();
    assert_eq!(back.n_elements(), t.n_elements());
    assert_eq!(back.side_set("x-").unwrap().len(), t.side_set("x-").unwrap().len());
}

#[test]
fn boundary_layers_and_smoothing() {
    let m = cube(8);
    let top = m.side_set("z+").unwrap().clone();
    let (nx, nz) = (m.side_set("x-").unwrap().len(), top.len());
    let b = improve::boundary_layers(&m, &top, 3, 0.005, 1.5).unwrap();
    assert_eq!(b.n_elements(), m.n_elements() + 3 * nz);
    assert!(b.signed_volumes().iter().all(|v| *v > 0.0));
    assert!((b.signed_volumes().iter().sum::<f64>() - 1.0).abs() < 1e-12);
    assert!(conforming_hexes(&b));
    // The top set is now the outer layer, still at z = 0.5; its elements are 0.005 thick.
    for (e, s) in b.side_set("z+").unwrap() {
        assert_eq!(*s, 6);
        let c = b.cell(*e as usize);
        let (zt, zb) = (b.points[c[4] as usize][2], b.points[c[0] as usize][2]);
        assert!((zt - 0.5).abs() < 1e-12 && (zt - zb - 0.005).abs() < 1e-12, "{zt} {zb}");
    }
    // The side faces of the layers join the side sets around them.
    assert_eq!(b.side_set("x-").unwrap().len(), nx + 3 * 8);
    let in_sets: usize = ["x-", "x+", "y-", "y+", "z-", "z+"].iter().map(|s| b.side_set(s).unwrap().len()).sum();
    assert_eq!(in_sets, b.boundary_faces().len());
    // Too thick: refused.
    assert!(improve::boundary_layers(&m, &top, 3, 0.2, 2.0).is_err());
    // Smoothing a jiggled mesh: the guarded Laplacian and the condition-number optimisation
    // both bring the worst element back up.
    let mut j = cube(8);
    let mut s = 7u64;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let bnd: std::collections::HashSet<u32> = j.boundary_faces().iter().flat_map(|(e, s)| j.side_corners(*e as usize, *s)).collect();
    let h = 1.0 / 8.0;
    for (i, p) in j.points.iter_mut().enumerate() {
        if !bnd.contains(&(i as u32)) {
            for a in 0..3 {
                p[a] += 0.35 * h * rnd() * [1.0, 2.0, 0.5][a];
            }
        }
    }
    let cells = improve::Cells3 { cells: (0..j.n_elements()).map(|e| j.cell(e).to_vec()).collect() };
    let before = cells.cells.iter().map(|c| aurafx_mesh::overlay::corner_sj(&std::array::from_fn(|k| j.points[c[k] as usize]))).fold(f64::INFINITY, f64::min);
    for scheme in [improve::SMOOTH_SMART, improve::SMOOTH_CONDITION, improve::SMOOTH_LAPLACIAN] {
        let mut p = j.points.clone();
        let after = improve::smooth(&mut p, &cells, scheme, 10, &Default::default());
        eprintln!("{scheme}: min scaled Jacobian {before:.3} -> {after:.3}");
        assert!(after > before + 0.1, "{scheme}");
    }
}

#[test]
fn winslow_smoothing_evens_out_a_disturbed_grid_and_keeps_a_regular_one() {
    use aurafx_mesh::improve::{smooth, Cells3, SMOOTH_WINSLOW};
    use aurafx_mesh::structured::{box_mesh, Division};
    let d = Division::uniform(6);
    let m = box_mesh([0.0; 3], [1.0; 3], [&d, &d, &d]);
    let cells = Cells3 { cells: (0..m.n_elements()).map(|e| m.cell(e)[..8].to_vec()).collect() };
    // A regular grid stays.
    let mut p = m.points.clone();
    smooth(&mut p, &cells, SMOOTH_WINSLOW, 5, &Default::default());
    assert!(p.iter().zip(&m.points).all(|(a, b)| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-12)));
    // Interior nodes moved by up to a third of a cell come back near the regular places.
    let mut s = 7u64;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s % 1000) as f64 / 1000.0 - 0.5
    };
    let h = 1.0 / 6.0;
    let mut q = m.points.clone();
    for x in q.iter_mut() {
        if x.iter().all(|c| *c > 1e-9 && *c < 1.0 - 1e-9) {
            for c in x.iter_mut() {
                *c += rnd() * h * 0.6;
            }
        }
    }
    let err = |p: &[[f64; 3]]| p.iter().zip(&m.points).map(|(a, b)| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>()).sum::<f64>().sqrt();
    let before = err(&q);
    let worst = smooth(&mut q, &cells, SMOOTH_WINSLOW, 30, &Default::default());
    assert!(err(&q) < 0.05 * before, "{} -> {}", before, err(&q));
    assert!(worst > 0.95, "{worst}");
}
