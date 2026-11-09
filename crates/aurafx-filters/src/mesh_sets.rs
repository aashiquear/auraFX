//! Mesh sets and edits on solver meshes (unstructured grids that carry `NodeSet <name>` and
//! `SideSet <name>` field arrays): node and side sets chosen by an expression of the
//! coordinates, nodes merged within a tolerance, and a region of hexahedra turned into
//! tetrahedra with pyramids where it meets the hexahedra that stay.

use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::collections::HashMap;

/// Exodus side faces (outward), per cell type.
const HEX_SIDES: [[usize; 4]; 6] = [[0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7], [0, 3, 2, 1], [4, 5, 6, 7]];
const TET_SIDES: [[usize; 3]; 4] = [[0, 1, 3], [1, 2, 3], [0, 3, 2], [0, 2, 1]];

fn grid(ds: &DataSet) -> Result<UnstructuredGrid> {
    match ds {
        DataSet::UnstructuredGrid(u) => Ok(u.clone()),
        d => d.to_unstructured().ok_or_else(|| anyhow!("this takes a mesh (an unstructured grid)")),
    }
}

/// The points where `expression` (of coordsX, coordsY, coordsZ and the point arrays) is not 0.
fn point_mask(ds: &DataSet, expression: &str) -> Result<Vec<bool>> {
    let out = crate::calculator::calculator(ds, Association::Points, expression, "__mask")?;
    let a = out.point_data().and_then(|d| d.get("__mask")).ok_or_else(|| anyhow!("the expression gave no values"))?;
    Ok((0..a.len()).map(|i| a.get(i, 0) != 0.0).collect())
}

fn sides_of(ty: CellType) -> Vec<Vec<usize>> {
    match ty {
        CellType::Hexahedron => HEX_SIDES.iter().map(|s| s.to_vec()).collect(),
        CellType::Tetra => TET_SIDES.iter().map(|s| s.to_vec()).collect(),
        _ => Vec::new(),
    }
}

pub const SET_NODES: &str = "Node set";
pub const SET_SIDES: &str = "Side set";

/// A node set (the nodes where the expression holds) or a side set (the boundary faces of
/// hexahedra and tetrahedra all of whose nodes satisfy it), added as field data under `name`.
pub fn define_set(ds: &DataSet, kind: &str, name: &str, expression: &str) -> Result<DataSet> {
    if name.trim().is_empty() {
        bail!("give the set a name");
    }
    let mask = point_mask(ds, expression)?;
    let mut ug = grid(ds)?;
    if kind == SET_NODES {
        let ids: Vec<i32> = (0..mask.len()).filter(|&i| mask[i]).map(|i| i as i32).collect();
        if ids.is_empty() {
            bail!("no node satisfies {expression}");
        }
        ug.field_data.insert(Array::new(format!("NodeSet {name}"), 1, ArrayData::I32(ids)));
    } else {
        // Boundary faces: in one cell only.
        let mut count: HashMap<Vec<u32>, usize> = HashMap::new();
        let cells: Vec<(CellType, Vec<u32>)> = ug.cells.iter().map(|(t, c)| (t, c.to_vec())).collect();
        for (ty, c) in &cells {
            for s in sides_of(*ty) {
                let mut k: Vec<u32> = s.iter().map(|&i| c[i]).collect();
                k.sort_unstable();
                *count.entry(k).or_default() += 1;
            }
        }
        let mut faces = Vec::new();
        for (e, (ty, c)) in cells.iter().enumerate() {
            for (si, s) in sides_of(*ty).iter().enumerate() {
                let mut k: Vec<u32> = s.iter().map(|&i| c[i]).collect();
                k.sort_unstable();
                if count[&k] == 1 && k.iter().all(|&v| mask[v as usize]) {
                    faces.push(e as i32);
                    faces.push(si as i32 + 1);
                }
            }
        }
        if faces.is_empty() {
            bail!("no boundary face has all its nodes where {expression}");
        }
        ug.field_data.insert(Array::new(format!("SideSet {name}"), 2, ArrayData::I32(faces)));
    }
    Ok(DataSet::UnstructuredGrid(ug))
}

