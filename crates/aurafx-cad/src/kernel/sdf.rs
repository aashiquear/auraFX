//! SDF expression tree. Distances in mm; negative inside.
//!
//! Ported from EZ-YX-craft (`ezyx-cad`, MIT OR Apache-2.0).
//!
//! Every node is (approximately) 1-Lipschitz so a sphere tracer can step by the field
//! value; fields that are not (loft interpolation, helical threads, drafted prisms) are scaled down.

use crate::document::{Axis, EdgeTreatment, EndCondition, HoleKind, MirrorPlane, Modifier, Shape};
use crate::path::{dist_point_segment, Bounds2, Polyline};
use glam::{DMat3, DQuat, DVec2, DVec3, EulerRot};
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

/// Field value of an empty solid (finite so smooth blends stay well-defined).
pub const EMPTY_D: f64 = 1e9;
/// Half-depth of a `ThroughAll` extrusion (mm).
pub const THROUGH_ALL: f64 = 5000.0;

#[derive(Debug, Clone)]
pub enum Sdf {
    Box {
        half: DVec3,
        r: f64,
    },
    Sphere {
        r: f64,
    },
    Cylinder {
        r: f64,
        h: f64,
    },
    Cone {
        r0: f64,
        r1: f64,
        h: f64,
    },
    Torus {
        major: f64,
        minor: f64,
    },
    Extrude {
        loops: Vec<Polyline>,
        h: f64,
        draft: f64,
    },
    Revolve {
        loops: Vec<Polyline>,
        angle: f64,
    },
    Union(Box<Sdf>, Box<Sdf>, f64),
    Subtract(Box<Sdf>, Box<Sdf>, f64),
    Intersect(Box<Sdf>, Box<Sdf>, f64),
    Offset(Box<Sdf>, f64),
    Shell(Box<Sdf>, f64),
    Transform {
        inner: Box<Sdf>,
        translate: [f64; 3],
        rotate_deg: [f64; 3],
    },
    /// Keeps the original and adds its reflection.
    Mirror(Box<Sdf>, MirrorPlane),
    Repeat {
        inner: Box<Sdf>,
        count: usize,
        step: DVec3,
    },
    Polar {
        inner: Box<Sdf>,
        count: usize,
    },
    // ---- solid-modeler additions ----
    /// Nothing (e.g. an empty embedded body).
    Empty,
    /// Even-odd 2D profile between `z0..z1` with draft (tan) and cap-edge treatments.
    Prism {
        profile: Profile2,
        z0: f64,
        z1: f64,
        draft: f64,
        top: EdgeTreatment,
        bottom: EdgeTreatment,
    },
    /// Rigid frame: `inner` is evaluated at `rot⁻¹ (p − origin)` (rot maps local → world).
    Frame {
        inner: Box<Sdf>,
        rot: DMat3,
        origin: DVec3,
    },
    /// Profile in the (radius, z) half-plane revolved around Z by `angle` radians from +X.
    RevolveProfile {
        profile: Profile2,
        angle: f64,
    },
    /// Profile swept along a polyline with mitred joints and parallel-transported frames.
    Sweep {
        profile: Profile2,
        segs: Vec<SweepSeg>,
        twist: f64,
        reach: f64,
    },
    /// Piecewise-linear interpolation of 2D fields between sections at increasing Z.
    Loft {
        sections: Vec<(Profile2, f64)>,
        scale: f64,
    },
    /// Cylinder around Z between `z0..z1`.
    CylZ {
        r: f64,
        z0: f64,
        z1: f64,
    },
    /// Truncated cone around Z: radius `r0` at `z0`, `r1` at `z1` (z0 < z1).
    ConeZ {
        r0: f64,
        z0: f64,
        r1: f64,
        z1: f64,
    },
    /// Helical 60° V thread (single start, right hand) along +Z from 0 to `len`.
    Thread {
        r: f64,
        pitch: f64,
        depth: f64,
        len: f64,
    },
    UnionChamfer(Box<Sdf>, Box<Sdf>, f64),
    SubtractChamfer(Box<Sdf>, Box<Sdf>, f64),
    IntersectChamfer(Box<Sdf>, Box<Sdf>, f64),
    /// Keep one side of the plane `p[axis] = offset`.
    Split {
        inner: Box<Sdf>,
        axis: usize,
        offset: f64,
        keep_positive: bool,
    },
    /// Inward shell of thickness `t`; with `open_z` the cavity is extruded upward from that height.
    ShellOpen {
        inner: Box<Sdf>,
        t: f64,
        open_z: Option<f64>,
    },
    Scale(Box<Sdf>, f64),
    /// `count` copies rotated by `step` radians about `axis` (0 = X, 1 = Y, 2 = Z).
    RotRepeat {
        inner: Box<Sdf>,
        count: usize,
        step: f64,
        axis: usize,
    },
}

/// One mitred segment of a sweep.
#[derive(Debug, Clone)]
pub struct SweepSeg {
    pub a: DVec3,
    pub t: DVec3,
    pub n: DVec3,
    pub b: DVec3,
    pub len: f64,
    /// Arc length at `a`.
    pub s0: f64,
    /// Forward-pointing normals of the start / end (mitre) planes.
    pub m0: DVec3,
    pub m1: DVec3,
}

