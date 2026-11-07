//! 2D polylines and shape builders.
//!
//! Curves (arcs, béziers, text outlines) are flattened to polylines with a chord tolerance at creation
//! time. The original parametric description is kept by the document layer (`ezyx-project`) so shapes
//! stay editable; CAM only ever sees polylines.

use glam::{DAffine2, DVec2};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// Axis aligned 2D bounding box.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bounds2 {
    pub min: DVec2,
    pub max: DVec2,
}

impl Bounds2 {
    pub const EMPTY: Bounds2 = Bounds2 { min: DVec2::splat(f64::INFINITY), max: DVec2::splat(f64::NEG_INFINITY) };
    pub fn from_points<'a>(pts: impl IntoIterator<Item = &'a DVec2>) -> Self {
        let mut b = Self::EMPTY;
        for p in pts {
            b.include(*p);
        }
        b
    }
    pub fn include(&mut self, p: DVec2) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn union(&self, o: &Bounds2) -> Bounds2 {
        Bounds2 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }
    pub fn size(&self) -> DVec2 {
        if self.is_empty() {
            DVec2::ZERO
        } else {
            self.max - self.min
        }
    }
    pub fn center(&self) -> DVec2 {
        (self.min + self.max) * 0.5
    }
}

/// Default chord tolerance for flattening curves (mm).
pub const DEFAULT_TOLERANCE: f64 = 0.01;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Polyline {
    pub pts: Vec<DVec2>,
    pub closed: bool,
}

impl Polyline {
    pub fn new(pts: Vec<DVec2>, closed: bool) -> Self {
        let mut p = Self { pts, closed };
        if p.closed && p.pts.len() > 1 && p.pts.first() == p.pts.last() {
            p.pts.pop();
        }
        p
    }
    pub fn open(pts: Vec<DVec2>) -> Self {
        Self::new(pts, false)
    }
    pub fn closed(pts: Vec<DVec2>) -> Self {
        Self::new(pts, true)
    }
    pub fn len(&self) -> usize {
        self.pts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// Iterate segments (including the closing segment for closed paths).
    pub fn segments(&self) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
        let n = self.pts.len();
        let count = if self.closed && n > 1 { n } else { n.saturating_sub(1) };
        (0..count).map(move |i| (self.pts[i], self.pts[(i + 1) % n]))
    }

    pub fn length(&self) -> f64 {
        self.segments().map(|(a, b)| a.distance(b)).sum()
    }

    /// Signed area (shoelace). Positive = counter-clockwise. Zero for open paths.
    pub fn signed_area(&self) -> f64 {
        if !self.closed || self.pts.len() < 3 {
            return 0.0;
        }
        self.segments().map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>() * 0.5
    }
    pub fn is_ccw(&self) -> bool {
        self.signed_area() > 0.0
    }
    pub fn reversed(&self) -> Self {
        let mut p = self.clone();
        p.pts.reverse();
        p
    }
    /// Orient a closed path counter-clockwise (or clockwise when `ccw == false`).
    pub fn oriented(mut self, ccw: bool) -> Self {
        if self.closed && self.is_ccw() != ccw {
            self.pts.reverse();
        }
        self
    }

    pub fn bounds(&self) -> Bounds2 {
        Bounds2::from_points(&self.pts)
    }

