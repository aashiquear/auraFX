//! Mesh improvement: smoothing of hexahedral and tetrahedral meshes (Laplacian, guarded
//! Laplacian, condition-number optimisation) with their boundary held, and boundary layers
//! inserted under chosen faces of a hexahedral mesh.

use crate::hex::{HexMesh, P3, HEX8_SIDES, HEX_EDGES};
use crate::overlay::corner_sj;
use crate::quality::{tet_metric, TetMetric};
use std::collections::{BTreeSet, HashMap, HashSet};

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Cells of a mesh to smooth: hexahedra (8 corners) and tetrahedra (4).
pub struct Cells3 {
    pub cells: Vec<Vec<u32>>,
}

const TET_EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
const TET_FACES: [[usize; 3]; 4] = [[0, 1, 3], [1, 2, 3], [0, 3, 2], [0, 2, 1]];
const CORNER_NB: [[usize; 3]; 8] = [[1, 3, 4], [2, 0, 5], [3, 1, 6], [0, 2, 7], [7, 5, 0], [4, 6, 1], [5, 7, 2], [6, 4, 3]];

/// The worst scaled Jacobian of a cell (hexahedra: over their corners).
fn quality(p: &[P3], c: &[u32]) -> f64 {
    if c.len() == 8 {
        corner_sj(&std::array::from_fn(|k| p[c[k] as usize]))
    } else {
        tet_metric(TetMetric::ScaledJacobian, &std::array::from_fn(|k| p[c[k] as usize]), 0.0)
    }
}

/// The worst condition number of a cell (infinite when inverted).
fn condition(p: &[P3], c: &[u32]) -> f64 {
    if c.len() == 4 {
        let q = tet_metric(TetMetric::Condition, &std::array::from_fn(|k| p[c[k] as usize]), 0.0);
        return if q > 0.0 && q < 1e29 { q } else { f64::INFINITY };
    }
    let mut worst: f64 = 1.0;
    for (k, nb) in CORNER_NB.iter().enumerate() {
        let a = sub(p[c[nb[0]] as usize], p[c[k] as usize]);
        let b = sub(p[c[nb[1]] as usize], p[c[k] as usize]);
        let d = sub(p[c[nb[2]] as usize], p[c[k] as usize]);
        let det = dot(a, cross(b, d));
        if det <= 0.0 {
            return f64::INFINITY;
        }
        let f = (dot(a, a) + dot(b, b) + dot(d, d)).sqrt();
        let adj = cross(b, d);
        let adj2 = cross(d, a);
        let adj3 = cross(a, b);
        let g = (dot(adj, adj) + dot(adj2, adj2) + dot(adj3, adj3)).sqrt();
        worst = worst.max(f * g / (3.0 * det));
    }
    worst
}

pub const SMOOTH_LAPLACIAN: &str = "Laplacian";
pub const SMOOTH_SMART: &str = "Smart Laplacian";
pub const SMOOTH_CONDITION: &str = "Condition number";

/// Smooth a mesh in place: nodes on its boundary (and those in `fixed`) stay put. Laplacian:
/// each free node to the mean of its edge neighbours. Smart Laplacian: the same, kept only if
/// the worst scaled Jacobian around the node does not drop. Condition number: each free node
/// searches for the position that lowers the sum of the worst condition numbers of its
/// cells. Returns the smallest scaled Jacobian after.
pub fn smooth(points: &mut [P3], m: &Cells3, scheme: &str, iterations: usize, fixed: &HashSet<u32>) -> f64 {
    let n = points.len();
    let mut nbr: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut cells_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut faces: HashMap<Vec<u32>, usize> = HashMap::new();
    for (e, c) in m.cells.iter().enumerate() {
        for v in c {
            cells_of[*v as usize].push(e);
        }
        let edges: &[(usize, usize)] = if c.len() == 8 { &HEX_EDGES } else { &TET_EDGES };
        for (a, b) in edges {
            nbr[c[*a] as usize].insert(c[*b]);
            nbr[c[*b] as usize].insert(c[*a]);
        }
        if c.len() == 8 {
            for f in HEX8_SIDES {
                let mut k: Vec<u32> = f.iter().map(|i| c[*i]).collect();
                k.sort_unstable();
                *faces.entry(k).or_insert(0) += 1;
            }
        } else {
            for f in TET_FACES {
                let mut k: Vec<u32> = f.iter().map(|i| c[*i]).collect();
                k.sort_unstable();
                *faces.entry(k).or_insert(0) += 1;
            }
        }
    }
    let mut boundary = vec![false; n];
    for (k, cnt) in &faces {
        if *cnt == 1 {
            for v in k {
                boundary[*v as usize] = true;
            }
        }
    }
    let free: Vec<usize> = (0..n).filter(|v| !boundary[*v] && !fixed.contains(&(*v as u32)) && !cells_of[*v].is_empty()).collect();
    let local_q = |p: &[P3], v: usize| cells_of[v].iter().map(|e| quality(p, &m.cells[*e])).fold(f64::INFINITY, f64::min);
    let local_c = |p: &[P3], v: usize| cells_of[v].iter().map(|e| condition(p, &m.cells[*e])).sum::<f64>();
    for it in 0..iterations {
        for &v in &free {
            let k = nbr[v].len() as f64;
            let c: P3 = std::array::from_fn(|a| nbr[v].iter().map(|j| points[*j as usize][a]).sum::<f64>() / k);
            match scheme {
                SMOOTH_SMART => {
                    let old = points[v];
                    let before = local_q(points, v);
                    points[v] = c;
                    if local_q(points, v) < before.min(1.0) - 1e-12 {
                        points[v] = old;
                    }
                }
                SMOOTH_CONDITION => {
                    // Pattern search around the node, steps a fraction of its edge lengths.
                    let h = nbr[v].iter().map(|j| dot(sub(points[*j as usize], points[v]), sub(points[*j as usize], points[v])).sqrt()).fold(f64::INFINITY, f64::min);
                    let mut step = 0.2 * h / (1 << (it.min(8) / 2)) as f64;
                    let mut best = local_c(points, v);
                    for _ in 0..4 {
                        let mut moved = false;
                        // The Laplacian target first, then the six axis moves.
                        let mut cands: Vec<P3> = vec![c];
                        for a in 0..3 {
                            for s in [-1.0, 1.0] {
                                let mut q = points[v];
                                q[a] += s * step;
                                cands.push(q);
                            }
                        }
                        for q in cands {
                            let old = points[v];
                            points[v] = q;
                            let e = local_c(points, v);
                            if e < best - 1e-12 {
                                best = e;
                                moved = true;
                            } else {
                                points[v] = old;
                            }
                        }
                        if !moved {
                            step *= 0.5;
                        }
                    }
                }
                _ => points[v] = c,
            }
        }
    }
    m.cells.iter().map(|c| quality(points, c)).fold(f64::INFINITY, f64::min)
}