/// Even-odd 2D profile with per-loop bounds for fast rejection.
#[derive(Debug, Clone)]
pub struct Profile2 {
    pub loops: Vec<(Polyline, Bounds2)>,
    pub bounds: Bounds2,
}

impl Default for Profile2 {
    fn default() -> Self {
        Self { loops: vec![], bounds: Bounds2::EMPTY }
    }
}

impl Profile2 {
    pub fn new(loops: &[Polyline]) -> Self {
        let loops: Vec<(Polyline, Bounds2)> = loops.iter().filter(|l| l.closed && l.len() >= 3).map(|l| (l.clone(), l.bounds())).collect();
        let bounds = loops.iter().fold(Bounds2::EMPTY, |a, l| a.union(&l.1));
        Self { loops, bounds }
    }
    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }
    /// Signed distance (negative inside, even-odd).
    pub fn dist(&self, p: DVec2) -> f64 {
        if self.loops.is_empty() {
            return EMPTY_D;
        }
        let mut d = f64::INFINITY;
        let mut inside = false;
        for (l, b) in &self.loops {
            let bd = (b.min - p).max(p - b.max).max(DVec2::ZERO).length();
            if bd < d {
                d = d.min(l.segments().map(|(a, b)| dist_point_segment(p, a, b)).fold(f64::INFINITY, f64::min));
            }
            if bd == 0.0 && l.contains(p) {
                inside = !inside;
            }
        }
        if inside {
            -d
        } else {
            d
        }
    }
    /// Largest distance of any vertex from the profile origin.
    pub fn max_radius(&self) -> f64 {
        self.loops.iter().flat_map(|l| l.0.pts.iter()).map(|p| p.length()).fold(0.0, f64::max)
    }
    /// Approximate Hausdorff distance between the boundaries of two profiles.
    fn hausdorff(&self, o: &Profile2) -> f64 {
        let one = |a: &Profile2, b: &Profile2| a.loops.iter().flat_map(|l| l.0.pts.iter()).map(|p| b.dist(*p).abs()).fold(0.0, f64::max);
        one(self, o).max(one(o, self))
    }
}

/// SDF of a 2D quarter-plane intersection `a ≤ 0 ∧ b ≤ 0` with an edge treatment at the corner.
fn edge2(a: f64, b: f64, e: &EdgeTreatment) -> f64 {
    let exact = |a: f64, b: f64| a.max(b).min(0.0) + DVec2::new(a.max(0.0), b.max(0.0)).length();
    match e {
        EdgeTreatment::None => exact(a, b),
        EdgeTreatment::Fillet { radius } => {
            let r = radius.max(0.0);
            exact(a + r, b + r) - r
        }
        EdgeTreatment::Chamfer { distance } => exact(a, b).max((a + b + distance.max(0.0)) * FRAC_1_SQRT_2),
    }
}

/// 2D angular sector [0, angle] around the origin (half-plane combination; exact on the zero set).
fn wedge(q: DVec2, angle: f64) -> f64 {
    let a = -q.y;
    let (s, c) = angle.sin_cos();
    let b = c * q.y - s * q.x;
    if angle <= PI {
        a.max(b)
    } else {
        a.min(b)
    }
}

pub fn shape(s: &Shape) -> Sdf {
    match s {
        Shape::Box { size, corner_radius } => Sdf::Box { half: DVec3::from_array(*size) * 0.5, r: *corner_radius },
        Shape::Sphere { radius } => Sdf::Sphere { r: *radius },
        Shape::Cylinder { radius, height } => Sdf::Cylinder { r: *radius, h: *height },
        Shape::Cone { r_bottom, r_top, height } => Sdf::Cone { r0: *r_bottom, r1: *r_top, h: *height },
        Shape::Torus { major, minor } => Sdf::Torus { major: *major, minor: *minor },
        Shape::Extrude { sketch, height, draft_deg } => Sdf::Extrude { loops: sketch.clone(), h: *height, draft: draft_deg.to_radians().tan() },
        Shape::Revolve { profile, angle_deg } => Sdf::Revolve { loops: profile.clone(), angle: angle_deg.to_radians() },
        Shape::SketchExtrude { sketch, depth, end, draft_deg, top_edge, bottom_edge, reverse } => {
            let d = depth.abs();
            let (z0, z1, draft) = match end {
                EndCondition::Blind if *reverse => (-d, 0.0, draft_deg.to_radians().tan()),
                EndCondition::Blind => (0.0, d, draft_deg.to_radians().tan()),
                EndCondition::MidPlane => (-d * 0.5, d * 0.5, draft_deg.to_radians().tan()),
                EndCondition::ThroughAll => (-THROUGH_ALL, THROUGH_ALL, 0.0),
            };
            let cap = (z1 - z0) * 0.5;
            let clamp_edge = |e: &EdgeTreatment| match e {
                EdgeTreatment::Fillet { radius } => EdgeTreatment::Fillet { radius: radius.min(cap) },
                EdgeTreatment::Chamfer { distance } => EdgeTreatment::Chamfer { distance: distance.min(cap) },
                EdgeTreatment::None => EdgeTreatment::None,
            };
            let prism = Sdf::Prism { profile: Profile2::new(&sketch.profiles()), z0, z1, draft, top: clamp_edge(top_edge), bottom: clamp_edge(bottom_edge) };
            let (rot, origin) = sketch.plane.frame(sketch.offset);
            Sdf::Frame { inner: Box::new(prism), rot, origin }
        }
        Shape::SketchRevolve { sketch, angle_deg } => {
            let rev = Sdf::RevolveProfile { profile: Profile2::new(&sketch.profiles()), angle: angle_deg.to_radians().clamp(0.0, TAU) };
            let (rot, origin) = sketch.plane.frame(sketch.offset);
            // Revolve space (axis Z) → sketch local (axis = local y): Rx(−90°).
            Sdf::Frame { inner: Box::new(rev), rot: rot * DMat3::from_rotation_x(-PI / 2.0), origin }
        }
        Shape::Sweep { profile, path, twist_deg } => sweep(profile, path, *twist_deg),
        Shape::Loft { sections } => {
            let mut secs: Vec<(Profile2, f64)> = sections.iter().map(|s| (Profile2::new(&s.profile), s.z)).filter(|s| !s.0.is_empty()).collect();
            secs.sort_by(|a, b| a.1.total_cmp(&b.1));
            let mut lip: f64 = 1.0;
            for w in secs.windows(2) {
                let dz = (w[1].1 - w[0].1).max(1e-6);
                let h = w[0].0.hausdorff(&w[1].0) * 1.05;
                lip = lip.max((1.0 + (h / dz).powi(2)).sqrt());
            }
            Sdf::Loft { sections: secs, scale: 1.0 / lip }
        }
        Shape::Hole { kind, diameter, depth } => hole(kind, *diameter, *depth),
        Shape::Thread { major_diameter, pitch, length, internal, depth } => {
            let p = pitch.max(1e-3);
            let d = if *depth > 0.0 {
                *depth
            } else if *internal {
                0.5413 * p
            } else {
                0.6134 * p
            };
            Sdf::Thread { r: major_diameter * 0.5, pitch: p, depth: d.min(major_diameter * 0.45), len: *length }
        }
        Shape::Body { part } => part.sdf().unwrap_or(Sdf::Empty),
    }
}

