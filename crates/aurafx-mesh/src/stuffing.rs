//! Tetrahedral meshing of implicit geometry by isosurface stuffing (after Labelle and
//! Shewchuk, 2007): a body-centred cubic lattice of tetrahedra is laid over the shape, lattice
//! nodes close to the surface are moved onto it (so no element is cut thin), and the
//! tetrahedra the surface crosses are cut and their inside parts split into tetrahedra
//! (coned from their lowest-numbered vertex, so neighbours agree on shared faces).
//! Then local refinement: chosen tetrahedra split into eight, their neighbours closed
//! with a centre point, new nodes on the surface put back onto it.

use crate::hex::P3;
use crate::tet::{orient, TetMesh, TET_SIDES};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]
}

/// Where along a -> b (values fa < 0 < fb or the reverse) the function vanishes.
fn root(f: &(dyn Fn(P3) -> f64 + Sync), a: P3, b: P3, fa: f64, fb: f64) -> f64 {
    let (mut lo, mut hi, mut flo) = (0.0, 1.0, fa);
    let _ = fb;
    for _ in 0..48 {
        let m = 0.5 * (lo + hi);
        let fm = f(lerp(a, b, m));
        if (fm < 0.0) == (flo < 0.0) && fm != 0.0 {
            lo = m;
            flo = fm;
        } else {
            hi = m;
        }
    }
    0.5 * (lo + hi)
}