/// Boundary layers: `layers` thin layers of hexahedra inserted under the boundary faces
/// `faces` of a HEX8 mesh, the first `first` thick and each next `growth` times thicker. The
/// faces' nodes keep their places (as the outermost layer's nodes); the original nodes move
/// inwards by the total thickness, sliding along neighbouring boundary faces at the rim.
/// Side sets holding the faces move to the outermost layer; the layers' side faces join the
/// side sets of the boundary faces beside them; node sets follow the nodes.
pub fn boundary_layers(m: &HexMesh, faces: &[(u32, u8)], layers: usize, first: f64, growth: f64) -> Result<HexMesh, String> {
    if m.npe != 8 {
        return Err("boundary layers need a HEX8 mesh".into());
    }
    if layers == 0 || faces.is_empty() {
        return Ok(m.clone());
    }
    let key = |f: [u32; 4]| -> [u32; 4] {
        let mut k = f;
        k.sort_unstable();
        k
    };
    let all_boundary: HashSet<[u32; 4]> = m.boundary_faces().iter().map(|(e, s)| key(m.side_corners(*e as usize, *s))).collect();
    let chosen: Vec<(u32, u8, [u32; 4])> = faces.iter().map(|(e, s)| (*e, *s, m.side_corners(*e as usize, *s))).collect();
    if chosen.iter().any(|f| !all_boundary.contains(&key(f.2))) {
        return Err("the faces must lie on the boundary of the mesh".into());
    }
    let chosen_keys: HashSet<[u32; 4]> = chosen.iter().map(|f| key(f.2)).collect();
    let normal = |f: [u32; 4]| -> P3 {
        let p = f.map(|k| m.points[k as usize]);
        cross(sub(p[2], p[0]), sub(p[3], p[1]))
    };
    // Outward normals at the nodes of the chosen faces, and of the other boundary faces.
    let mut nrm: HashMap<u32, P3> = HashMap::new();
    for (_, _, f) in &chosen {
        let n = normal(*f);
        for v in f {
            let e = nrm.entry(*v).or_insert([0.0; 3]);
            for a in 0..3 {
                e[a] += n[a];
            }
        }
    }
    let mut other: HashMap<u32, Vec<P3>> = HashMap::new();
    let mut other_faces: Vec<(u32, u8, [u32; 4])> = Vec::new();
    for (e, s) in m.boundary_faces() {
        let f = m.side_corners(e as usize, s);
        if chosen_keys.contains(&key(f)) {
            continue;
        }
        if f.iter().any(|v| nrm.contains_key(v)) {
            other_faces.push((e, s, f));
            let n = normal(f);
            let l = dot(n, n).sqrt();
            for v in f {
                if nrm.contains_key(&v) {
                    other.entry(v).or_default().push(n.map(|x| x / l));
                }
            }
        }
    }
    // Directions: the mean normal, slid along the other faces at the rim (Gram-Schmidt on
    // their normals), scaled so the layer keeps its thickness along the chosen faces.
    let mut dir: HashMap<u32, P3> = HashMap::new();
    for (v, n) in &nrm {
        let l = dot(*n, *n).sqrt();
        let mut d = n.map(|x| x / l);
        let base = d;
        if let Some(list) = other.get(v) {
            let mut basis: Vec<P3> = Vec::new();
            for o in list {
                let mut o = *o;
                for b in &basis {
                    let k = dot(o, *b);
                    o = [o[0] - k * b[0], o[1] - k * b[1], o[2] - k * b[2]];
                }
                let lo = dot(o, o).sqrt();
                if lo > 1e-6 {
                    basis.push(o.map(|x| x / lo));
                }
            }
            for b in &basis {
                let k = dot(d, *b);
                d = [d[0] - k * b[0], d[1] - k * b[1], d[2] - k * b[2]];
            }
        }
        let c = dot(d, base);
        if c < 0.2 {
            return Err(format!("node {v}: no room to grow the layers along the neighbouring faces"));
        }
        dir.insert(*v, d.map(|x| x / c));
    }
    // Depths of the layer surfaces: 0 (the faces) .. total (the original nodes).
    let g = if growth > 0.0 { growth } else { 1.0 };
    let mut depth = vec![0.0];
    let mut t = first;
    for _ in 0..layers {
        depth.push(depth.last().unwrap() + t);
        t *= g;
    }
    let total = *depth.last().unwrap();
    let mut out = m.clone();
    // surf[i][v]: the node of surface i (0 outermost) for face node v; surface `layers` is v.
    let mut surf: Vec<HashMap<u32, u32>> = vec![HashMap::new(); layers + 1];
    let mut verts: Vec<u32> = nrm.keys().copied().collect();
    verts.sort_unstable();
    for &v in &verts {
        let p = m.points[v as usize];
        let d = dir[&v];
        for (i, s) in surf.iter_mut().enumerate().take(layers) {
            let q: P3 = std::array::from_fn(|a| p[a] - d[a] * depth[i]);
            out.points.push(q);
            s.insert(v, (out.points.len() - 1) as u32);
        }
        surf[layers].insert(v, v);
        out.points[v as usize] = std::array::from_fn(|a| p[a] - d[a] * total);
    }
    // The layer elements: between surface i (outer) and i + 1 (inner) under each face.
    let mut outer_of: HashMap<(u32, u8), u32> = HashMap::new();
    let mut lateral: Vec<(u32, u8, [u32; 2])> = Vec::new();
    for (e, s, f) in &chosen {
        for i in 0..layers {
            let o = f.map(|v| surf[i][&v]);
            let n = f.map(|v| surf[i + 1][&v]);
            let id = out.n_elements() as u32;
            out.cells.extend([n[0], n[1], n[2], n[3], o[0], o[1], o[2], o[3]]);
            out.block.push(m.block[*e as usize]);
            for (_, vals) in out.elem_fields.iter_mut() {
                let x = vals[*e as usize];
                vals.push(x);
            }
            if i == 0 {
                outer_of.insert((*e, *s), id);
            }
            // Lateral sides 1..4 of the new element run along the face's edges.
            for side in 1..=4u8 {
                let c = HEX8_SIDES[side as usize - 1];
                // The side's two top (outer) corners are face nodes k, k+1.
                let tops: Vec<usize> = c.iter().copied().filter(|k| *k >= 4).collect();
                lateral.push((id, side, [f[tops[0] - 4], f[tops[1] - 4]]));
            }
        }
    }
    // Side sets: the chosen faces move outwards (the outer element's top, side 6); rim faces
    // gain the lateral sides of the layers along the edges they share.
    let mut edge_faces: HashMap<(u32, u32), Vec<(u32, u8)>> = HashMap::new();
    for (e, s, f) in &other_faces {
        for k in 0..4 {
            let (a, b) = (f[k], f[(k + 1) % 4]);
            edge_faces.entry((a.min(b), a.max(b))).or_default().push((*e, *s));
        }
    }
    for (_, list) in out.side_sets.iter_mut() {
        let set: HashSet<(u32, u8)> = list.iter().copied().collect();
        let mut extra = Vec::new();
        for f in list.iter_mut() {
            if let Some(o) = outer_of.get(f) {
                *f = (*o, 6);
            }
        }
        for (id, side, [a, b]) in &lateral {
            if let Some(owners) = edge_faces.get(&((*a).min(*b), (*a).max(*b))) {
                if owners.iter().any(|o| set.contains(o)) {
                    extra.push((*id, *side));
                }
            }
        }
        list.extend(extra);
    }
    // Node sets: a face node in a set is replaced by its outermost copy if the set lay on the
    // chosen faces only, else all its copies join.
    for (_, nodes) in out.node_sets.iter_mut() {
        let mut add = Vec::new();
        for v in nodes.iter_mut() {
            if surf[0].contains_key(v) {
                let orig = *v;
                let rim = other.contains_key(&orig);
                *v = surf[0][&orig];
                if rim {
                    add.extend((1..=layers).map(|i| surf[i][&orig]));
                }
            }
        }
        nodes.extend(add);
        nodes.sort_unstable();
        nodes.dedup();
    }
    let bad = out.signed_volumes().iter().filter(|v| **v <= 0.0).count();
    if bad > 0 {
        return Err(format!("{bad} elements turned inside out: the layers ({total:.4} thick in all) are thicker than the elements beside them"));
    }
    Ok(out)
}
