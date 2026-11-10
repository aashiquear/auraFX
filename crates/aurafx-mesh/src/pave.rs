//! Surface meshing at a size: triangles on planar faces bounded by outlines (with holes) and on
//! any triangle surface (remeshing), all-quadrilateral meshes from them, and the size fields
//! that drive them.
//!
//! A planar face: its boundary loops are resampled at the size (corners kept), triangulated by
//! conforming Delaunay (boundary edges recovered by splitting), then improved like any surface:
//! edges longer than 4/3 of the local size are split, shorter than 4/5 collapsed, edges flipped
//! towards Delaunay and free nodes smoothed (on a curved surface, along it and projected back).
//! Quadrilaterals: neighbouring triangles are paired where they make good quads, then every quad
//! is split into four and every remaining triangle into three, so the result is all quads of
//! half the triangles' size.

use anyhow::{bail, Result};
// Ordered maps and sets: the same input always gives the same mesh (hash order would vary).
use std::collections::{BTreeMap as HashMap, BTreeSet as HashSet};

pub type P = [f64; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: P) -> f64 {
    dot(a, a).sqrt()
}
fn dist(a: P, b: P) -> f64 {
    norm(sub(a, b))
}
fn mid(a: P, b: P) -> P {
    scale(add(a, b), 0.5)
}
fn tri_normal(a: P, b: P, c: P) -> P {
    cross(sub(b, a), sub(c, a))
}
fn angle_at(a: P, b: P, c: P) -> f64 {
    // The angle at a of triangle a b c.
    let (u, v) = (sub(b, a), sub(c, a));
    (dot(u, v) / (norm(u) * norm(v)).max(1e-300)).clamp(-1.0, 1.0).acos()
}

// ------------------------------------------------------------------ size fields

/// Where the mesh size comes from: a base size, refined near curves (a size there growing
/// linearly with distance), from a scalar field at points, and from the curvature of a
/// reference surface (the chord error kept under a tolerance), all limited to [min, max].
#[derive(Default)]
pub struct Sizing {
    pub base: f64,
    pub min: f64,
    pub max: f64,
    /// (sample points of the curve, size on it, growth per unit distance).
    pub near: Vec<(Vec<P>, f64, f64)>,
    /// Points with a size at each: the nearest point's value.
    pub field: Option<(aurafx_data::kdtree::KdTree, Vec<f64>)>,
    /// Points with a curvature (1/length) at each and the chord tolerance.
    pub curvature: Option<(aurafx_data::kdtree::KdTree, Vec<f64>, f64)>,
    /// Every size times this (2 for triangles that are split into quads of the size).
    pub scale: f64,
    near_trees: Vec<aurafx_data::kdtree::KdTree>,
}

impl Sizing {
    pub fn uniform(h: f64) -> Sizing {
        Sizing { base: h, min: 0.0, max: f64::INFINITY, scale: 1.0, ..Default::default() }
    }

    /// Size `h0` on the curve through `pts`, growing by `growth` per unit of distance.
    pub fn refine_near(&mut self, pts: Vec<P>, h0: f64, growth: f64) {
        self.near_trees.push(aurafx_data::kdtree::KdTree::new(&pts));
        self.near.push((pts, h0, growth));
    }

    /// Sizes given at points (a scalar array of a data set): the nearest point's value.
    pub fn from_field(&mut self, pts: &[P], values: Vec<f64>) {
        self.field = Some((aurafx_data::kdtree::KdTree::new(pts), values));
    }

    /// Curvature from a reference surface: sizes keeping the chord error below `tolerance`.
    pub fn from_curvature(&mut self, pts: &[P], k: Vec<f64>, tolerance: f64) {
        self.curvature = Some((aurafx_data::kdtree::KdTree::new(pts), k, tolerance));
    }

    pub fn at(&self, p: P) -> f64 {
        let mut h = self.base;
        for ((_, h0, g), t) in self.near.iter().zip(&self.near_trees) {
            if let Some((_, d2)) = t.nearest_one(p) {
                h = h.min(h0 + g * d2.sqrt());
            }
        }
        if let Some((t, v)) = &self.field {
            if let Some((i, _)) = t.nearest_one(p) {
                if v[i] > 0.0 && v[i].is_finite() {
                    h = v[i].min(if self.base > 0.0 { f64::INFINITY } else { v[i] });
                }
            }
        }
        if let Some((t, k, tol)) = &self.curvature {
            if let Some((i, _)) = t.nearest_one(p) {
                if k[i] > 1e-12 {
                    // Chord of an arc of radius R with sagitta tol: 2 sqrt(2 R tol - tol^2).
                    let r = 1.0 / k[i];
                    let c = 2.0 * (2.0 * r * tol - tol * tol).max(0.0).sqrt();
                    if c > 0.0 {
                        h = h.min(c);
                    }
                }
            }
        }
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        s * h.clamp(self.min.max(1e-12), self.max.max(self.min.max(1e-12)))
    }
}

// ------------------------------------------------------------------ boundary loops