/// Merge nodes closer than `tolerance` (each to the lowest-numbered one): cells and node sets
/// are renumbered, the unused nodes dropped. Field data `MergedNodes` gives how many went.
pub fn merge_nodes(ds: &DataSet, tolerance: f64) -> Result<DataSet> {
    let ug = grid(ds)?;
    let n = ug.points.len();
    let pts: Vec<[f64; 3]> = (0..n).map(|i| ug.points.get(i)).collect();
    let tree = aurafx_data::kdtree::KdTree::new(&pts);
    let mut to = (0..n).collect::<Vec<usize>>();
    let t2 = tolerance * tolerance;
    for i in 0..n {
        if to[i] != i {
            continue;
        }
        for (j, d2) in tree.within(pts[i], tolerance) {
            if j > i && d2 <= t2 && to[j] == j {
                to[j] = i;
            }
        }
    }
    // Renumber the kept nodes.
    let mut new_id = vec![u32::MAX; n];
    let mut keep = Vec::new();
    for i in 0..n {
        if to[i] == i {
            new_id[i] = keep.len() as u32;
            keep.push(i);
        }
    }
    let map = |v: u32| new_id[to[v as usize]];
    let mut out = UnstructuredGrid { points: Points::new_f64(keep.iter().flat_map(|&i| pts[i]).collect()), ..Default::default() };
    for (ty, c) in ug.cells.iter() {
        let ids: Vec<u32> = c.iter().map(|&v| map(v)).collect();
        out.cells.push(ty, &ids);
    }
    out.point_data = ug.point_data.gather(&keep);
    out.cell_data = ug.cell_data.clone();
    for a in ug.field_data.iter() {
        if a.name.starts_with("NodeSet ") {
            let mut v: Vec<i32> = (0..a.len()).map(|i| map(a.get(i, 0) as u32) as i32).collect();
            v.sort_unstable();
            v.dedup();
            out.field_data.insert(Array::new(a.name.clone(), 1, ArrayData::I32(v)));
        } else {
            out.field_data.insert(a.clone());
        }
    }
    out.field_data.insert(Array::new("MergedNodes", 1, ArrayData::I64(vec![(n - keep.len()) as i64])));
    Ok(DataSet::UnstructuredGrid(out))
}

fn signed_volume(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> f64 {
    let (u, v, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
    (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0])) / 6.0
}