/// Isosurface stuffing of the region where `f` < 0 inside the box `lo`..`hi` (which should
/// reach a cell beyond the surface), lattice spacing `h`.
pub fn stuff(f: &(dyn Fn(P3) -> f64 + Sync), lo: P3, hi: P3, h: f64) -> Result<TetMesh, String> {
    let n: [usize; 3] = std::array::from_fn(|a| (((hi[a] - lo[a]) / h).ceil() as usize).max(1));
    let ng = (n[0] + 1) * (n[1] + 1) * (n[2] + 1);
    let nc = n[0] * n[1] * n[2];
    if ng + nc > 60_000_000 {
        return Err("more than 60 million lattice nodes: make the elements larger".into());
    }
    let gid = |i: usize, j: usize, k: usize| i + (n[0] + 1) * (j + (n[1] + 1) * k);
    let cid = |i: usize, j: usize, k: usize| ng + i + n[0] * (j + n[1] * k);
    let pos = |v: usize| -> P3 {
        if v < ng {
            let (i, j, k) = (v % (n[0] + 1), (v / (n[0] + 1)) % (n[1] + 1), v / ((n[0] + 1) * (n[1] + 1)));
            [lo[0] + i as f64 * h, lo[1] + j as f64 * h, lo[2] + k as f64 * h]
        } else {
            let c = v - ng;
            let (i, j, k) = (c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1]));
            [lo[0] + (i as f64 + 0.5) * h, lo[1] + (j as f64 + 0.5) * h, lo[2] + (k as f64 + 0.5) * h]
        }
    };
    // The lattice tetrahedra: for every face shared by two cubes, the two centres and each
    // pair of consecutive corners of the face.
    let mut tets: Vec<[u32; 4]> = Vec::new();
    for a in 0..3 {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    let q = [i, j, k];
                    if q[a] + 1 >= n[a] {
                        continue;
                    }
                    let mut q2 = q;
                    q2[a] += 1;
                    let (c1, c2) = (cid(q[0], q[1], q[2]), cid(q2[0], q2[1], q2[2]));
                    // Face corners at plane q[a] + 1 along a.
                    let corner = |db: usize, dc: usize| -> usize {
                        let mut r = q;
                        r[a] += 1;
                        r[b] += db;
                        r[c] += dc;
                        gid(r[0], r[1], r[2])
                    };
                    let ring = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)];
                    for t in 0..4 {
                        tets.push([c1 as u32, c2 as u32, ring[t] as u32, ring[(t + 1) % 4] as u32]);
                    }
                }
            }
        }
    }
    // Values at the lattice nodes, only near the region (tetrahedra with a corner below a
    // cell's distance are kept).
    let mut used = vec![false; ng + nc];
    for t in &tets {
        for v in t {
            used[*v as usize] = true;
        }
    }
    let mut pts: Vec<P3> = (0..ng + nc).map(pos).collect();
    let mut val: Vec<f64> = (0..ng + nc).into_par_iter().map(|v| if used[v] { f(pos(v)) } else { f64::INFINITY }).collect();
    // Lattice edges: (a, b) with a < b, and whether long (black: between centres or grid
    // nodes) or short (red: centre to corner).
    let mut edges: BTreeMap<(u32, u32), bool> = BTreeMap::new();
    for t in &tets {
        for (x, y) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
            let (p, q) = (t[x].min(t[y]), t[x].max(t[y]));
            let long = (p as usize >= ng) == (q as usize >= ng);
            edges.insert((p, q), long);
        }
    }
    let crossing = |val: &[f64], e: &(u32, u32)| -> bool {
        let (fa, fb) = (val[e.0 as usize], val[e.1 as usize]);
        (fa < 0.0 && fb > 0.0) || (fa > 0.0 && fb < 0.0)
    };
    // Cut points of the crossing edges (parameter from the edge's first node).
    let cut: HashMap<(u32, u32), f64> = edges
        .keys()
        .filter(|e| crossing(&val, e))
        .collect::<Vec<_>>()
        .par_iter()
        .map(|e| {
            let (a, b) = (pts[e.0 as usize], pts[e.1 as usize]);
            (**e, root(f, a, b, val[e.0 as usize], val[e.1 as usize]))
        })
        .collect();
    // Warping: a node within alpha of a cut point on one of its edges moves onto the nearest
    // such cut point (alpha 0.24999 on long edges, 0.41189 on short ones) and lies on the
    // surface from then on.
    let mut by_node: HashMap<u32, Vec<((u32, u32), bool)>> = HashMap::new();
    for (e, long) in &edges {
        if cut.contains_key(e) {
            by_node.entry(e.0).or_default().push((*e, *long));
            by_node.entry(e.1).or_default().push((*e, *long));
        }
    }
    let mut nodes: Vec<u32> = by_node.keys().copied().collect();
    nodes.sort_unstable();
    for v in nodes {
        if val[v as usize] == 0.0 {
            continue;
        }
        let mut best: Option<(f64, P3)> = None;
        for (e, long) in &by_node[&v] {
            if !crossing(&val, e) {
                continue;
            }
            let t = cut[e];
            let frac = if e.0 == v { t } else { 1.0 - t };
            let alpha = if *long { 0.24999 } else { 0.41189 };
            if frac < alpha && best.is_none_or(|b| frac < b.0) {
                best = Some((frac, lerp(pts[e.0 as usize], pts[e.1 as usize], t)));
            }
        }
        if let Some((_, p)) = best {
            pts[v as usize] = p;
            val[v as usize] = 0.0;
        }
    }
    // Ids: lattice nodes keep theirs; cut points get ng + nc + edge index.
    let base = (ng + nc) as u32;
    let mut cut_id: HashMap<(u32, u32), u32> = HashMap::new();
    let mut extra: Vec<P3> = Vec::new();
    // (In edge order, so the numbering and the cuts are the same every run.)
    for e in edges.keys() {
        if let Some(t) = cut.get(e) {
            if crossing(&val, e) {
                cut_id.insert(*e, base + extra.len() as u32);
                extra.push(lerp(pts[e.0 as usize], pts[e.1 as usize], *t));
            }
        }
    }
    let cut_point = |e: (u32, u32)| -> u32 { cut_id[&e] };
    let all: Vec<P3> = pts.iter().copied().chain(extra.iter().copied()).collect();
    let sign = |v: u32| -> i8 {
        let x = val[v as usize];
        if x < 0.0 {
            -1
        } else if x > 0.0 {
            1
        } else {
            0
        }
    };
    let mut out_tets: Vec<[u32; 4]> = Vec::new();
    for t in &tets {
        let s = t.map(sign);
        if !s.contains(&-1) {
            continue;
        }
        if !s.contains(&1) {
            out_tets.push(*t);
            continue;
        }
        // The inside part: faces of the cut tetrahedron (each original face clipped, plus the
        // cap of cut points and surface nodes).
        let faces3 = [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];
        let mut polys: Vec<Vec<u32>> = Vec::new();
        let mut cap: Vec<u32> = Vec::new();
        for fc in faces3 {
            let mut poly = Vec::new();
            for k in 0..3 {
                let (a, b) = (t[fc[k]], t[fc[(k + 1) % 3]]);
                if sign(a) <= 0 {
                    poly.push(a);
                }
                let e = (a.min(b), a.max(b));
                if crossing(&val, &e) {
                    poly.push(cut_point(e));
                }
            }
            if poly.len() >= 3 {
                polys.push(poly);
            }
        }
        for v in t {
            if sign(*v) == 0 {
                cap.push(*v);
            }
        }
        for (x, y) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
            let e = (t[x].min(t[y]), t[x].max(t[y]));
            if crossing(&val, &e) {
                cap.push(cut_point(e));
            }
        }
        let at = |v: u32| all[v as usize];
        // Order the cap around its centre.
        if cap.len() >= 3 {
            let cen: P3 = std::array::from_fn(|a| cap.iter().map(|v| at(*v)[a]).sum::<f64>() / cap.len() as f64);
            let nrm = {
                let mut nn = [0.0; 3];
                for k in 0..cap.len() {
                    let c = cross(sub(at(cap[k]), cen), sub(at(cap[(k + 1) % cap.len()]), cen));
                    for a in 0..3 {
                        nn[a] += c[a];
                    }
                }
                // Any normal will do as long as the ordering is consistent; use a stable one.
                let p0 = sub(at(cap[0]), cen);
                let p1 = sub(at(cap[1]), cen);
                let c = cross(p0, p1);
                if dot(c, c) > dot(nn, nn) { c } else { nn }
            };
            let u = {
                let p = sub(at(cap[0]), cen);
                let l = dot(p, p).sqrt().max(1e-300);
                p.map(|x| x / l)
            };
            let w = cross(nrm, u);
            cap.sort_by(|a, b| {
                let (pa, pb) = (sub(at(*a), cen), sub(at(*b), cen));
                dot(pa, w).atan2(dot(pa, u)).total_cmp(&dot(pb, w).atan2(dot(pb, u)))
            });
            polys.push(cap.clone());
        }
        // Cone from the lowest vertex over the faces without it, each fanned from its lowest.
        let v0 = *polys.iter().flatten().min().unwrap();
        for poly in &polys {
            if poly.contains(&v0) {
                continue;
            }
            let k0 = (0..poly.len()).min_by_key(|k| poly[*k]).unwrap();
            for k in 1..poly.len() - 1 {
                let (a, b) = (poly[(k0 + k) % poly.len()], poly[(k0 + k + 1) % poly.len()]);
                let mut tet = [v0, poly[k0], a, b];
                let vol = orient(&all, tet);
                if vol.abs() < 1e-12 * h * h * h {
                    continue;
                }
                if vol < 0.0 {
                    tet.swap(2, 3);
                }
                out_tets.push(tet);
            }
        }
    }
    // Compact the nodes used.
    let mut map: HashMap<u32, u32> = HashMap::new();
    let mut m = TetMesh::default();
    for t in out_tets.iter_mut() {
        for v in t.iter_mut() {
            *v = *map.entry(*v).or_insert_with(|| {
                m.points.push(all[*v as usize]);
                (m.points.len() - 1) as u32
            });
        }
        if orient(&m.points, *t) < 0.0 {
            t.swap(2, 3);
        }
    }
    m.tets = out_tets;
    m.block = vec![1; m.tets.len()];
    m.block_names.insert(1, "solid".into());
    if m.tets.is_empty() {
        return Err("no part of the shape lies inside the box".into());
    }
    Ok(m)
}

