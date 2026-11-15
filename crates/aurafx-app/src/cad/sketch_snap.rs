//! Snapping and guide lines of the sketch canvas: where the cursor lands (an existing point, a
//! centre, a midpoint, a quadrant, an intersection, a point on a curve, a guide line through a
//! point far away, or the grid), each kind switched on or off, and the constraint a new point
//! placed there gets.

use aurafx_cad::sketch::{Constraint, Entity, Sketch};
use glam::DVec2;

/// Which snaps are on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snaps {
    /// All snapping (Ctrl held turns it off for a moment).
    pub on: bool,
    pub grid: bool,
    /// Ends of lines and arcs, spline points and lone points.
    pub points: bool,
    pub midpoints: bool,
    /// Centres of circles and arcs.
    pub centers: bool,
    pub quadrants: bool,
    pub intersections: bool,
    /// The nearest point on a line, circle, arc or spline.
    pub curves: bool,
    /// Dashed horizontal and vertical guides through points (also far away) and the last point.
    pub guides: bool,
}

impl Default for Snaps {
    fn default() -> Self {
        Snaps { on: true, grid: true, points: true, midpoints: true, centers: true, quadrants: true, intersections: true, curves: true, guides: true }
    }
}

/// What the cursor snapped to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapKind {
    /// An existing point (end, lone point).
    Point(usize),
    /// The centre point of a circle or arc.
    Center(usize),
    /// The middle of a line.
    Midpoint(usize),
    /// A quadrant of a circle or arc.
    Quadrant(usize),
    /// Where two shapes cross.
    Intersection(usize, usize),
    /// On a shape, the nearest point.
    OnCurve(usize),
    /// Lined up with points: the vertical guide's point and the horizontal guide's point.
    Guide(Option<usize>, Option<usize>),
    Grid,
    Free,
}