fn sweep(profile: &[Polyline], path: &[[f64; 3]], twist_deg: f64) -> Sdf {
    let pts: Vec<DVec3> = path.iter().map(|p| DVec3::from_array(*p)).fold(Vec::new(), |mut v, p| {
        if v.last().is_none_or(|l: &DVec3| l.distance(p) > 1e-9) {
            v.push(p);
        }
        v
    });
    let prof = Profile2::new(profile);
    if pts.len() < 2 || prof.is_empty() {
        return Sdf::Empty;
    }
    let tangents: Vec<DVec3> = pts.windows(2).map(|w| (w[1] - w[0]).normalize()).collect();
    // Initial frame: profile y follows world Z (or Y when the path starts vertical).
    let t0 = tangents[0];
    let up = if t0.z.abs() < 0.999 { DVec3::Z } else { DVec3::Y };
    let mut b = (up - t0 * t0.dot(up)).normalize();
    let mut n = b.cross(t0);
    let mut segs = Vec::new();
    let mut s0 = 0.0;
    for (i, t) in tangents.iter().enumerate() {
        if i > 0 {
            // Parallel transport (discrete rotation-minimising frame).
            let q = DQuat::from_rotation_arc(tangents[i - 1], *t);
            n = q * n;
            b = q * b;
        }
        let len = pts[i].distance(pts[i + 1]);
        let m0 = if i == 0 { *t } else { (tangents[i - 1] + *t).normalize_or(*t) };
        let m1 = if i + 1 == tangents.len() { *t } else { (*t + tangents[i + 1]).normalize_or(*t) };
        segs.push(SweepSeg { a: pts[i], t: *t, n, b, len, s0, m0, m1 });
        s0 += len;
    }
    let reach = prof.max_radius();
    Sdf::Sweep { profile: prof, segs, twist: twist_deg.to_radians() / s0.max(1e-9), reach }
}

fn hole(kind: &HoleKind, diameter: f64, depth: f64) -> Sdf {
    let r = diameter.abs() * 0.5;
    let depth = depth.abs();
    let union = |a: Sdf, b: Sdf| Sdf::Union(Box::new(a), Box::new(b), 0.0);
    // Every piece runs up to a clearance height above the origin so the cut breaks cleanly through
    // the face, and pieces overlap rather than abut: a union's interior field is ≈ 0 on an abutting
    // seam, which would leave a false surface inside the cut.
    let top = |r: f64| r + 1.0;
    match kind {
        HoleKind::Simple => Sdf::CylZ { r, z0: -depth, z1: top(r) },
        HoleKind::Drilled => {
            let tip = r / 59f64.to_radians().tan();
            let apex = -depth - tip;
            // Cylinder ∩ upward cone with its apex at the drill point (118° included angle).
            let z1 = top(r);
            let cone = Sdf::ConeZ { r0: 0.0, z0: apex, r1: (z1 - apex) * 59f64.to_radians().tan(), z1 };
            Sdf::Intersect(Box::new(Sdf::CylZ { r, z0: apex, z1 }), Box::new(cone), 0.0)
        }
        HoleKind::Counterbore { diameter: cd, depth: cdepth } => {
            let rc = (cd * 0.5).max(r);
            union(Sdf::CylZ { r, z0: -depth, z1: top(rc) }, Sdf::CylZ { r: rc, z0: -cdepth.abs(), z1: top(rc) })
        }
        HoleKind::Countersink { diameter: cd, angle_deg } => {
            let rc = (cd * 0.5).max(r);
            let tan = (angle_deg * 0.5).clamp(1.0, 89.0).to_radians().tan();
            let h = (rc - r) / tan;
            let z1 = top(rc);
            // The sink cone keeps widening above the face (only air there).
            union(Sdf::CylZ { r, z0: -depth, z1 }, Sdf::ConeZ { r0: r, z0: -h, r1: rc + z1 * tan, z1 })
        }
        HoleKind::Tapped { pitch, cosmetic } => {
            let p = pitch.abs().max(1e-3);
            let minor = (r - 0.5413 * p).max(r * 0.3);
            let core = Sdf::CylZ { r: minor, z0: -depth, z1: top(minor) };
            if *cosmetic {
                core
            } else {
                let th = Sdf::Thread { r, pitch: p, depth: r - minor, len: depth };
                union(Sdf::Frame { inner: Box::new(th), rot: DMat3::IDENTITY, origin: DVec3::new(0.0, 0.0, -depth) }, core)
            }
        }
    }
}