/// Local refinement: the tetrahedra `marked` split into eight; each neighbour with split edges
/// gets a centre point joined to its faces (faces with one split edge in two triangles, two in
/// three, three in four), so the mesh stays conforming. New nodes on the boundary go through
/// `project` (onto the surface). Side sets follow the faces; node sets gain the new nodes.
pub fn refine_marked(m: &TetMesh, marked: &[bool], project: &dyn Fn(P3) -> P3) -> TetMesh {
    let mut out = TetMesh { points: m.points.clone(), block_names: m.block_names.clone(), ..Default::default() };
    // Edges to split: every edge of a marked tetrahedron.
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let edges_of = |t: &[u32; 4]| [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)].map(|(x, y)| key(t[x], t[y]));
    let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
    let bnd: std::collections::HashSet<u32> = m.boundary_faces().iter().flat_map(|(e, s)| m.side_corners(*e as usize, *s)).collect();
    // Edges whose both ends are on the boundary and that lie on a boundary face.
    let bface: std::collections::HashSet<(u32, u32)> = m.boundary_faces().iter().flat_map(|(e, s)| {
        let c = m.side_corners(*e as usize, *s);
        [key(c[0], c[1]), key(c[1], c[2]), key(c[0], c[2])]
    }).collect();
    for (e, t) in m.tets.iter().enumerate() {
        if marked[e] {
            for k in edges_of(t) {
                mid.entry(k).or_insert_with(|| {
                    let (a, b) = (m.points[k.0 as usize], m.points[k.1 as usize]);
                    let mut p = lerp(a, b, 0.5);
                    if bface.contains(&k) && bnd.contains(&k.0) && bnd.contains(&k.1) {
                        p = project(p);
                    }
                    out.points.push(p);
                    (out.points.len() - 1) as u32
                });
            }
        }
    }
    let mut parent = Vec::new();
    // (parent element, child, child side) on each parent side.
    let mut side_children: HashMap<(u32, u8), Vec<(u32, u8)>> = HashMap::new();
    for (e, t) in m.tets.iter().enumerate() {
        let split: Vec<(u32, u32)> = edges_of(t).into_iter().filter(|k| mid.contains_key(k)).collect();
        let first_child = out.tets.len();
        if split.is_empty() {
            out.tets.push(*t);
        } else if split.len() == 6 {
            let mm = |a: usize, b: usize| mid[&key(t[a], t[b])];
            let (m01, m02, m03, m12, m13, m23) = (mm(0, 1), mm(0, 2), mm(0, 3), mm(1, 2), mm(1, 3), mm(2, 3));
            let mut kids = vec![[t[0], m01, m02, m03], [m01, t[1], m12, m13], [m02, m12, t[2], m23], [m03, m13, m23, t[3]]];
            let d2 = |a: u32, b: u32| {
                let d = sub(out.points[a as usize], out.points[b as usize]);
                dot(d, d)
            };
            let diags = [(m01, m23, [m02, m03, m13, m12]), (m02, m13, [m01, m12, m23, m03]), (m03, m12, [m01, m13, m23, m02])];
            let d = diags.iter().min_by(|a, b| d2(a.0, a.1).total_cmp(&d2(b.0, b.1))).unwrap();
            for k in 0..4 {
                kids.push([d.0, d.1, d.2[k], d.2[(k + 1) % 4]]);
            }
            for c in kids {
                out.tets.push(c);
            }
        } else {
            // Closure: a centre point coned over the split faces.
            let cen: P3 = std::array::from_fn(|a| t.iter().map(|v| m.points[*v as usize][a]).sum::<f64>() / 4.0);
            out.points.push(cen);
            let c = (out.points.len() - 1) as u32;
            for s in TET_SIDES {
                let f = s.map(|k| t[k]);
                for tri in face_triangles(f, &mid) {
                    out.tets.push([tri[0], tri[1], tri[2], c]);
                }
            }
        }
        for k in first_child..out.tets.len() {
            if orient(&out.points, out.tets[k]) < 0.0 {
                out.tets[k].swap(2, 3);
            }
            out.block.push(m.block[e]);
            parent.push(e);
        }
        // Children faces lying on each parent face.
        for (si, s) in TET_SIDES.iter().enumerate() {
            let f = s.map(|k| t[k]);
            let mut on: Vec<u32> = f.to_vec();
            for (a, b) in [(0, 1), (1, 2), (0, 2)] {
                if let Some(x) = mid.get(&key(f[a], f[b])) {
                    on.push(*x);
                }
            }
            for k in first_child..out.tets.len() {
                for cs in 1..=4u8 {
                    if out.side_corners(k, cs).iter().all(|n| on.contains(n)) {
                        side_children.entry((e as u32, si as u8 + 1)).or_default().push((k as u32, cs));
                    }
                }
            }
        }
    }
    for (name, v) in &m.elem_fields {
        out.elem_fields.push((name.clone(), parent.iter().map(|p| v[*p]).collect()));
    }
    for (name, faces) in &m.side_sets {
        let nf: Vec<(u32, u8)> = faces.iter().flat_map(|f| side_children.get(f).cloned().unwrap_or_default()).collect();
        out.side_sets.push((name.clone(), nf));
    }
    for (name, nodes) in &m.node_sets {
        let set: std::collections::HashSet<u32> = nodes.iter().copied().collect();
        let mut v = nodes.clone();
        v.extend(mid.iter().filter(|(k, _)| set.contains(&k.0) && set.contains(&k.1)).map(|(_, x)| *x));
        v.sort_unstable();
        out.node_sets.push((name.clone(), v));
    }
    out
}