/// A closed loop resampled at the size: corners (turns of more than `corner_deg`) are kept, each
/// stretch between them divided into pieces of about the local size (or `intervals` pieces in
/// all when given), by arc length.
pub fn resample_loop(pts: &[[f64; 2]], size: &dyn Fn(P) -> f64, corner_deg: f64, intervals: usize) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = pts.to_vec();
    if p.len() > 1 && (p[0][0] - p[p.len() - 1][0]).abs() + (p[0][1] - p[p.len() - 1][1]).abs() < 1e-12 {
        p.pop();
    }
    let n = p.len();
    if n < 3 {
        return p;
    }
    let p3 = |q: [f64; 2]| [q[0], q[1], 0.0];
    let turn = |i: usize| {
        let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let cr = u[0] * v[1] - u[1] * v[0];
        let dt = u[0] * v[0] + u[1] * v[1];
        cr.atan2(dt).abs().to_degrees()
    };
    let mut corners: Vec<usize> = (0..n).filter(|&i| turn(i) > corner_deg).collect();
    let whole_count = intervals > 0;
    if corners.is_empty() {
        corners.push(0);
    }
    // Total "count" (integral of ds / h) per stretch.
    let stretches: Vec<Vec<[f64; 2]>> = (0..corners.len())
        .map(|k| {
            let (s, e) = (corners[k], corners[(k + 1) % corners.len()]);
            let mut v = vec![p[s]];
            let mut i = s;
            loop {
                i = (i + 1) % n;
                v.push(p[i]);
                if i == e {
                    break;
                }
            }
            v
        })
        .collect();
    let counts: Vec<f64> = stretches
        .iter()
        .map(|st| st.windows(2).map(|w| dist(p3(w[0]), p3(w[1])) / size(p3([(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0])).max(1e-300)).sum())
        .collect();
    let lengths: Vec<f64> = stretches.iter().map(|st| st.windows(2).map(|w| dist(p3(w[0]), p3(w[1]))).sum()).collect();
    let total_len: f64 = lengths.iter().sum();
    let mut out = Vec::new();
    for (k, st) in stretches.iter().enumerate() {
        let m = if whole_count { ((intervals as f64 * lengths[k] / total_len).round() as usize).max(1) } else { (counts[k].round() as usize).max(1) };
        let m = if corners.len() == 1 { m.max(3) } else { m };
        // Walk the stretch, placing points at equal steps of the count (or length).
        let seg: Vec<f64> = st
            .windows(2)
            .map(|w| {
                let l = dist(p3(w[0]), p3(w[1]));
                if whole_count {
                    l
                } else {
                    l / size(p3([(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0])).max(1e-300)
                }
            })
            .collect();
        let tot: f64 = seg.iter().sum();
        out.push(st[0]);
        let mut acc = 0.0;
        let mut j = 0;
        for q in 1..m {
            let target = tot * q as f64 / m as f64;
            while j < seg.len() - 1 && acc + seg[j] < target {
                acc += seg[j];
                j += 1;
            }
            let t = ((target - acc) / seg[j].max(1e-300)).clamp(0.0, 1.0);
            out.push([st[j][0] + t * (st[j + 1][0] - st[j][0]), st[j][1] + t * (st[j + 1][1] - st[j][1])]);
        }
    }
    out
}

fn closest_on_loop(lp: &[[f64; 2]], q: [f64; 2]) -> [f64; 2] {
    let mut best = (f64::INFINITY, q);
    for i in 0..lp.len() {
        let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 { (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let c = [a[0] + t * dx, a[1] + t * dy];
        let d = (c[0] - q[0]).powi(2) + (c[1] - q[1]).powi(2);
        if d < best.0 {
            best = (d, c);
        }
    }
    best.1
}

fn inside_loops(loops: &[Vec<[f64; 2]>], q: [f64; 2]) -> bool {
    let mut inside = false;
    for lp in loops {
        let n = lp.len();
        for i in 0..n {
            let (a, b) = (lp[i], lp[(i + 1) % n]);
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                inside = !inside;
            }
        }
    }
    inside
}

// ------------------------------------------------------------------ Delaunay

struct Delaunay {
    pts: Vec<[f64; 2]>,
    tris: Vec<[usize; 3]>,
    alive: Vec<bool>,
}

impl Delaunay {
    fn new(lo: [f64; 2], hi: [f64; 2]) -> Self {
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let r = ((hi[0] - lo[0]).max(hi[1] - lo[1]) + 1.0) * 20.0;
        let pts = vec![[c[0] - 2.0 * r, c[1] - r], [c[0] + 2.0 * r, c[1] - r], [c[0], c[1] + 2.0 * r]];
        Delaunay { pts, tris: vec![[0, 1, 2]], alive: vec![true] }
    }

    fn in_circle(&self, t: [usize; 3], p: [f64; 2]) -> bool {
        let [a, b, c] = t.map(|i| self.pts[i]);
        let (ax, ay, bx, by, cx, cy) = (a[0] - p[0], a[1] - p[1], b[0] - p[0], b[1] - p[1], c[0] - p[0], c[1] - p[1]);
        let det = (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay) + (cx * cx + cy * cy) * (ax * by - bx * ay);
        det > 0.0
    }

    fn insert(&mut self, p: [f64; 2]) -> usize {
        let idx = self.pts.len();
        self.pts.push(p);
        let bad: Vec<usize> = (0..self.tris.len()).filter(|&t| self.alive[t] && self.in_circle(self.tris[t], p)).collect();
        let mut edges: HashMap<(usize, usize), (usize, usize, u32)> = HashMap::new();
        for &t in &bad {
            self.alive[t] = false;
            let tr = self.tris[t];
            for k in 0..3 {
                let (a, b) = (tr[k], tr[(k + 1) % 3]);
                let e = edges.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
                e.2 += 1;
            }
        }
        for (_, (a, b, n)) in edges {
            if n == 1 {
                self.tris.push([a, b, idx]);
                self.alive.push(true);
            }
        }
        idx
    }

    fn has_edge(&self, a: usize, b: usize) -> bool {
        self.tris.iter().zip(&self.alive).any(|(t, al)| *al && t.contains(&a) && t.contains(&b))
    }
}

/// A planar face meshed with triangles: `loops` are the outer boundary and the holes (any
/// orientation; a point is inside when it is inside an odd number of loops), each resampled at
/// the size (or into `intervals[k]` pieces). Returns points (z = 0), triangles (counterclockwise)
/// and, per point, the loop it lies on (-1 inside).
pub fn mesh_planar_face(loops: &[Vec<[f64; 2]>], sizing: &Sizing, intervals: &[usize], iterations: usize) -> Result<(Vec<P>, Vec<[u32; 3]>, Vec<i32>)> {
    if loops.is_empty() {
        bail!("a face needs an outer boundary");
    }
    let size = |p: P| sizing.at(p);
    let sampled: Vec<Vec<[f64; 2]>> = loops.iter().enumerate().map(|(k, l)| resample_loop(l, &size, 30.0, intervals.get(k).copied().unwrap_or(0))).collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for l in &sampled {
        for p in l {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    let mut dt = Delaunay::new(lo, hi);
    let mut on_loop: Vec<i32> = vec![-2, -2, -2];
    // Boundary segments to recover: (a, b, loop).
    let mut segs: Vec<(usize, usize, usize)> = Vec::new();
    for (k, l) in sampled.iter().enumerate() {
        let first = dt.pts.len();
        for p in l {
            dt.insert(*p);
            on_loop.push(k as i32);
        }
        let n = l.len();
        for i in 0..n {
            segs.push((first + i, first + (i + 1) % n, k));
        }
    }
    // Conforming recovery: split a missing boundary edge at its midpoint (on the loop) until all are edges.
    let mut guard = 0;
    loop {
        let missing: Vec<usize> = (0..segs.len()).filter(|&s| !dt.has_edge(segs[s].0, segs[s].1)).collect();
        if missing.is_empty() {
            break;
        }
        guard += 1;
        if guard > 30 {
            bail!("the boundary could not be recovered (loops that cross or touch?)");
        }
        let mut new_segs = Vec::new();
        let missing: HashSet<usize> = missing.into_iter().collect();
        for (s, &(a, b, k)) in segs.iter().enumerate() {
            if missing.contains(&s) {
                let m = [(dt.pts[a][0] + dt.pts[b][0]) / 2.0, (dt.pts[a][1] + dt.pts[b][1]) / 2.0];
                let m = closest_on_loop(&loops[k], m);
                let i = dt.insert(m);
                on_loop.push(k as i32);
                new_segs.push((a, i, k));
                new_segs.push((i, b, k));
            } else {
                new_segs.push((a, b, k));
            }
        }
        segs = new_segs;
    }
    // Keep the triangles inside the face (not touching the enclosing triangle).
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for (t, al) in dt.tris.iter().zip(&dt.alive) {
        if !*al || t.iter().any(|&i| i < 3) {
            continue;
        }
        let [a, b, c] = t.map(|i| dt.pts[i]);
        let cen = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        if !inside_loops(loops, cen) {
            continue;
        }
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        let tr = if area > 0.0 { [t[0], t[1], t[2]] } else { [t[0], t[2], t[1]] };
        tris.push(tr.map(|i| (i - 3) as u32));
    }
    let pts: Vec<P> = dt.pts[3..].iter().map(|p| [p[0], p[1], 0.0]).collect();
    let on_loop: Vec<i32> = on_loop[3..].to_vec();
    let fixed: Vec<bool> = on_loop.iter().map(|&k| k >= 0).collect();
    let mut r = Remesher::new(pts, tris, fixed, &size);
    r.planar = true;
    r.run(iterations);
    let (pts, tris, keep) = r.finish();
    let on_loop = keep.iter().map(|&i| on_loop.get(i).copied().unwrap_or(-1)).collect();
    Ok((pts, tris, on_loop))
}

// ------------------------------------------------------------------ remeshing

/// Size-driven triangle improvement (split, collapse, flip, smooth). Fixed nodes never move or
/// go; edges between two fixed nodes are kept (feature and boundary edges are split only when
/// `split_fixed_edges`, the new node fixed too).
pub struct Remesher<'a> {
    pub pts: Vec<P>,
    pub tris: Vec<[u32; 3]>,
    pub fixed: Vec<bool>,
    size: &'a dyn Fn(P) -> f64,
    pub project: Option<&'a dyn Fn(P) -> P>,
    pub planar: bool,
    pub split_fixed_edges: bool,
    /// Edges that are features (kept, split only along themselves).
    pub features: HashSet<(u32, u32)>,
    dead: Vec<bool>,
    gone: Vec<bool>,
}

fn ek(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl<'a> Remesher<'a> {
    pub fn new(pts: Vec<P>, tris: Vec<[u32; 3]>, fixed: Vec<bool>, size: &'a dyn Fn(P) -> f64) -> Self {
        let n = tris.len();
        let np = pts.len();
        Remesher {
            pts,
            tris,
            fixed,
            size,
            project: None,
            planar: false,
            split_fixed_edges: false,
            features: HashSet::new(),
            dead: vec![false; n],
            gone: vec![false; np],
        }
    }

    fn edge_map(&self) -> HashMap<(u32, u32), Vec<usize>> {
        let mut m: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (t, tr) in self.tris.iter().enumerate() {
            if self.dead[t] {
                continue;
            }
            for k in 0..3 {
                m.entry(ek(tr[k], tr[(k + 1) % 3])).or_default().push(t);
            }
        }
        m
    }

    /// Boundary and feature edges: never collapsed or flipped, split only along themselves.
    fn is_locked_edge(&self, e: (u32, u32), inc: usize) -> bool {
        inc != 2 || self.features.contains(&e)
    }

    fn put(&mut self, p: P) -> u32 {
        let p = match self.project {
            Some(f) => f(p),
            None => p,
        };
        self.pts.push(p);
        self.fixed.push(false);
        self.gone.push(false);
        (self.pts.len() - 1) as u32
    }

    fn normal(&self, t: [u32; 3]) -> P {
        tri_normal(self.pts[t[0] as usize], self.pts[t[1] as usize], self.pts[t[2] as usize])
    }

    fn split_pass(&mut self) -> usize {
        let em = self.edge_map();
        let mut long: Vec<((u32, u32), f64)> = em
            .iter()
            .filter_map(|(&e, ts)| {
                let (a, b) = (self.pts[e.0 as usize], self.pts[e.1 as usize]);
                let l = dist(a, b);
                let h = (self.size)(mid(a, b));
                let locked = self.is_locked_edge(e, ts.len());
                if l > 4.0 / 3.0 * h && (!locked || self.split_fixed_edges) {
                    Some((e, l / h))
                } else {
                    None
                }
            })
            .collect();
        long.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut used = vec![false; self.tris.len()];
        let mut done = 0;
        for (e, _) in long {
            let ts = &em[&e];
            if ts.iter().any(|&t| used[t]) {
                continue;
            }
            let ts = ts.clone();
            let locked = self.is_locked_edge(e, ts.len());
            let m = mid(self.pts[e.0 as usize], self.pts[e.1 as usize]);
            let i = if locked {
                // On a feature or boundary: the new node stays on the straight edge, fixed.
                self.pts.push(m);
                self.fixed.push(true);
                self.gone.push(false);
                (self.pts.len() - 1) as u32
            } else {
                self.put(m)
            };
            if self.features.remove(&e) {
                self.features.insert(ek(e.0, i));
                self.features.insert(ek(i, e.1));
            }
            for t in ts {
                used[t] = true;
                let tr = self.tris[t];
                let k = (0..3).find(|&k| ek(tr[k], tr[(k + 1) % 3]) == e).unwrap();
                let (a, b, c) = (tr[k], tr[(k + 1) % 3], tr[(k + 2) % 3]);
                self.tris[t] = [a, i, c];
                self.tris.push([i, b, c]);
                self.dead.push(false);
                used.push(true);
            }
            done += 1;
        }
        done
    }

    fn collapse_pass(&mut self) -> usize {
        let em = self.edge_map();
        let mut vt: Vec<Vec<usize>> = vec![Vec::new(); self.pts.len()];
        for (t, tr) in self.tris.iter().enumerate() {
            if !self.dead[t] {
                for &v in tr {
                    vt[v as usize].push(t);
                }
            }
        }
        let mut short: Vec<((u32, u32), f64)> = em
            .iter()
            .filter_map(|(&e, ts)| {
                let (a, b) = (self.pts[e.0 as usize], self.pts[e.1 as usize]);
                let l = dist(a, b);
                let h = (self.size)(mid(a, b));
                (l < 0.8 * h && !self.is_locked_edge(e, ts.len()) && !(self.fixed[e.0 as usize] && self.fixed[e.1 as usize])).then_some((e, l / h))
            })
            .collect();
        short.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut touched = vec![false; self.pts.len()];
        let mut done = 0;
        for (e, _) in short {
            let (a, b) = e;
            if touched[a as usize] || touched[b as usize] {
                continue;
            }
            // Keep `keep`, remove `rm`.
            let (keep, rm, target) = if self.fixed[a as usize] {
                (a, b, self.pts[a as usize])
            } else if self.fixed[b as usize] {
                (b, a, self.pts[b as usize])
            } else {
                let m = mid(self.pts[a as usize], self.pts[b as usize]);
                (a, b, self.project.map(|f| f(m)).unwrap_or(m))
            };
            // A node on an open boundary that is not fixed would leave the boundary: skip.
            let nb = |v: u32| -> HashSet<u32> { vt[v as usize].iter().flat_map(|&t| self.tris[t]).filter(|&x| x != v).collect() };
            let (na, nbb) = (nb(a), nb(b));
            let common = na.intersection(&nbb).count();
            if common != 2 {
                continue;
            }
            // The triangles around either end after the collapse: no flips, no long edges.
            let mut ok = true;
            let hmax = 4.0 / 3.0 * (self.size)(target);
            let around: Vec<usize> = vt[a as usize].iter().chain(&vt[b as usize]).copied().collect::<HashSet<_>>().into_iter().collect();
            for &t in &around {
                let tr = self.tris[t];
                if tr.contains(&a) && tr.contains(&b) {
                    continue;
                }
                let old_n = self.normal(tr);
                let nt = tr.map(|v| if v == rm || v == keep { u32::MAX } else { v });
                let q: Vec<P> = nt.iter().zip(tr.iter()).map(|(&x, &v)| if x == u32::MAX { target } else { self.pts[v as usize] }).collect();
                let new_n = tri_normal(q[0], q[1], q[2]);
                if dot(new_n, old_n) <= 0.1 * dot(old_n, old_n).sqrt() * norm(new_n) || norm(new_n) < 1e-14 {
                    ok = false;
                    break;
                }
                for k in 0..3 {
                    if dist(q[k], q[(k + 1) % 3]) > hmax {
                        ok = false;
                    }
                }
                // Not too thin.
                let minang = (0..3).map(|k| angle_at(q[k], q[(k + 1) % 3], q[(k + 2) % 3])).fold(f64::INFINITY, f64::min);
                if minang < 10f64.to_radians() {
                    ok = false;
                }
                if !ok {
                    break;
                }
            }
            if !ok {
                continue;
            }
            self.pts[keep as usize] = target;
            for &t in &around {
                let tr = self.tris[t];
                if tr.contains(&a) && tr.contains(&b) {
                    self.dead[t] = true;
                } else {
                    self.tris[t] = tr.map(|v| if v == rm { keep } else { v });
                }
            }
            self.gone[rm as usize] = true;
            for v in na.iter().chain(&nbb) {
                touched[*v as usize] = true;
            }
            touched[a as usize] = true;
            touched[b as usize] = true;
            done += 1;
        }
        done
    }

    fn flip_pass(&mut self) -> usize {
        let em = self.edge_map();
        let mut used = vec![false; self.tris.len()];
        let mut done = 0;
        for (&e, ts) in &em {
            if ts.len() != 2 || self.features.contains(&e) || used[ts[0]] || used[ts[1]] {
                continue;
            }
            let (t1, t2) = (self.tris[ts[0]], self.tris[ts[1]]);
            let c = *t1.iter().find(|&&v| v != e.0 && v != e.1).unwrap();
            let d = *t2.iter().find(|&&v| v != e.0 && v != e.1).unwrap();
            if c == d || em.contains_key(&ek(c, d)) {
                continue;
            }
            let (pa, pb, pc, pd) = (self.pts[e.0 as usize], self.pts[e.1 as usize], self.pts[c as usize], self.pts[d as usize]);
            if angle_at(pc, pa, pb) + angle_at(pd, pa, pb) <= std::f64::consts::PI + 1e-9 {
                continue;
            }
            // t1 has edge (x, y) in its order; the new triangles keep that orientation.
            let k = (0..3).find(|&k| ek(t1[k], t1[(k + 1) % 3]) == e).unwrap();
            let (x, y) = (t1[k], t1[(k + 1) % 3]);
            let n1 = [x, d, c];
            let n2 = [d, y, c];
            let on = add(self.normal(t1), self.normal(t2));
            let (m1, m2) = (self.normal(n1), self.normal(n2));
            if dot(m1, on) <= 0.0 || dot(m2, on) <= 0.0 || norm(m1) < 1e-14 || norm(m2) < 1e-14 {
                continue;
            }
            // On a curved surface, do not flip across a sharp fold.
            if !self.planar && dot(m1, m2) < 0.5 * norm(m1) * norm(m2) {
                continue;
            }
            self.tris[ts[0]] = n1;
            self.tris[ts[1]] = n2;
            used[ts[0]] = true;
            used[ts[1]] = true;
            done += 1;
        }
        done
    }

    fn smooth_pass(&mut self) {
        let mut nbr: Vec<HashSet<u32>> = vec![HashSet::new(); self.pts.len()];
        let mut vt: Vec<Vec<usize>> = vec![Vec::new(); self.pts.len()];
        for (t, tr) in self.tris.iter().enumerate() {
            if self.dead[t] {
                continue;
            }
            for k in 0..3 {
                nbr[tr[k] as usize].insert(tr[(k + 1) % 3]);
                nbr[tr[k] as usize].insert(tr[(k + 2) % 3]);
                vt[tr[k] as usize].push(t);
            }
        }
        // Nodes on open boundaries (an edge with one triangle) stay.
        let em = self.edge_map();
        let mut on_edge = vec![false; self.pts.len()];
        for (e, ts) in &em {
            if ts.len() != 2 || self.features.contains(e) {
                on_edge[e.0 as usize] = true;
                on_edge[e.1 as usize] = true;
            }
        }
        for v in 0..self.pts.len() {
            if self.fixed[v] || self.gone[v] || on_edge[v] || nbr[v].is_empty() {
                continue;
            }
            // Average weighted by the local size (denser where the size is small).
            let mut acc = [0.0; 3];
            let mut wsum = 0.0;
            for &u in &nbr[v] {
                let w = 1.0;
                acc = add(acc, scale(self.pts[u as usize], w));
                wsum += w;
            }
            let avg = scale(acc, 1.0 / wsum);
            let x = self.pts[v];
            let mut np = if self.planar {
                avg
            } else {
                // Along the surface: drop the move's normal part.
                let mut n = [0.0; 3];
                for &t in &vt[v] {
                    n = add(n, self.normal(self.tris[t]));
                }
                let l = norm(n).max(1e-300);
                let n = scale(n, 1.0 / l);
                let d = sub(avg, x);
                add(x, sub(d, scale(n, dot(d, n))))
            };
            if let Some(f) = self.project {
                np = f(np);
            }
            let ok = vt[v].iter().all(|&t| {
                let tr = self.tris[t];
                let old = self.normal(tr);
                let q: Vec<P> = tr.iter().map(|&u| if u as usize == v { np } else { self.pts[u as usize] }).collect();
                let new = tri_normal(q[0], q[1], q[2]);
                dot(old, new) > 0.0 && norm(new) > 1e-14
            });
            if ok {
                self.pts[v] = np;
            }
        }
    }

    pub fn run(&mut self, iterations: usize) {
        for _ in 0..iterations.max(1) {
            for _ in 0..12 {
                if self.split_pass() == 0 {
                    break;
                }
            }
            for _ in 0..6 {
                if self.collapse_pass() == 0 {
                    break;
                }
            }
            for _ in 0..6 {
                if self.flip_pass() == 0 {
                    break;
                }
            }
            for _ in 0..3 {
                self.smooth_pass();
            }
        }
        for _ in 0..6 {
            if self.flip_pass() == 0 {
                break;
            }
        }
        self.smooth_pass();
    }

    /// The mesh with removed nodes and triangles dropped, and the old index of each kept node.
    pub fn finish(self) -> (Vec<P>, Vec<[u32; 3]>, Vec<usize>) {
        let mut used = vec![false; self.pts.len()];
        let tris: Vec<[u32; 3]> = self.tris.iter().zip(&self.dead).filter(|(_, d)| !**d).map(|(t, _)| *t).collect();
        for t in &tris {
            for &v in t {
                used[v as usize] = true;
            }
        }
        let mut map = vec![u32::MAX; self.pts.len()];
        let mut pts = Vec::new();
        let mut keep = Vec::new();
        for (i, u) in used.iter().enumerate() {
            if *u {
                map[i] = pts.len() as u32;
                pts.push(self.pts[i]);
                keep.push(i);
            }
        }
        (pts, tris.iter().map(|t| t.map(|v| map[v as usize])).collect(), keep)
    }
}

/// Remesh a triangle surface at the size: boundary edges and sharp edges (where the surface
/// turns by more than `feature_deg`) are kept as features, split along themselves; the other
/// nodes move along the surface and are projected back onto it.
pub fn remesh_surface(pts: &[P], tris: &[[u32; 3]], sizing: &Sizing, feature_deg: f64, iterations: usize) -> Result<(Vec<P>, Vec<[u32; 3]>)> {
    let reference = crate::trimesh::TriMesh::new(pts, tris)?;
    // Work on the merged copy of the reference.
    let (rp, rt) = (reference.verts.clone(), reference.tris.clone());
    let mut feat = HashSet::new();
    let mut fixed = vec![false; rp.len()];
    let mut em: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (t, tr) in rt.iter().enumerate() {
        for k in 0..3 {
            em.entry(ek(tr[k], tr[(k + 1) % 3])).or_default().push(t);
        }
    }
    let cosf = feature_deg.to_radians().cos();
    let n = |t: usize| {
        let tr = rt[t];
        let v = tri_normal(rp[tr[0] as usize], rp[tr[1] as usize], rp[tr[2] as usize]);
        scale(v, 1.0 / norm(v).max(1e-300))
    };
    let mut feat_count = vec![0usize; rp.len()];
    for (e, ts) in &em {
        let sharp = ts.len() != 2 || dot(n(ts[0]), n(ts[1])) < cosf;
        if sharp {
            feat.insert(*e);
            feat_count[e.0 as usize] += 1;
            feat_count[e.1 as usize] += 1;
        }
    }
    // Corners of the feature network (not two features through them) are fixed; feature nodes are
    // kept in place too (they only gain nodes by splitting).
    for (v, c) in feat_count.iter().enumerate() {
        if *c > 0 {
            fixed[v] = true;
        }
    }
    let size = |p: P| sizing.at(p);
    let proj = |p: P| reference.closest_point(p).0;
    let mut r = Remesher::new(rp, rt, fixed, &size);
    r.features = feat;
    r.split_fixed_edges = true;
    r.project = Some(&proj);
    r.run(iterations);
    let (p, t, _) = r.finish();
    Ok((p, t))
}

/// Per-node curvature of a triangle surface (1/length), measured over a neighbourhood about
/// three mean edges wide so that facets and noise of the surface do not dominate: the largest
/// turn of the (area-weighted) normal to a node between half and all of that distance away, over
/// their distance.
pub fn node_curvature(pts: &[P], tris: &[[u32; 3]]) -> Vec<f64> {
    let mut vn = vec![[0.0; 3]; pts.len()];
    let mut edge_sum = 0.0;
    for t in tris {
        let nn = tri_normal(pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        for k in 0..3 {
            vn[t[k] as usize] = add(vn[t[k] as usize], nn);
            edge_sum += dist(pts[t[k] as usize], pts[t[(k + 1) % 3] as usize]);
        }
    }
    let used: Vec<bool> = {
        let mut u = vec![false; pts.len()];
        for t in tris {
            for &v in t {
                u[v as usize] = true;
            }
        }
        u
    };
    for v in vn.iter_mut() {
        let l = norm(*v).max(1e-300);
        *v = scale(*v, 1.0 / l);
    }
    let rho = 3.0 * edge_sum / (3 * tris.len().max(1)) as f64;
    let tree = aurafx_data::kdtree::KdTree::new(pts);
    (0..pts.len())
        .map(|v| {
            if !used[v] {
                return 0.0;
            }
            let mut k = 0.0f64;
            for (u, d2) in tree.within(pts[v], rho) {
                let d = d2.sqrt();
                if u != v && used[u] && d >= 0.5 * rho {
                    k = k.max(dot(vn[v], vn[u]).clamp(-1.0, 1.0).acos() / d);
                }
            }
            k
        })
        .collect()
}

// ------------------------------------------------------------------ quadrilaterals

/// All quadrilaterals from triangles: neighbouring triangles that make a good quad (every angle
/// within 60 degrees of a right angle) are paired, best first; then every quad is split into
/// four and every triangle left into three at edge midpoints and centres. `boundary` moves the
/// midpoint of an edge with one triangle (onto the true boundary). The quads keep the triangles'
/// orientation.
pub fn quads_from_triangles(pts: &[P], tris: &[[u32; 3]], boundary: &dyn Fn(P) -> P, project: &dyn Fn(P) -> P) -> (Vec<P>, Vec<[u32; 4]>) {
    let mut em: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (t, tr) in tris.iter().enumerate() {
        for k in 0..3 {
            em.entry(ek(tr[k], tr[(k + 1) % 3])).or_default().push(t);
        }
    }
    let quad_quality = |q: [u32; 4]| -> f64 {
        // The worst corner's distance from 90 degrees (0 best); convex and not folded.
        let p: Vec<P> = q.iter().map(|&v| pts[v as usize]).collect();
        let mut worst = 0.0f64;
        let n0 = add(tri_normal(p[0], p[1], p[2]), tri_normal(p[0], p[2], p[3]));
        for k in 0..4 {
            let (a, b, c) = (p[(k + 3) % 4], p[k], p[(k + 1) % 4]);
            if dot(tri_normal(b, c, a), n0) <= 0.0 {
                return f64::INFINITY;
            }
            worst = worst.max((angle_at(b, a, c) - std::f64::consts::FRAC_PI_2).abs());
        }
        worst
    };
    let mut cand: Vec<(f64, usize, usize, [u32; 4])> = Vec::new();
    for (e, ts) in &em {
        if ts.len() != 2 {
            continue;
        }
        let t1 = tris[ts[0]];
        let t2 = tris[ts[1]];
        let k = (0..3).find(|&k| ek(t1[k], t1[(k + 1) % 3]) == *e).unwrap();
        let (x, y, c) = (t1[k], t1[(k + 1) % 3], t1[(k + 2) % 3]);
        let d = *t2.iter().find(|&&v| v != x && v != y).unwrap();
        let q = [x, d, y, c];
        let w = quad_quality(q);
        if w < 60f64.to_radians() {
            cand.push((w, ts[0], ts[1], q));
        }
    }
    cand.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used = vec![false; tris.len()];
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for (_, a, b, q) in cand {
        if used[a] || used[b] {
            continue;
        }
        used[a] = true;
        used[b] = true;
        polys.push(q.to_vec());
    }
    for (t, tr) in tris.iter().enumerate() {
        if !used[t] {
            polys.push(tr.to_vec());
        }
    }
    // Split: midpoints shared between neighbours.
    let mut out: Vec<P> = pts.to_vec();
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mut quads = Vec::new();
    for poly in &polys {
        let n = poly.len();
        let m: Vec<u32> = (0..n)
            .map(|k| {
                let e = ek(poly[k], poly[(k + 1) % n]);
                *mids.entry(e).or_insert_with(|| {
                    let p = mid(pts[e.0 as usize], pts[e.1 as usize]);
                    let p = if em.get(&e).is_some_and(|v| v.len() == 1) { boundary(p) } else { project(p) };
                    out.push(p);
                    (out.len() - 1) as u32
                })
            })
            .collect();
        let cen = project(scale(poly.iter().fold([0.0; 3], |acc, &v| add(acc, pts[v as usize])), 1.0 / n as f64));
        out.push(cen);
        let c = (out.len() - 1) as u32;
        for k in 0..n {
            quads.push([poly[k], m[k], c, m[(k + n - 1) % n]]);
        }
    }
    (out, quads)
}

/// The quads smoothed a little (free nodes to the average of their quad neighbours' centres),
/// boundary nodes kept.
pub fn smooth_quads(pts: &mut [P], quads: &[[u32; 4]], fixed: &[bool], project: &dyn Fn(P) -> P, iterations: usize) {
    let mut em: HashMap<(u32, u32), usize> = HashMap::new();
    for q in quads {
        for k in 0..4 {
            *em.entry(ek(q[k], q[(k + 1) % 4])).or_default() += 1;
        }
    }
    let mut on_b = fixed.to_vec();
    on_b.resize(pts.len(), false);
    for (e, c) in &em {
        if *c == 1 {
            on_b[e.0 as usize] = true;
            on_b[e.1 as usize] = true;
        }
    }
    let mut vq: Vec<Vec<usize>> = vec![Vec::new(); pts.len()];
    for (i, q) in quads.iter().enumerate() {
        for &v in q {
            vq[v as usize].push(i);
        }
    }
    for _ in 0..iterations {
        for v in 0..pts.len() {
            if on_b[v] || vq[v].is_empty() {
                continue;
            }
            let mut acc = [0.0; 3];
            for &qi in &vq[v] {
                let q = quads[qi];
                let c = scale(q.iter().fold([0.0; 3], |a, &u| add(a, pts[u as usize])), 0.25);
                acc = add(acc, c);
            }
            let np = project(scale(acc, 1.0 / vq[v].len() as f64));
            // Keep every quad around convex.
            let ok = vq[v].iter().all(|&qi| {
                let q = quads[qi];
                let p: Vec<P> = q.iter().map(|&u| if u as usize == v { np } else { pts[u as usize] }).collect();
                let n0 = add(tri_normal(p[0], p[1], p[2]), tri_normal(p[0], p[2], p[3]));
                (0..4).all(|k| dot(tri_normal(p[k], p[(k + 1) % 4], p[(k + 3) % 4]), n0) > 0.0)
            });
            if ok {
                pts[v] = np;
            }
        }
    }
}

/// The scaled Jacobian of a planar or nearly planar quad (the smallest corner value).
pub fn quad_scaled_jacobian(p: [P; 4]) -> f64 {
    let n0 = add(tri_normal(p[0], p[1], p[2]), tri_normal(p[0], p[2], p[3]));
    let nl = norm(n0).max(1e-300);
    (0..4)
        .map(|k| {
            let (a, b, c) = (p[k], p[(k + 1) % 4], p[(k + 3) % 4]);
            let (u, v) = (sub(b, a), sub(c, a));
            dot(cross(u, v), n0) / nl / (norm(u) * norm(v)).max(1e-300)
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(c: [f64; 2], r: f64, n: usize) -> Vec<[f64; 2]> {
        (0..n).map(|k| k as f64 / n as f64 * std::f64::consts::TAU).map(|t| [c[0] + r * t.cos(), c[1] + r * t.sin()]).collect()
    }

    #[test]
    fn a_plate_with_a_hole_meshes_at_the_size() {
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]];
        let hole = circle([2.0, 1.0], 0.5, 200);
        let s = Sizing::uniform(0.2);
        let (p, t, _) = mesh_planar_face(&[outer.clone(), hole.clone()], &s, &[], 3).unwrap();
        // Area: 8 - pi 0.25 (the hole's polygon, nearly).
        let area: f64 = t.iter().map(|tr| 0.5 * tri_normal(p[tr[0] as usize], p[tr[1] as usize], p[tr[2] as usize])[2]).sum();
        // (the hole is resampled as a polygon of about 16 sides at this size)
        assert!((area - (8.0 - std::f64::consts::PI * 0.25)).abs() < 0.03, "{area}");
        assert!(t.iter().all(|tr| tri_normal(p[tr[0] as usize], p[tr[1] as usize], p[tr[2] as usize])[2] > 0.0));
        // Edge lengths near the size, angles not too small.
        let mut lens = Vec::new();
        let mut minang = f64::INFINITY;
        for tr in &t {
            for k in 0..3 {
                let (a, b, c) = (p[tr[k] as usize], p[tr[(k + 1) % 3] as usize], p[tr[(k + 2) % 3] as usize]);
                lens.push(dist(a, b));
                minang = minang.min(angle_at(a, b, c));
            }
        }
        let mean = lens.iter().sum::<f64>() / lens.len() as f64;
        assert!((mean / 0.2 - 1.0).abs() < 0.2, "{mean}");
        assert!(minang.to_degrees() > 20.0, "{}", minang.to_degrees());
        // Quads.
        let all = [outer, hole];
        let (qp, q) = quads_from_triangles(
            &p,
            &t,
            &|x: P| {
                let c = all
                    .iter()
                    .map(|l| closest_on_loop(l, [x[0], x[1]]))
                    .min_by(|a, b| ((a[0] - x[0]).powi(2) + (a[1] - x[1]).powi(2)).total_cmp(&((b[0] - x[0]).powi(2) + (b[1] - x[1]).powi(2))))
                    .unwrap();
                [c[0], c[1], 0.0]
            },
            &|x| x,
        );
        let qa: f64 = q
            .iter()
            .map(|c| {
                0.5 * (tri_normal(qp[c[0] as usize], qp[c[1] as usize], qp[c[2] as usize])[2]
                    + tri_normal(qp[c[0] as usize], qp[c[2] as usize], qp[c[3] as usize])[2])
            })
            .sum();
        assert!((qa - area).abs() < 0.03, "{qa} {area}");
        let worst = q.iter().map(|c| quad_scaled_jacobian(c.map(|v| qp[v as usize]))).fold(f64::INFINITY, f64::min);
        assert!(worst > 0.3, "{worst}");
    }

    #[test]
    fn graded_size_near_a_curve() {
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let mut s = Sizing::uniform(0.5);
        s.refine_near(vec![[0.0, 0.0, 0.0]], 0.05, 0.3);
        let (p, t, _) = mesh_planar_face(&[outer], &s, &[], 3).unwrap();
        let near = t.iter().filter(|tr| tr.iter().all(|&v| norm(p[v as usize]) < 0.3)).count();
        let far = t.iter().filter(|tr| tr.iter().all(|&v| norm(sub(p[v as usize], [3.5, 3.5, 0.0])) < 0.3)).count();
        assert!(near > 4 * far.max(1), "{near} {far}");
    }

    #[test]
    fn a_sphere_remeshes_on_its_surface() {
        // An octahedron pushed onto the unit sphere as a coarse reference, remeshed at 0.15.
        let mut pts: Vec<P> = vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        let mut tris: Vec<[u32; 3]> = vec![[0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4], [2, 0, 5], [1, 2, 5], [3, 1, 5], [0, 3, 5]];
        // Subdivide four times so the reference is round.
        for _ in 0..4 {
            let mut m: HashMap<(u32, u32), u32> = HashMap::new();
            let mut nt = Vec::new();
            for t in &tris {
                let mut e = [0u32; 3];
                for k in 0..3 {
                    let key = ek(t[k], t[(k + 1) % 3]);
                    e[k] = *m.entry(key).or_insert_with(|| {
                        let q = mid(pts[key.0 as usize], pts[key.1 as usize]);
                        let l = norm(q);
                        pts.push(scale(q, 1.0 / l));
                        (pts.len() - 1) as u32
                    });
                }
                nt.push([t[0], e[0], e[2]]);
                nt.push([e[0], t[1], e[1]]);
                nt.push([e[2], e[1], t[2]]);
                nt.push([e[0], e[1], e[2]]);
            }
            tris = nt;
        }
        let (p, t) = remesh_surface(&pts, &tris, &Sizing::uniform(0.15), 45.0, 4).unwrap();
        assert!(p.iter().all(|q| (norm(*q) - 1.0).abs() < 0.01));
        let mut lens = Vec::new();
        for tr in &t {
            for k in 0..3 {
                lens.push(dist(p[tr[k] as usize], p[tr[(k + 1) % 3] as usize]));
            }
        }
        let mean = lens.iter().sum::<f64>() / lens.len() as f64;
        assert!((mean / 0.15 - 1.0).abs() < 0.2, "{mean}");
        // Closed: every edge in two triangles.
        let mut em: HashMap<(u32, u32), usize> = HashMap::new();
        for tr in &t {
            for k in 0..3 {
                *em.entry(ek(tr[k], tr[(k + 1) % 3])).or_default() += 1;
            }
        }
        assert!(em.values().all(|&c| c == 2));
    }
}
