//! Surface operations: reverse the sense of a surface, extrude lines and surfaces along a vector
//! or round an axis, make ribbons of polylines, subdivide triangle meshes (linear and Loop), curvature
//! of a surface, and the Delaunay triangulation of points in a plane.

use anyhow::{bail, Result};
use aurafx_data::*;
use std::collections::HashMap;

fn poly_of(ds: &DataSet) -> Result<PolyData> {
    Ok(match ds {
        DataSet::PolyData(p) => p.clone(),
        other => crate::surface::extract_surface(other)?,
    })
}

fn pts(pd: &PolyData) -> Vec<[f64; 3]> {
    (0..pd.points.len()).map(|i| pd.points.get(i)).collect()
}

fn from_parts(points: Vec<[f64; 3]>, polys: Vec<Vec<u32>>) -> PolyData {
    let mut out = PolyData { points: Points::new_f64(points.into_iter().flatten().collect()), ..Default::default() };
    for c in polys {
        let ty = match c.len() {
            3 => CellType::Triangle,
            4 => CellType::Quad,
            _ => CellType::Polygon,
        };
        out.polys.push(ty, &c);
    }
    out
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = len(a);
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        a
    }
}

/// Reverse Sense: the vertex order of every polygon (so the faces look the other way), and the
/// sign of the vector arrays named `Normals`.
pub fn reverse_sense(ds: &DataSet, flip_normals: bool) -> Result<PolyData> {
    let mut pd = poly_of(ds)?;
    let mut polys = Cells::new();
    for i in 0..pd.polys.len() {
        let c: Vec<u32> = pd.polys.cell(i).iter().rev().copied().collect();
        let ty = pd.polys.cell_type(i);
        polys.push(ty, &c);
    }
    pd.polys = polys;
    if flip_normals {
        for attrs in [&mut pd.point_data, &mut pd.cell_data] {
            let names: Vec<String> = attrs.iter().filter(|a| a.ncomp == 3 && a.name.to_lowercase().contains("normal")).map(|a| a.name.clone()).collect();
            for n in names {
                if let Some(a) = attrs.get(&n) {
                    let v: Vec<f64> = (0..a.len()).flat_map(|i| (0..3).map(move |c| -a.get(i, c))).collect();
                    attrs.insert(Array::new(n.clone(), 3, ArrayData::F64(v)));
                }
            }
        }
    }
    Ok(pd)
}

/// The edges used by exactly one polygon (the boundary), as ordered pairs in the polygon's sense.
fn boundary_edges(pd: &PolyData) -> Vec<(u32, u32)> {
    let mut count: HashMap<(u32, u32), (u32, (u32, u32))> = HashMap::new();
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        for k in 0..c.len() {
            let (a, b) = (c[k], c[(k + 1) % c.len()]);
            let e = count.entry((a.min(b), a.max(b))).or_insert((0, (a, b)));
            e.0 += 1;
        }
    }
    let mut out: Vec<(u32, u32)> = count.into_values().filter(|(n, _)| *n == 1).map(|(_, e)| e).collect();
    out.sort_unstable();
    out
}

/// Linear Extrusion: lines become strips of quads swept along `vector` (scaled by `factor`);
/// polygons become closed prisms (the surface: bottom, top and the walls of the boundary).
pub fn linear_extrusion(ds: &DataSet, vector: [f64; 3], factor: f64, capping: bool) -> Result<PolyData> {
    let pd = poly_of(ds)?;
    let p = pts(&pd);
    let n = p.len() as u32;
    let v = mul(vector, factor);
    let mut points = p.clone();
    points.extend(p.iter().map(|q| add(*q, v)));
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for i in 0..pd.lines.len() {
        let c = pd.lines.cell(i);
        for w in c.windows(2) {
            polys.push(vec![w[0], w[1], n + w[1], n + w[0]]);
        }
    }
    if pd.polys.len() > 0 {
        if capping {
            for i in 0..pd.polys.len() {
                let c = pd.polys.cell(i);
                polys.push(c.iter().rev().copied().collect());
                polys.push(c.iter().map(|x| n + x).collect());
            }
        }
        for (a, b) in boundary_edges(&pd) {
            polys.push(vec![a, b, n + b, n + a]);
        }
    }
    if polys.is_empty() {
        bail!("Linear Extrusion needs lines or polygons");
    }
    Ok(from_parts(points, polys))
}