pub fn modify(s: Sdf, m: &Modifier) -> Sdf {
    match m {
        // Fillet all edges: shrink then grow (rounds convex edges by r).
        Modifier::Fillet { radius } => Sdf::Offset(Box::new(Sdf::Offset(Box::new(s), -*radius)), *radius),
        Modifier::Shell { thickness } => Sdf::Shell(Box::new(s), *thickness),
        Modifier::LinearPattern { count, step } => Sdf::Repeat { inner: Box::new(s), count: *count, step: DVec3::from_array(*step) },
        Modifier::CircularPattern { count, .. } => Sdf::Polar { inner: Box::new(s), count: *count },
        Modifier::Mirror { plane } => Sdf::Mirror(Box::new(s), *plane),
        Modifier::Split { plane, offset, keep_positive } => {
            Sdf::Split { inner: Box::new(s), axis: plane_axis(*plane), offset: *offset, keep_positive: *keep_positive }
        }
        Modifier::ShellOpen { thickness, open_top } => {
            let t = thickness.abs();
            let open_z = open_top.then(|| top_z(&s) - 2.0 * t);
            Sdf::ShellOpen { inner: Box::new(s), t, open_z }
        }
        Modifier::GridPattern { counts, step } => {
            let row = Sdf::Repeat { inner: Box::new(s), count: counts[0], step: DVec3::new(step[0], 0.0, 0.0) };
            Sdf::Repeat { inner: Box::new(row), count: counts[1], step: DVec3::new(0.0, step[1], 0.0) }
        }
        Modifier::CircularPatternAngle { count, total_deg, axis } => {
            let count = (*count).max(1);
            let step = if total_deg.abs() >= 360.0 - 1e-9 {
                TAU / count as f64
            } else if count > 1 {
                total_deg.to_radians() / (count - 1) as f64
            } else {
                0.0
            };
            let axis = match axis {
                Axis::X => 0,
                Axis::Y => 1,
                Axis::Z => 2,
            };
            Sdf::RotRepeat { inner: Box::new(s), count, step, axis }
        }
        Modifier::Scale { factor } => Sdf::Scale(Box::new(s), if factor.abs() > 1e-9 { factor.abs() } else { 1.0 }),
    }
}

fn plane_axis(p: MirrorPlane) -> usize {
    match p {
        MirrorPlane::YZ => 0,
        MirrorPlane::XZ => 1,
        MirrorPlane::XY => 2,
    }
}

/// Highest Z of the actual solid (sphere-traced on a coarse XY grid), falls back to the bounds.
pub fn top_z(s: &Sdf) -> f64 {
    let (lo, hi) = s.bounds();
    let mut best = f64::NEG_INFINITY;
    let n = 12;
    let step = ((hi.z - lo.z) / 2000.0).max(1e-3);
    for j in 0..=n {
        for i in 0..=n {
            let x = lo.x + (hi.x - lo.x) * i as f64 / n as f64;
            let y = lo.y + (hi.y - lo.y) * j as f64 / n as f64;
            let mut z = hi.z + 1.0;
            for _ in 0..400 {
                let d = s.eval(DVec3::new(x, y, z));
                if d < 1e-4 {
                    best = best.max(z);
                    break;
                }
                z -= d.max(step);
                if z < lo.z {
                    break;
                }
            }
        }
    }
    if best.is_finite() {
        best
    } else {
        hi.z
    }
}

fn smin(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}
fn smax(a: f64, b: f64, k: f64) -> f64 {
    -smin(-a, -b, k)
}

/// Signed distance to even-odd polygon set.
pub fn polygon_sdf(loops: &[Polyline], p: DVec2) -> f64 {
    let mut d = f64::INFINITY;
    let mut inside = false;
    for l in loops {
        d = d.min(l.segments().map(|(a, b)| dist_point_segment(p, a, b)).fold(f64::INFINITY, f64::min));
        if l.contains(p) {
            inside = !inside;
        }
    }
    if inside {
        -d
    } else {
        d
    }
}

