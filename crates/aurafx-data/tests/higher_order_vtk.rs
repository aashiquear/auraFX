//! Node numbering of Lagrange cells against VTK (parametric coordinates dumped from
//! ParaView 6.1.1's vtkLagrange* cells for orders 1 to 6).

use aurafx_data::higher_order::HighOrderCell;
use aurafx_data::CellType;
use std::collections::HashMap;

#[test]
fn lagrange_node_order_matches_vtk() {
    let text = include_str!("data/vtk_lagrange_pcoords.json");
    let all: HashMap<String, Vec<f64>> = serde_json::from_str(text).unwrap();
    let types = [
        ("curve", CellType::LagrangeCurve),
        ("tri", CellType::LagrangeTriangle),
        ("quad", CellType::LagrangeQuadrilateral),
        ("tet", CellType::LagrangeTetrahedron),
        ("hex", CellType::LagrangeHexahedron),
        ("wedge", CellType::LagrangeWedge),
    ];
    let mut checked = 0;
    for (name, ty) in types {
        for p in 1..=6 {
            let vtk = &all[&format!("{name}{p}")];
            let n = vtk.len() / 3;
            let c = HighOrderCell::new(ty, n, None).unwrap_or_else(|| panic!("{name}{p}: no cell for {n} points"));
            assert_eq!(c.len(), n);
            for k in 0..n {
                let ours = c.node_pcoords(k);
                for a in 0..3 {
                    assert!((ours[a] - vtk[3 * k + a]).abs() < 1e-6, "{name} order {p} node {k}: {ours:?} vs VTK {:?}", &vtk[3 * k..3 * k + 3]);
                }
            }
            checked += n;
        }
    }
    assert!(checked > 1500);
}

#[test]
fn anisotropic_orders_match_vtk() {
    let all: HashMap<String, Vec<f64>> = serde_json::from_str(include_str!("data/vtk_lagrange_pcoords_aniso.json")).unwrap();
    for (key, vtk) in &all {
        let (name, deg) = key.split_once('(').unwrap();
        let d: Vec<f64> = deg.trim_end_matches(')').split(',').map(|s| s.trim().parse().unwrap()).collect();
        let ty = match name {
            "hex" => CellType::LagrangeHexahedron,
            "quad" => CellType::LagrangeQuadrilateral,
            _ => CellType::LagrangeWedge,
        };
        let n = vtk.len() / 3;
        let c = HighOrderCell::new(ty, n, Some([d[0], d[1], d[2]])).unwrap();
        for k in 0..n {
            let ours = c.node_pcoords(k);
            for a in 0..3 {
                assert!((ours[a] - vtk[3 * k + a]).abs() < 1e-6, "{key} node {k}: {ours:?} vs VTK {:?}", &vtk[3 * k..3 * k + 3]);
            }
        }
    }
}
