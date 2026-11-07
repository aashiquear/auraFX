//! Hexahedron quality metrics, defined as the Verdict library (Sandia National Laboratories,
//! BSD licence: Copyright 2003, 2006, 2019 National Technology & Engineering Solutions of
//! Sandia, LLC (NTESS)) computes them, with the ranges its manual calls acceptable.

pub type P = [f64; 3];

const DBL_MIN: f64 = 2.2204460492503131e-15;
const DBL_MAX: f64 = 1.0e30;

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add4(a: P, b: P, c: P, d: P) -> P {
    std::array::from_fn(|k| a[k] + b[k] + c[k] + d[k])
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: P) -> f64 {
    dot(a, a).sqrt()
}
fn clamp(x: f64) -> f64 {
    if x > 0.0 {
        x.min(DBL_MAX)
    } else {
        x.max(-DBL_MAX)
    }
}
fn safe_ratio(n: f64, d: f64) -> f64 {
    if n.abs() <= DBL_MAX && d.abs() >= DBL_MIN {
        n / d
    } else {
        DBL_MAX
    }
}

/// The metrics, with their names, acceptable range and value for a unit cube.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexMetric {
    Volume,
    Jacobian,
    ScaledJacobian,
    EdgeRatio,
    MaxEdgeRatio,
    MaxAspectFrobenius,
    MeanAspectFrobenius,
    Skew,
    Taper,
    Stretch,
    Diagonal,
    Dimension,
    Oddy,
    Shear,
    Shape,
    RelativeSizeSquared,
    ShapeAndSize,
    ShearAndSize,
    Distortion,
}

impl HexMetric {
    pub const ALL: [HexMetric; 19] = [
        HexMetric::Volume,
        HexMetric::Jacobian,
        HexMetric::ScaledJacobian,
        HexMetric::EdgeRatio,
        HexMetric::MaxEdgeRatio,
        HexMetric::MaxAspectFrobenius,
        HexMetric::MeanAspectFrobenius,
        HexMetric::Skew,
        HexMetric::Taper,
        HexMetric::Stretch,
        HexMetric::Diagonal,
        HexMetric::Dimension,
        HexMetric::Oddy,
        HexMetric::Shear,
        HexMetric::Shape,
        HexMetric::RelativeSizeSquared,
        HexMetric::ShapeAndSize,
        HexMetric::ShearAndSize,
        HexMetric::Distortion,
    ];
    pub fn name(self) -> &'static str {
        match self {
            HexMetric::Volume => "Volume",
            HexMetric::Jacobian => "Jacobian",
            HexMetric::ScaledJacobian => "ScaledJacobian",
            HexMetric::EdgeRatio => "EdgeRatio",
            HexMetric::MaxEdgeRatio => "MaxEdgeRatio",
            HexMetric::MaxAspectFrobenius => "MaxAspectFrobenius",
            HexMetric::MeanAspectFrobenius => "MeanAspectFrobenius",
            HexMetric::Skew => "Skew",
            HexMetric::Taper => "Taper",
            HexMetric::Stretch => "Stretch",
            HexMetric::Diagonal => "Diagonal",
            HexMetric::Dimension => "Dimension",
            HexMetric::Oddy => "Oddy",
            HexMetric::Shear => "Shear",
            HexMetric::Shape => "Shape",
            HexMetric::RelativeSizeSquared => "RelativeSizeSquared",
            HexMetric::ShapeAndSize => "ShapeAndSize",
            HexMetric::ShearAndSize => "ShearAndSize",
            HexMetric::Distortion => "Distortion",
        }
    }
    /// The acceptable range (inclusive) of the Verdict manual, None where it gives none.
    pub fn acceptable(self) -> Option<(f64, f64)> {
        Some(match self {
            HexMetric::Volume | HexMetric::Jacobian | HexMetric::Dimension => (0.0, f64::INFINITY),
            HexMetric::ScaledJacobian => (0.5, 1.0),
            HexMetric::EdgeRatio => return None,
            HexMetric::MaxEdgeRatio => (1.0, 1.3),
            HexMetric::MaxAspectFrobenius | HexMetric::MeanAspectFrobenius => (1.0, 3.0),
            HexMetric::Skew => (0.0, 0.5),
            HexMetric::Taper => (0.0, 0.5),
            HexMetric::Stretch => (0.25, 1.0),
            HexMetric::Diagonal => (0.65, 1.0),
            HexMetric::Oddy => (0.0, 0.5),
            HexMetric::Shear | HexMetric::Shape => (0.3, 1.0),
            HexMetric::RelativeSizeSquared => (0.5, 1.0),
            HexMetric::ShapeAndSize | HexMetric::ShearAndSize => (0.2, 1.0),
            HexMetric::Distortion => (0.5, 1.0),
        })
    }
}

