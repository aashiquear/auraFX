//! Mapped hexahedral meshes of six-sided bodies: the structured grid of the body's bounding box
//! has its boundary nodes moved onto the body (along rays from the body's centre), and its inner
//! nodes placed by transfinite interpolation of the six moved faces, so a body that a box can be
//! bent into (a ball, a rounded or twisted block, a superquadric) gets one structured block.

use crate::hex::HexMesh;
use crate::shape::{Shape, V3};
use crate::structured::{box_mesh, Division};

/// The point where the ray from `c` through `p` leaves the body (bisection on the distance).
fn to_surface(shape: &Shape, c: V3, p: V3) -> Option<V3> {
    let d: V3 = std::array::from_fn(|k| p[k] - c[k]);
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-300 {
        return Some(c);
    }
    let u: V3 = d.map(|x| x / len);
    let at = |t: f64| -> V3 { std::array::from_fn(|k| c[k] + u[k] * t) };
    let (mut lo, mut hi) = (0.0, len.max(1e-9));
    let mut guard = 0;
    while shape.eval(at(hi)).0 < 0.0 {
        hi *= 2.0;
        guard += 1;
        if guard > 60 {
            return None;
        }
    }
    for _ in 0..80 {
        let m = 0.5 * (lo + hi);
        if shape.eval(at(m)).0 < 0.0 {
            lo = m;
        } else {
            hi = m;
        }
    }
    Some(at(0.5 * (lo + hi)))
}

