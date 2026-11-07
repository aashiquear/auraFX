//! Test specimens meshed with hexahedra, after hexaform's plate and dog-bone generators: a
//! rectangular plate (one element through the thickness) with an optional circular or
//! elliptical through-hole meshed by a conforming O-grid ring, and a flat dog-bone tensile bar
//! with shoulder fillets and a thickness profile along the gage.

use crate::hex::{HexMesh, P3, HEX8_SIDES};
use std::collections::HashMap;
use std::f64::consts::PI;

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    let step = (b - a) / n as f64;
    let mut v: Vec<f64> = (0..=n).map(|i| i as f64 * step + a).collect();
    v[n] = b;
    v
}

/// A circular or elliptical hole: semi-axes `a`, `b`, centre, rotation of `a` from +x (degrees).
#[derive(Clone, Debug)]
pub struct Hole {
    pub ellipse: bool,
    pub a: f64,
    pub b: f64,
    pub centre: Option<[f64; 2]>,
    pub angle: f64,
}

impl Hole {
    pub fn radius(&self, theta: f64) -> f64 {
        let phi = theta - self.angle.to_radians();
        self.a * self.b / ((self.b * phi.cos()).powi(2) + (self.a * phi.sin()).powi(2)).sqrt()
    }
    pub fn perimeter(&self) -> f64 {
        let (a, b) = (self.a, self.b);
        let hh = ((a - b) / (a + b)).powi(2);
        PI * (a + b) * (1.0 + 3.0 * hh / (10.0 + (4.0 - 3.0 * hh).sqrt()))
    }
    pub fn extent(&self) -> (f64, f64) {
        let ang = self.angle.to_radians();
        ((self.a * ang.cos()).hypot(self.b * ang.sin()), (self.a * ang.sin()).hypot(self.b * ang.cos()))
    }
}

#[derive(Clone, Debug)]
pub struct PlateOptions {
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    pub h: f64,
    pub hole: Option<Hole>,
    /// Half-extent of the ring's rectangle in grid cells (None: the smallest leaving
    /// `ring_cells` cells beyond the hole's bounding box).
    pub transition: Option<[usize; 2]>,
    pub ring_cells: usize,
    /// Radial layers and grading of the ring (None: sized from the hole's arc length to h).
    pub n_r: Option<usize>,
    pub grading: Option<f64>,
    pub origin: P3,
}

impl Default for PlateOptions {
    fn default() -> Self {
        PlateOptions { width: 100.0, height: 50.0, thickness: 1.0, h: 2.5, hole: None, transition: None, ring_cells: 2, n_r: None, grading: None, origin: [0.0; 3] }
    }
}

fn divisions(len: f64, h: f64, name: &str) -> Result<usize, String> {
    let n = len / h;
    if (n - n.round()).abs() > 1e-9 * n.max(1.0) {
        return Err(format!("{name} = {len} is not a whole multiple of the cell size {h}"));
    }
    Ok(n.round() as usize)
}

fn graded(n: usize, ratio: f64) -> Vec<f64> {
    if (ratio - 1.0).abs() < 1e-12 || n == 1 {
        return linspace(0.0, 1.0, n);
    }
    let r = ratio.powf(1.0 / (n - 1) as f64);
    let mut s = vec![0.0];
    let mut acc = 0.0;
    for i in 0..n {
        acc += r.powi(i as i32);
        s.push(acc);
    }
    let last = s[n];
    s.iter().map(|x| x / last).collect()
}