    /// Even-odd point in polygon test (closed paths only).
    pub fn contains(&self, p: DVec2) -> bool {
        if !self.closed {
            return false;
        }
        let mut inside = false;
        for (a, b) in self.segments() {
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    pub fn transform(&self, t: &DAffine2) -> Self {
        Self { pts: self.pts.iter().map(|p| t.transform_point2(*p)).collect(), closed: self.closed }
    }

    /// Point at arc-length `s` along the path (clamped).
    pub fn point_at(&self, s: f64) -> DVec2 {
        let mut acc = 0.0;
        for (a, b) in self.segments() {
            let l = a.distance(b);
            if acc + l >= s && l > 0.0 {
                return a.lerp(b, (s - acc) / l);
            }
            acc += l;
        }
        if self.closed {
            self.pts.first().copied().unwrap_or_default()
        } else {
            self.pts.last().copied().unwrap_or_default()
        }
    }

    /// Ramer–Douglas–Peucker simplification.
    pub fn simplify(&self, tol: f64) -> Self {
        if self.pts.len() < 3 {
            return self.clone();
        }
        let mut pts = self.pts.clone();
        if self.closed {
            pts.push(pts[0]);
        }
        let mut keep = vec![false; pts.len()];
        keep[0] = true;
        *keep.last_mut().unwrap() = true;
        let mut stack = vec![(0usize, pts.len() - 1)];
        while let Some((s, e)) = stack.pop() {
            let (a, b) = (pts[s], pts[e]);
            let mut best = (0.0, 0usize);
            for (i, p) in pts.iter().enumerate().take(e).skip(s + 1) {
                let d = dist_point_segment(*p, a, b);
                if d > best.0 {
                    best = (d, i);
                }
            }
            if best.0 > tol {
                keep[best.1] = true;
                stack.push((s, best.1));
                stack.push((best.1, e));
            }
        }
        let mut out: Vec<DVec2> = pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect();
        if self.closed {
            out.pop();
        }
        Self::new(out, self.closed)
    }

    /// Chaikin corner-cutting smoothing (useful after bitmap tracing).
    pub fn chaikin(&self, iterations: usize) -> Self {
        let mut cur = self.clone();
        for _ in 0..iterations {
            if cur.pts.len() < 3 {
                break;
            }
            let mut out = Vec::with_capacity(cur.pts.len() * 2);
            if !cur.closed {
                out.push(cur.pts[0]);
            }
            for (a, b) in cur.segments() {
                out.push(a.lerp(b, 0.25));
                out.push(a.lerp(b, 0.75));
            }
            if !cur.closed {
                out.push(*cur.pts.last().unwrap());
            }
            cur = Self::new(out, cur.closed);
        }
        cur
    }

    /// Resample so no segment is longer than `max_seg`.
    pub fn densify(&self, max_seg: f64) -> Self {
        let mut out = Vec::new();
        for (a, b) in self.segments() {
            let n = ((a.distance(b) / max_seg).ceil() as usize).max(1);
            for i in 0..n {
                out.push(a.lerp(b, i as f64 / n as f64));
            }
        }
        if !self.closed {
            if let Some(l) = self.pts.last() {
                out.push(*l);
            }
        }
        Self::new(out, self.closed)
    }
}

pub fn dist_point_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Number of segments for an arc of `radius` spanning `sweep` radians under chord tolerance `tol`.
pub fn arc_segments(radius: f64, sweep: f64, tol: f64) -> usize {
    if radius <= tol {
        return 4;
    }
    let step = 2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos();
    ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(4, 4096)
}

// ---------------------------------------------------------------------------------------------
// Shape builders (all CCW, closed)
// ---------------------------------------------------------------------------------------------

pub mod shapes {
    use super::*;

    pub fn rectangle(origin: DVec2, w: f64, h: f64, corner_radius: f64) -> Polyline {
        let r = corner_radius.clamp(0.0, w.min(h) * 0.5);
        if r <= 0.0 {
            return Polyline::closed(vec![origin, origin + DVec2::new(w, 0.0), origin + DVec2::new(w, h), origin + DVec2::new(0.0, h)]);
        }
        let mut pts = Vec::new();
        let corners = [
            (origin + DVec2::new(w - r, r), -0.25 * TAU),
            (origin + DVec2::new(w - r, h - r), 0.0),
            (origin + DVec2::new(r, h - r), 0.25 * TAU),
            (origin + DVec2::new(r, r), 0.5 * TAU),
        ];
        let n = arc_segments(r, TAU / 4.0, DEFAULT_TOLERANCE);
        for (c, a0) in corners {
            for i in 0..=n {
                let a = a0 + TAU / 4.0 * i as f64 / n as f64;
                pts.push(c + DVec2::new(a.cos(), a.sin()) * r);
            }
        }
        Polyline::closed(pts)
    }

    pub fn circle(center: DVec2, r: f64) -> Polyline {
        ellipse(center, r, r, 0.0)
    }

    pub fn ellipse(center: DVec2, rx: f64, ry: f64, rotation: f64) -> Polyline {
        let n = arc_segments(rx.max(ry), TAU, DEFAULT_TOLERANCE);
        let (s, c) = rotation.sin_cos();
        Polyline::closed(
            (0..n)
                .map(|i| {
                    let a = TAU * i as f64 / n as f64;
                    let p = DVec2::new(a.cos() * rx, a.sin() * ry);
                    center + DVec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
                })
                .collect(),
        )
    }

    pub fn polygon(center: DVec2, r: f64, sides: usize, rotation: f64) -> Polyline {
        let sides = sides.max(3);
        Polyline::closed(
            (0..sides)
                .map(|i| {
                    let a = rotation + TAU * i as f64 / sides as f64 + TAU / 4.0;
                    center + DVec2::new(a.cos(), a.sin()) * r
                })
                .collect(),
        )
    }

    pub fn star(center: DVec2, r_outer: f64, r_inner: f64, points: usize, rotation: f64) -> Polyline {
        let points = points.max(3);
        Polyline::closed(
            (0..points * 2)
                .map(|i| {
                    let r = if i % 2 == 0 { r_outer } else { r_inner };
                    let a = rotation + TAU * i as f64 / (points * 2) as f64 + TAU / 4.0;
                    center + DVec2::new(a.cos(), a.sin()) * r
                })
                .collect(),
        )
    }

    /// Circular arc from `start_angle` sweeping `sweep` radians (positive = CCW).
    pub fn arc(center: DVec2, r: f64, start_angle: f64, sweep: f64) -> Polyline {
        let n = arc_segments(r, sweep, DEFAULT_TOLERANCE);
        Polyline::open(
            (0..=n)
                .map(|i| {
                    let a = start_angle + sweep * i as f64 / n as f64;
                    center + DVec2::new(a.cos(), a.sin()) * r
                })
                .collect(),
        )
    }

    /// Flatten a cubic bézier (p0 excluded so segments can be chained).
    pub fn cubic_bezier(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, tol: f64) -> Vec<DVec2> {
        let est = p0.distance(p1) + p1.distance(p2) + p2.distance(p3);
        let n = ((est / tol.max(1e-4)).sqrt().ceil() as usize).clamp(2, 512);
        (1..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                let u = 1.0 - t;
                p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::shapes::*;
    use super::*;

    #[test]
    fn rect_area_and_contains() {
        let r = rectangle(DVec2::ZERO, 10.0, 5.0, 0.0);
        assert!((r.signed_area() - 50.0).abs() < 1e-9);
        assert!(r.contains(DVec2::new(5.0, 2.5)));
        assert!(!r.contains(DVec2::new(11.0, 2.5)));
        assert!((r.length() - 30.0).abs() < 1e-9);
    }
    #[test]
    fn circle_area_close() {
        let c = circle(DVec2::ZERO, 10.0);
        assert!((c.signed_area() - std::f64::consts::PI * 100.0).abs() < 0.5);
        assert!(c.is_ccw());
    }
    #[test]
    fn simplify_collinear() {
        let p = Polyline::open((0..=10).map(|i| DVec2::new(i as f64, 0.0)).collect());
        assert_eq!(p.simplify(0.01).len(), 2);
    }
    #[test]
    fn rounded_rect_smaller_area() {
        let r = rectangle(DVec2::ZERO, 10.0, 10.0, 2.0);
        let expected = 100.0 - (4.0 - std::f64::consts::PI) * 4.0;
        assert!((r.signed_area() - expected).abs() < 0.15, "{} vs {}", r.signed_area(), expected);
    }
}