/// A mapped mesh of `shape`: `size` per cell (or `div`: pieces and bias per axis, as for
/// structured bodies). Fails when the body's centre is outside it.
pub fn mapped_mesh(shape: &Shape, size: f64, div: Option<&(Vec<usize>, Vec<f64>, String)>) -> Result<HexMesh, String> {
    let (lo, hi) = shape.bounds();
    if (0..3).any(|k| !lo[k].is_finite() || !hi[k].is_finite()) {
        return Err("the body has no bounds".into());
    }
    let c: V3 = std::array::from_fn(|k| 0.5 * (lo[k] + hi[k]));
    if shape.eval(c).0 >= 0.0 {
        return Err("the centre of the body's box is outside it: a mapped mesh needs a body that every ray from that centre leaves once".into());
    }
    let pick = |a: usize| -> Division {
        let n = ((hi[a] - lo[a]) / size).round().max(1.0) as usize;
        let cells = div.and_then(|d| d.0.get(a).copied()).unwrap_or(n).max(1);
        let ratio = div.and_then(|d| d.1.get(a).copied()).unwrap_or(1.0).max(1.0);
        let cluster = if ratio > 1.0 { div.map(|d| d.2.clone()).unwrap_or_else(|| "start".into()) } else { "none".into() };
        Division { cells, ratio, cluster }
    };
    let d = [pick(0), pick(1), pick(2)];
    let mut m = box_mesh(lo, hi, [&d[0], &d[1], &d[2]]);
    let w = [d[0].weights(), d[1].weights(), d[2].weights()];
    let n = [w[0].len(), w[1].len(), w[2].len()];
    let idx = |i: usize, j: usize, k: usize| i + n[0] * (j + n[1] * k);
    if m.points.len() != n[0] * n[1] * n[2] {
        return Err("unexpected grid".into());
    }
    // Boundary nodes onto the body.
    let mut p = m.points.clone();
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let on = i == 0 || j == 0 || k == 0 || i + 1 == n[0] || j + 1 == n[1] || k + 1 == n[2];
                if on {
                    let q = idx(i, j, k);
                    // Spread the rays evenly: the box point is first bent the way a cube is mapped
                    // onto a sphere (so the box's corners do not crowd the rays together).
                    let half: V3 = std::array::from_fn(|a| 0.5 * (hi[a] - lo[a]));
                    let x: V3 = std::array::from_fn(|a| (m.points[q][a] - c[a]) / half[a].max(1e-300));
                    let (x2, y2, z2) = (x[0] * x[0], x[1] * x[1], x[2] * x[2]);
                    let bent = [
                        x[0] * (1.0 - y2 / 2.0 - z2 / 2.0 + y2 * z2 / 3.0).max(0.0).sqrt(),
                        x[1] * (1.0 - z2 / 2.0 - x2 / 2.0 + z2 * x2 / 3.0).max(0.0).sqrt(),
                        x[2] * (1.0 - x2 / 2.0 - y2 / 2.0 + x2 * y2 / 3.0).max(0.0).sqrt(),
                    ];
                    let target: V3 = std::array::from_fn(|a| c[a] + bent[a] * half[a]);
                    p[q] = to_surface(shape, c, target).ok_or("a ray from the centre does not leave the body")?;
                }
            }
        }
    }
    // Inner nodes: transfinite interpolation of the faces, less the edges, plus the corners.
    let (n0, n1, n2) = (n[0] - 1, n[1] - 1, n[2] - 1);
    let mut out = p.clone();
    for k in 1..n2 {
        for j in 1..n1 {
            for i in 1..n0 {
                let (u, v, s) = (w[0][i], w[1][j], w[2][k]);
                let (iu, iv, is) = (1.0 - u, 1.0 - v, 1.0 - s);
                let g = |a: usize, b: usize, c2: usize| p[idx(a, b, c2)];
                let mut x = [0.0; 3];
                for t in 0..3 {
                    let faces = iu * g(0, j, k)[t] + u * g(n0, j, k)[t] + iv * g(i, 0, k)[t] + v * g(i, n1, k)[t] + is * g(i, j, 0)[t] + s * g(i, j, n2)[t];
                    let edges = iu * iv * g(0, 0, k)[t]
                        + iu * v * g(0, n1, k)[t]
                        + u * iv * g(n0, 0, k)[t]
                        + u * v * g(n0, n1, k)[t]
                        + iu * is * g(0, j, 0)[t]
                        + iu * s * g(0, j, n2)[t]
                        + u * is * g(n0, j, 0)[t]
                        + u * s * g(n0, j, n2)[t]
                        + iv * is * g(i, 0, 0)[t]
                        + iv * s * g(i, 0, n2)[t]
                        + v * is * g(i, n1, 0)[t]
                        + v * s * g(i, n1, n2)[t];
                    let corners = iu * iv * is * g(0, 0, 0)[t]
                        + u * iv * is * g(n0, 0, 0)[t]
                        + iu * v * is * g(0, n1, 0)[t]
                        + u * v * is * g(n0, n1, 0)[t]
                        + iu * iv * s * g(0, 0, n2)[t]
                        + u * iv * s * g(n0, 0, n2)[t]
                        + iu * v * s * g(0, n1, n2)[t]
                        + u * v * s * g(n0, n1, n2)[t];
                    x[t] = faces - edges + corners;
                }
                out[idx(i, j, k)] = x;
            }
        }
    }
    // Even the inner nodes out (Winslow, guarded so no element folds), the boundary held.
    let cells = crate::improve::Cells3 { cells: (0..m.n_elements()).map(|e| m.cell(e)[..8].to_vec()).collect() };
    crate::improve::smooth(&mut out, &cells, crate::improve::SMOOTH_WINSLOW, 20, &Default::default());
    m.points = out;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::{hex_metric, HexMetric};

    #[test]
    fn a_ball_gets_one_mapped_block_with_its_volume() {
        let ball = Shape::Sphere { r: 1.0, face: 1 };
        let m = mapped_mesh(&ball, 0.2, None).unwrap();
        assert_eq!(m.n_elements(), 10 * 10 * 10);
        let mut vol = 0.0;
        let mut worst = f64::INFINITY;
        for e in 0..m.n_elements() {
            let c: [[f64; 3]; 8] = std::array::from_fn(|k| m.points[m.cell(e)[k] as usize]);
            vol += crate::quality::volume(&c);
            worst = worst.min(hex_metric(HexMetric::ScaledJacobian, &c, 0.0));
        }
        // Flat faces between nodes on the sphere: a little less than 4/3 pi.
        let exact = 4.0 / 3.0 * std::f64::consts::PI;
        assert!(vol < exact && vol > 0.97 * exact, "{vol}");
        // A single block on a smooth ball: the eight corner elements are nearly flat.
        assert!(worst > 0.05, "worst scaled Jacobian {worst}");
        // Every boundary node is on the sphere.
        let on = m.points.iter().filter(|p| ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0).abs() < 1e-9).count();
        assert_eq!(on, 11 * 11 * 11 - 9 * 9 * 9);
    }

    #[test]
    fn a_body_whose_centre_is_outside_is_refused() {
        let ring = Shape::Torus { major: 2.0, minor: 0.5, face: 1 };
        assert!(mapped_mesh(&ring, 0.3, None).is_err());
    }
}
