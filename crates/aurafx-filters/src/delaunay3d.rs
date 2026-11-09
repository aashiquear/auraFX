//! Delaunay 3D: the points of a dataset joined into tetrahedra so that no point lies inside the
//! sphere through the corners of any tetrahedron (Bowyer–Watson insertion: each point empties the
//! tetrahedra whose spheres contain it and fills the hole with tetrahedra to it; the convex hull is
//! kept with a symbolic vertex at infinity). Points that lie
//! exactly on a common sphere (a regular grid) are moved by a tiny amount while triangulating, so
//! the result is a valid tetrahedral mesh of their convex hull; the output keeps the true positions.
//! An alpha value removes the tetrahedra whose sphere is larger than it (concavities, holes).

use anyhow::{bail, Result};
use aurafx_data::*;

const NONE: usize = usize::MAX;

struct Tet {
    v: [usize; 4],
    /// Neighbour across the face opposite vertex i.
    nb: [usize; 4],
    alive: bool,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn orient(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> f64 {
    let (u, v, w) = (sub(b, a), sub(c, a), sub(d, a));
    u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0])
}

/// The in-sphere determinant of e against a, b, c, d (its sign, times the orientation's, says whether
/// e is inside their sphere); computed from the lifted coordinates relative to e, which stays
/// accurate for nearly flat tetrahedra where a sphere's center does not.
fn insphere(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3], e: [f64; 3]) -> f64 {
    let r = |p: [f64; 3]| {
        let q = sub(p, e);
        [q[0], q[1], q[2], q[0] * q[0] + q[1] * q[1] + q[2] * q[2]]
    };
    let m = [r(a), r(b), r(c), r(d)];
    let det3 =
        |x: [f64; 3], y: [f64; 3], z: [f64; 3]| x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
    let col = |row: [f64; 4], skip: usize| -> [f64; 3] {
        let mut o = [0.0; 3];
        let mut k = 0;
        for (j, v) in row.iter().enumerate() {
            if j != skip {
                o[k] = *v;
                k += 1;
            }
        }
        o
    };
    // Expansion along the last column.
    let mut det = 0.0;
    for i in 0..4 {
        let rows: Vec<[f64; 3]> = (0..4).filter(|k| *k != i).map(|k| col(m[k], 3)).collect();
        let minor = det3(rows[0], rows[1], rows[2]);
        let sign = if (i + 3) % 2 == 0 { 1.0 } else { -1.0 };
        det += sign * m[i][3] * minor;
    }
    det
}

/// The circumsphere of four points: center and squared radius.
fn sphere(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> ([f64; 3], f64) {
    let (u, v, w) = (sub(b, a), sub(c, a), sub(d, a));
    let (uu, vv, ww) = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2], v[0] * v[0] + v[1] * v[1] + v[2] * v[2], w[0] * w[0] + w[1] * w[1] + w[2] * w[2]);
    let cr = |p: [f64; 3], q: [f64; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    let (vw, wu, uv) = (cr(v, w), cr(w, u), cr(u, v));
    let det = 2.0 * (u[0] * vw[0] + u[1] * vw[1] + u[2] * vw[2]);
    if det.abs() < 1e-300 {
        return ([f64::INFINITY; 3], f64::INFINITY);
    }
    let o: [f64; 3] = std::array::from_fn(|k| (uu * vw[k] + vv * wu[k] + ww * uv[k]) / det);
    ([a[0] + o[0], a[1] + o[1], a[2] + o[2]], o[0] * o[0] + o[1] * o[1] + o[2] * o[2])
}

