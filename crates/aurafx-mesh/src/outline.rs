//! Closed 2-D outlines: polylines, arcs, circles, ellipses, splines and rounded corners, kept as
//! a fine polygon whose edges remember which named curve they came from.

use anyhow::{bail, Result};
use std::f64::consts::TAU;

/// Segments used for a full circle (arcs use their share).
pub const CIRCLE_SEGMENTS: usize = 144;

/// One piece of an outline: points that follow each other, with the name of the curve each
/// segment (from a point to the next) belongs to and whether it is straight.
#[derive(Debug, Clone)]
struct Piece {
    points: Vec<[f64; 2]>,
    /// Curve index of the segment leaving each point (the last one leads to the next piece).
    curve: Vec<usize>,
    straight: Vec<bool>,
}

/// An outline being built and its finished polygon.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    points: Vec<[f64; 2]>,
    /// Curve of the segment from point i to point i + 1 (wrapping).
    curve: Vec<usize>,
    straight: Vec<bool>,
    /// Names of the curves ("edge 1", "arc 2", "circle", ...).
    pub curves: Vec<String>,
}

impl Outline {
    pub fn points(&self) -> &[[f64; 2]] {
        &self.points
    }

    fn push_curve(&mut self, name: String) -> usize {
        self.curves.push(name);
        self.curves.len() - 1
    }

    fn add(&mut self, p: Piece) {
        for i in 0..p.points.len() {
            // A point equal to the last one is dropped (pieces that meet).
            if let Some(last) = self.points.last() {
                if (last[0] - p.points[i][0]).abs() < 1e-12 && (last[1] - p.points[i][1]).abs() < 1e-12 {
                    let n = self.curve.len();
                    self.curve[n - 1] = p.curve[i];
                    self.straight[n - 1] = p.straight[i];
                    continue;
                }
            }
            self.points.push(p.points[i]);
            self.curve.push(p.curve[i]);
            self.straight.push(p.straight[i]);
        }
    }

    /// Straight segments through `pts`, each its own edge; the segment back to the start is
    /// named when the outline is closed.
    pub fn add_lines(&mut self, pts: &[[f64; 2]]) {
        let mut piece = Piece { points: Vec::new(), curve: Vec::new(), straight: Vec::new() };
        for p in pts {
            let c = self.push_curve(String::new());
            piece.points.push(*p);
            piece.curve.push(c);
            piece.straight.push(true);
        }
        self.add(piece);
        self.rename_edges();
    }

    /// An arc about `center` of `radius` from angle `a0` to `a1` (radians, counterclockwise if
    /// `a1 > a0`).
    pub fn add_arc(&mut self, center: [f64; 2], radius: f64, a0: f64, a1: f64) {
        let n = (((a1 - a0).abs() / TAU) * CIRCLE_SEGMENTS as f64).ceil().max(2.0) as usize;
        let c = self.push_curve(format!("arc {}", self.curves.iter().filter(|c| c.starts_with("arc")).count() + 1));
        let mut piece = Piece { points: Vec::new(), curve: Vec::new(), straight: Vec::new() };
        for k in 0..=n {
            let a = a0 + (a1 - a0) * k as f64 / n as f64;
            piece.points.push([center[0] + radius * a.cos(), center[1] + radius * a.sin()]);
            piece.curve.push(c);
            piece.straight.push(false);
        }
        // The segment leaving the arc's last point is a straight edge to what follows.
        let e = self.push_curve(String::new());
        if let (Some(s), Some(cv)) = (piece.straight.last_mut(), piece.curve.last_mut()) {
            *s = true;
            *cv = e;
        }
        self.add(piece);
        self.rename_edges();
    }

    /// A closed curve sampled from `f(t)`, t in [0, 1).
    fn closed_curve(&mut self, name: &str, n: usize, f: impl Fn(f64) -> [f64; 2]) {
        let c = self.push_curve(name.to_string());
        let piece = Piece { points: (0..n).map(|k| f(k as f64 / n as f64)).collect(), curve: vec![c; n], straight: vec![false; n] };
        self.add(piece);
    }

    pub fn circle(center: [f64; 2], r: f64) -> Outline {
        let mut o = Outline::default();
        o.closed_curve("circle", CIRCLE_SEGMENTS, |t| [center[0] + r * (TAU * t).cos(), center[1] + r * (TAU * t).sin()]);
        o
    }

    pub fn ellipse(center: [f64; 2], a: f64, b: f64) -> Outline {
        let mut o = Outline::default();
        o.closed_curve("ellipse", CIRCLE_SEGMENTS, |t| [center[0] + a * (TAU * t).cos(), center[1] + b * (TAU * t).sin()]);
        o
    }