fn rotate(p: [f64; 3], axis: [f64; 3], origin: [f64; 3], ang: f64) -> [f64; 3] {
    let k = unit(axis);
    let v = sub(p, origin);
    let (c, s) = (ang.cos(), ang.sin());
    let r = add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)));
    add(r, origin)
}

/// Rotational Extrusion: lines (and the boundary of polygons) swept round `axis` through
/// `origin` by `angle` degrees in `steps` steps, with `translation` along the axis over the sweep
/// and the radius scaled by `delta_radius` per turn (a screw or a cone).
#[allow(clippy::too_many_arguments)]
pub fn rotational_extrusion(
    ds: &DataSet,
    angle_deg: f64,
    steps: usize,
    axis: [f64; 3],
    origin: [f64; 3],
    translation: f64,
    delta_radius: f64,
) -> Result<PolyData> {
    let pd = poly_of(ds)?;
    let p = pts(&pd);
    let n = p.len() as u32;
    let steps = steps.max(1);
    let k = unit(axis);
    let mut points = Vec::with_capacity(p.len() * (steps + 1));
    for s in 0..=steps {
        let t = s as f64 / steps as f64;
        for q in &p {
            let v = sub(*q, origin);
            let along = dot(k, v);
            let radial = sub(v, mul(k, along));
            let scaled = add(mul(k, along + translation * t), mul(radial, 1.0 + delta_radius * t * (angle_deg / 360.0)));
            points.push(rotate(add(origin, scaled), axis, origin, (angle_deg * t).to_radians()));
        }
    }
    let mut segs: Vec<(u32, u32)> = Vec::new();
    for i in 0..pd.lines.len() {
        for w in pd.lines.cell(i).windows(2) {
            segs.push((w[0], w[1]));
        }
    }
    segs.extend(boundary_edges(&pd));
    if segs.is_empty() {
        bail!("Rotational Extrusion needs lines or polygons");
    }
    let mut polys = Vec::new();
    for s in 0..steps as u32 {
        for (a, b) in &segs {
            polys.push(vec![s * n + a, s * n + b, (s + 1) * n + b, (s + 1) * n + a]);
        }
    }
    Ok(from_parts(points, polys))
}

/// Ribbon: each polyline becomes a strip `width` wide, turned about the line to face `normal`
/// (a default that is not along the line).
pub fn ribbon(ds: &DataSet, width: f64, normal: [f64; 3]) -> Result<PolyData> {
    let pd = poly_of(ds)?;
    let p = pts(&pd);
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for i in 0..pd.lines.len() {
        let c = pd.lines.cell(i);
        if c.len() < 2 {
            continue;
        }
        let base = points.len() as u32;
        for (j, &id) in c.iter().enumerate() {
            let t = unit(sub(p[c[(j + 1).min(c.len() - 1)] as usize], p[c[j.saturating_sub(1)] as usize]));
            let mut side = cross(t, normal);
            if len(side) < 1e-9 {
                side = cross(t, if t[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] });
            }
            let side = mul(unit(side), width * 0.5);
            points.push(sub(p[id as usize], side));
            points.push(add(p[id as usize], side));
        }
        for j in 0..c.len() as u32 - 1 {
            let (a, b) = (base + 2 * j, base + 2 * j + 2);
            polys.push(vec![a, b, b + 1, a + 1]);
        }
    }
    if polys.is_empty() {
        bail!("Ribbon needs polylines");
    }
    Ok(from_parts(points, polys))
}

fn triangles_of(pd: &PolyData) -> Vec<[u32; 3]> {
    let mut t = Vec::new();
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        for k in 1..c.len().saturating_sub(1) {
            t.push([c[0], c[k], c[k + 1]]);
        }
    }
    t
}