impl SnapKind {
    pub fn label(&self) -> &'static str {
        match self {
            SnapKind::Point(_) => "Point",
            SnapKind::Center(_) => "Center",
            SnapKind::Midpoint(_) => "Midpoint",
            SnapKind::Quadrant(_) => "Quadrant",
            SnapKind::Intersection(..) => "Intersection",
            SnapKind::OnCurve(_) => "On curve",
            SnapKind::Guide(..) => "",
            SnapKind::Grid | SnapKind::Free => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapped {
    pub pos: DVec2,
    pub kind: SnapKind,
    /// Guide lines to draw (from the point lined up with, to the snapped position).
    pub guides: Vec<(DVec2, DVec2)>,
}

/// The polyline of a shape (closed shapes repeat their first point at the end).
pub fn shape_polyline(sk: &Sketch, e: &Entity) -> Vec<DVec2> {
    match e {
        Entity::Line { a, b, .. } => vec![sk.point(*a), sk.point(*b)],
        Entity::Circle { center, radius, .. } => {
            let c = sk.point(*center);
            (0..=96).map(|k| c + DVec2::from_angle(k as f64 / 96.0 * std::f64::consts::TAU) * *radius).collect()
        }
        Entity::Arc { center, start, end, .. } => {
            let c = sk.point(*center);
            let (s, t) = (sk.point(*start) - c, sk.point(*end) - c);
            let r = s.length();
            let a0 = s.y.atan2(s.x);
            let mut a1 = t.y.atan2(t.x);
            while a1 <= a0 {
                a1 += std::f64::consts::TAU;
            }
            let n = (((a1 - a0) / std::f64::consts::TAU * 96.0).ceil() as usize).max(4);
            (0..=n).map(|k| c + DVec2::from_angle(a0 + (a1 - a0) * k as f64 / n as f64) * r).collect()
        }
        _ => {
            let (p, closed) = super::sketch::entity_points(sk, e);
            let mut p = p;
            if closed && !p.is_empty() {
                p.push(p[0]);
            }
            p
        }
    }
}

fn closest_on(poly: &[DVec2], q: DVec2) -> (f64, DVec2) {
    let mut best = (f64::INFINITY, q);
    for w in poly.windows(2) {
        let ab = w[1] - w[0];
        let t = ((q - w[0]).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
        let c = w[0] + ab * t;
        let d = c.distance(q);
        if d < best.0 {
            best = (d, c);
        }
    }
    best
}

/// Crossings of two polylines.
fn crossings(p: &[DVec2], q: &[DVec2]) -> Vec<DVec2> {
    let mut out = Vec::new();
    for a in p.windows(2) {
        for b in q.windows(2) {
            let (r, s) = (a[1] - a[0], b[1] - b[0]);
            let den = r.perp_dot(s);
            if den.abs() < 1e-14 {
                continue;
            }
            let t = (b[0] - a[0]).perp_dot(s) / den;
            let u = (b[0] - a[0]).perp_dot(r) / den;
            if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                out.push(a[0] + r * t);
            }
        }
    }
    out
}

fn on_arc(sk: &Sketch, e: &Entity, p: DVec2) -> bool {
    match e {
        Entity::Arc { center, start, end, .. } => {
            let c = sk.point(*center);
            let ang = |v: DVec2| v.y.atan2(v.x).rem_euclid(std::f64::consts::TAU);
            let (a0, a1, a) = (ang(sk.point(*start) - c), ang(sk.point(*end) - c), ang(p - c));
            let span = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            (a - a0).rem_euclid(std::f64::consts::TAU) <= span + 1e-9
        }
        _ => true,
    }
}

/// Where the cursor at `cur` lands: `tol` is the snapping distance (sketch units, a few pixels),
/// `grid` the grid step, `anchor` the last point of the shape being drawn (for its guides),
/// `skip` a point to leave out (the one being dragged).
pub fn snap(sk: &Sketch, cur: DVec2, tol: f64, grid: f64, anchor: Option<DVec2>, skip: Option<usize>, s: &Snaps) -> Snapped {
    let free = Snapped { pos: cur, kind: SnapKind::Free, guides: vec![] };
    if !s.on {
        return free;
    }
    let centers: Vec<usize> = sk
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Circle { center, .. } | Entity::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    // Existing points and centres.
    let mut best: Option<(f64, Snapped)> = None;
    let offer = |d: f64, sn: Snapped, best: &mut Option<(f64, Snapped)>| {
        if d <= tol && best.as_ref().is_none_or(|(b, _)| d < *b) {
            *best = Some((d, sn));
        }
    };
    for (i, p) in sk.points.iter().enumerate() {
        if Some(i) == skip {
            continue;
        }
        let is_center = centers.contains(&i);
        if (is_center && !s.centers) || (!is_center && !s.points) {
            continue;
        }
        let pp = p.pos();
        offer(pp.distance(cur), Snapped { pos: pp, kind: if is_center { SnapKind::Center(i) } else { SnapKind::Point(i) }, guides: vec![] }, &mut best);
    }
    if let Some((_, b)) = best {
        return b;
    }
    let mut polys: Vec<(usize, Vec<DVec2>)> = Vec::new();
    for (ei, e) in sk.entities.iter().enumerate() {
        let uses_skip = skip.is_some_and(|k| match e {
            Entity::Line { a, b, .. } => *a == k || *b == k,
            Entity::Arc { center, start, end, .. } => *center == k || *start == k || *end == k,
            Entity::Circle { center, .. } => *center == k,
            _ => false,
        });
        if !uses_skip {
            polys.push((ei, shape_polyline(sk, e)));
        }
    }
    // Intersections (preferred over the other snaps on curves).
    if s.intersections {
        for i in 0..polys.len() {
            for j in i + 1..polys.len() {
                for x in crossings(&polys[i].1, &polys[j].1) {
                    offer(x.distance(cur), Snapped { pos: x, kind: SnapKind::Intersection(polys[i].0, polys[j].0), guides: vec![] }, &mut best);
                }
            }
        }
    }
    if s.midpoints {
        for (ei, e) in sk.entities.iter().enumerate() {
            if let Entity::Line { a, b, .. } = e {
                if Some(*a) == skip || Some(*b) == skip {
                    continue;
                }
                let m = (sk.point(*a) + sk.point(*b)) * 0.5;
                offer(m.distance(cur), Snapped { pos: m, kind: SnapKind::Midpoint(ei), guides: vec![] }, &mut best);
            }
        }
    }
    if s.quadrants {
        for (ei, e) in sk.entities.iter().enumerate() {
            let (c, r) = match e {
                Entity::Circle { center, radius, .. } => (sk.point(*center), *radius),
                Entity::Arc { center, start, .. } => (sk.point(*center), sk.point(*start).distance(sk.point(*center))),
                _ => continue,
            };
            for d in [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y] {
                let q = c + d * r;
                if on_arc(sk, e, q) {
                    offer(q.distance(cur), Snapped { pos: q, kind: SnapKind::Quadrant(ei), guides: vec![] }, &mut best);
                }
            }
        }
    }
    if let Some((_, b)) = best {
        return b;
    }
    if s.curves {
        for (ei, poly) in &polys {
            let (d, c) = closest_on(poly, cur);
            offer(d, Snapped { pos: c, kind: SnapKind::OnCurve(*ei), guides: vec![] }, &mut best);
        }
        if let Some((_, b)) = best {
            return b;
        }
    }
    // Guides: line up with points anywhere (and the anchor) horizontally or vertically.
    let mut pos = cur;
    let mut kind = SnapKind::Free;
    let mut guides = Vec::new();
    if s.guides {
        let mut cands: Vec<(Option<usize>, DVec2)> = sk.points.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(i, p)| (Some(i), p.pos())).collect();
        if let Some(a) = anchor {
            cands.push((None, a));
        }
        let bx = cands.iter().map(|(i, p)| ((p.x - cur.x).abs(), *i, *p)).filter(|x| x.0 <= tol).min_by(|a, b| a.0.total_cmp(&b.0));
        let by = cands.iter().map(|(i, p)| ((p.y - cur.y).abs(), *i, *p)).filter(|x| x.0 <= tol).min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, _, p)) = bx {
            pos.x = p.x;
        }
        if let Some((_, _, p)) = by {
            pos.y = p.y;
        }
        if let Some((_, _, p)) = bx {
            guides.push((p, pos));
        }
        if let Some((_, _, p)) = by {
            guides.push((p, pos));
        }
        if bx.is_some() || by.is_some() {
            kind = SnapKind::Guide(bx.and_then(|x| x.1), by.and_then(|x| x.1));
        }
    }
    if s.grid && grid > 0.0 {
        let g = DVec2::new((cur.x / grid).round() * grid, (cur.y / grid).round() * grid);
        if !matches!(kind, SnapKind::Guide(..)) {
            return Snapped { pos: g, kind: SnapKind::Grid, guides };
        }
        // A guide holds one coordinate; the grid gives the other.
        if guides.len() == 1 {
            if pos.x == cur.x {
                pos.x = g.x;
            } else {
                pos.y = g.y;
            }
            if let Some(gl) = guides.last_mut() {
                gl.1 = pos;
            }
        }
    }
    Snapped { pos, kind, guides }
}