fn rot(deg: [f64; 3]) -> DMat3 {
    DMat3::from_quat(DQuat::from_euler(EulerRot::XYZ, deg[0].to_radians(), deg[1].to_radians(), deg[2].to_radians()))
}

fn axis_rot(axis: usize, a: f64) -> DMat3 {
    match axis {
        0 => DMat3::from_rotation_x(a),
        1 => DMat3::from_rotation_y(a),
        _ => DMat3::from_rotation_z(a),
    }
}

fn transform_bounds(l: DVec3, h: DVec3, r: DMat3, t: DVec3) -> (DVec3, DVec3) {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for i in 0..8 {
        let c = DVec3::new(if i & 1 == 0 { l.x } else { h.x }, if i & 2 == 0 { l.y } else { h.y }, if i & 4 == 0 { l.z } else { h.z });
        let w = r * c + t;
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo, hi)
}

fn set_axis(mut v: DVec3, axis: usize, x: f64) -> DVec3 {
    v[axis] = x;
    v
}

impl Sdf {
    pub fn eval(&self, p: DVec3) -> f64 {
        match self {
            Sdf::Box { half, r } => {
                let q = p.abs() - (*half - DVec3::splat(*r));
                q.max(DVec3::ZERO).length() + q.x.max(q.y.max(q.z)).min(0.0) - r
            }
            Sdf::Sphere { r } => p.length() - r,
            Sdf::Cylinder { r, h } => {
                // Base at z = 0, top at z = h.
                let d = DVec2::new(p.truncate().length() - r, (p.z - h * 0.5).abs() - h * 0.5);
                d.x.max(d.y).min(0.0) + d.max(DVec2::ZERO).length()
            }
            Sdf::Cone { r0, r1, h } => {
                let t = (p.z / h).clamp(0.0, 1.0);
                let r = r0 + (r1 - r0) * t;
                let slope = ((r1 - r0) / h).atan().cos();
                let radial = (p.truncate().length() - r) * slope;
                radial.max(-p.z).max(p.z - h)
            }
            Sdf::Torus { major, minor } => DVec2::new(p.truncate().length() - major, p.z).length() - minor,
            Sdf::Extrude { loops, h, draft } => {
                let d2 = polygon_sdf(loops, p.truncate()) + p.z.clamp(0.0, *h) * draft;
                let dz = (p.z - h * 0.5).abs() - h * 0.5;
                let w = DVec2::new(d2, dz);
                w.x.max(w.y).min(0.0) + w.max(DVec2::ZERO).length()
            }
            Sdf::Revolve { loops, angle } => {
                let r = p.truncate().length();
                let d = polygon_sdf(loops, DVec2::new(r, p.z));
                if *angle < TAU - 1e-6 {
                    d.max(wedge(p.truncate(), *angle))
                } else {
                    d
                }
            }
            Sdf::Union(a, b, k) => smin(a.eval(p), b.eval(p), *k),
            Sdf::Subtract(a, b, k) => smax(a.eval(p), -b.eval(p), *k),
            Sdf::Intersect(a, b, k) => smax(a.eval(p), b.eval(p), *k),
            Sdf::Offset(a, d) => a.eval(p) - d,
            Sdf::Shell(a, t) => a.eval(p).abs() - t * 0.5,
            Sdf::Transform { inner, translate, rotate_deg } => {
                let q = rot(*rotate_deg).transpose() * (p - DVec3::from_array(*translate));
                inner.eval(q)
            }
            Sdf::Mirror(a, plane) => {
                let q = match plane {
                    MirrorPlane::YZ => DVec3::new(-p.x, p.y, p.z),
                    MirrorPlane::XZ => DVec3::new(p.x, -p.y, p.z),
                    MirrorPlane::XY => DVec3::new(p.x, p.y, -p.z),
                };
                a.eval(p).min(a.eval(q))
            }
            Sdf::Repeat { inner, count, step } => (0..(*count).max(1)).map(|i| inner.eval(p - *step * i as f64)).fold(f64::INFINITY, f64::min),
            Sdf::Polar { inner, count } => {
                let n = (*count).max(1) as f64;
                let seg = TAU / n;
                let a = p.y.atan2(p.x);
                let a = (a + seg * 0.5).rem_euclid(seg) - seg * 0.5;
                let r = p.truncate().length();
                inner.eval(DVec3::new(r * a.cos(), r * a.sin(), p.z))
            }
            Sdf::Empty => EMPTY_D,
            Sdf::Prism { profile, z0, z1, draft, top, bottom } => {
                let zc = p.z.clamp(*z0, *z1).abs();
                let d2 = profile.dist(p.truncate()) + zc * draft;
                let d = edge2(d2, p.z - z1, top).max(edge2(d2, z0 - p.z, bottom));
                d / (1.0 + draft * draft).sqrt()
            }
            Sdf::Frame { inner, rot, origin } => inner.eval(rot.transpose() * (p - *origin)),
            Sdf::RevolveProfile { profile, angle } => {
                let d = profile.dist(DVec2::new(p.truncate().length(), p.z));
                if *angle < TAU - 1e-6 {
                    d.max(wedge(p.truncate(), *angle))
                } else {
                    d
                }
            }
            Sdf::Sweep { profile, segs, twist, reach } => {
                let mut best = EMPTY_D;
                let last = segs.len().saturating_sub(1);
                for (i, s) in segs.iter().enumerate() {
                    let d = p - s.a;
                    let w = d.dot(s.t);
                    // Lower bound via distance to the segment axis: skip far segments.
                    let axial = (d - s.t * w.clamp(0.0, s.len)).length();
                    if axial - reach > best {
                        continue;
                    }
                    let ang = -twist * (s.s0 + w.clamp(0.0, s.len));
                    let q = DVec2::new(d.dot(s.n), d.dot(s.b)).rotate(DVec2::from_angle(ang));
                    let (lo, hi) = (-d.dot(s.m0), (p - (s.a + s.t * s.len)).dot(s.m1));
                    // Mitre planes only gate membership (so joints have no interior seam); the
                    // first/last planes are real end caps.
                    let gate = (if i > 0 { lo } else { f64::NEG_INFINITY }).max(if i < last { hi } else { f64::NEG_INFINITY });
                    let cap = (if i == 0 { lo } else { f64::NEG_INFINITY }).max(if i == last { hi } else { f64::NEG_INFINITY });
                    let d2 = profile.dist(q);
                    best = best.min(if gate <= 0.0 { d2.max(cap) } else { d2.max(lo.max(hi)) });
                }
                best
            }
            Sdf::Loft { sections, scale } => {
                let n = sections.len();
                if n == 0 {
                    return EMPTY_D;
                }
                let (zf, zl) = (sections[0].1, sections[n - 1].1);
                let q = p.truncate();
                let d2 = if n == 1 || p.z <= zf {
                    sections[0].0.dist(q)
                } else if p.z >= zl {
                    sections[n - 1].0.dist(q)
                } else {
                    let k = sections.windows(2).position(|w| p.z <= w[1].1).unwrap_or(n - 2);
                    let (a, b) = (&sections[k], &sections[k + 1]);
                    let t = ((p.z - a.1) / (b.1 - a.1).max(1e-9)).clamp(0.0, 1.0);
                    a.0.dist(q) * (1.0 - t) + b.0.dist(q) * t
                };
                edge2(d2, (zf - p.z).max(p.z - zl), &EdgeTreatment::None) * scale
            }
            Sdf::CylZ { r, z0, z1 } => edge2(p.truncate().length() - r, (z0 - p.z).max(p.z - z1), &EdgeTreatment::None),
            Sdf::ConeZ { r0, z0, r1, z1 } => {
                let h = (z1 - z0).max(1e-9);
                let t = ((p.z - z0) / h).clamp(0.0, 1.0);
                let c = ((r1 - r0) / h).atan().cos();
                ((p.truncate().length() - (r0 + (r1 - r0) * t)) * c).max(z0 - p.z).max(p.z - z1)
            }
            Sdf::Thread { r, pitch, depth, len } => {
                let rho = p.truncate().length();
                let theta = p.y.atan2(p.x);
                let phase = p.z / pitch - theta / TAU;
                let t = (phase - phase.floor() - 0.5).abs() * pitch; // 0 at the crest centre
                let sharp = r + 3f64.sqrt() * pitch / 16.0; // crest flat = P/8
                let rp = (sharp - 3f64.sqrt() * t).clamp(r - depth, *r);
                // |∇(ρ − rp)| ≤ √(4 + 3(P / 2πρ)²) on the flanks → scale to stay Lipschitz.
                let rmin = (r - depth).max(pitch * 0.25);
                let k = 1.0 / (4.0 + 3.0 * (pitch / (TAU * rmin)).powi(2)).sqrt();
                ((rho - rp) * k).max(-p.z).max(p.z - len)
            }
            Sdf::UnionChamfer(a, b, r) => {
                let (a, b) = (a.eval(p), b.eval(p));
                a.min(b).min((a + b - r) * FRAC_1_SQRT_2)
            }
            Sdf::SubtractChamfer(a, b, r) => {
                let (a, b) = (a.eval(p), -b.eval(p));
                a.max(b).max((a + b + r) * FRAC_1_SQRT_2)
            }
            Sdf::IntersectChamfer(a, b, r) => {
                let (a, b) = (a.eval(p), b.eval(p));
                a.max(b).max((a + b + r) * FRAC_1_SQRT_2)
            }
            Sdf::Split { inner, axis, offset, keep_positive } => {
                let hs = if *keep_positive { offset - p[*axis] } else { p[*axis] - offset };
                inner.eval(p).max(hs)
            }
            Sdf::ShellOpen { inner, t, open_z } => {
                let s = inner.eval(p);
                let c = match open_z {
                    Some(z) if p.z > *z => inner.eval(DVec3::new(p.x, p.y, *z)),
                    _ => s,
                };
                s.max(-(c + t))
            }
            Sdf::Scale(inner, f) => inner.eval(p / *f) * f,
            Sdf::RotRepeat { inner, count, step, axis } => {
                (0..(*count).max(1)).map(|i| inner.eval(axis_rot(*axis, -step * i as f64) * p)).fold(f64::INFINITY, f64::min)
            }
        }
    }

