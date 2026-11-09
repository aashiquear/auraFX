//! Implicit solids: signed distance (negative inside) with the face nearest each point.
//!
//! Distances are exact for the brick, sphere, cylinder, cone, torus, extrusions, full
//! revolutions, translations, rotations, uniform scaling and unions/intersections outside
//! their overlap; elsewhere (superquadrics, harmonic shapes, partial revolutions, sweeps,
//! non-uniform scaling, CSG interiors) they are bounds of the right sign that vanish on the
//! surface, which is all the contour and the meshers need.

use crate::outline::Outline;
use crate::trimesh::TriMesh;
use std::sync::Arc;

pub type V3 = [f64; 3];
/// Global face number (1-based; 0 means no face).
pub type FaceId = u32;

#[derive(Clone)]
pub enum Shape {
    /// Centred box with half sizes; faces x-, x+, y-, y+, z-, z+.
    Brick {
        half: V3,
        faces: [FaceId; 6],
    },
    Sphere {
        r: f64,
        face: FaceId,
    },
    /// Along z, centred; faces side, bottom, top.
    Cylinder {
        r: f64,
        h: f64,
        faces: [FaceId; 3],
    },
    /// Along z, centred, radius `r1` at the bottom and `r2` at the top; faces side, bottom, top.
    Cone {
        r1: f64,
        r2: f64,
        h: f64,
        faces: [FaceId; 3],
    },
    /// In the xy plane about z.
    Torus {
        major: f64,
        minor: f64,
        face: FaceId,
    },
    /// |x/a|^(2/e2) + |y/b|^(2/e2))^(e2/e1) + |z/c|^(2/e1) <= 1.
    Superquadric {
        abc: V3,
        e1: f64,
        e2: f64,
        face: FaceId,
    },
    /// r(theta, phi) = r (1 + sum c Y_lm) with real orthonormal spherical harmonics.
    Harmonic {
        r: f64,
        coef: Vec<(i32, i32, f64)>,
        face: FaceId,
    },
    /// The outline (xy) extruded from z = 0 to `h`; one face per outline curve, bottom, top.
    Extrude {
        outline: Arc<Outline>,
        h: f64,
        curve_faces: Vec<FaceId>,
        bottom: FaceId,
        top: FaceId,
    },
    /// The outline (x = radius, y = height) turned about z by `angle` (radians) from the x axis.
    Revolve {
        outline: Arc<Outline>,
        angle: f64,
        curve_faces: Vec<FaceId>,
        start: FaceId,
        end: FaceId,
    },
    /// The outline carried along a path (its plane normal to the path).
    Sweep {
        outline: Arc<Outline>,
        path: Arc<Path>,
        curve_faces: Vec<FaceId>,
        start: FaceId,
        end: FaceId,
    },
    /// A closed triangle surface.
    Surface {
        mesh: Arc<TriMesh>,
        face: FaceId,
    },
    /// `inner` placed by: p_local = rot^T (p - offset) / scale.
    Transform {
        inner: Box<Shape>,
        rot: [[f64; 3]; 3],
        offset: V3,
        scale: V3,
    },
    Union(Vec<Shape>),
    Intersect(Vec<Shape>),
    Subtract(Box<Shape>, Box<Shape>),
    /// The side of a plane its normal points away from: n·p <= offset (`normal` of unit length).
    /// Used by `webcut`; never a body on its own.
    HalfSpace {
        normal: V3,
        offset: f64,
        face: FaceId,
    },
    /// The side of an open surface its normals point away from (signed distance to the sheet).
    /// Used by `webcut`; never a body on its own.
    Sheet {
        mesh: Arc<TriMesh>,
        face: FaceId,
    },
}

