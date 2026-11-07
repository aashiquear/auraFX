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
    [
        sub(p[1], p[0]),
        sub(p[2], p[1]),
        sub(p[3], p[2]),
        sub(p[0], p[3]),
        sub(p[5], p[4]),
        sub(p[6], p[5]),
        sub(p[7], p[6]),
        sub(p[4], p[7]),
        sub(p[4], p[0]),
        sub(p[5], p[1]),
        sub(p[6], p[2]),
        sub(p[7], p[3]),
    ]
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
    let s =
        [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]];
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

// ------------------------------------------------------------------ tetrahedra

/// Tetrahedron quality metrics (Verdict's definitions; corners in VTK order, positive volume
/// when (p1 - p0) x (p2 - p0) points towards p3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TetMetric {
    Volume,
    Jacobian,
    ScaledJacobian,
    EdgeRatio,
    AspectRatio,
    AspectFrobenius,
    AspectGamma,
    RadiusRatio,
    MinimumDihedralAngle,
    CollapseRatio,
    Condition,
    Shape,
    RelativeSizeSquared,
    ShapeAndSize,
    Distortion,
    EquiangleSkew,
    SquishIndex,
    MeanRatio,
    Inradius,
    NormalizedInradius,
}

impl TetMetric {
    pub const ALL: [TetMetric; 20] = [
        TetMetric::Volume,
        TetMetric::Jacobian,
        TetMetric::ScaledJacobian,
        TetMetric::EdgeRatio,
        TetMetric::AspectRatio,
        TetMetric::AspectFrobenius,
        TetMetric::AspectGamma,
        TetMetric::RadiusRatio,
        TetMetric::MinimumDihedralAngle,
        TetMetric::CollapseRatio,
        TetMetric::Condition,
        TetMetric::Shape,
        TetMetric::RelativeSizeSquared,
        TetMetric::ShapeAndSize,
        TetMetric::Distortion,
        TetMetric::EquiangleSkew,
        TetMetric::SquishIndex,
        TetMetric::MeanRatio,
        TetMetric::Inradius,
        TetMetric::NormalizedInradius,
    ];
    /// The array name (and the measure's name without spaces).
    pub fn name(self) -> &'static str {
        match self {
            TetMetric::Volume => "Volume",
            TetMetric::Jacobian => "Jacobian",
            TetMetric::ScaledJacobian => "ScaledJacobian",
            TetMetric::EdgeRatio => "EdgeRatio",
            TetMetric::AspectRatio => "AspectRatio",
            TetMetric::AspectFrobenius => "AspectFrobenius",
            TetMetric::AspectGamma => "AspectGamma",
            TetMetric::RadiusRatio => "RadiusRatio",
            TetMetric::MinimumDihedralAngle => "MinimumDihedralAngle",
            TetMetric::CollapseRatio => "CollapseRatio",
            TetMetric::Condition => "Condition",
            TetMetric::Shape => "Shape",
            TetMetric::RelativeSizeSquared => "RelativeSizeSquared",
            TetMetric::ShapeAndSize => "ShapeAndSize",
            TetMetric::Distortion => "Distortion",
            TetMetric::EquiangleSkew => "EquiangleSkew",
            TetMetric::SquishIndex => "SquishIndex",
            TetMetric::MeanRatio => "MeanRatio",
            TetMetric::Inradius => "Inradius",
            TetMetric::NormalizedInradius => "NormalizedInradius",
        }
    }
    /// The acceptable range of the metric (Verdict's tables), where there is one.
    pub fn acceptable(self) -> Option<(f64, f64)> {
        Some(match self {
            TetMetric::Volume | TetMetric::Jacobian => (0.0, f64::INFINITY),
            TetMetric::ScaledJacobian => (0.5, 1.0),
            TetMetric::EdgeRatio | TetMetric::AspectRatio | TetMetric::AspectGamma | TetMetric::RadiusRatio | TetMetric::Condition => (1.0, 3.0),
            TetMetric::AspectFrobenius => (1.0, 1.3),
            TetMetric::MinimumDihedralAngle => (40.0, 70.528_779_365_509_31),
            TetMetric::CollapseRatio => (0.1, f64::INFINITY),
            TetMetric::Shape => (0.3, 1.0),
            TetMetric::RelativeSizeSquared => (0.3, 1.0),
            TetMetric::ShapeAndSize => (0.2, 1.0),
            TetMetric::Distortion => (0.5, 1.0),
            _ => return None,
        })
    }
}