fn edges(p: &[P; 8]) -> [P; 12] {
    [sub(p[1], p[0]), sub(p[2], p[1]), sub(p[3], p[2]), sub(p[0], p[3]), sub(p[5], p[4]), sub(p[6], p[5]), sub(p[7], p[6]), sub(p[4], p[7]), sub(p[4], p[0]), sub(p[5], p[1]), sub(p[6], p[2]), sub(p[7], p[3])]
}

fn diagonals(p: &[P; 8]) -> [f64; 4] {
    [len(sub(p[6], p[0])), len(sub(p[4], p[2])), len(sub(p[7], p[1])), len(sub(p[5], p[3]))]
}

/// Principal-axis vectors (Verdict's calc_hex_efg).
fn efg(p: &[P; 8], case: u32) -> P {
    let d = |a: usize, b: usize| sub(p[a], p[b]);
    match case {
        1 => add4(d(1, 0), d(2, 3), d(5, 4), d(6, 7)),
        2 => add4(d(2, 0), d(3, 1), d(6, 4), d(7, 5)),
        3 => add4(d(4, 0), d(5, 1), d(6, 2), d(7, 3)),
        12 => add4(d(0, 1), d(2, 3), d(4, 5), d(6, 7)),
        13 => add4(d(0, 1), d(3, 2), d(5, 4), d(6, 7)),
        23 => add4(d(0, 2), d(1, 3), d(6, 4), d(7, 5)),
        _ => [0.0; 3],
    }
}

/// The edge triple at each corner.
fn corners(p: &[P; 8]) -> [[P; 3]; 8] {
    let d = |a: usize, b: usize| sub(p[a], p[b]);
    [
        [d(1, 0), d(3, 0), d(4, 0)],
        [d(2, 1), d(0, 1), d(5, 1)],
        [d(3, 2), d(1, 2), d(6, 2)],
        [d(0, 3), d(2, 3), d(7, 3)],
        [d(7, 4), d(5, 4), d(0, 4)],
        [d(4, 5), d(6, 5), d(1, 5)],
        [d(5, 6), d(7, 6), d(2, 6)],
        [d(6, 7), d(4, 7), d(3, 7)],
    ]
}

fn centre_frame(p: &[P; 8]) -> [P; 3] {
    [efg(p, 1), efg(p, 2), efg(p, 3)]
}