/// Delaunay tetrahedra of `pts`, as index quadruples (positively oriented), with alpha filtering.
///
/// The hull is handled with one symbolic vertex at infinity (index `n`): a tetrahedron holding it
/// stands for a face of the convex hull, and a new point conflicts with it when it lies beyond
/// that face. So no large enclosing tetrahedron is needed and the hull comes out complete.
pub fn tetrahedralize(pts: &[[f64; 3]], alpha: f64) -> Result<Vec<[usize; 4]>> {
    let n = pts.len();
    if n < 4 {
        bail!("Delaunay 3D needs at least four points");
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max).max(1e-12);
    if (0..3).any(|k| hi[k] - lo[k] < 1e-9 * size) {
        bail!("Delaunay 3D: the points lie in a plane (use Delaunay 2D)");
    }
    // A tiny, reproducible jitter breaks exact cosphericity (grids).
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let v: Vec<[f64; 3]> = pts.iter().map(|p| [p[0] + 1e-9 * size * rnd(), p[1] + 1e-9 * size * rnd(), p[2] + 1e-9 * size * rnd()]).collect();
    let inf = n;
    let d2 = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    // The first tetrahedron: four well-spread points.
    let i0 = 0;
    let i1 = (0..n).max_by(|a, b| d2(v[*a], v[i0]).total_cmp(&d2(v[*b], v[i0]))).unwrap();
    let tri = |k: usize| {
        let (u, w) = (sub(v[i1], v[i0]), sub(v[k], v[i0]));
        (u[1] * w[2] - u[2] * w[1]).powi(2) + (u[2] * w[0] - u[0] * w[2]).powi(2) + (u[0] * w[1] - u[1] * w[0]).powi(2)
    };
    let i2 = (0..n).max_by(|a, b| tri(*a).total_cmp(&tri(*b))).unwrap();
    let i3 = (0..n).max_by(|a, b| orient(v[i0], v[i1], v[i2], v[*a]).abs().total_cmp(&orient(v[i0], v[i1], v[i2], v[*b]).abs())).unwrap();
    if orient(v[i0], v[i1], v[i2], v[i3]).abs() < 1e-12 * size.powi(3) {
        bail!("Delaunay 3D: the points lie in a plane (use Delaunay 2D)");
    }
    let mut first = [i0, i1, i2, i3];
    if orient(v[i0], v[i1], v[i2], v[i3]) < 0.0 {
        first.swap(0, 1);
    }
    let mk = |_v: &[[f64; 3]], q: [usize; 4]| -> Tet { Tet { v: q, nb: [NONE; 4], alive: true } };
    let mut tets: Vec<Tet> = vec![mk(&v, first)];
    // One infinite tetrahedron per face: the face's corners with infinity in the place of the
    // opposite corner, two corners swapped so that "beyond the face" is the positive side.
    for i in 0..4 {
        let mut q = first;
        q[i] = inf;
        let (a, b) = if i == 0 {
            (1, 2)
        } else if i == 1 {
            (0, 2)
        } else {
            (0, 1)
        };
        q.swap(a, b);
        tets.push(mk(&v, q));
    }
    // Neighbours of the start: match faces by their sorted corners.
    {
        let mut faces: std::collections::HashMap<[usize; 3], (usize, usize)> = std::collections::HashMap::new();
        for t in 0..tets.len() {
            for f in 0..4 {
                let mut k: Vec<usize> = (0..4).filter(|g| *g != f).map(|g| tets[t].v[g]).collect();
                k.sort();
                let key = [k[0], k[1], k[2]];
                if let Some((ot, of)) = faces.remove(&key) {
                    tets[t].nb[f] = ot;
                    tets[ot].nb[of] = t;
                } else {
                    faces.insert(key, (t, f));
                }
            }
        }
    }
    // The sign the in-sphere determinant has for a point inside a positively oriented tetrahedron's sphere.
    let inside_sign = insphere([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.25; 3]).signum();
    // Is x in conflict with tetrahedron t (inside its sphere, or beyond its hull face)?
    let conflict = |tets: &Vec<Tet>, t: usize, x: [f64; 3]| -> bool {
        let tt = &tets[t];
        match tt.v.iter().position(|&k| k == inf) {
            Some(j) => {
                let mut q = tt.v.map(|k| if k == inf { x } else { v[k] });
                q[j] = x;
                orient(q[0], q[1], q[2], q[3]) > 0.0
            }
            None => insphere(v[tt.v[0]], v[tt.v[1]], v[tt.v[2]], v[tt.v[3]], x) * inside_sign > 0.0,
        }
    };
    // Insert the others in a shuffled order (keeps the walk short and the cavities small).
    let mut order: Vec<usize> = (0..n).filter(|i| !first.contains(i)).collect();
    for i in (1..order.len()).rev() {
        let j = ((rnd() + 0.5) * (i + 1) as f64) as usize % (i + 1);
        order.swap(i, j);
    }
    let mut last = 0usize;
    let mut in_cavity: Vec<u32> = Vec::new();
    let mut stamp = 0u32;
    for &p in &order {
        let x = v[p];
        // Walk from the last finite tetrahedron towards x; stepping into an infinite one means x is
        // outside the hull, beyond that face.
        let mut t = last;
        let mut steps = 0;
        'walk: loop {
            steps += 1;
            if tets[t].v.contains(&inf) {
                break;
            }
            if steps > 4 * tets.len() + 100 {
                t = (0..tets.len()).find(|&k| tets[k].alive && conflict(&tets, k, x)).unwrap_or(t);
                break;
            }
            let tt = &tets[t];
            for i in 0..4 {
                let mut q = tt.v.map(|j| v[j]);
                q[i] = x;
                if orient(q[0], q[1], q[2], q[3]) < 0.0 {
                    t = tt.nb[i];
                    continue 'walk;
                }
            }
            break;
        }
        if !conflict(&tets, t, x) {
            // On a face or edge (flat) of where the walk ended: any tetrahedron in conflict will do.
            match (0..tets.len()).find(|&k| tets[k].alive && conflict(&tets, k, x)) {
                Some(k) => t = k,
                None => continue,
            }
        }
        // The cavity: every tetrahedron in conflict, grown from t over neighbours.
        stamp += 1;
        if in_cavity.len() < tets.len() {
            in_cavity.resize(tets.len(), 0);
        }
        let mut cavity = vec![t];
        in_cavity[t] = stamp;
        let mut k = 0;
        while k < cavity.len() {
            let ct = cavity[k];
            k += 1;
            for i in 0..4 {
                let nb = tets[ct].nb[i];
                if nb == NONE || in_cavity[nb] == stamp {
                    continue;
                }
                if conflict(&tets, nb, x) {
                    in_cavity[nb] = stamp;
                    cavity.push(nb);
                }
            }
        }
        // Its boundary faces, each joined to p (replacing the corner opposite the face keeps the orientation).
        let mut edge_face: std::collections::HashMap<(usize, usize), (usize, usize)> = std::collections::HashMap::new();
        let mut newest_finite = None;
        for &ct in &cavity {
            for i in 0..4 {
                let nb = tets[ct].nb[i];
                if nb != NONE && in_cavity.get(nb) == Some(&stamp) {
                    continue;
                }
                let mut q = tets[ct].v;
                q[i] = p;
                let id = tets.len();
                let mut nt = mk(&v, q);
                nt.nb[i] = nb;
                tets.push(nt);
                if !q.contains(&inf) {
                    newest_finite = Some(id);
                }
                if nb != NONE {
                    if let Some(j) = tets[nb].nb.iter().position(|&z| z == ct) {
                        tets[nb].nb[j] = id;
                    }
                }
                for f in (0..4).filter(|f| *f != i) {
                    let others: Vec<usize> = (0..4).filter(|g| *g != f && *g != i).map(|g| q[g]).collect();
                    let key = (others[0].min(others[1]), others[0].max(others[1]));
                    if let Some((oid, of)) = edge_face.remove(&key) {
                        tets[id].nb[f] = oid;
                        tets[oid].nb[of] = id;
                    } else {
                        edge_face.insert(key, (id, f));
                    }
                }
            }
        }
        for &ct in &cavity {
            tets[ct].alive = false;
        }
        if let Some(t) = newest_finite {
            last = t;
        }
    }
    let a2 = if alpha > 0.0 { alpha * alpha } else { f64::INFINITY };
    Ok(tets
        .iter()
        .filter(|t| t.alive && !t.v.contains(&inf))
        .filter(|t| {
            // The alpha test on the true positions.
            let (_, r2) = sphere(pts[t.v[0]], pts[t.v[1]], pts[t.v[2]], pts[t.v[3]]);
            r2 <= a2
        })
        .map(|t| t.v)
        .collect())
}