fn tet_fix(v: f64) -> f64 {
    if v.is_nan() || v >= DBL_MAX {
        DBL_MAX
    } else if v <= -DBL_MAX {
        -DBL_MAX
    } else {
        v
    }
}

fn tri_area(a: P, b: P, c: P) -> f64 {
    0.5 * len(cross(sub(b, a), sub(c, a)))
}

fn unit(v: P) -> P {
    let l = len(v);
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        v
    }
}

/// One metric of one tetrahedron. `avg_volume` is the mean tetrahedron volume of the mesh (for
/// the size metrics).
pub fn tet_metric(m: TetMetric, p: &[P; 4], avg_volume: f64) -> f64 {
    let s0 = sub(p[1], p[0]);
    let s1 = sub(p[2], p[1]);
    let s2 = sub(p[0], p[2]);
    let s3 = sub(p[3], p[0]);
    let s4 = sub(p[3], p[1]);
    let s5 = sub(p[3], p[2]);
    let jac = dot(s3, cross(s2, s0));
    let vol = jac / 6.0;
    let l2 = [dot(s0, s0), dot(s1, s1), dot(s2, s2), dot(s3, s3), dot(s4, s4), dot(s5, s5)];
    match m {
        TetMetric::Volume => vol,
        TetMetric::Jacobian => jac,
        TetMetric::ScaledJacobian => {
            let lp = [l2[0] * l2[2] * l2[3], l2[0] * l2[1] * l2[4], l2[1] * l2[2] * l2[5], l2[3] * l2[4] * l2[5]];
            let mut w = 0;
            for k in 1..4 {
                if lp[k] > lp[w] {
                    w = k;
                }
            }
            let mut prod = lp[w].sqrt();
            if prod < jac.abs() {
                prod = jac.abs();
            }
            if prod < DBL_MIN {
                return DBL_MAX;
            }
            std::f64::consts::SQRT_2 * jac / prod
        }
        TetMetric::EdgeRatio => {
            let mn = l2.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = l2.iter().cloned().fold(0.0, f64::max);
            if mn < DBL_MIN {
                return DBL_MAX;
            }
            tet_fix((mx / mn).sqrt())
        }
        TetMetric::AspectRatio => {
            let (ab, ac, ad) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[3], p[0]));
            let det = dot(ab, cross(ac, ad));
            if det.abs() < DBL_MIN {
                return DBL_MAX;
            }
            let (bc, bd, cd) = (sub(p[2], p[1]), sub(p[3], p[1]), sub(p[3], p[2]));
            let hm = [dot(ab, ab), dot(bc, bc), dot(ac, ac), dot(ad, ad), dot(bd, bd), dot(cd, cd)].iter().cloned().fold(0.0, f64::max).sqrt();
            let areas = len(cross(ab, bc)) + len(cross(ab, ad)) + len(cross(ac, ad)) + len(cross(bc, cd));
            tet_fix(6f64.sqrt() / 12.0 * hm * areas / det.abs())
        }
        TetMetric::AspectFrobenius => {
            let (u, v, w) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[3], p[0]));
            let d = dot(u, cross(v, w));
            let den = 3.0 * (2.0 * d * d).powf(1.0 / 3.0);
            if den < DBL_MIN {
                return DBL_MAX;
            }
            let num = 1.5 * (dot(u, u) + dot(v, v) + dot(w, w)) - dot(v, u) - dot(w, u) - dot(w, v);
            tet_fix(num / den)
        }
        TetMetric::AspectGamma => {
            if vol.abs() < DBL_MIN {
                return DBL_MAX;
            }
            let srms = (l2.iter().sum::<f64>() / 6.0).sqrt();
            srms * srms * srms / (8.485_281_374_238_57 * vol.abs())
        }
        TetMetric::RadiusRatio => {
            let num = add3(scale(cross(s2, s0), l2[3]), scale(cross(s3, s0), l2[2]), scale(cross(s3, s2), l2[0]));
            let area = 0.5 * (len(cross(s2, s0)) + len(cross(s3, s0)) + len(cross(s4, s1)) + len(cross(s3, s2)));
            if vol.abs() < DBL_MIN {
                return DBL_MAX;
            }
            tet_fix(len(num) * area / (108.0 * vol * vol))
        }
        TetMetric::MinimumDihedralAngle => {
            let (ab, ad, bc, cd) = (s0, s3, s1, s5);
            let (abc, abd, acd, bcd) = (cross(ab, bc), cross(ab, ad), cross(ad, cd), cross(bc, cd));
            let (na, nb, nc, nd) = (len(abc), len(abd), len(acd), len(bcd));
            let a = [
                (dot(abc, abd) / (na * nb)).acos(),
                (dot(abc, acd) / (na * nc)).acos(),
                (dot(abc, bcd) / (na * nd)).acos(),
                (dot(abd, acd) / (nb * nc)).acos(),
                (dot(abd, bcd) / (nb * nd)).acos(),
                (dot(acd, bcd) / (nc * nd)).acos(),
            ];
            tet_fix(a.iter().cloned().fold(f64::INFINITY, f64::min) * 180.0 * std::f64::consts::FRAC_1_PI)
        }
        TetMetric::CollapseRatio => {
            let (e01, e02, e03, e12, e13, e23) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[3], p[0]), sub(p[2], p[1]), sub(p[3], p[1]), sub(p[3], p[2]));
            let l = [len(e01), len(e02), len(e03), len(e12), len(e13), len(e23)];
            let l012 = l[4].max(l[0]).max(l[1]);
            let l031 = l[0].max(l[2]).max(l[3]);
            let l023 = l[2].max(l[1]).max(l[5]);
            let l132 = l[4].max(l[3]).max(l[5]);
            let h = |a: P, b: P, c: P| -> f64 {
                let n = cross(a, b);
                dot(c, n) / len(n)
            };
            let mut cr = h(e01, e02, e03) / l012;
            cr = cr.min(h(e03, e01, e02) / l031);
            cr = cr.min(h(e02, e03, e01) / l023);
            cr = cr.min(h(e12, e13, e01) / l132);
            tet_fix(cr)
        }
        TetMetric::Condition => {
            let side0 = sub(p[1], p[0]);
            let side2 = sub(p[0], p[2]);
            let side3 = sub(p[3], p[0]);
            let c1 = side0;
            let c2 = scale(add3(scale(side2, -2.0), scale(side0, -1.0), [0.0; 3]), 1.0 / 3f64.sqrt());
            let c3 = scale(add3(scale(side3, 3.0), side2, scale(side0, -1.0)), 1.0 / 6f64.sqrt());
            let t1 = dot(c1, c1) + dot(c2, c2) + dot(c3, c3);
            let (x, y, z) = (cross(c1, c2), cross(c2, c3), cross(c1, c3));
            let t2 = dot(x, x) + dot(y, y) + dot(z, z);
            let det = dot(c1, cross(c2, c3));
            if det.abs() <= DBL_MIN {
                return DBL_MAX;
            }
            t1.sqrt() * t2.sqrt() / (3.0 * det)
        }
        TetMetric::Shape => tet_shape(p),
        TetMetric::RelativeSizeSquared => tet_rel_size(vol, avg_volume),
        TetMetric::ShapeAndSize => tet_shape(p) * tet_rel_size(vol, avg_volume),
        TetMetric::Distortion => 1.0,
        TetMetric::EquiangleSkew => {
            let (ab, ac, ad, bc, bd, cd) = (unit(s0), unit(sub(p[2], p[0])), unit(s3), unit(s1), unit(s4), unit(s5));
            let (abc, abd, acd, bcd) = (unit(cross(bc, ab)), unit(cross(ab, ad)), unit(cross(cd, ad)), unit(cross(bc, cd)));
            let k = 180.0 * std::f64::consts::FRAC_1_PI;
            let dih = [
                (-dot(abc, abd)).acos(),
                (-dot(abc, acd)).acos(),
                (-dot(abc, bcd)).acos(),
                (-dot(abd, acd)).acos(),
                (-dot(abd, bcd)).acos(),
                (-dot(acd, bcd)).acos(),
            ];
            let (mn, mx) = dih.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
            let theta = (1.0f64 / 3.0).acos() * k;
            let dmax = (mx * k - theta) / (180.0 - theta);
            let dmin = (theta - mn * k) / theta;
            let ang = [
                (-dot(ab, bc)).acos(),
                dot(bc, ac).acos(),
                dot(ac, ab).acos(),
                (-dot(ab, bd)).acos(),
                dot(bd, ad).acos(),
                dot(ad, ab).acos(),
                (-dot(bc, cd)).acos(),
                dot(cd, bd).acos(),
                dot(bd, bc).acos(),
                dot(ad, cd).acos(),
                (-dot(cd, ac)).acos(),
                dot(ac, ad).acos(),
            ];
            let (fmn, fmx) = ang.iter().fold((360.0f64, 0.0f64), |(a, b), x| (a.min(*x), b.max(*x)));
            let smax = (fmx * k - 60.0) / 120.0;
            let smin = (60.0 - fmn * k) / 60.0;
            dmin.max(dmax).max(smin).max(smax)
        }
        TetMetric::SquishIndex => {
            let c = scale(add3(add3(p[0], p[1], p[2]), p[3], [0.0; 3]), 0.25);
            let faces: [([usize; 3], P); 4] = [
                ([0, 1, 3], scale(cross(s0, s4), 0.5)),
                ([1, 2, 3], scale(cross(s1, s5), 0.5)),
                ([0, 2, 3], scale(cross(sub(p[0], p[2]), s3), 0.5)),
                ([0, 1, 2], scale(cross(s1, s0), 0.5)),
            ];
            let mut best: f64 = 0.0;
            for (f, a) in faces {
                let fc = scale(add3(p[f[0]], p[f[1]], p[f[2]]), 1.0 / 3.0);
                let cc = sub(fc, c);
                let s = 1.0 - dot(a, cc) / (len(a) * len(cc));
                best = best.max(s);
            }
            best
        }
        TetMetric::MeanRatio => {
            if vol.abs() < DBL_MIN {
                return 0.0;
            }
            let sum = l2.iter().sum::<f64>() / 6.0;
            6.0 * std::f64::consts::SQRT_2 * vol / sum.powf(1.5)
        }
        TetMetric::Inradius => {
            let a = tri_area(p[0], p[1], p[2]) + tri_area(p[0], p[3], p[1]) + tri_area(p[0], p[2], p[3]) + tri_area(p[1], p[3], p[2]);
            3.0 * vol / a
        }
        TetMetric::NormalizedInradius => tet_normalized_inradius(p),
    }
}