    /// Conservative axis-aligned bounds (min, max).
    pub fn bounds(&self) -> (DVec3, DVec3) {
        match self {
            Sdf::Box { half, .. } => (-*half, *half),
            Sdf::Sphere { r } => (DVec3::splat(-r), DVec3::splat(*r)),
            Sdf::Cylinder { r, h } => (DVec3::new(-r, -r, 0.0), DVec3::new(*r, *r, *h)),
            Sdf::Cone { r0, r1, h } => {
                let r = r0.max(*r1);
                (DVec3::new(-r, -r, 0.0), DVec3::new(r, r, *h))
            }
            Sdf::Torus { major, minor } => (DVec3::new(-(major + minor), -(major + minor), -minor), DVec3::new(major + minor, major + minor, *minor)),
            Sdf::Extrude { loops, h, draft } => {
                let b = loops.iter().fold(Bounds2::EMPTY, |a, l| a.union(&l.bounds()));
                let grow = (h * draft).max(0.0);
                (DVec3::new(b.min.x - grow, b.min.y - grow, 0.0), DVec3::new(b.max.x + grow, b.max.y + grow, *h))
            }
            Sdf::Revolve { loops, .. } => {
                let b = loops.iter().fold(Bounds2::EMPTY, |a, l| a.union(&l.bounds()));
                let r = b.max.x.abs().max(b.min.x.abs());
                (DVec3::new(-r, -r, b.min.y), DVec3::new(r, r, b.max.y))
            }
            Sdf::Union(a, b, k) | Sdf::UnionChamfer(a, b, k) => {
                let (a0, a1) = a.bounds();
                let (b0, b1) = b.bounds();
                (a0.min(b0) - DVec3::splat(*k), a1.max(b1) + DVec3::splat(*k))
            }
            Sdf::Subtract(a, _, _) | Sdf::SubtractChamfer(a, _, _) => a.bounds(),
            Sdf::Intersect(a, b, _) | Sdf::IntersectChamfer(a, b, _) => {
                let (a0, a1) = a.bounds();
                let (b0, b1) = b.bounds();
                (a0.max(b0), a1.min(b1))
            }
            Sdf::Offset(a, d) => {
                let (l, h) = a.bounds();
                (l - DVec3::splat(d.max(0.0)), h + DVec3::splat(d.max(0.0)))
            }
            Sdf::Shell(a, t) => {
                let (l, h) = a.bounds();
                (l - DVec3::splat(*t), h + DVec3::splat(*t))
            }
            Sdf::Transform { inner, translate, rotate_deg } => {
                let (l, h) = inner.bounds();
                transform_bounds(l, h, rot(*rotate_deg), DVec3::from_array(*translate))
            }
            Sdf::Mirror(a, plane) => {
                let (l, h) = a.bounds();
                let k = plane_axis(*plane);
                (set_axis(l, k, l[k].min(-h[k])), set_axis(h, k, h[k].max(-l[k])))
            }
            Sdf::Repeat { inner, count, step } => {
                let (l, h) = inner.bounds();
                let e = *step * ((*count).max(1) - 1) as f64;
                (l.min(l + e), h.max(h + e))
            }
            Sdf::Polar { inner, .. } => {
                let (l, h) = inner.bounds();
                let r = l.truncate().length().max(h.truncate().length());
                (DVec3::new(-r, -r, l.z), DVec3::new(r, r, h.z))
            }
            Sdf::Empty => (DVec3::ZERO, DVec3::ZERO),
            Sdf::Prism { profile, z0, z1, draft, .. } => {
                let b = profile.bounds;
                if profile.is_empty() {
                    return (DVec3::ZERO, DVec3::ZERO);
                }
                let grow = (-draft * z0.abs().max(z1.abs())).max(0.0);
                (DVec3::new(b.min.x - grow, b.min.y - grow, *z0), DVec3::new(b.max.x + grow, b.max.y + grow, *z1))
            }
            Sdf::Frame { inner, rot, origin } => {
                let (l, h) = inner.bounds();
                transform_bounds(l, h, *rot, *origin)
            }
            Sdf::RevolveProfile { profile, .. } => {
                let b = profile.bounds;
                if profile.is_empty() {
                    return (DVec3::ZERO, DVec3::ZERO);
                }
                let r = b.max.x.abs().max(b.min.x.abs());
                (DVec3::new(-r, -r, b.min.y), DVec3::new(r, r, b.max.y))
            }
            Sdf::Sweep { segs, reach, .. } => {
                let mut lo = DVec3::splat(f64::INFINITY);
                let mut hi = DVec3::splat(f64::NEG_INFINITY);
                for s in segs {
                    for p in [s.a, s.a + s.t * s.len] {
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                }
                // Mitres at sharp bends can poke out a little further than the profile radius.
                (lo - DVec3::splat(reach * 1.5), hi + DVec3::splat(reach * 1.5))
            }
            Sdf::Loft { sections, .. } => {
                if sections.is_empty() {
                    return (DVec3::ZERO, DVec3::ZERO);
                }
                let b = sections.iter().fold(Bounds2::EMPTY, |a, s| a.union(&s.0.bounds));
                (DVec3::new(b.min.x, b.min.y, sections[0].1), DVec3::new(b.max.x, b.max.y, sections[sections.len() - 1].1))
            }
            Sdf::CylZ { r, z0, z1 } => (DVec3::new(-r, -r, *z0), DVec3::new(*r, *r, *z1)),
            Sdf::ConeZ { r0, z0, r1, z1 } => {
                let r = r0.max(*r1);
                (DVec3::new(-r, -r, *z0), DVec3::new(r, r, *z1))
            }
            Sdf::Thread { r, len, .. } => (DVec3::new(-r, -r, 0.0), DVec3::new(*r, *r, *len)),
            Sdf::Split { inner, axis, offset, keep_positive } => {
                let (mut l, mut h) = inner.bounds();
                if *keep_positive {
                    l[*axis] = l[*axis].max(*offset);
                    h[*axis] = h[*axis].max(l[*axis]);
                } else {
                    h[*axis] = h[*axis].min(*offset);
                    l[*axis] = l[*axis].min(h[*axis]);
                }
                (l, h)
            }
            Sdf::ShellOpen { inner, .. } => inner.bounds(),
            Sdf::Scale(inner, f) => {
                let (l, h) = inner.bounds();
                (l * *f, h * *f)
            }
            Sdf::RotRepeat { inner, count, step, axis } => {
                let (l, h) = inner.bounds();
                let mut lo = DVec3::splat(f64::INFINITY);
                let mut hi = DVec3::splat(f64::NEG_INFINITY);
                for i in 0..(*count).max(1) {
                    let (a, b) = transform_bounds(l, h, axis_rot(*axis, step * i as f64), DVec3::ZERO);
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
                (lo, hi)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Evaluating a whole document as one distance field.

use crate::document::{BlendKind, BooleanOp, Document};
use crate::mesh::{mt, TriMesh};

/// Grids never extend beyond ±this (mm): guards against through-all extrusions used as bosses.
pub const GRID_LIMIT: f64 = 2000.0;

/// The bounds of a field, clamped to [`GRID_LIMIT`].
pub fn grid_bounds(s: &Sdf) -> (DVec3, DVec3) {
    let (lo, hi) = s.bounds();
    (lo.clamp(DVec3::splat(-GRID_LIMIT), DVec3::splat(GRID_LIMIT)), hi.clamp(DVec3::splat(-GRID_LIMIT), DVec3::splat(GRID_LIMIT)))
}

/// The field of one feature on its own: its shape, its modifiers and its placement.
pub fn feature_sdf(f: &crate::document::Feature) -> Sdf {
    let mut s = shape(&f.shape);
    for m in f.modifiers.iter().filter(|m| !m.is_world()) {
        s = modify(s, m);
    }
    s = Sdf::Transform { inner: Box::new(s), translate: f.placement.translate, rotate_deg: f.placement.rotate_deg };
    for m in f.modifiers.iter().filter(|m| m.is_world()) {
        s = modify(s, m);
    }
    s
}

/// Rotation of a feature placement (XYZ Euler degrees).
pub fn placement_rotation(deg: [f64; 3]) -> DMat3 {
    rot(deg)
}

impl Document {
    /// Build the SDF tree. Parameters, bindings and sketch constraints are resolved first
    /// ([`Document::resolved`]); if that fails the raw feature values are used.
    pub fn sdf(&self) -> Option<Sdf> {
        if self.needs_resolution() {
            if let Ok(p) = self.resolved() {
                return p.raw_sdf();
            }
        }
        self.raw_sdf()
    }

    /// SDF of the feature values as stored (no parameter resolution / sketch solving).
    pub fn raw_sdf(&self) -> Option<Sdf> {
        let mut body: Option<Sdf> = None;
        for f in self.features.iter().filter(|f| !f.suppressed) {
            let s = feature_sdf(f);
            let chamfer = f.blend_kind == BlendKind::Chamfer && f.blend > 0.0;
            body = match (body.take(), f.op) {
                (None, BooleanOp::Add) => Some(s),
                (None, _) => None, // cutting from nothing is a no-op
                (Some(b), BooleanOp::Add) if chamfer => Some(Sdf::UnionChamfer(Box::new(b), Box::new(s), f.blend)),
                (Some(b), BooleanOp::Cut) if chamfer => Some(Sdf::SubtractChamfer(Box::new(b), Box::new(s), f.blend)),
                (Some(b), BooleanOp::Intersect) if chamfer => Some(Sdf::IntersectChamfer(Box::new(b), Box::new(s), f.blend)),
                (Some(b), BooleanOp::Add) => Some(Sdf::Union(Box::new(b), Box::new(s), f.blend)),
                (Some(b), BooleanOp::Cut) => Some(Sdf::Subtract(Box::new(b), Box::new(s), f.blend)),
                (Some(b), BooleanOp::Intersect) => Some(Sdf::Intersect(Box::new(b), Box::new(s), f.blend)),
            };
        }
        let mut b = body?;
        for m in &self.finish {
            b = modify(b, m);
        }
        Some(b)
    }

    /// Triangulate at resolution `h` (mm).
    pub fn mesh(&self, h: f64) -> TriMesh {
        let Some(s) = self.sdf() else { return TriMesh::default() };
        let (lo, hi) = grid_bounds(&s);
        let g = mt::Grid::sample(lo.as_vec3(), hi.as_vec3(), h as f32, |p| s.eval(p.as_dvec3()) as f32);
        let mut m = mt::extract(&g, 0.0);
        m.smooth_taubin(2, 0.4, -0.42);
        m
    }
}