/// Layers and grading so the first spacing is `d_inner` and the last `d_outer`.
fn auto_layers(length: f64, d_inner: f64, d_outer: f64) -> Result<(usize, f64), String> {
    if length <= 0.0 {
        return Err("the transition rectangle does not clear the hole".into());
    }
    let d_inner = d_inner.min(length);
    let d_outer = d_outer.max(d_inner).min(length);
    let total = |n: usize, ratio: f64| -> f64 {
        let r = ratio.powf(1.0 / (n - 1) as f64);
        if (r - 1.0).abs() > 1e-12 {
            d_inner * (r.powi(n as i32) - 1.0) / (r - 1.0)
        } else {
            d_inner * n as f64
        }
    };
    let mut best = (1usize, 1.0f64);
    for n in 1..10_000usize {
        if n == 1 {
            if length <= d_outer * 1.0000001 {
                return Ok((1, 1.0));
            }
            continue;
        }
        let ratio = d_outer / d_inner;
        best = (n, ratio);
        if total(n, ratio) >= length {
            break;
        }
    }
    let (n, ratio) = best;
    let (mut lo, mut hi) = (1.0f64, ratio.max(1.0) * 4.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if total(n, mid) < length {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok((n, 0.5 * (lo + hi)))
}

/// Shares 2-D nodes between blocks (keys rounded to `tol`), numbered in order of first use.
struct Pool {
    pts: Vec<[f64; 2]>,
    index: HashMap<(i64, i64), u32>,
    tol: f64,
}

impl Pool {
    fn add(&mut self, x: f64, y: f64) -> u32 {
        let k = ((x / self.tol).round_ties_even() as i64, (y / self.tol).round_ties_even() as i64);
        if let Some(i) = self.index.get(&k) {
            return *i;
        }
        self.pts.push([x, y]);
        let i = self.pts.len() as u32 - 1;
        self.index.insert(k, i);
        i
    }
    /// A block's nodes: x-major grid of `xs` by `ys`.
    fn rect(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, nx: usize, ny: usize) -> Vec<Vec<u32>> {
        let (xs, ys) = (linspace(x0, x1, nx), linspace(y0, y1, ny));
        xs.iter().map(|x| ys.iter().map(|y| self.add(*x, *y)).collect()).collect()
    }
}

fn rect_quads(ids: &[Vec<u32>], quads: &mut Vec<[u32; 4]>) {
    for i in 0..ids.len() - 1 {
        for j in 0..ids[0].len() - 1 {
            quads.push([ids[i][j], ids[i + 1][j], ids[i + 1][j + 1], ids[i][j + 1]]);
        }
    }
}

/// The plate: blocks 1 `plate` and 2 `ring`; side and node sets `x-` ... `z+`, node sets
/// `outer` (the four in-plane edges) and `grips` (both x faces), node and side set `hole`.
pub fn plate(o: &PlateOptions) -> Result<HexMesh, String> {
    let (w, ht, t, h) = (o.width, o.height, o.thickness, o.h);
    if w.min(ht).min(t).min(h) <= 0.0 {
        return Err("width, height, thickness and cell size must be positive".into());
    }
    let (nx, ny) = (divisions(w, h, "the width")?, divisions(ht, h, "the height")?);
    let tol = 1e-9 * h;
    let mut pool = Pool { pts: Vec::new(), index: HashMap::new(), tol };
    let mut quads: Vec<[u32; 4]> = Vec::new();
    let mut block: Vec<i32> = Vec::new();
    let mut hole_centre = [0.0; 2];
    match &o.hole {
        None => {
            let ids = pool.rect(0.0, w, 0.0, ht, nx, ny);
            rect_quads(&ids, &mut quads);
            block.resize(quads.len(), 1);
        }
        Some(hole) => {
            let [cx, cy] = hole.centre.unwrap_or([0.5 * w, 0.5 * ht]);
            hole_centre = [cx, cy];
            let (ex, ey) = hole.extent();
            let [kx, ky] = o.transition.unwrap_or([(ex / h - 1e-9).ceil() as usize + o.ring_cells, (ey / h - 1e-9).ceil() as usize + o.ring_cells]);
            let (sx, sy) = (kx as f64 * h, ky as f64 * h);
            for (name, c, s, l) in [("x", cx, sx, w), ("y", cy, sy, ht)] {
                if (c - s) / h < -1e-9 || (c + s) / h > l / h + 1e-9 {
                    return Err(format!("the ring around the hole leaves the plate along {name}"));
                }
                let i0 = (c - s) / h;
                if (i0 - i0.round()).abs() > 1e-9 {
                    return Err(format!("the hole centre {name} = {c} minus the ring half-size {s} is not on the grid of cell size {h}"));
                }
            }
            if ex >= sx || ey >= sy {
                return Err("the hole does not fit inside the ring".into());
            }
            let (x0, x1, y0, y1) = (cx - sx, cx + sx, cy - sy, cy + sy);
            let (i0, i1) = ((x0 / h).round() as usize, (x1 / h).round() as usize);
            let (j0, j1) = ((y0 / h).round() as usize, (y1 / h).round() as usize);
            // Rectangle perimeter, counter-clockwise from the +x edge's lower corner.
            let (xs, ys) = (linspace(x0, x1, 2 * kx), linspace(y0, y1, 2 * ky));
            let mut rect: Vec<[f64; 2]> = Vec::new();
            rect.extend(ys[..ys.len() - 1].iter().map(|y| [x1, *y]));
            rect.extend(xs.iter().rev().take(xs.len() - 1).map(|x| [*x, y1]));
            rect.extend(ys.iter().rev().take(ys.len() - 1).map(|y| [x0, *y]));
            rect.extend(xs[..xs.len() - 1].iter().map(|x| [*x, y0]));
            let n_theta = rect.len();
            let ring_in: Vec<[f64; 2]> = rect
                .iter()
                .map(|p| {
                    let th = (p[1] - cy).atan2(p[0] - cx);
                    let r = hole.radius(th);
                    [cx + r * th.cos(), cy + r * th.sin()]
                })
                .collect();
            let gap = rect.iter().zip(&ring_in).map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1])).fold(f64::INFINITY, f64::min);
            let arc = hole.perimeter() / n_theta as f64;
            let (n_r, grading) = match (o.n_r, o.grading) {
                (Some(n), Some(g)) => (n, g),
                (n, g) => {
                    let (na, ga) = auto_layers(gap, arc, h)?;
                    (n.unwrap_or(na), g.unwrap_or(ga))
                }
            };
            let tw = graded(n_r, grading);
            let og: Vec<Vec<u32>> = tw
                .iter()
                .map(|t| (0..n_theta).map(|j| pool.add((1.0 - t) * ring_in[j][0] + t * rect[j][0], (1.0 - t) * ring_in[j][1] + t * rect[j][1])).collect())
                .collect();
            for i in 0..n_r {
                for j in 0..n_theta {
                    let jn = (j + 1) % n_theta;
                    quads.push([og[i][j], og[i + 1][j], og[i + 1][jn], og[i][jn]]);
                }
            }
            block.resize(quads.len(), 2);
            let blocks = [
                (0.0, x0, y0, y1, i0, j1 - j0),
                (x1, w, y0, y1, nx - i1, j1 - j0),
                (x0, x1, 0.0, y0, i1 - i0, j0),
                (x0, x1, y1, ht, i1 - i0, ny - j1),
                (0.0, x0, 0.0, y0, i0, j0),
                (x1, w, 0.0, y0, nx - i1, j0),
                (0.0, x0, y1, ht, i0, ny - j1),
                (x1, w, y1, ht, nx - i1, ny - j1),
            ];
            for (bx0, bx1, by0, by1, bnx, bny) in blocks {
                if bnx == 0 || bny == 0 {
                    continue;
                }
                let ids = pool.rect(bx0, bx1, by0, by1, bnx, bny);
                rect_quads(&ids, &mut quads);
                block.resize(quads.len(), 1);
            }
        }
    }
    let n2 = pool.pts.len() as u32;
    let mut m = HexMesh { npe: 8, ..Default::default() };
    let og = o.origin;
    for z in [0.0, t] {
        for p in &pool.pts {
            m.points.push([p[0] + og[0], p[1] + og[1], z + og[2]]);
        }
    }
    for q in &quads {
        m.cells.extend_from_slice(&[q[0], q[1], q[2], q[3], q[0] + n2, q[1] + n2, q[2] + n2, q[3] + n2]);
    }
    m.block = block;
    m.block_names.insert(1, "plate".into());
    if o.hole.is_some() {
        m.block_names.insert(2, "ring".into());
    }
    let bounds = [(0, og[0]), (0, og[0] + w), (1, og[1]), (1, og[1] + ht), (2, og[2]), (2, og[2] + t)];
    for (name, (axis, value)) in ["x-", "x+", "y-", "y+", "z-", "z+"].iter().zip(bounds) {
        let f = m.faces_on_plane(axis, value, tol);
        m.add_side_set(name, f);
        let n = m.nodes_on_plane(axis, value, tol);
        m.add_node_set(name, n);
    }
    let cat = |m: &HexMesh, names: &[&str]| -> Vec<u32> { names.iter().flat_map(|n| m.node_set(n).unwrap().clone()).collect() };
    let outer = cat(&m, &["x-", "x+", "y-", "y+"]);
    m.add_node_set("outer", outer);
    let grips = cat(&m, &["x-", "x+"]);
    m.add_node_set("grips", grips);
    if let Some(hole) = &o.hole {
        let [cx, cy] = hole_centre;
        let on: Vec<u32> = (0..m.points.len() as u32)
            .filter(|i| {
                let p = m.points[*i as usize];
                let (x, y) = (p[0] - og[0], p[1] - og[1]);
                let th = (y - cy).atan2(x - cx);
                ((x - cx).hypot(y - cy) - hole.radius(th)).abs() <= 1e-6 * h
            })
            .collect();
        let mut member = vec![false; m.points.len()];
        for n in &on {
            member[*n as usize] = true;
        }
        m.add_node_set("hole", on);
        let faces: Vec<(u32, u8)> = m.boundary_faces().into_iter().filter(|(e, s)| HEX8_SIDES[*s as usize - 1].iter().all(|k| member[m.cell(*e as usize)[*k] as usize])).collect();
        m.add_side_set("hole", faces);
    }
    Ok(m)
}