/// Triangles of a face (a, b, c) with midpoints on some of its edges: none, as is; one, two
/// triangles from the midpoint; two, three triangles (the quadrilateral cut through its
/// lowest-numbered node); three, the four of the regular split.
fn face_triangles(f: [u32; 3], mid: &HashMap<(u32, u32), u32>) -> Vec<[u32; 3]> {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let m = [mid.get(&key(f[0], f[1])).copied(), mid.get(&key(f[1], f[2])).copied(), mid.get(&key(f[2], f[0])).copied()];
    let count = m.iter().filter(|x| x.is_some()).count();
    match count {
        0 => vec![f],
        3 => {
            let (a, b, c) = (m[0].unwrap(), m[1].unwrap(), m[2].unwrap());
            vec![[f[0], a, c], [a, f[1], b], [c, b, f[2]], [a, b, c]]
        }
        1 => {
            // Rotate so the split edge is (f0, f1).
            let k = m.iter().position(|x| x.is_some()).unwrap();
            let g = [f[k], f[(k + 1) % 3], f[(k + 2) % 3]];
            let x = m[k].unwrap();
            vec![[g[0], x, g[2]], [x, g[1], g[2]]]
        }
        _ => {
            // Two split edges: the corner between them is cut off; the remaining
            // quadrilateral is cut through its lowest-numbered node.
            let k = m.iter().position(|x| x.is_none()).unwrap();
            // Unsplit edge (g0, g1); split edges (g1, g2) with x and (g2, g0) with y.
            let g = [f[k], f[(k + 1) % 3], f[(k + 2) % 3]];
            let x = m[(k + 1) % 3].unwrap();
            let y = m[(k + 2) % 3].unwrap();
            let mut tris = vec![[y, x, g[2]]];
            let quad = [g[0], g[1], x, y];
            let q0 = (0..4).min_by_key(|i| quad[*i]).unwrap();
            let q = [quad[q0], quad[(q0 + 1) % 4], quad[(q0 + 2) % 4], quad[(q0 + 3) % 4]];
            tris.push([q[0], q[1], q[2]]);
            tris.push([q[0], q[2], q[3]]);
            tris
        }
    }
}