/// The constraints a new point `p` placed by `kind` gets (it is on the shape it snapped to).
pub fn constraints_for(sk: &Sketch, p: usize, kind: SnapKind) -> Vec<Constraint> {
    let on = |e: usize| -> Option<Constraint> {
        match sk.entities.get(e)? {
            Entity::Line { .. } => Some(Constraint::PointOnLine(p, e)),
            Entity::Circle { .. } | Entity::Arc { .. } => Some(Constraint::PointOnCircle(p, e)),
            _ => None,
        }
    };
    match kind {
        SnapKind::Midpoint(e) => vec![Constraint::Midpoint(p, e)],
        SnapKind::OnCurve(e) | SnapKind::Quadrant(e) => on(e).into_iter().collect(),
        SnapKind::Intersection(a, b) => on(a).into_iter().chain(on(b)).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sketch {
        let mut s = Sketch::default();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        s.add_line(a, b);
        let c = s.add_point(20.0, 20.0);
        s.push(Entity::Circle { center: c, radius: 5.0, construction: false });
        let d = s.add_point(5.0, -5.0);
        let e = s.add_point(5.0, 5.0);
        s.add_line(d, e);
        s
    }

    #[test]
    fn each_kind_of_snap_and_its_switch() {
        let sk = sample();
        let all = Snaps::default();
        let at = |x: f64, y: f64, s: &Snaps| snap(&sk, DVec2::new(x, y), 0.6, 1.0, None, None, s);
        assert_eq!(at(10.3, 0.2, &all).kind, SnapKind::Point(1));
        assert_eq!(at(20.2, 19.9, &all).kind, SnapKind::Center(2));
        // (5, 0): the middle of the first line and where the two lines cross; the crossing wins.
        assert_eq!(at(5.2, 0.1, &all).kind, SnapKind::Intersection(0, 2));
        let no_x = Snaps { intersections: false, ..all };
        assert_eq!(at(5.2, 0.1, &no_x).kind, SnapKind::Midpoint(0));
        let q = at(25.3, 20.2, &all);
        assert_eq!((q.kind, q.pos), (SnapKind::Quadrant(1), DVec2::new(25.0, 20.0)));
        let c = at(20.0 + 5.0 * 0.6, 20.0 + 5.0 * 0.8 + 0.3, &all);
        assert_eq!(c.kind, SnapKind::OnCurve(1));
        assert!((c.pos.distance(DVec2::new(20.0, 20.0)) - 5.0).abs() < 0.02);
        // Far from everything: a guide through the circle's centre (x = 20) and the grid for y.
        let g = at(20.3, 41.7, &all);
        assert_eq!(g.kind, SnapKind::Guide(Some(2), None));
        assert_eq!(g.pos, DVec2::new(20.0, 42.0));
        assert_eq!(g.guides.len(), 1);
        // Guides off: the grid alone; all off: free.
        assert_eq!(at(20.3, 41.7, &Snaps { guides: false, ..all }).pos, DVec2::new(20.0, 42.0));
        assert_eq!(at(20.3, 41.7, &Snaps { guides: false, grid: false, ..all }).kind, SnapKind::Free);
        assert_eq!(at(10.3, 0.2, &Snaps { on: false, ..all }).pos, DVec2::new(10.3, 0.2));
        // Constraints for a new point on what it snapped to.
        assert_eq!(constraints_for(&sk, 9, SnapKind::Intersection(0, 1)), vec![Constraint::PointOnLine(9, 0), Constraint::PointOnCircle(9, 1)]);
    }
}