/// Subdivide: every triangle into four, `levels` times. `loop_scheme`: Loop's smoothing rules (the
/// surface becomes the smooth limit); otherwise the midpoints stay on the flat triangles.
pub fn subdivide(ds: &DataSet, levels: usize, loop_scheme: bool) -> Result<PolyData> {
    let pd = poly_of(ds)?;
    let mut p = pts(&pd);
    let mut t = triangles_of(&pd);
    if t.is_empty() {
        bail!("Subdivide needs a surface with polygons");
    }
    for _ in 0..levels {
        // Edge -> the triangles on it (to find the opposite vertices) and the new midpoint's index.
        let mut edges: HashMap<(u32, u32), (Vec<u32>, u32)> = HashMap::new();
        for tr in &t {
            for k in 0..3 {
                let (a, b, c) = (tr[k], tr[(k + 1) % 3], tr[(k + 2) % 3]);
                edges.entry((a.min(b), a.max(b))).or_insert((Vec::new(), 0)).0.push(c);
            }
        }
        let mut keys: Vec<(u32, u32)> = edges.keys().copied().collect();
        keys.sort_unstable();
        let n_old = p.len();
        let mut newp = p.clone();
        for (i, k) in keys.iter().enumerate() {
            let (opp, id) = edges.get_mut(k).unwrap();
            *id = (n_old + i) as u32;
            let (a, b) = (p[k.0 as usize], p[k.1 as usize]);
            let m = if loop_scheme && opp.len() == 2 {
                add(mul(add(a, b), 3.0 / 8.0), mul(add(p[opp[0] as usize], p[opp[1] as usize]), 1.0 / 8.0))
            } else {
                mul(add(a, b), 0.5)
            };
            newp.push(m);
        }
        if loop_scheme {
            let mut nbr: Vec<Vec<u32>> = vec![Vec::new(); n_old];
            let mut bnd: Vec<Vec<u32>> = vec![Vec::new(); n_old];
            for (k, (opp, _)) in &edges {
                nbr[k.0 as usize].push(k.1);
                nbr[k.1 as usize].push(k.0);
                if opp.len() == 1 {
                    bnd[k.0 as usize].push(k.1);
                    bnd[k.1 as usize].push(k.0);
                }
            }
            for v in 0..n_old {
                if nbr[v].is_empty() {
                    continue;
                }
                newp[v] = if bnd[v].len() == 2 {
                    add(mul(p[v], 0.75), mul(add(p[bnd[v][0] as usize], p[bnd[v][1] as usize]), 0.125))
                } else if bnd[v].is_empty() {
                    let n = nbr[v].len() as f64;
                    let beta = (0.625 - (0.375 + 0.25 * (std::f64::consts::TAU / n).cos()).powi(2)) / n;
                    let sum = nbr[v].iter().fold([0.0; 3], |s, j| add(s, p[*j as usize]));
                    add(mul(p[v], 1.0 - n * beta), mul(sum, beta))
                } else {
                    p[v]
                };
            }
        }
        let mid = |a: u32, b: u32| edges[&(a.min(b), a.max(b))].1;
        let mut nt = Vec::with_capacity(t.len() * 4);
        for tr in &t {
            let (a, b, c) = (tr[0], tr[1], tr[2]);
            let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
            nt.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        p = newp;
        t = nt;
    }
    Ok(from_parts(p, t.into_iter().map(|x| x.to_vec()).collect()))
}

/// Curvature of a triangle surface at its points: the Gaussian curvature by the angle deficit
/// over the point's third of its triangles' area (`mean = false`), or the mean curvature by the
/// cotangent Laplacian (`mean = true`; positive where the surface is convex seen from outside, with
/// outward-facing triangles). Returns the surface with the point array `Gauss_Curvature` or
/// `Mean_Curvature`.
pub fn curvature(ds: &DataSet, mean: bool) -> Result<PolyData> {
    let mut pd = poly_of(ds)?;
    let p = pts(&pd);
    let tris = triangles_of(&pd);
    let n = p.len();
    let mut area = vec![0.0; n];
    let mut angle = vec![0.0; n];
    let mut lap = vec![[0.0; 3]; n];
    let mut normal = vec![[0.0; 3]; n];
    let mut boundary = vec![false; n];
    for (a, b) in boundary_edges(&pd) {
        boundary[a as usize] = true;
        boundary[b as usize] = true;
    }
    for t in &tris {
        let q = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
        let nn = cross(sub(q[1], q[0]), sub(q[2], q[0]));
        let a = 0.5 * len(nn);
        for k in 0..3 {
            area[t[k] as usize] += a / 3.0;
            normal[t[k] as usize] = add(normal[t[k] as usize], nn);
            let (e1, e2) = (sub(q[(k + 1) % 3], q[k]), sub(q[(k + 2) % 3], q[k]));
            let c = dot(e1, e2);
            let s = len(cross(e1, e2));
            angle[t[k] as usize] += s.atan2(c);
            // The cotangent of this corner weighs the opposite edge.
            let cot = c / s.max(1e-300);
            let (i, j) = (t[(k + 1) % 3] as usize, t[(k + 2) % 3] as usize);
            let d = sub(q[(k + 2) % 3], q[(k + 1) % 3]);
            lap[i] = add(lap[i], mul(d, 0.5 * cot));
            lap[j] = add(lap[j], mul(d, -0.5 * cot));
        }
    }
    let vals: Vec<f64> = (0..n)
        .map(|i| {
            if area[i] <= 0.0 {
                return 0.0;
            }
            if mean {
                // Laplace-Beltrami of the position is -2 H n; n outward from the triangles' own sense.
                -0.5 * dot(lap[i], unit(normal[i])) / area[i]
            } else {
                let total = if boundary[i] { std::f64::consts::PI } else { std::f64::consts::TAU };
                (total - angle[i]) / area[i]
            }
        })
        .collect();
    pd.point_data.insert(Array::scalars_f64(if mean { "Mean_Curvature" } else { "Gauss_Curvature" }, vals));
    Ok(pd)
}

/// Delaunay 2D: the triangulation of the points projected onto the xy plane (Bowyer-Watson),
/// keeping the points' own z. With `alpha` > 0 the triangles whose circumradius exceeds it are
/// dropped (so holes and concavities appear).
pub fn delaunay_2d(ds: &DataSet, alpha: f64) -> Result<PolyData> {
    let src = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => poly_of(other)?,
    };
    let p = pts(&src);
    let n = p.len();
    if n < 3 {
        bail!("Delaunay 2D needs at least three points");
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in &p {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-12);
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    // A super triangle far outside the points.
    let m = 100.0 * size;
    let mut v: Vec<[f64; 2]> = p.iter().map(|q| [q[0], q[1]]).collect();
    v.extend([[c[0] - m, c[1] - m], [c[0] + m, c[1] - m], [c[0], c[1] + m]]);
    let mut tris: Vec<[usize; 3]> = vec![[n, n + 1, n + 2]];
    let circ = |t: &[usize; 3], v: &[[f64; 2]]| -> ([f64; 2], f64) {
        let (a, b, c) = (v[t[0]], v[t[1]], v[t[2]]);
        let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
        if d.abs() < 1e-300 {
            return ([0.0; 2], f64::INFINITY);
        }
        let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
        let ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
        let uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
        ([ux, uy], ((a[0] - ux).powi(2) + (a[1] - uy).powi(2)).sqrt())
    };
    // Points in a jittered-free but spatially sorted order keep the cavity searches short.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| v[*a][0].total_cmp(&v[*b][0]).then(v[*a][1].total_cmp(&v[*b][1])));
    for &i in &order {
        let q = v[i];
        let mut bad = Vec::new();
        for (ti, t) in tris.iter().enumerate() {
            let (cc, r) = circ(t, &v);
            if ((q[0] - cc[0]).powi(2) + (q[1] - cc[1]).powi(2)).sqrt() < r * (1.0 - 1e-12) {
                bad.push(ti);
            }
        }
        let mut edges: HashMap<(usize, usize), u32> = HashMap::new();
        for &ti in &bad {
            let t = tris[ti];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let mut keep: Vec<[usize; 3]> = Vec::new();
        for (ti, t) in tris.iter().enumerate() {
            if !bad.contains(&ti) {
                keep.push(*t);
            }
        }
        let mut hole: Vec<(usize, usize)> = edges.into_iter().filter(|(_, c)| *c == 1).map(|(e, _)| e).collect();
        hole.sort_unstable();
        for (a, b) in hole {
            keep.push([a, b, i]);
        }
        tris = keep;
    }
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for t in &tris {
        if t.iter().any(|x| *x >= n) {
            continue;
        }
        if alpha > 0.0 && circ(t, &v).1 > alpha {
            continue;
        }
        let (a, b, c) = (v[t[0]], v[t[1]], v[t[2]]);
        let ccw = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0;
        polys.push(if ccw { vec![t[0] as u32, t[1] as u32, t[2] as u32] } else { vec![t[0] as u32, t[2] as u32, t[1] as u32] });
    }
    Ok(from_parts(p, polys))
}

/// A plane's quadric (squared distance to it), as the upper triangle of a symmetric 4 x 4.
type Quadric = [f64; 10];

fn plane_quadric(n: [f64; 3], d: f64) -> Quadric {
    let (a, b, c) = (n[0], n[1], n[2]);
    [a * a, a * b, a * c, a * d, b * b, b * c, b * d, c * c, c * d, d * d]
}

fn q_add(a: &Quadric, b: &Quadric) -> Quadric {
    std::array::from_fn(|i| a[i] + b[i])
}

fn q_eval(q: &Quadric, v: [f64; 3]) -> f64 {
    let (x, y, z) = (v[0], v[1], v[2]);
    q[0] * x * x
        + 2.0 * q[1] * x * y
        + 2.0 * q[2] * x * z
        + 2.0 * q[3] * x
        + q[4] * y * y
        + 2.0 * q[5] * y * z
        + 2.0 * q[6] * y
        + q[7] * z * z
        + 2.0 * q[8] * z
        + q[9]
}

/// The point that minimizes the quadric, or `None` when it is (nearly) singular.
fn q_optimum(q: &Quadric) -> Option<[f64; 3]> {
    let m = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
    let b = [-q[3], -q[6], -q[8]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = |r: usize, c: usize| -> f64 {
        let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        (m[c1][r1] * m[c2][r2] - m[c1][r2] * m[c2][r1]) / det
    };
    Some(std::array::from_fn(|i| (0..3).map(|j| inv(i, j) * b[j]).sum()))
}

/// Decimate: quadric edge collapse of a triangle surface until `reduction` (0 to 0.99) of its
/// triangles are gone, keeping the shape (each collapse goes to the point nearest the planes of the
/// triangles around it) and refusing collapses that fold a triangle over. With `preserve_boundary` the
/// open edges of the surface stay.
pub fn decimate(ds: &DataSet, reduction: f64, preserve_boundary: bool) -> Result<PolyData> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashSet};
    let pd = poly_of(ds)?;
    let mut p = pts(&pd);
    let mut tris = triangles_of(&pd);
    if tris.is_empty() {
        bail!("Decimate needs a surface with polygons");
    }
    let n = p.len();
    let target = ((tris.len() as f64) * (1.0 - reduction.clamp(0.0, 0.99))).ceil() as usize;
    let mut alive = vec![true; tris.len()];
    let mut n_alive = tris.len();
    let mut q: Vec<Quadric> = vec![[0.0; 10]; n];
    let mut inc: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (ti, t) in tris.iter().enumerate() {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        let nn = cross(sub(b, a), sub(c, a));
        let l = len(nn);
        if l > 0.0 {
            let nu = mul(nn, 1.0 / l);
            let pq = plane_quadric(nu, -dot(nu, a));
            let w = 0.5 * l;
            for &v in t {
                q[v as usize] = q_add(&q[v as usize], &pq.map(|x| x * w));
            }
        }
        for &v in t {
            inc[v as usize].insert(ti);
        }
    }
    let mut boundary = vec![false; n];
    if preserve_boundary {
        for (a, b) in boundary_edges(&pd) {
            boundary[a as usize] = true;
            boundary[b as usize] = true;
        }
    }
    let mut ver = vec![0u32; n];
    let mut vert_alive = vec![true; n];
    // The best collapse of an edge: where the merged vertex goes and what it costs.
    let plan = |a: usize, b: usize, p: &[[f64; 3]], q: &[Quadric]| -> (f64, [f64; 3]) {
        let s = q_add(&q[a], &q[b]);
        let mut best = (f64::INFINITY, p[a]);
        for cand in [q_optimum(&s), Some(mul(add(p[a], p[b]), 0.5)), Some(p[a]), Some(p[b])].into_iter().flatten() {
            let c = q_eval(&s, cand);
            if c < best.0 {
                best = (c, cand);
            }
        }
        (best.0.max(0.0), best.1)
    };
    let key = |c: f64| Reverse(c.to_bits());
    let mut heap: BinaryHeap<(Reverse<u64>, usize, usize, u32)> = BinaryHeap::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for t in &tris {
        for k in 0..3 {
            let (a, b) = (t[k] as usize, t[(k + 1) % 3] as usize);
            if seen.insert((a.min(b), a.max(b))) {
                heap.push((key(plan(a, b, &p, &q).0), a, b, 0));
            }
        }
    }
    while n_alive > target {
        let Some((_, a, b, stamp)) = heap.pop() else { break };
        if !vert_alive[a] || !vert_alive[b] || ver[a] + ver[b] != stamp || (preserve_boundary && (boundary[a] || boundary[b])) {
            continue;
        }
        let shared: Vec<usize> = inc[a].intersection(&inc[b]).copied().collect();
        if shared.is_empty() {
            continue;
        }
        let (_, pos) = plan(a, b, &p, &q);
        // No triangle around either vertex may turn over or collapse to a sliver.
        let ok = inc[a].iter().chain(inc[b].iter()).filter(|t| alive[**t] && !shared.contains(t)).all(|&ti| {
            let t = tris[ti];
            let (before, after): (Vec<[f64; 3]>, Vec<[f64; 3]>) = t
                .iter()
                .map(|&v| {
                    let v = v as usize;
                    (p[v], if v == a || v == b { pos } else { p[v] })
                })
                .unzip();
            let (n0, n1) = (cross(sub(before[1], before[0]), sub(before[2], before[0])), cross(sub(after[1], after[0]), sub(after[2], after[0])));
            len(n1) > 1e-12 * (1.0 + len(n0)) && dot(n0, n1) > 0.2 * len(n0) * len(n1)
        });
        if !ok {
            continue;
        }
        // Collapse a into b at `pos`.
        p[b] = pos;
        q[b] = q_add(&q[a], &q[b]);
        vert_alive[a] = false;
        let moved: Vec<usize> = inc[a].iter().copied().collect();
        for ti in moved {
            if shared.contains(&ti) {
                alive[ti] = false;
                n_alive -= 1;
                for &v in &tris[ti] {
                    inc[v as usize].remove(&ti);
                }
            } else {
                for v in tris[ti].iter_mut() {
                    if *v as usize == a {
                        *v = b as u32;
                    }
                }
                inc[b].insert(ti);
            }
        }
        inc[a].clear();
        ver[b] += 1;
        let mut nbrs: HashSet<usize> = HashSet::new();
        for &ti in &inc[b] {
            for &v in &tris[ti] {
                if v as usize != b {
                    nbrs.insert(v as usize);
                }
            }
        }
        for v in nbrs {
            ver[v] += 0;
            heap.push((key(plan(b, v, &p, &q).0), b, v, ver[b] + ver[v]));
        }
    }
    let mut remap = vec![u32::MAX; n];
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for (ti, t) in tris.iter().enumerate() {
        if !alive[ti] || t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            continue;
        }
        let ids: Vec<u32> = t
            .iter()
            .map(|&v| {
                if remap[v as usize] == u32::MAX {
                    remap[v as usize] = points.len() as u32;
                    points.push(p[v as usize]);
                }
                remap[v as usize]
            })
            .collect();
        polys.push(ids);
    }
    Ok(from_parts(points, polys))
}