/// Delaunay 3D of a dataset's points: an unstructured grid of tetrahedra with the points' data.
pub fn delaunay_3d(ds: &DataSet, alpha: f64) -> Result<UnstructuredGrid> {
    let (points, pd) = match crate::simplex::parts(ds) {
        Some((p, pd, _, _)) => (p.clone(), pd.clone()),
        None => bail!("Delaunay 3D needs a dataset with points"),
    };
    let pts: Vec<[f64; 3]> = (0..points.len()).map(|i| points.get(i)).collect();
    let tets = tetrahedralize(&pts, alpha)?;
    let mut cells = Cells::new();
    for t in &tets {
        // Keep positive orientation on the true positions (the jitter can flip a flat sliver).
        let p = t.map(|i| pts[i]);
        if orient(p[0], p[1], p[2], p[3]) < 0.0 {
            cells.push(CellType::Tetra, &[t[1] as u32, t[0] as u32, t[2] as u32, t[3] as u32]);
        } else {
            cells.push(CellType::Tetra, &t.map(|i| i as u32));
        }
    }
    Ok(UnstructuredGrid { points, cells, point_data: pd, cell_data: Attributes::new(), field_data: Attributes::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(pts: &[[f64; 3]], tets: &[[usize; 4]]) -> f64 {
        tets.iter().map(|t| orient(pts[t[0]], pts[t[1]], pts[t[2]], pts[t[3]]).abs() / 6.0).sum()
    }

    #[test]
    fn random_points_give_delaunay_tetrahedra_filling_their_hull() {
        let mut s = 7u64;
        let mut r = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pts: Vec<[f64; 3]> = (0..400).map(|_| [r(), r(), r()]).collect();
        // The cube's corners make the hull the unit cube.
        for k in 0..8 {
            pts.push([(k & 1) as f64, ((k >> 1) & 1) as f64, ((k >> 2) & 1) as f64]);
        }
        let tets = tetrahedralize(&pts, 0.0).unwrap();
        assert!((volume(&pts, &tets) - 1.0).abs() < 1e-6, "{}", volume(&pts, &tets));
        // Empty spheres: no point strictly inside any tetrahedron's sphere.
        for t in &tets {
            let (c, r2) = sphere(pts[t[0]], pts[t[1]], pts[t[2]], pts[t[3]]);
            for (i, p) in pts.iter().enumerate() {
                if t.contains(&i) {
                    continue;
                }
                let d = sub(*p, c);
                assert!(d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= r2 * (1.0 - 1e-6), "point {i} inside the sphere of {t:?}");
            }
        }
    }

    #[test]
    fn a_regular_grid_is_cut_into_tetrahedra_of_its_whole_volume() {
        let pts: Vec<[f64; 3]> = (0..5).flat_map(|k| (0..5).flat_map(move |j| (0..5).map(move |i| [i as f64, j as f64, k as f64]))).collect();
        let tets = tetrahedralize(&pts, 0.0).unwrap();
        assert!((volume(&pts, &tets) - 64.0).abs() < 1e-6, "{}", volume(&pts, &tets));
        // Every face is shared by at most two tetrahedra, and the boundary faces cover the cube's surface.
        let mut faces: std::collections::HashMap<[usize; 3], usize> = std::collections::HashMap::new();
        for t in &tets {
            for i in 0..4 {
                let mut f: Vec<usize> = (0..4).filter(|j| *j != i).map(|j| t[j]).collect();
                f.sort();
                *faces.entry([f[0], f[1], f[2]]).or_insert(0) += 1;
            }
        }
        assert!(faces.values().all(|c| *c <= 2));
        let area: f64 = faces
            .iter()
            .filter(|(_, c)| **c == 1)
            .map(|(f, _)| {
                let (a, b, c) = (pts[f[0]], pts[f[1]], pts[f[2]]);
                let (u, v) = (sub(b, a), sub(c, a));
                0.5 * ((u[1] * v[2] - u[2] * v[1]).powi(2) + (u[2] * v[0] - u[0] * v[2]).powi(2) + (u[0] * v[1] - u[1] * v[0]).powi(2)).sqrt()
            })
            .sum();
        assert!((area - 96.0).abs() < 1e-6, "boundary area {area}");
    }

    #[test]
    fn alpha_removes_the_large_tetrahedra_of_a_concave_cloud() {
        // Two blobs of points far apart: alpha keeps the blobs and drops the bridge between them.
        let mut s = 3u64;
        let mut r = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pts: Vec<[f64; 3]> = (0..150).map(|_| [r(), r(), r()]).collect();
        pts.extend((0..150).map(|_| [r() + 10.0, r(), r()]));
        let all = tetrahedralize(&pts, 0.0).unwrap();
        let kept = tetrahedralize(&pts, 1.0).unwrap();
        assert!(volume(&pts, &all) > 5.0, "the hull spans both blobs");
        let v = volume(&pts, &kept);
        assert!(v < 2.0 && v > 1.0, "only the blobs: {v}");
        assert!(kept.iter().all(|t| t.iter().all(|&i| i < 150) || t.iter().all(|&i| i >= 150)));
    }
}