    pub fn rectangle(center: [f64; 2], w: f64, h: f64) -> Outline {
        let (x0, x1, y0, y1) = (center[0] - w / 2.0, center[0] + w / 2.0, center[1] - h / 2.0, center[1] + h / 2.0);
        let mut o = Outline::default();
        o.add_lines(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
        o
    }

    /// A regular polygon with `n` sides, its first corner on the x axis.
    pub fn regular(n: usize, r: f64) -> Outline {
        let mut o = Outline::default();
        let pts: Vec<[f64; 2]> = (0..n).map(|k| [r * (TAU * k as f64 / n as f64).cos(), r * (TAU * k as f64 / n as f64).sin()]).collect();
        o.add_lines(&pts);
        o
    }

    /// A smooth closed curve through `pts` (Catmull-Rom).
    pub fn spline(pts: &[[f64; 2]]) -> Outline {
        let mut o = Outline::default();
        o.add_spline(pts, true);
        o
    }

    /// Append a smooth curve through `pts` (closed: back to its start).
    pub fn add_spline(&mut self, pts: &[[f64; 2]], closed: bool) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        let per = 24;
        let c = self.push_curve(format!("spline {}", self.curves.iter().filter(|c| c.starts_with("spline")).count() + 1));
        let get = |i: i64| -> [f64; 2] {
            if closed {
                pts[i.rem_euclid(n as i64) as usize]
            } else {
                pts[i.clamp(0, n as i64 - 1) as usize]
            }
        };
        let segs = if closed { n } else { n - 1 };
        let mut piece = Piece { points: Vec::new(), curve: Vec::new(), straight: Vec::new() };
        for s in 0..segs as i64 {
            let (p0, p1, p2, p3) = (get(s - 1), get(s), get(s + 1), get(s + 2));
            for k in 0..per {
                let t = k as f64 / per as f64;
                let (t2, t3) = (t * t, t * t * t);
                let q = |a: f64, b: f64, c2: f64, d: f64| 0.5 * (2.0 * b + (-a + c2) * t + (2.0 * a - 5.0 * b + 4.0 * c2 - d) * t2 + (-a + 3.0 * b - 3.0 * c2 + d) * t3);
                piece.points.push([q(p0[0], p1[0], p2[0], p3[0]), q(p0[1], p1[1], p2[1], p3[1])]);
                piece.curve.push(c);
                piece.straight.push(false);
            }
        }
        if !closed {
            let e = self.push_curve(String::new());
            piece.points.push(pts[n - 1]);
            piece.curve.push(e);
            piece.straight.push(true);
        }
        self.add(piece);
        self.rename_edges();
    }

    /// Names straight segments "edge 1", "edge 2", ... in order.
    fn rename_edges(&mut self) {
        let mut k = 0;
        let mut seen = std::collections::HashSet::new();
        for (i, c) in self.curve.iter().enumerate() {
            if self.straight[i] && (self.curves[*c].is_empty() || self.curves[*c].starts_with("edge ")) && seen.insert(*c) {
                k += 1;
                self.curves[*c] = format!("edge {k}");
            }
        }
    }

    /// Round every corner between two straight segments with an arc of `radius`.
    pub fn fillet(&mut self, radius: f64) -> Result<()> {
        let n = self.points.len();
        if n < 3 {
            bail!("the outline has fewer than 3 points");
        }
        let mut out = Outline { curves: self.curves.clone(), ..Default::default() };
        let mut count = 0;
        for i in 0..n {
            let (prev, cur, next) = (self.points[(i + n - 1) % n], self.points[i], self.points[(i + 1) % n]);
            let both = self.straight[(i + n - 1) % n] && self.straight[i];
            let (u, v) = (unit(sub(prev, cur)), unit(sub(next, cur)));
            let cosang = (u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0);
            let ang = cosang.acos();
            if !both || ang > std::f64::consts::PI - 1e-3 || ang < 1e-6 {
                out.points.push(cur);
                out.curve.push(self.curve[i]);
                out.straight.push(self.straight[i]);
                continue;
            }
            let t = (radius / (ang / 2.0).tan()).min(0.5 * len(sub(prev, cur))).min(0.5 * len(sub(next, cur)));
            let r = t * (ang / 2.0).tan();
            let (a, b) = ([cur[0] + u[0] * t, cur[1] + u[1] * t], [cur[0] + v[0] * t, cur[1] + v[1] * t]);
            let bis = unit([u[0] + v[0], u[1] + v[1]]);
            let d = r / (ang / 2.0).sin();
            let c = [cur[0] + bis[0] * d, cur[1] + bis[1] * d];
            let (a0, mut a1) = ((a[1] - c[1]).atan2(a[0] - c[0]), (b[1] - c[1]).atan2(b[0] - c[0]));
            // The short way round.
            while a1 - a0 > std::f64::consts::PI {
                a1 -= TAU;
            }
            while a0 - a1 > std::f64::consts::PI {
                a1 += TAU;
            }
            count += 1;
            out.curves.push(format!("fillet {count}"));
            let fc = out.curves.len() - 1;
            let m = (((a1 - a0).abs() / TAU) * CIRCLE_SEGMENTS as f64).ceil().max(2.0) as usize;
            for k in 0..=m {
                let ak = a0 + (a1 - a0) * k as f64 / m as f64;
                out.points.push([c[0] + r * ak.cos(), c[1] + r * ak.sin()]);
                let last = k == m;
                out.curve.push(if last { self.curve[i] } else { fc });
                out.straight.push(last);
            }
        }
        *self = out;
        Ok(())
    }