#[derive(Clone, Debug)]
pub struct DogboneOptions {
    pub length: f64,
    pub gage_length: f64,
    pub gage_width: f64,
    pub grip_width: f64,
    pub fillet_radius: f64,
    pub grip_thickness: f64,
    /// (position from the gage start, thickness), sorted.
    pub stations: Vec<(f64, f64)>,
    pub h: f64,
    /// Grip cell size along x (default 2 h).
    pub h_grip: Option<f64>,
    pub n_z: usize,
    pub origin: P3,
}

impl Default for DogboneOptions {
    fn default() -> Self {
        DogboneOptions { length: 25.4, gage_length: 8.0, gage_width: 1.85, grip_width: 4.8, fillet_radius: 3.175, grip_thickness: 0.35, stations: Vec::new(), h: 0.2, h_grip: None, n_z: 1, origin: [0.0; 3] }
    }
}

/// Natural cubic spline through the points (clamped to their range).
fn spline(xs: &[f64], ys: &[f64]) -> impl Fn(f64) -> f64 {
    let n = xs.len();
    let mut b = vec![0.0; n.max(1)];
    let mut c = vec![0.0; n.max(1)];
    let mut d = vec![0.0; n.max(1)];
    if n >= 3 {
        let h: Vec<f64> = (0..n - 1).map(|i| xs[i + 1] - xs[i]).collect();
        // Tridiagonal solve for c (natural ends).
        let mut a = vec![[0.0; 3]; n];
        let mut r = vec![0.0; n];
        a[0][1] = 1.0;
        a[n - 1][1] = 1.0;
        for i in 1..n - 1 {
            a[i] = [h[i - 1], 2.0 * (h[i - 1] + h[i]), h[i]];
            r[i] = 3.0 * ((ys[i + 1] - ys[i]) / h[i] - (ys[i] - ys[i - 1]) / h[i - 1]);
        }
        for i in 1..n {
            let m = a[i][0] / a[i - 1][1];
            a[i][1] -= m * a[i - 1][2];
            r[i] -= m * r[i - 1];
        }
        c[n - 1] = r[n - 1] / a[n - 1][1];
        for i in (0..n - 1).rev() {
            c[i] = (r[i] - a[i][2] * c[i + 1]) / a[i][1];
        }
        for i in 0..n - 1 {
            b[i] = (ys[i + 1] - ys[i]) / h[i] - h[i] * (2.0 * c[i] + c[i + 1]) / 3.0;
            d[i] = (c[i + 1] - c[i]) / (3.0 * h[i]);
        }
    }
    let (xs, ys) = (xs.to_vec(), ys.to_vec());
    move |x: f64| -> f64 {
        match n {
            0 => 0.0,
            1 => ys[0],
            2 => {
                let x = x.clamp(xs[0], xs[1]);
                ys[0] + (ys[1] - ys[0]) * (x - xs[0]) / (xs[1] - xs[0])
            }
            _ => {
                let x = x.clamp(xs[0], xs[n - 1]);
                let i = (xs.partition_point(|v| *v <= x)).saturating_sub(1).min(n - 2);
                let dx = x - xs[i];
                ys[i] + b[i] * dx + c[i] * dx * dx + d[i] * dx * dx * dx
            }
        }
    }
}