fn len2(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

/// Distance of a 2-D "box" (from the distances to two families of planes): the extrusion,
/// cylinder and cap combination.
fn box2(dx: f64, dy: f64) -> f64 {
    dx.max(dy).min(0.0) + len2(dx.max(0.0), dy.max(0.0))
}

/// A path for sweeps, with rotation-minimising frames at its points.
pub struct Path {
    pub points: Vec<V3>,
    frames: Vec<(V3, V3, V3)>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> V3 {
    let n = dot(a, a).sqrt().max(1e-300);
    [a[0] / n, a[1] / n, a[2] / n]
}

impl Path {
    /// Frames (tangent, normal, binormal) per segment, carried along without twist.
    pub fn new(points: Vec<V3>) -> Path {
        let mut frames = Vec::new();
        let mut prev: Option<(V3, V3)> = None;
        for w in points.windows(2) {
            let t = unit(sub(w[1], w[0]));
            let n = match prev {
                None => {
                    let r = if t[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
                    unit(cross(r, t))
                }
                Some((t0, n0)) => {
                    // Rotate n0 by the rotation taking t0 to t.
                    let ax = cross(t0, t);
                    let s = dot(ax, ax).sqrt();
                    let c = dot(t0, t);
                    if s < 1e-12 {
                        n0
                    } else {
                        let k = [ax[0] / s, ax[1] / s, ax[2] / s];
                        let kn = cross(k, n0);
                        let kd = dot(k, n0);
                        unit(std::array::from_fn(|i| n0[i] * c + kn[i] * s + k[i] * kd * (1.0 - c)))
                    }
                }
            };
            let b = cross(t, n);
            frames.push((t, n, b));
            prev = Some((t, n));
        }
        Path { points, frames }
    }
}

/// Real orthonormal spherical harmonic Y_lm at (cos theta, phi).
pub fn real_sh(l: i32, m: i32, ct: f64, phi: f64) -> f64 {
    let am = m.unsigned_abs() as i32;
    if am > l {
        return 0.0;
    }
    // Associated Legendre P_l^|m|(x) without the Condon-Shortley phase.
    let x = ct.clamp(-1.0, 1.0);
    let st = (1.0 - x * x).max(0.0).sqrt();
    let mut pmm = 1.0;
    let mut fact = 1.0;
    for _ in 0..am {
        pmm *= fact * st;
        fact += 2.0;
    }
    let p = if l == am {
        pmm
    } else {
        let mut pm1 = x * (2 * am + 1) as f64 * pmm;
        if l == am + 1 {
            pm1
        } else {
            let mut pll = 0.0;
            let mut p0 = pmm;
            for ll in am + 2..=l {
                pll = (x * (2 * ll - 1) as f64 * pm1 - (ll + am - 1) as f64 * p0) / (ll - am) as f64;
                p0 = pm1;
                pm1 = pll;
            }
            pll
        }
    };
    let mut ratio = 1.0;
    for k in (l - am + 1)..=(l + am) {
        ratio /= k as f64;
    }
    let k = ((2 * l + 1) as f64 / (4.0 * std::f64::consts::PI) * ratio).sqrt();
    match m.cmp(&0) {
        std::cmp::Ordering::Equal => k * p,
        std::cmp::Ordering::Greater => std::f64::consts::SQRT_2 * k * p * (am as f64 * phi).cos(),
        std::cmp::Ordering::Less => std::f64::consts::SQRT_2 * k * p * (am as f64 * phi).sin(),
    }
}

/// The radius of a harmonic shape in direction `d` (unit).
pub fn harmonic_radius(r: f64, coef: &[(i32, i32, f64)], d: V3) -> f64 {
    let phi = d[1].atan2(d[0]);
    r * (1.0 + coef.iter().map(|(l, m, c)| c * real_sh(*l, *m, d[2], phi)).sum::<f64>())
}

impl Shape {
    /// Signed distance at `p` and the face nearest it.
    pub fn eval(&self, p: V3) -> (f64, FaceId) {
        match self {
            Shape::Brick { half, faces } => {
                let q: V3 = std::array::from_fn(|k| p[k].abs() - half[k]);
                let out = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2) + q[2].max(0.0).powi(2)).sqrt();
                let m = q[0].max(q[1]).max(q[2]);
                let axis = if q[0] >= q[1] && q[0] >= q[2] {
                    0
                } else if q[1] >= q[2] {
                    1
                } else {
                    2
                };
                (out + m.min(0.0), faces[2 * axis + (p[axis] >= 0.0) as usize])
            }
            Shape::Sphere { r, face } => (dot(p, p).sqrt() - r, *face),
            Shape::Cylinder { r, h, faces } => {
                let (dx, dz) = (len2(p[0], p[1]) - r, p[2].abs() - h / 2.0);
                (
                    box2(dx, dz),
                    if dx >= dz {
                        faces[0]
                    } else if p[2] < 0.0 {
                        faces[1]
                    } else {
                        faces[2]
                    },
                )
            }
            Shape::Cone { r1, r2, h, faces } => {
                let hh = h / 2.0;
                let q = [len2(p[0], p[1]), p[2]];
                let k1 = [*r2, hh];
                let k2 = [r2 - r1, 2.0 * hh];
                let ca = [q[0] - q[0].min(if q[1] < 0.0 { *r1 } else { *r2 }), q[1].abs() - hh];
                let t = (((k1[0] - q[0]) * k2[0] + (k1[1] - q[1]) * k2[1]) / (k2[0] * k2[0] + k2[1] * k2[1])).clamp(0.0, 1.0);
                let cb = [q[0] - k1[0] + k2[0] * t, q[1] - k1[1] + k2[1] * t];
                let s = if cb[0] < 0.0 && ca[1] < 0.0 { -1.0 } else { 1.0 };
                let (da, db) = (ca[0] * ca[0] + ca[1] * ca[1], cb[0] * cb[0] + cb[1] * cb[1]);
                let face = if db <= da {
                    faces[0]
                } else if p[2] < 0.0 {
                    faces[1]
                } else {
                    faces[2]
                };
                (s * da.min(db).sqrt(), face)
            }
            Shape::Torus { major, minor, face } => (len2(len2(p[0], p[1]) - major, p[2]) - minor, *face),
            Shape::Superquadric { abc, e1, e2, face } => {
                let f = ((p[0] / abc[0]).abs().powf(2.0 / e2) + (p[1] / abc[1]).abs().powf(2.0 / e2)).powf(e2 / e1) + (p[2] / abc[2]).abs().powf(2.0 / e1);
                let g = f.powf(e1 / 2.0);
                let r = dot(p, p).sqrt();
                // Radial distance to the surface (homogeneous of degree 1 in p).
                let d = if g > 1e-300 { r * (1.0 - 1.0 / g) } else { -abc[0].min(abc[1]).min(abc[2]) };
                (d, *face)
            }
            Shape::Harmonic { r, coef, face } => {
                let n = dot(p, p).sqrt();
                if n < 1e-12 {
                    return (-harmonic_radius(*r, coef, [0.0, 0.0, 1.0]), *face);
                }
                let rr = harmonic_radius(*r, coef, [p[0] / n, p[1] / n, p[2] / n]);
                // Scaled down: the radial distance overestimates where the surface is slanted.
                ((n - rr) * 0.7, *face)
            }
            Shape::Extrude { outline, h, curve_faces, bottom, top } => {
                let (d2, c) = outline.distance([p[0], p[1]]);
                let dz = (p[2] - h / 2.0).abs() - h / 2.0;
                (
                    box2(d2, dz),
                    if d2 >= dz {
                        curve_faces[c]
                    } else if p[2] < h / 2.0 {
                        *bottom
                    } else {
                        *top
                    },
                )
            }
            Shape::Revolve { outline, angle, curve_faces, start, end } => {
                let r = len2(p[0], p[1]);
                let (d2, c) = outline.distance([r, p[2]]);
                if *angle >= std::f64::consts::TAU - 1e-9 {
                    return (d2, curve_faces[c]);
                }
                let phi = p[1].atan2(p[0]).rem_euclid(std::f64::consts::TAU);
                if phi <= *angle {
                    let delta = phi.min(angle - phi);
                    let dcut = r * delta.min(std::f64::consts::FRAC_PI_2).sin();
                    if -dcut > d2 {
                        (-dcut, if phi < angle - phi { *start } else { *end })
                    } else {
                        (d2, curve_faces[c])
                    }
                } else {
                    // Outside the wedge: distance to the nearer cut face (a planar region).
                    let (to0, to1) = (std::f64::consts::TAU - phi, phi - angle);
                    let (a, f) = if to0 < to1 { (0.0, *start) } else { (*angle, *end) };
                    let u = r * (phi - a).cos();
                    let w = r * (phi - a).sin();
                    let (dp, _) = outline.distance([u, p[2]]);
                    (len2(dp.max(0.0), w), f)
                }
            }
            Shape::Sweep { outline, path, curve_faces, start, end } => {
                // Nearest segment of the path.
                let mut best = (f64::INFINITY, 0usize, 0.0);
                for (i, w) in path.points.windows(2).enumerate() {
                    let e = sub(w[1], w[0]);
                    let t = (dot(sub(p, w[0]), e) / dot(e, e).max(1e-300)).clamp(0.0, 1.0);
                    let q = [w[0][0] + e[0] * t, w[0][1] + e[1] * t, w[0][2] + e[2] * t];
                    let d = dot(sub(p, q), sub(p, q));
                    if d < best.0 {
                        best = (d, i, t);
                    }
                }
                let (_, i, t) = best;
                let (tn, n, b) = path.frames[i];
                let a = path.points[i];
                let rel = sub(p, a);
                let along = dot(rel, tn);
                let (d2, c) = outline.distance([dot(rel, n), dot(rel, b)]);
                let last = path.frames.len() - 1;
                let cap = if i == 0 && t <= 0.0 && along < 0.0 {
                    Some((-along, *start))
                } else if i == last && t >= 1.0 {
                    let e = sub(path.points[i + 1], a);
                    Some((along - dot(e, tn), *end))
                } else {
                    None
                };
                match cap {
                    Some((dc, f)) if dc > d2 => (box2(d2, dc), f),
                    Some((dc, _)) => (box2(d2, dc), curve_faces[c]),
                    None => (d2, curve_faces[c]),
                }
            }
            Shape::Surface { mesh, face } => {
                // Patches between sharp edges are faces of their own, numbered from `face`.
                let (d, k) = mesh.distance_patch(p);
                (d, if *face == 0 { 0 } else { face + k })
            }
            Shape::Transform { inner, rot, offset, scale } => {
                let q = sub(p, *offset);
                // rot^T q
                let l: V3 = std::array::from_fn(|k| (rot[0][k] * q[0] + rot[1][k] * q[1] + rot[2][k] * q[2]) / scale[k]);
                let (d, f) = inner.eval(l);
                (d * scale[0].min(scale[1]).min(scale[2]), f)
            }
            Shape::HalfSpace { normal, offset, face } => (normal[0] * p[0] + normal[1] * p[1] + normal[2] * p[2] - offset, *face),
            Shape::Sheet { mesh, face } => (mesh.distance(p), *face),
            Shape::Union(v) => v.iter().map(|s| s.eval(p)).min_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or((f64::INFINITY, 0)),
            Shape::Intersect(v) => v.iter().map(|s| s.eval(p)).max_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or((f64::INFINITY, 0)),
            Shape::Subtract(a, b) => {
                let (da, fa) = a.eval(p);
                let (db, fb) = b.eval(p);
                if da >= -db {
                    (da, fa)
                } else {
                    (-db, fb)
                }
            }
        }
    }

    /// A box holding the solid (lower, upper corner).
    pub fn bounds(&self) -> (V3, V3) {
        match self {
            Shape::Brick { half, .. } => (half.map(|h| -h), *half),
            Shape::Sphere { r, .. } => ([-r; 3], [*r; 3]),
            Shape::Cylinder { r, h, .. } => ([-r, -r, -h / 2.0], [*r, *r, h / 2.0]),
            Shape::Cone { r1, r2, h, .. } => {
                let r = r1.max(*r2);
                ([-r, -r, -h / 2.0], [r, r, h / 2.0])
            }
            Shape::Torus { major, minor, .. } => {
                let r = major + minor;
                ([-r, -r, -minor], [r, r, *minor])
            }
            Shape::Superquadric { abc, .. } => (abc.map(|a| -a), *abc),
            Shape::Harmonic { r, coef, .. } => {
                // Bound of the radius over the sphere by sampling, with a margin.
                let mut m: f64 = 0.0;
                for i in 0..=24 {
                    for j in 0..48 {
                        let th = std::f64::consts::PI * i as f64 / 24.0;
                        let ph = std::f64::consts::TAU * j as f64 / 48.0;
                        m = m.max(harmonic_radius(*r, coef, [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()]));
                    }
                }
                let m = m * 1.1;
                ([-m; 3], [m; 3])
            }
            Shape::Extrude { outline, h, .. } => {
                let (lo, hi) = outline.bounds();
                ([lo[0], lo[1], 0.0], [hi[0], hi[1], *h])
            }
            Shape::Revolve { outline, .. } => {
                let (lo, hi) = outline.bounds();
                let r = hi[0].abs().max(lo[0].abs());
                ([-r, -r, lo[1]], [r, r, hi[1]])
            }
            Shape::Sweep { outline, path, .. } => {
                let (lo, hi) = outline.bounds();
                let rad = lo[0].abs().max(hi[0].abs()).max(lo[1].abs()).max(hi[1].abs()) * std::f64::consts::SQRT_2;
                let (mut a, mut b) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for q in &path.points {
                    for k in 0..3 {
                        a[k] = a[k].min(q[k] - rad);
                        b[k] = b[k].max(q[k] + rad);
                    }
                }
                (a, b)
            }
            Shape::Surface { mesh, .. } => (mesh.lo, mesh.hi),
            Shape::Transform { inner, rot, offset, scale } => {
                let (lo, hi) = inner.bounds();
                let (mut a, mut b) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for c in 0..8 {
                    let l: V3 = std::array::from_fn(|k| if (c >> k) & 1 == 0 { lo[k] } else { hi[k] } * scale[k]);
                    let w: V3 = std::array::from_fn(|i| rot[i][0] * l[0] + rot[i][1] * l[1] + rot[i][2] * l[2] + offset[i]);
                    for k in 0..3 {
                        a[k] = a[k].min(w[k]);
                        b[k] = b[k].max(w[k]);
                    }
                }
                (a, b)
            }
            Shape::Union(v) => v.iter().map(|s| s.bounds()).fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(a, b), (c, d)| {
                (std::array::from_fn(|k| a[k].min(c[k])), std::array::from_fn(|k| b[k].max(d[k])))
            }),
            Shape::Intersect(v) => v.iter().map(|s| s.bounds()).fold(([f64::NEG_INFINITY; 3], [f64::INFINITY; 3]), |(a, b), (c, d)| {
                (std::array::from_fn(|k| a[k].max(c[k])), std::array::from_fn(|k| b[k].min(d[k])))
            }),
            Shape::Subtract(a, _) => a.bounds(),
            Shape::HalfSpace { .. } | Shape::Sheet { .. } => ([f64::NEG_INFINITY; 3], [f64::INFINITY; 3]),
        }
    }

    /// The faces the solid can show, in order of first appearance.
    pub fn faces(&self) -> Vec<FaceId> {
        let mut out = Vec::new();
        self.collect_faces(&mut out);
        let mut seen = std::collections::HashSet::new();
        out.retain(|f| *f > 0 && seen.insert(*f));
        out
    }

    fn collect_faces(&self, out: &mut Vec<FaceId>) {
        match self {
            Shape::Brick { faces, .. } => out.extend(faces),
            Shape::Cylinder { faces, .. } | Shape::Cone { faces, .. } => out.extend(faces),
            Shape::Sphere { face, .. } | Shape::Torus { face, .. } | Shape::Superquadric { face, .. } | Shape::Harmonic { face, .. } => out.push(*face),
            Shape::Surface { face, mesh } => out.extend((0..mesh.n_patches).map(|k| face + k)),
            Shape::Extrude { curve_faces, bottom, top, .. } => {
                out.extend(curve_faces);
                out.extend([bottom, top]);
            }
            Shape::Revolve { curve_faces, start, end, angle, .. } => {
                out.extend(curve_faces);
                if *angle < std::f64::consts::TAU - 1e-9 {
                    out.extend([start, end]);
                }
            }
            Shape::Sweep { curve_faces, start, end, .. } => {
                out.extend(curve_faces);
                out.extend([start, end]);
            }
            Shape::Transform { inner, .. } => inner.collect_faces(out),
            Shape::Union(v) | Shape::Intersect(v) => v.iter().for_each(|s| s.collect_faces(out)),
            Shape::HalfSpace { face, .. } | Shape::Sheet { face, .. } => {
                if !out.contains(face) {
                    out.push(*face);
                }
            }
            Shape::Subtract(a, b) => {
                a.collect_faces(out);
                b.collect_faces(out);
            }
        }
    }
}