    /// The same outline traversed the other way (segment names kept).
    pub fn reverse_in_place(&mut self) {
        let n = self.points.len() as i64;
        let curve: Vec<usize> = (0..n).map(|i| self.curve[(n - 2 - i).rem_euclid(n) as usize]).collect();
        let straight: Vec<bool> = (0..n).map(|i| self.straight[(n - 2 - i).rem_euclid(n) as usize]).collect();
        self.points.reverse();
        self.curve = curve;
        self.straight = straight;
    }

    /// Check the outline can bound a region.
    pub fn validate(&self) -> Result<()> {
        if self.points.len() < 3 {
            bail!("an outline needs at least 3 points");
        }
        if self.area().abs() < 1e-14 {
            bail!("the outline encloses no area");
        }
        Ok(())
    }

    /// Signed area (positive counterclockwise).
    pub fn area(&self) -> f64 {
        let n = self.points.len();
        (0..n).map(|i| {
            let (a, b) = (self.points[i], self.points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
            * 0.5
    }

    /// Signed distance from `q` (negative inside) and the curve of the nearest segment.
    pub fn distance(&self, q: [f64; 2]) -> (f64, usize) {
        let v = &self.points;
        let n = v.len();
        let mut best = f64::INFINITY;
        let mut curve = 0;
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (v[j], v[i]);
            let e = sub(b, a);
            let w = sub(q, a);
            let t = ((w[0] * e[0] + w[1] * e[1]) / (e[0] * e[0] + e[1] * e[1]).max(1e-300)).clamp(0.0, 1.0);
            let d = [w[0] - e[0] * t, w[1] - e[1] * t];
            let dd = d[0] * d[0] + d[1] * d[1];
            if dd < best {
                best = dd;
                curve = self.curve[j];
            }
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
            j = i;
        }
        let d = best.sqrt();
        (if inside { -d } else { d }, curve)
    }

    pub fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in &self.points {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn len(a: [f64; 2]) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}
fn unit(a: [f64; 2]) -> [f64; 2] {
    let l = len(a).max(1e-300);
    [a[0] / l, a[1] / l]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_and_names() {
        let r = Outline::rectangle([0.0, 0.0], 2.0, 1.0);
        assert_eq!(r.curves, vec!["edge 1", "edge 2", "edge 3", "edge 4"]);
        let (d, c) = r.distance([0.0, 0.0]);
        assert!((d + 0.5).abs() < 1e-12 && (r.curves[c] == "edge 1" || r.curves[c] == "edge 3"));
        let (d, c) = r.distance([3.0, 0.0]);
        assert!((d - 2.0).abs() < 1e-12 && r.curves[c] == "edge 2");
        assert!((r.area() - 2.0).abs() < 1e-12);
        let mut f = Outline::rectangle([0.0, 0.0], 2.0, 2.0);
        f.fillet(0.5).unwrap();
        // Area of a square with rounded corners: 4 - (4 - pi) r^2.
        let want = 4.0 - (4.0 - std::f64::consts::PI) * 0.25;
        assert!((f.area() - want).abs() < 2e-3, "{}", f.area());
        assert!(f.curves.iter().filter(|c| c.starts_with("fillet")).count() == 4);
        let c = Outline::circle([1.0, 0.0], 2.0);
        assert!((c.distance([1.0, 0.0]).0 + 2.0).abs() < 1e-3);
    }
}