/// The dog-bone: blocks 1 `gage`, 2 `shoulder`, 3 `grip`; side and node sets `x-`, `x+`;
/// node sets `grips`, `z-`, `z+`, `edge-`, `edge+`, `gage-`, `gage+` and `station_k`.
pub fn dogbone(o: &DogboneOptions) -> Result<HexMesh, String> {
    if [o.length, o.gage_length, o.gage_width, o.grip_width, o.fillet_radius, o.grip_thickness, o.h].iter().any(|v| *v <= 0.0) {
        return Err("every dog-bone dimension must be positive".into());
    }
    if o.gage_width >= o.grip_width {
        return Err("the gage must be narrower than the grips".into());
    }
    let step = 0.5 * (o.grip_width - o.gage_width);
    if o.fillet_radius < step {
        return Err("the fillet radius must be at least half the width step".into());
    }
    let d = (o.fillet_radius.powi(2) - (o.fillet_radius - step).powi(2)).sqrt();
    if o.gage_length + 2.0 * d >= o.length {
        return Err("the gage and the fillets do not fit inside the length".into());
    }
    let (h, hg) = (o.h, o.h_grip.unwrap_or(2.0 * o.h));
    let x0 = 0.5 * (o.length - o.gage_length);
    let x1 = x0 + o.gage_length;
    let round = |v: f64| -> usize { v.round_ties_even().max(1.0) as usize };
    let n_g = round(o.gage_length / h);
    let gage = linspace(x0, x1, n_g);
    let n_f = round(2.0 * d / (h + hg));
    let r = if n_f > 1 { (hg / h).powf(1.0 / (n_f - 1) as f64) } else { 1.0 };
    let mut s = vec![0.0];
    let mut acc = 0.0;
    for i in 0..n_f {
        acc += h * r.powi(i as i32);
        s.push(acc);
    }
    let last = s[n_f];
    let s: Vec<f64> = s.iter().map(|v| v / last * d).collect();
    let left_f: Vec<f64> = s.iter().map(|v| x0 - d + v).collect();
    let right_f: Vec<f64> = s.iter().map(|v| x1 + v).collect();
    let n_grip = round((x0 - d) / hg);
    let left_g = linspace(0.0, x0 - d, n_grip);
    let right_g = linspace(x1 + d, o.length, n_grip);
    let mut xs: Vec<f64> = Vec::new();
    xs.extend(&left_g[..n_grip]);
    xs.extend(&left_f[..n_f]);
    xs.extend(&gage[..n_g]);
    xs.extend(&right_f[..n_f]);
    xs.extend(&right_g);
    let mut region: Vec<i32> = Vec::new();
    for (n, code) in [(n_grip, 2), (n_f, 1), (n_g, 0), (n_f, 1), (n_grip, 2)] {
        region.extend(std::iter::repeat_n(code, n));
    }
    let (w2, ww2, rr) = (0.5 * o.gage_width, 0.5 * o.grip_width, o.fillet_radius);
    let half = |x: f64| -> f64 {
        if x >= x0 && x <= x1 {
            w2
        } else if x >= x0 - d && x < x0 {
            w2 + rr - (rr * rr - (x - x0).powi(2)).max(0.0).sqrt()
        } else if x > x1 && x <= x1 + d {
            w2 + rr - (rr * rr - (x - x1).powi(2)).max(0.0).sqrt()
        } else {
            ww2
        }
    };
    let sx: Vec<f64> = o.stations.iter().map(|(p, _)| x0 + p).collect();
    let st: Vec<f64> = o.stations.iter().map(|(_, t)| *t).collect();
    let f = spline(&sx, &st);
    let thick = |x: f64| -> f64 {
        if o.stations.is_empty() {
            return o.grip_thickness;
        }
        let (tl, tr) = (f(x0), f(x1));
        if x >= x0 && x <= x1 {
            f(x)
        } else if x >= x0 - d && x < x0 {
            o.grip_thickness + (tl - o.grip_thickness) * (x - (x0 - d)) / d
        } else if x > x1 && x <= x1 + d {
            tr + (o.grip_thickness - tr) * (x - x1) / d
        } else {
            o.grip_thickness
        }
    };
    let ny = round(o.gage_width / h);
    let nz = o.n_z.max(1);
    let nx = xs.len() - 1;
    let eta = linspace(-1.0, 1.0, ny);
    let zeta = linspace(0.0, 1.0, nz);
    let og = o.origin;
    let mut m = HexMesh { npe: 8, ..Default::default() };
    for &x in &xs {
        let (yw, tz) = (half(x), thick(x));
        for e in &eta {
            for z in &zeta {
                m.points.push([og[0] + x, og[1] + yw * e, og[2] + tz * z]);
            }
        }
    }
    let id = |i: usize, j: usize, k: usize| ((i * (ny + 1) + j) * (nz + 1) + k) as u32;
    for i in 0..nx {
        for j in 0..ny {
            for k in 0..nz {
                m.cells.extend_from_slice(&[id(i, j, k), id(i + 1, j, k), id(i + 1, j + 1, k), id(i, j + 1, k), id(i, j, k + 1), id(i + 1, j, k + 1), id(i + 1, j + 1, k + 1), id(i, j + 1, k + 1)]);
                m.block.push(region[i] + 1);
            }
        }
    }
    for (b, n) in [(1, "gage"), (2, "shoulder"), (3, "grip")] {
        m.block_names.insert(b, n.into());
    }
    let tol = 1e-9 * h;
    for (name, value) in [("x-", og[0]), ("x+", og[0] + o.length)] {
        let f = m.faces_on_plane(0, value, tol);
        m.add_side_set(name, f);
        let n = m.nodes_on_plane(0, value, tol);
        m.add_node_set(name, n);
    }
    let grips: Vec<u32> = [m.node_set("x-").unwrap().clone(), m.node_set("x+").unwrap().clone()].concat();
    m.add_node_set("grips", grips);
    let all = |f: &dyn Fn(usize, usize, usize) -> bool| -> Vec<u32> {
        let mut v = Vec::new();
        for i in 0..=nx {
            for j in 0..=ny {
                for k in 0..=nz {
                    if f(i, j, k) {
                        v.push(id(i, j, k));
                    }
                }
            }
        }
        v
    };
    m.add_node_set("z-", all(&|_, _, k| k == 0));
    m.add_node_set("z+", all(&|_, _, k| k == nz));
    m.add_node_set("edge-", all(&|_, j, _| j == 0));
    m.add_node_set("edge+", all(&|_, j, _| j == ny));
    for (name, value) in [("gage-", x0), ("gage+", x1)] {
        let n = m.nodes_on_plane(0, og[0] + value, tol);
        m.add_node_set(name, n);
    }
    for (k, (p, _)) in o.stations.iter().enumerate() {
        let target = x0 + p;
        let i = (0..xs.len()).min_by(|a, b| (xs[*a] - target).abs().total_cmp(&(xs[*b] - target).abs())).unwrap();
        m.add_node_set(&format!("station_{}", k + 1), all(&|ii, _, _| ii == i));
    }
    Ok(m)
}