fn scale(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn add3(a: P, b: P, c: P) -> P {
    [a[0] + b[0] + c[0], a[1] + b[1] + c[1], a[2] + b[2] + c[2]]
}

fn tet_shape(p: &[P; 4]) -> f64 {
    let e0 = sub(p[1], p[0]);
    let e1 = sub(p[0], p[2]);
    let e2 = sub(p[3], p[0]);
    let j = dot(e2, cross(e1, e0));
    if j < DBL_MIN {
        return 0.0;
    }
    let num = 3.0 * (std::f64::consts::SQRT_2 * j).powf(2.0 / 3.0);
    let m1 = scale(e1, -1.0);
    let den = 1.5 * (dot(e0, e0) + dot(e1, e1) + dot(e2, e2)) - (dot(e0, m1) + dot(m1, e2) + dot(e2, e0));
    if den < DBL_MIN {
        return 0.0;
    }
    tet_fix((num / den).max(0.0))
}

fn tet_rel_size(vol: f64, avg: f64) -> f64 {
    // The weighted reference tetrahedron has the mean volume.
    let avg_volume = avg;
    if avg_volume < DBL_MIN {
        return 0.0;
    }
    let mut size = vol / avg_volume;
    if size <= DBL_MIN {
        return 0.0;
    }
    if size > 1.0 {
        size = 1.0 / size;
    }
    size * size
}

fn tet_inradius4(p: &[P; 4]) -> f64 {
    let vol = dot(sub(p[3], p[0]), cross(sub(p[1], p[0]), sub(p[2], p[0]))) / 6.0;
    let a = tri_area(p[0], p[1], p[2]) + tri_area(p[0], p[3], p[1]) + tri_area(p[0], p[2], p[3]) + tri_area(p[1], p[3], p[2]);
    3.0 * vol / a
}

/// Verdict's normalised inradius of a linear tetrahedron: its 12 sub-tetrahedra (the
/// quadratic subdivision about the centre of the edge midpoints) against the circumradius.
fn tet_normalized_inradius(p: &[P; 4]) -> f64 {
    let mid = |a: usize, b: usize| scale(add3(p[a], p[b], [0.0; 3]), 0.5);
    let mut c = [[0.0; 3]; 11];
    c[..4].copy_from_slice(&p[..]);
    for (i, (a, b)) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)].iter().enumerate() {
        c[4 + i] = mid(*a, *b);
    }
    let aux = scale((4..10).fold([0.0; 3], |s, i| add3(s, c[i], [0.0; 3])), 1.0 / 6.0);
    c[10] = aux;
    const SUB: [[usize; 4]; 12] = [
        [0, 4, 6, 7],
        [1, 5, 4, 8],
        [2, 6, 5, 9],
        [3, 8, 7, 9],
        [4, 8, 5, 10],
        [5, 8, 9, 10],
        [9, 8, 7, 10],
        [7, 8, 4, 10],
        [4, 5, 6, 10],
        [5, 9, 6, 10],
        [9, 7, 6, 10],
        [7, 4, 6, 10],
    ];
    let min_in = |lo: usize, hi: usize| (lo..=hi).map(|i| tet_inradius4(&SUB[i].map(|k| c[k]))).fold(DBL_MAX, f64::min);
    let (with_parent, without) = (min_in(0, 3), min_in(4, 11));
    let (a, b, cc) = (len(sub(p[1], p[0])), len(sub(p[2], p[0])), len(sub(p[3], p[0])));
    let (aa, bb, ccc) = (len(sub(p[3], p[2])), len(sub(p[3], p[1])), len(sub(p[2], p[1])));
    let vp = (dot(cross(sub(p[1], p[0]), sub(p[2], p[0])), sub(p[3], p[0])) / 6.0).abs();
    let outer = ((a * aa + b * bb + cc * ccc) * (a * aa + b * bb - cc * ccc) * (a * aa - b * bb + cc * ccc) * (-a * aa + b * bb + cc * ccc)).sqrt() / 24.0 / vp;
    let n1 = 6.0 * with_parent / outer;
    let n2 = 3.0 * (1.0 + 3f64.sqrt()) * without / outer;
    tet_fix(n1.min(n2))
}