/// Nodes moved to their mean and divided by the bounding-box diagonal.
fn scaled(p: &[P; 8]) -> [P; 8] {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for q in p {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let mut s = len(sub(hi, lo));
    let mut c: P = std::array::from_fn(|k| p.iter().map(|q| q[k]).sum::<f64>() / 8.0);
    if s < DBL_MIN {
        c = [0.0; 3];
        s = 1.0;
    }
    p.map(|q| std::array::from_fn(|k| (q[k] - c[k]) / s))
}

fn condition(f: &[P; 3]) -> f64 {
    let det = dot(f[0], cross(f[1], f[2]));
    if det <= DBL_MIN {
        return DBL_MAX;
    }
    let t1 = dot(f[0], f[0]) + dot(f[1], f[1]) + dot(f[2], f[2]);
    let (a, b, c) = (cross(f[0], f[1]), cross(f[1], f[2]), cross(f[2], f[0]));
    let t2 = dot(a, a) + dot(b, b) + dot(c, c);
    (t1 * t2).sqrt() / det
}

fn oddy_comp(f: &[P; 3]) -> f64 {
    let g11 = dot(f[0], f[0]);
    let g12 = dot(f[0], f[1]);
    let g13 = dot(f[0], f[2]);
    let g22 = dot(f[1], f[1]);
    let g23 = dot(f[1], f[2]);
    let g33 = dot(f[2], f[2]);
    let rt_g = dot(f[0], cross(f[1], f[2]));
    if rt_g > DBL_MIN {
        let ng = g11 * g11 + 2.0 * g12 * g12 + 2.0 * g13 * g13 + g22 * g22 + 2.0 * g23 * g23 + g33 * g33;
        let nj = g11 + g22 + g33;
        (ng - (1.0 / 3.0) * nj * nj) / rt_g.powf(4.0 / 3.0)
    } else {
        DBL_MAX
    }
}

pub fn volume(p: &[P; 8]) -> f64 {
    const FACES: [[usize; 4]; 6] = [[0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7], [3, 2, 1, 0], [4, 5, 6, 7]];
    let hc: P = std::array::from_fn(|k| p.iter().map(|q| q[k]).sum::<f64>() / 8.0);
    let mut v = 0.0;
    for f in FACES {
        let fc: P = std::array::from_fn(|k| f.iter().map(|i| p[*i][k]).sum::<f64>() / 4.0);
        let s4 = sub(hc, fc);
        let s: [P; 4] = std::array::from_fn(|i| sub(hc, p[f[i]]));
        for j in 0..3 {
            v += dot(s4, cross(s[j + 1], s[j])) / 6.0;
        }
        v += dot(s4, cross(s[0], s[3])) / 6.0;
    }
    clamp(v)
}

/// Trilinear shape-function derivatives at (xi, eta, zeta) in [-1, 1]^3.
fn dshape(x: f64, y: f64, z: f64) -> [[f64; 3]; 8] {
    let s = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]];
    std::array::from_fn(|i| {
        let [a, b, c]: [f64; 3] = s[i];
        [a * (1.0 + b * y) * (1.0 + c * z) / 8.0, b * (1.0 + a * x) * (1.0 + c * z) / 8.0, c * (1.0 + a * x) * (1.0 + b * y) / 8.0]
    })
}

fn det_at(p: &[P; 8], x: f64, y: f64, z: f64) -> f64 {
    let d = dshape(x, y, z);
    let col = |k: usize| -> P { std::array::from_fn(|a| (0..8).map(|i| d[i][k] * p[i][a]).sum()) };
    dot(col(0), cross(col(1), col(2)))
}

/// Exact volume of the trilinear map (2-point Gauss is exact for its Jacobian).
fn trilinear_volume(p: &[P; 8]) -> f64 {
    let g = 1.0 / 3f64.sqrt();
    let mut v = 0.0;
    for x in [-g, g] {
        for y in [-g, g] {
            for z in [-g, g] {
                v += det_at(p, x, y, z);
            }
        }
    }
    v
}