/// The hexahedra all of whose nodes satisfy `expression` become tetrahedra: each gets a node at
/// its centre; across a face shared by two such hexahedra the two pyramids on it make four
/// tetrahedra; on a face against a hexahedron that stays, or on the boundary, the pyramid is
/// kept, so the mesh stays conforming. Cell arrays and side sets follow (a side on a converted
/// hexahedron moves to its pyramid's base, side 5).
pub fn hex_to_tets(ds: &DataSet, expression: &str) -> Result<DataSet> {
    let ug = grid(ds)?;
    let mask = point_mask(ds, expression)?;
    let cells: Vec<(CellType, Vec<u32>)> = ug.cells.iter().map(|(t, c)| (t, c.to_vec())).collect();
    let sel: Vec<bool> = cells.iter().map(|(t, c)| *t == CellType::Hexahedron && c.iter().all(|&v| mask[v as usize])).collect();
    if !sel.iter().any(|s| *s) {
        bail!("no hexahedron has all its nodes where {expression}");
    }
    let mut pts: Vec<[f64; 3]> = (0..ug.points.len()).map(|i| ug.points.get(i)).collect();
    let n0 = pts.len();
    // Face -> cells.
    let mut faces: HashMap<Vec<u32>, Vec<(usize, usize)>> = HashMap::new();
    for (e, (ty, c)) in cells.iter().enumerate() {
        for (si, s) in sides_of(*ty).iter().enumerate() {
            let mut k: Vec<u32> = s.iter().map(|&i| c[i]).collect();
            k.sort_unstable();
            faces.entry(k).or_default().push((e, si));
        }
    }
    let mut centre = vec![u32::MAX; cells.len()];
    for (e, (_, c)) in cells.iter().enumerate() {
        if sel[e] {
            let m: [f64; 3] = std::array::from_fn(|k| c.iter().map(|&v| pts[v as usize][k]).sum::<f64>() / 8.0);
            pts.push(m);
            centre[e] = (pts.len() - 1) as u32;
        }
    }
    let mut out = UnstructuredGrid::default();
    let mut parent = Vec::new();
    // (old element, side) -> new element (the pyramid on that side).
    let mut side_map: HashMap<(usize, usize), (usize, i32)> = HashMap::new();
    let push = |out: &mut UnstructuredGrid, parent: &mut Vec<usize>, ty: CellType, ids: &[u32], e: usize| {
        out.cells.push(ty, ids);
        parent.push(e);
        parent.len() - 1
    };
    for (e, (ty, c)) in cells.iter().enumerate() {
        if !sel[e] {
            let k = push(&mut out, &mut parent, *ty, c, e);
            for si in 0..sides_of(*ty).len() {
                side_map.insert((e, si), (k, si as i32 + 1));
            }
            continue;
        }
        let ce = centre[e];
        for (si, s) in HEX_SIDES.iter().enumerate() {
            let f: Vec<u32> = s.iter().map(|&i| c[i]).collect();
            let mut key = f.clone();
            key.sort_unstable();
            let other = faces[&key].iter().find(|x| x.0 != e).copied();
            match other {
                Some((o, _)) if sel[o] => {
                    // Two pyramids on this face: four tetrahedra around the line between the centres (made once).
                    if o < e {
                        continue;
                    }
                    let co = centre[o];
                    for k in 0..4 {
                        let (a, b) = (f[k], f[(k + 1) % 4]);
                        let mut t = [a, b, ce, co];
                        if signed_volume(pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize], pts[t[3] as usize]) < 0.0 {
                            t.swap(0, 1);
                        }
                        push(&mut out, &mut parent, CellType::Tetra, &t, e);
                    }
                }
                _ => {
                    // A pyramid: base the face turned towards the centre, apex the centre.
                    let base = [f[0], f[3], f[2], f[1]];
                    let k = push(&mut out, &mut parent, CellType::Pyramid, &[base[0], base[1], base[2], base[3], ce], e);
                    side_map.insert((e, si), (k, 5));
                }
            }
        }
    }
    out.points = Points::new_f64(pts.iter().flatten().copied().collect());
    // Point data for the new centre nodes: the mean of their hexahedron's nodes.
    let mut pd = Attributes::default();
    for a in ug.point_data.iter() {
        let nc = a.ncomp;
        let mut v = a.data.to_f64_vec();
        for (e, (_, c)) in cells.iter().enumerate() {
            if sel[e] {
                for k in 0..nc {
                    v.push(c.iter().map(|&p| a.get(p as usize, k)).sum::<f64>() / 8.0);
                }
            }
        }
        pd.insert(Array::new(a.name.clone(), nc, ArrayData::F64(v)));
    }
    debug_assert_eq!(n0 + centre.iter().filter(|c| **c != u32::MAX).count(), pts.len());
    out.point_data = pd;
    out.cell_data = ug.cell_data.gather(&parent);
    // The kind of each element: 12 hexahedron, 14 pyramid, 10 tetrahedron (VTK numbers).
    let kinds: Vec<i32> = out.cells.iter().map(|(ty, _)| ty as i32).collect();
    out.cell_data.insert(Array::new("ElementType", 1, ArrayData::I32(kinds)));
    for a in ug.field_data.iter() {
        if a.name.starts_with("SideSet ") && a.ncomp == 2 {
            let mut v = Vec::new();
            for i in 0..a.len() {
                if let Some((k, s)) = side_map.get(&(a.get(i, 0) as usize, a.get(i, 1) as usize - 1)) {
                    v.push(*k as i32);
                    v.push(*s);
                }
            }
            out.field_data.insert(Array::new(a.name.clone(), 2, ArrayData::I32(v)));
        } else {
            out.field_data.insert(a.clone());
        }
    }
    Ok(DataSet::UnstructuredGrid(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_mesh(n: usize) -> DataSet {
        let d = aurafx_mesh::structured::Division::uniform(n);
        aurafx_mesh::structured::box_mesh([0.0; 3], [1.0; 3], [&d, &d, &d]).to_dataset()
    }

    #[test]
    fn sets_by_expression() {
        let m = cube_mesh(4);
        let s = define_set(&m, SET_SIDES, "left", "coordsX < 1e-9").unwrap();
        assert_eq!(s.field_data().get("SideSet left").unwrap().len(), 16);
        let n = define_set(&m, SET_NODES, "corner", "coordsX + coordsY + coordsZ < 0.3").unwrap();
        // Nodes with i + j + k <= 1 (spacing 0.25): 1 + 3.
        assert_eq!(n.field_data().get("NodeSet corner").unwrap().len(), 4);
    }

    #[test]
    fn merging_two_touching_meshes() {
        // Two cubes side by side, written as one grid with duplicated interface nodes.
        let DataSet::UnstructuredGrid(a) = cube_mesh(2) else { panic!() };
        let mut b = a.clone();
        let shifted: Vec<f64> = (0..b.points.len())
            .flat_map(|i| {
                let p = b.points.get(i);
                [p[0] + 1.0, p[1], p[2]]
            })
            .collect();
        b.points = Points::new_f64(shifted);
        let both = DataSet::UnstructuredGrid(crate::append::append_datasets(&[&DataSet::UnstructuredGrid(a.clone()), &DataSet::UnstructuredGrid(b)]).unwrap());
        let m = merge_nodes(&both, 1e-6).unwrap();
        assert_eq!(m.field_data().get("MergedNodes").unwrap().get(0, 0), 9.0);
        assert_eq!(m.n_points(), 2 * 27 - 9);
    }

    #[test]
    fn a_region_of_hexahedra_becomes_tetrahedra_with_pyramids_and_keeps_its_volume() {
        let m = cube_mesh(4);
        let m = define_set(&m, SET_SIDES, "bottom", "coordsZ < 1e-9").unwrap();
        let t = hex_to_tets(&m, "coordsX <= 0.5").unwrap();
        let DataSet::UnstructuredGrid(u) = &t else { panic!() };
        let mut vol = 0.0;
        let (mut hexes, mut pyr, mut tets) = (0, 0, 0);
        for (ty, c) in u.cells.iter() {
            let p: Vec<[f64; 3]> = c.iter().map(|&v| u.points.get(v as usize)).collect();
            match ty {
                CellType::Tetra => {
                    tets += 1;
                    let v = signed_volume(p[0], p[1], p[2], p[3]);
                    assert!(v > 0.0);
                    vol += v;
                }
                CellType::Pyramid => {
                    pyr += 1;
                    let (a, b) = (signed_volume(p[0], p[1], p[2], p[4]), signed_volume(p[0], p[2], p[3], p[4]));
                    assert!(a > 0.0 && b > 0.0, "{a} {b}");
                    vol += a + b;
                }
                _ => {
                    hexes += 1;
                    vol += 1.0 / 64.0;
                }
            }
        }
        assert!((vol - 1.0).abs() < 1e-12, "{vol}");
        // 2 of 4 columns of x converted: 32 hexes stay; 32 converted.
        assert_eq!(hexes, 32);
        assert!(pyr > 0 && tets > 0);
        // The bottom side set: 16 faces still, 8 of them now pyramid bases (side 5).
        let ss = t.field_data().get("SideSet bottom").unwrap();
        assert_eq!(ss.len(), 16);
        let types: Vec<CellType> = u.cells.iter().map(|(ty, _)| ty).collect();
        assert_eq!((0..ss.len()).filter(|&i| types[ss.get(i, 0) as usize] == CellType::Pyramid && ss.get(i, 1) == 5.0).count(), 8);
    }
}