/// One metric of one hexahedron (corners in Exodus/VTK order). `avg_volume` is the mean
/// hexahedron volume of the mesh (for the size metrics).
pub fn hex_metric(m: HexMetric, p: &[P; 8], avg_volume: f64) -> f64 {
    match m {
        HexMetric::Volume => volume(p),
        HexMetric::Jacobian => {
            let c = centre_frame(p);
            let mut j = dot(c[0], cross(c[1], c[2])) / 64.0;
            for f in corners(p) {
                j = j.min(dot(f[0], cross(f[1], f[2])));
            }
            clamp(j)
        }
        HexMetric::ScaledJacobian => {
            let s = scaled(p);
            let mut frames = vec![centre_frame(&s)];
            frames.extend(corners(&s));
            let mut q = f64::INFINITY;
            for f in frames {
                let l: [f64; 3] = f.map(|v| dot(v, v));
                if l.iter().any(|x| *x <= DBL_MIN) {
                    return DBL_MAX;
                }
                q = q.min(dot(f[0], cross(f[1], f[2])) / (l[0] * l[1] * l[2]).sqrt());
            }
            clamp(q)
        }
        HexMetric::EdgeRatio => {
            let l2: Vec<f64> = edges(p).iter().map(|e| dot(*e, *e)).collect();
            let mx = l2[..6].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mn = l2.iter().cloned().fold(f64::INFINITY, f64::min);
            if mn < DBL_MIN {
                return DBL_MAX;
            }
            clamp((mx / mn).sqrt())
        }
        HexMetric::MaxEdgeRatio => {
            let s = scaled(p);
            let (a, b, c) = (len(efg(&s, 1)), len(efg(&s, 2)), len(efg(&s, 3)));
            let r = |x: f64, y: f64| safe_ratio(x.max(y), x.min(y));
            clamp(r(a, b).max(r(a, c)).max(r(b, c)))
        }
        HexMetric::MaxAspectFrobenius => {
            let s = scaled(p);
            let mut c: f64 = 0.0;
            for f in corners(&s) {
                c = c.max(condition(&f));
                if c >= DBL_MAX {
                    return DBL_MAX;
                }
            }
            c / 3.0
        }
        HexMetric::MeanAspectFrobenius => {
            let s = scaled(p);
            let mut sum = 0.0;
            for f in corners(&s) {
                let c = condition(&f);
                if c >= DBL_MAX {
                    return DBL_MAX;
                }
                sum += c;
            }
            sum / 24.0
        }
        HexMetric::Skew => {
            let s = scaled(p);
            let mut v = [efg(&s, 1), efg(&s, 2), efg(&s, 3)];
            for x in v.iter_mut() {
                let l = len(*x);
                if l <= DBL_MIN {
                    return DBL_MAX;
                }
                *x = x.map(|c| c / l);
            }
            clamp(dot(v[0], v[1]).abs().max(dot(v[0], v[2]).abs()).max(dot(v[1], v[2]).abs()))
        }
        HexMetric::Taper => {
            let s = scaled(p);
            let (e1, e2, e3) = (len(efg(&s, 1)), len(efg(&s, 2)), len(efg(&s, 3)));
            let t1 = safe_ratio(len(efg(&s, 12)), e1.min(e2)).abs();
            let t2 = safe_ratio(len(efg(&s, 13)), e1.min(e3)).abs();
            let t3 = safe_ratio(len(efg(&s, 23)), e2.min(e3)).abs();
            clamp(t1.max(t2).max(t3))
        }
        HexMetric::Stretch => {
            let min_e = edges(p).iter().map(|e| len(*e)).fold(f64::INFINITY, f64::min);
            let max_d = diagonals(p).iter().cloned().fold(0.0, f64::max);
            clamp(3f64.sqrt() * safe_ratio(min_e, max_d))
        }
        HexMetric::Diagonal => {
            let d = diagonals(p);
            clamp(safe_ratio(d.iter().cloned().fold(f64::INFINITY, f64::min), d.iter().cloned().fold(0.0, f64::max)))
        }
        HexMetric::Dimension => {
            // Volume over twice the length of its gradient with respect to the nodes.
            let v = trilinear_volume(p);
            let mut g2 = 0.0;
            for i in 0..8 {
                for a in 0..3 {
                    let (mut q, mut r) = (*p, *p);
                    q[i][a] += 0.5;
                    r[i][a] -= 0.5;
                    let g = trilinear_volume(&q) - trilinear_volume(&r);
                    g2 += g * g;
                }
            }
            (0.5 * v * v / g2).sqrt()
        }
        HexMetric::Oddy => {
            let mut o: f64 = 0.0;
            o = o.max(oddy_comp(&centre_frame(p)));
            for f in corners(p) {
                o = o.max(oddy_comp(&f));
            }
            clamp(o)
        }
        HexMetric::Shear => {
            let s = scaled(p);
            let mut q: f64 = 1.0;
            for f in corners(&s) {
                let l: [f64; 3] = f.map(|v| dot(v, v));
                if l.iter().any(|x| *x <= DBL_MIN) {
                    return 0.0;
                }
                let det = dot(f[0], cross(f[1], f[2]));
                if det < DBL_MIN {
                    return 0.0;
                }
                q = q.min(det / (l[0] * l[1] * l[2]).sqrt());
            }
            if q <= DBL_MIN {
                q = 0.0;
            }
            clamp(q)
        }
        HexMetric::Shape => {
            let s = scaled(p);
            let mut q: f64 = 1.0;
            for f in corners(&s) {
                let det = dot(f[0], cross(f[1], f[2]));
                if det <= DBL_MIN {
                    return 0.0;
                }
                q = q.min(3.0 * det.powf(2.0 / 3.0) / (dot(f[0], f[0]) + dot(f[1], f[1]) + dot(f[2], f[2])));
            }
            if q <= DBL_MIN {
                q = 0.0;
            }
            clamp(q)
        }
        HexMetric::RelativeSizeSquared => {
            if avg_volume == 0.0 || avg_volume < DBL_MIN {
                return 0.0;
            }
            let det_sum: f64 = corners(p).iter().map(|f| dot(f[0], cross(f[1], f[2]))).sum();
            if det_sum > DBL_MIN {
                let tau = det_sum / (8.0 * avg_volume);
                let tau = tau.min(1.0 / tau);
                clamp(tau * tau)
            } else {
                0.0
            }
        }
        HexMetric::ShapeAndSize => clamp(hex_metric(HexMetric::RelativeSizeSquared, p, avg_volume) * hex_metric(HexMetric::Shape, p, avg_volume)),
        HexMetric::ShearAndSize => clamp(hex_metric(HexMetric::RelativeSizeSquared, p, avg_volume) * hex_metric(HexMetric::Shear, p, avg_volume)),
        HexMetric::Distortion => {
            let g = 1.0 / 3f64.sqrt();
            let (mut min_j, mut vol) = (f64::INFINITY, 0.0);
            for z in [-g, g] {
                for y in [-g, g] {
                    for x in [-g, g] {
                        let j = det_at(p, x, y, z);
                        min_j = min_j.min(j);
                        vol += j;
                    }
                }
            }
            // The Jacobian at the eight nodes counts towards the minimum too.
            for z in [-1.0, 1.0] {
                for y in [-1.0, 1.0] {
                    for x in [-1.0, 1.0] {
                        min_j = min_j.min(det_at(p, x, y, z));
                    }
                }
            }
            if vol.abs() == 0.0 {
                return DBL_MAX;
            }
            clamp(min_j / vol * 8.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_cube_values() {
        let p: [P; 8] = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]];
        let want = [
            (HexMetric::Volume, 1.0),
            (HexMetric::Jacobian, 1.0),
            (HexMetric::ScaledJacobian, 1.0),
            (HexMetric::EdgeRatio, 1.0),
            (HexMetric::MaxEdgeRatio, 1.0),
            (HexMetric::MaxAspectFrobenius, 1.0),
            (HexMetric::MeanAspectFrobenius, 1.0),
            (HexMetric::Skew, 0.0),
            (HexMetric::Taper, 0.0),
            (HexMetric::Stretch, 1.0),
            (HexMetric::Diagonal, 1.0),
            (HexMetric::Oddy, 0.0),
            (HexMetric::Shear, 1.0),
            (HexMetric::Shape, 1.0),
            (HexMetric::RelativeSizeSquared, 1.0),
            (HexMetric::Distortion, 1.0),
        ];
        for (m, w) in want {
            assert!((hex_metric(m, &p, 1.0) - w).abs() < 1e-12, "{m:?}: {}", hex_metric(m, &p, 1.0));
        }
    }
}
