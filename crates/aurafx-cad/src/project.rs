//! Sketch geometry taken from the part: an edge of the part is laid flat on the sketch plane
//! and becomes a line (a straight edge), a circle or an arc (a round edge seen face on) or a
//! fixed outline through its points (any other curve). The edges come from the B-rep kernel when
//! it builds the part, else from the borders between the faces of the part's surface.

use crate::sketch::{Constraint, Dim, Entity, Sketch};
use crate::{CadError, Document};
use aurafx_data::{ArrayData, DataSet};
use glam::{DMat3, DVec2, DVec3};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{PI, TAU};

/// What an edge of the part is once it lies on the sketch plane.
#[derive(Debug, Clone, PartialEq)]
pub enum Fitted {
    Line {
        a: DVec2,
        b: DVec2,
    },
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end`.
    Arc {
        center: DVec2,
        start: DVec2,
        end: DVec2,
    },
    Polyline {
        pts: Vec<DVec2>,
        closed: bool,
    },
}

impl Fitted {
    /// A word for messages.
    pub fn word(&self) -> &'static str {
        match self {
            Fitted::Line { .. } => "line",
            Fitted::Circle { .. } => "circle",
            Fitted::Arc { .. } => "arc",
            Fitted::Polyline { .. } => "outline",
        }
    }
}

/// An edge of the part: the faces on either side (their names, when known) and its points.
#[derive(Debug, Clone, PartialEq)]
pub struct PartEdge {
    pub faces: Vec<String>,
    pub polyline: Vec<[f64; 3]>,
}

/// A point of the part in the coordinates of a sketch plane (rotation with columns x, y,
/// normal, and the plane's origin): where it lands on the plane, and its height above it.
pub fn to_plane(frame: &(DMat3, DVec3), p: DVec3) -> (DVec2, f64) {
    let d = p - frame.1;
    (DVec2::new(d.dot(frame.0.x_axis), d.dot(frame.0.y_axis)), d.dot(frame.0.z_axis))
}

/// The points of an edge laid on a sketch plane.
pub fn flatten(frame: &(DMat3, DVec3), polyline: &[[f64; 3]]) -> Vec<DVec2> {
    polyline.iter().map(|p| to_plane(frame, DVec3::from_array(*p)).0).collect()
}

/// The least-squares circle through points: center and radius (`None`: they are on a line).
fn circle_through(pts: &[DVec2]) -> Option<(DVec2, f64)> {
    let n = pts.len() as f64;
    let mean = pts.iter().fold(DVec2::ZERO, |a, p| a + *p) / n;
    // x² + y² + D x + E y + F = 0 about the mean, by the normal equations.
    let (mut sxx, mut sxy, mut syy, mut sxz, mut syz, mut sz) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in pts {
        let q = *p - mean;
        let z = q.length_squared();
        sxx += q.x * q.x;
        sxy += q.x * q.y;
        syy += q.y * q.y;
        sxz += q.x * z;
        syz += q.y * z;
        sz += z;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-12 * (sxx + syy).powi(2).max(1e-300) {
        return None;
    }
    let cx = 0.5 * (sxz * syy - syz * sxy) / det;
    let cy = 0.5 * (syz * sxx - sxz * sxy) / det;
    let r = (cx * cx + cy * cy + sz / n).sqrt();
    (r.is_finite() && r > 0.0).then_some((mean + DVec2::new(cx, cy), r))
}

/// What the points of an edge on the sketch plane are: a line, a circle, an arc, or the
/// outline through them. `rel_tol` is how far a point may be off the line or the circle, as a
/// part of the size of the edge (1e-4 suits the points a kernel puts on its curves). `None`:
/// the edge is seen end on (it lands on one point).
pub fn fit_curve(pts: &[DVec2], rel_tol: f64) -> Option<Fitted> {
    let (lo, hi) = pts.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let size = (hi - lo).length();
    if pts.len() < 2 || !size.is_finite() || size < 1e-9 {
        return None;
    }
    let tol = rel_tol * size;
    let mut p: Vec<DVec2> = Vec::with_capacity(pts.len());
    for q in pts {
        if p.last().is_none_or(|l| l.distance(*q) > tol * 1e-3) {
            p.push(*q);
        }
    }
    let closed = p.len() >= 4 && p[0].distance(p[p.len() - 1]) <= tol;
    if closed {
        p.pop();
    }
    // A line: every point on the line through the two that are farthest apart (a round edge
    // seen from its side is one too).
    let far = |from: DVec2| p.iter().copied().max_by(|a, b| a.distance(from).total_cmp(&b.distance(from))).unwrap();
    let e0 = far(p[0]);
    let e1 = far(e0);
    let dir = (e1 - e0).normalize_or_zero();
    if p.iter().all(|q| dir.perp_dot(*q - e0).abs() <= tol) {
        // In the order the edge runs when its ends are the far points.
        let (a, b) = if !closed && p[0].distance(e1) < p[0].distance(e0) { (e1, e0) } else { (e0, e1) };
        return Some(Fitted::Line { a, b });
    }
    let outline = |p: Vec<DVec2>| Some(Fitted::Polyline { pts: p, closed });
    if p.len() < 4 {
        return outline(p);
    }
    let Some((c, r)) = circle_through(&p) else { return outline(p) };
    if p.iter().any(|q| (q.distance(c) - r).abs() > tol) {
        return outline(p);
    }
    // The turn from point to point about the center: all one way, in steps small enough that
    // the points are a curve and not the corners of a polygon.
    let ang = |q: DVec2| (q - c).y.atan2((q - c).x);
    let step = |a: DVec2, b: DVec2| {
        let d = (ang(b) - ang(a)).rem_euclid(TAU);
        if d > PI {
            d - TAU
        } else {
            d
        }
    };
    let n = p.len();
    let steps: Vec<f64> = (0..if closed { n } else { n - 1 }).map(|i| step(p[i], p[(i + 1) % n])).collect();
    let sweep: f64 = steps.iter().sum();
    let one_way = steps.iter().all(|s| *s > 0.0) || steps.iter().all(|s| *s < 0.0);
    if !one_way || steps.iter().any(|s| s.abs() > 50f64.to_radians()) {
        return outline(p);
    }
    if closed || sweep.abs() > TAU - 1e-6 {
        return if (sweep.abs() - TAU).abs() < 1e-3 { Some(Fitted::Circle { center: c, radius: r }) } else { outline(p) };
    }
    let (start, end) = if sweep > 0.0 { (p[0], p[n - 1]) } else { (p[n - 1], p[0]) };
    Some(Fitted::Arc { center: c, start, end })
}

impl Sketch {
    /// Add geometry taken from the part. Its points are fixed (it stays where the part's edge
    /// was), and shared with points already there, so edges that meet make one outline; a
    /// circle comes with its radius as a dimension. Returns the entities added (none when the
    /// same line, circle or arc is there already).
    pub fn add_fitted(&mut self, f: &Fitted, construction: bool) -> Vec<usize> {
        let point = |s: &mut Sketch, q: DVec2| -> usize {
            match s.points.iter().position(|p| p.pos().distance(q) < 1e-6) {
                Some(i) => i,
                None => s.add_fixed_point(q.x, q.y),
            }
        };
        let ent = match f {
            Fitted::Line { a, b } => {
                let (a, b) = (point(self, *a), point(self, *b));
                if a == b || self.entities.iter().any(|e| matches!(e, Entity::Line { a: x, b: y, .. } if (*x, *y) == (a, b) || (*x, *y) == (b, a))) {
                    return Vec::new();
                }
                Entity::Line { a, b, construction }
            }
            Fitted::Circle { center, radius } => {
                let c = point(self, *center);
                if self.entities.iter().any(|e| matches!(e, Entity::Circle { center: x, radius: r, .. } if *x == c && (r - radius).abs() < 1e-6)) {
                    return Vec::new();
                }
                let e = self.push(Entity::Circle { center: c, radius: *radius, construction });
                self.constrain(Constraint::Radius(e, Dim::new(*radius)));
                return vec![e];
            }
            Fitted::Arc { center, start, end } => {
                let (c, a, b) = (point(self, *center), point(self, *start), point(self, *end));
                if self.entities.iter().any(|e| matches!(e, Entity::Arc { center: x, start: y, end: z, .. } if (*x, *y, *z) == (c, a, b))) {
                    return Vec::new();
                }
                Entity::Arc { center: c, start: a, end: b, construction }
            }
            Fitted::Polyline { pts, closed } => Entity::Polyline { pts: pts.iter().map(|p| p.to_array()).collect(), closed: *closed, construction },
        };
        if self.entities.contains(&ent) {
            return Vec::new();
        }
        vec![self.push(ent)]
    }

    /// Lay edges of the part on the sketch plane and add them (see [`Sketch::add_fitted`]).
    /// Returns what each became; an edge seen end on adds nothing.
    pub fn add_part_edges(&mut self, edges: &[&PartEdge]) -> Vec<Fitted> {
        let frame = self.placement_frame();
        let mut out = Vec::new();
        for e in edges {
            if let Some(f) = fit_curve(&flatten(&frame, &e.polyline), 1e-4) {
                self.add_fitted(&f, false);
                out.push(f);
            }
        }
        out
    }
}

/// The triangles of a part's surface: points, triangles, the face of each triangle and the
/// names of the faces.
pub struct SurfaceTriangles {
    pub points: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub face: Vec<u32>,
    pub names: Vec<String>,
}

/// Read the triangles and their faces from the surface a kernel made.
pub fn surface_triangles(ds: &DataSet) -> Option<SurfaceTriangles> {
    let DataSet::PolyData(pd) = ds else { return None };
    let ids = pd.cell_data.get("FaceId");
    let names = match pd.field_data.get("FaceNames").map(|a| &a.data) {
        Some(ArrayData::String(v)) => v.clone(),
        _ => Vec::new(),
    };
    let points: Vec<[f64; 3]> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let (mut tris, mut face) = (Vec::new(), Vec::new());
    for (ci, (_, c)) in pd.polys.iter().enumerate() {
        let f = ids.map(|a| a.get(ci, 0) as u32).unwrap_or(0);
        // A polygon as a fan.
        for k in 1..c.len().saturating_sub(1) {
            tris.push([c[0] as u32, c[k] as u32, c[k + 1] as u32]);
            face.push(f);
        }
    }
    Some(SurfaceTriangles { points, tris, face, names })
}

/// The borders between the faces of a triangulated surface, as edges: the sides of triangles
/// whose two triangles belong to different faces, joined end to end for every pair of faces.
pub fn surface_edges(s: &SurfaceTriangles) -> Vec<PartEdge> {
    // Points at one place are one point, whatever the triangles say.
    let (lo, hi) = s.points.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(mut lo, mut hi), p| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
        (lo, hi)
    });
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f64, f64::max);
    if !size.is_finite() || size <= 0.0 {
        return Vec::new();
    }
    let q = 1e-7 * size;
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut place: Vec<[f64; 3]> = Vec::new();
    let weld: Vec<u32> = s
        .points
        .iter()
        .map(|p| {
            let key = [(p[0] / q).round() as i64, (p[1] / q).round() as i64, (p[2] / q).round() as i64];
            *ids.entry(key).or_insert_with(|| {
                place.push(*p);
                place.len() as u32 - 1
            })
        })
        .collect();
    let mut sides: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
    for (t, f) in s.tris.iter().zip(&s.face) {
        for k in 0..3 {
            let (a, b) = (weld[t[k] as usize], weld[t[(k + 1) % 3] as usize]);
            if a != b {
                sides.entry((a.min(b), a.max(b))).or_default().push(*f);
            }
        }
    }
    // The sides on a border, by the pair of faces they part.
    let mut borders: BTreeMap<(u32, u32), Vec<(u32, u32)>> = BTreeMap::new();
    for (side, faces) in &sides {
        let (f0, f1) = (faces.iter().min().copied().unwrap(), faces.iter().max().copied().unwrap());
        if f0 != f1 || faces.len() == 1 {
            borders.entry((f0, if faces.len() == 1 { u32::MAX } else { f1 })).or_default().push(*side);
        }
    }
    let mut out = Vec::new();
    for ((f0, f1), segs) in borders {
        let mut at: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (i, (a, b)) in segs.iter().enumerate() {
            at.entry(*a).or_default().push(i);
            at.entry(*b).or_default().push(i);
        }
        let mut used = vec![false; segs.len()];
        // Chains from their ends first (points where one side or three meet), then the loops.
        let mut starts: Vec<u32> = at.iter().filter(|(_, v)| v.len() != 2).map(|(p, _)| *p).collect();
        starts.extend(at.keys().copied());
        for start in starts {
            loop {
                let Some(first) = at[&start].iter().copied().find(|i| !used[*i]) else { break };
                let mut chain = vec![start];
                let (mut seg, mut cur) = (first, start);
                loop {
                    used[seg] = true;
                    cur = if segs[seg].0 == cur { segs[seg].1 } else { segs[seg].0 };
                    chain.push(cur);
                    if at[&cur].len() != 2 {
                        break;
                    }
                    match at[&cur].iter().copied().find(|i| !used[*i]) {
                        Some(next) => seg = next,
                        None => break,
                    }
                }
                let faces = [f0, f1].iter().filter_map(|f| s.names.get(*f as usize).cloned()).collect();
                out.push(PartEdge { faces, polyline: chain.iter().map(|p| place[*p as usize]).collect() });
            }
        }
    }
    out
}

/// The edges of the part a document makes: the B-rep kernel's when it builds the part (every
/// edge one curve), else the borders between the faces of the part's surface.
pub fn part_edges(doc: &Document) -> Result<Vec<PartEdge>, CadError> {
    if let Ok(v) = crate::kernel::brep::list_edges(doc) {
        if !v.is_empty() {
            return Ok(v.into_iter().map(|e| PartEdge { faces: e.faces, polyline: e.polyline }).collect());
        }
    }
    let ev = crate::kernel::evaluate_with(crate::KernelChoice::Auto, doc, 64, true)?;
    Ok(surface_triangles(&ev.surface).map(|s| surface_edges(&s)).unwrap_or_default())
}

/// The edges a description chooses: `{"face": "Plate z+"}` (every edge around that face),
/// `{"edge": ["Plate z+", "Bore side"]}` (the edges between two faces), or `{}` (all of them).
pub fn choose_edges<'a>(edges: &'a [PartEdge], how: &serde_json::Value) -> Result<Vec<&'a PartEdge>, CadError> {
    let has = |e: &PartEdge, n: &str| e.faces.iter().any(|f| f == n);
    let chosen: Vec<&PartEdge> = if let Some(pair) = how.get("edge").and_then(|e| e.as_array()) {
        let names: Vec<&str> = pair.iter().filter_map(|n| n.as_str()).collect();
        if names.len() != 2 {
            return Err(CadError::Edit("\"edge\" takes the names of the two faces the edge is between".into()));
        }
        edges.iter().filter(|e| has(e, names[0]) && has(e, names[1])).collect()
    } else if let Some(face) = how.get("face").and_then(|f| f.as_str()) {
        edges.iter().filter(|e| has(e, face)).collect()
    } else {
        edges.iter().collect()
    };
    if chosen.is_empty() {
        let mut names: Vec<&str> = edges.iter().flat_map(|e| e.faces.iter().map(String::as_str)).collect();
        names.sort();
        names.dedup();
        return Err(CadError::Edit(format!("the part has no such edge; its edges are around the faces: {}", names.join(", "))));
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn circle_pts(c: DVec2, r: f64, a0: f64, a1: f64, n: usize) -> Vec<DVec2> {
        (0..=n).map(|k| c + DVec2::from_angle(a0 + (a1 - a0) * k as f64 / n as f64) * r).collect()
    }

    #[test]
    fn straight_round_and_other_edges_become_lines_circles_arcs_and_outlines() {
        // A straight edge with points along it.
        let l = fit_curve(&[DVec2::new(1.0, 1.0), DVec2::new(2.0, 2.0), DVec2::new(5.0, 5.0)], 1e-4).unwrap();
        assert_eq!(l, Fitted::Line { a: DVec2::new(1.0, 1.0), b: DVec2::new(5.0, 5.0) });
        // A whole circle, closed on its first point, either way round.
        for (a0, a1) in [(0.3, 0.3 + TAU), (0.3 + TAU, 0.3)] {
            match fit_curve(&circle_pts(DVec2::new(3.0, -2.0), 7.5, a0, a1, 48), 1e-4).unwrap() {
                Fitted::Circle { center, radius } => assert!(center.distance(DVec2::new(3.0, -2.0)) < 1e-9 && (radius - 7.5).abs() < 1e-9),
                other => panic!("{other:?}"),
            }
        }
        // A quarter, drawn clockwise: the arc runs counter-clockwise from its other end.
        let quarter = circle_pts(DVec2::ZERO, 10.0, PI / 2.0, 0.0, 12);
        match fit_curve(&quarter, 1e-4).unwrap() {
            Fitted::Arc { center, start, end } => {
                assert!(center.length() < 1e-9, "{center}");
                assert!(start.distance(DVec2::new(10.0, 0.0)) < 1e-9 && end.distance(DVec2::new(0.0, 10.0)) < 1e-9, "{start} {end}");
            }
            other => panic!("{other:?}"),
        }
        // More than a half turn keeps its ends too.
        match fit_curve(&circle_pts(DVec2::ZERO, 4.0, 0.0, 1.5 * PI, 30), 1e-4).unwrap() {
            Fitted::Arc { start, end, .. } => assert!(start.distance(DVec2::new(4.0, 0.0)) < 1e-9 && end.distance(DVec2::new(0.0, -4.0)) < 1e-9),
            other => panic!("{other:?}"),
        }
        // A circle seen at a slant is an ellipse: an outline, closed.
        let ellipse: Vec<DVec2> = circle_pts(DVec2::ZERO, 10.0, 0.0, TAU, 48).iter().map(|p| DVec2::new(p.x, p.y * 0.8)).collect();
        assert!(matches!(fit_curve(&ellipse, 1e-4).unwrap(), Fitted::Polyline { closed: true, ref pts } if pts.len() == 48));
        // Seen from its side it is a line across its width; seen end on, an edge is nothing.
        let flat: Vec<DVec2> = circle_pts(DVec2::ZERO, 10.0, 0.0, TAU, 48).iter().map(|p| DVec2::new(p.x, 0.0)).collect();
        assert!(matches!(fit_curve(&flat, 1e-4).unwrap(), Fitted::Line { a, b } if (a.x - b.x).abs() > 19.99));
        assert_eq!(fit_curve(&[DVec2::new(2.0, 2.0), DVec2::new(2.0, 2.0)], 1e-4), None);
        // The corners of a hexagon are on a circle and are not one.
        let hexagon = circle_pts(DVec2::ZERO, 10.0, 0.0, TAU, 6);
        assert!(matches!(fit_curve(&hexagon, 1e-4).unwrap(), Fitted::Polyline { closed: true, .. }));
        // A wavy curve is an open outline through its points.
        let wave: Vec<DVec2> = (0..20).map(|k| DVec2::new(k as f64, (k as f64 * 0.7).sin())).collect();
        assert!(matches!(fit_curve(&wave, 1e-4).unwrap(), Fitted::Polyline { closed: false, ref pts } if pts.len() == 20));
    }

    #[test]
    fn edges_that_meet_share_their_corners_and_make_one_outline() {
        let mut s = Sketch::default();
        let c = [DVec2::new(0.0, 0.0), DVec2::new(20.0, 0.0), DVec2::new(20.0, 10.0), DVec2::new(0.0, 10.0)];
        for k in 0..4 {
            assert_eq!(s.add_fitted(&Fitted::Line { a: c[k], b: c[(k + 1) % 4] }, false).len(), 1);
        }
        assert_eq!(s.points.len(), 4);
        assert!(s.points.iter().all(|p| p.fixed));
        // The same edge again (either way round) adds nothing.
        assert!(s.add_fitted(&Fitted::Line { a: c[1], b: c[0] }, false).is_empty());
        let e = s.add_fitted(&Fitted::Circle { center: DVec2::new(10.0, 5.0), radius: 3.0 }, false);
        assert!(matches!(s.constraints.last(), Some(Constraint::Radius(i, d)) if *i == e[0] && d.value == 3.0));
        assert!(s.add_fitted(&Fitted::Circle { center: DVec2::new(10.0, 5.0), radius: 3.0 }, false).is_empty());
        let prof = s.profiles();
        assert_eq!(prof.len(), 2, "the rectangle and the circle in it");
        assert_eq!(s.solve().status, crate::SolveStatus::FullyDefined);
    }

    #[test]
    fn a_point_of_the_part_lands_on_a_slanted_plane_with_its_height() {
        let f = crate::PlaneFrame::new([0.0, 0.0, 5.0], [0.0, -1.0, 1.0], None).unwrap();
        let frame = (f.basis(), DVec3::from_array(f.origin));
        // A point straight above the origin of the plane along its normal.
        let n = frame.0.z_axis;
        let (uv, h) = to_plane(&frame, frame.1 + n * 3.0 + frame.0.x_axis * 2.0);
        assert!((uv - DVec2::new(2.0, 0.0)).length() < 1e-12 && (h - 3.0).abs() < 1e-12);
    }

    /// A block with a bore through it.
    fn block() -> Document {
        let mut d = Document::new("block");
        crate::edit::apply_op(&mut d, "add", &json!({"kind": "box", "name": "Block", "size": [20, 20, 20]})).unwrap();
        crate::edit::apply_op(&mut d, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 6, "depth": 20, "at": [0, 0, 10]})).unwrap();
        d
    }

    #[test]
    fn the_edges_around_the_top_of_a_bored_block_are_four_lines_and_a_circle() {
        let d = block();
        let edges = part_edges(&d).unwrap();
        let top = choose_edges(&edges, &json!({"face": "Block z+"})).unwrap();
        let mut s = Sketch::new(crate::SketchPlane::XY).on(crate::SketchPlane::XY, 10.0);
        let made = s.add_part_edges(&top);
        assert_eq!(made.iter().filter(|f| matches!(f, Fitted::Line { .. })).count(), 4, "{made:?}");
        // The rim of the bore: one circle, or arcs that go round it once.
        let round: Vec<&Fitted> = made.iter().filter(|f| matches!(f, Fitted::Circle { .. } | Fitted::Arc { .. })).collect();
        assert!(!round.is_empty(), "{made:?}");
        for f in &round {
            let (c, r) = match f {
                Fitted::Circle { center, radius } => (*center, *radius),
                Fitted::Arc { center, start, .. } => (*center, start.distance(*center)),
                _ => unreachable!(),
            };
            assert!(c.length() < 1e-6 && (r - 3.0).abs() < 1e-6, "{f:?}");
        }
        // The outline of the top and the rim in it: 20 x 20 less the bore.
        let areas: Vec<f64> = s.profiles().iter().map(|p| p.signed_area().abs()).collect();
        let outer = areas.iter().copied().fold(0.0, f64::max);
        let area = 2.0 * outer - areas.iter().sum::<f64>();
        assert!((area - (400.0 - PI * 9.0)).abs() < 0.2, "{area}");
        // The edge between the top and the bore's wall alone, and a wrong name.
        let rim = choose_edges(&edges, &json!({"edge": ["Block z+", "Bore side"]})).unwrap();
        assert!(rim.len() < top.len());
        assert!(choose_edges(&edges, &json!({"face": "Nothing"})).unwrap_err().to_string().contains("Block z+"));
    }

    #[test]
    fn the_borders_between_the_faces_of_a_surface_are_edges_too() {
        let d = block();
        let ev = crate::kernel::evaluate_with(crate::KernelChoice::Auto, &d, 48, true).unwrap();
        let s = surface_triangles(&ev.surface).unwrap();
        let edges = surface_edges(&s);
        // The border of the top with the +x side is the straight edge at x = 10, z = 10.
        let e = edges.iter().find(|e| e.faces.contains(&"Block z+".to_string()) && e.faces.contains(&"Block x+".to_string())).expect("an edge");
        assert!(e.polyline.iter().all(|p| (p[0] - 10.0).abs() < 1e-6 && (p[2] - 10.0).abs() < 1e-6), "{e:?}");
        let frame = (DMat3::IDENTITY, DVec3::new(0.0, 0.0, 10.0));
        assert!(matches!(fit_curve(&flatten(&frame, &e.polyline), 1e-4), Some(Fitted::Line { .. })));
        // The rim of the bore goes round once.
        let rim: Vec<&PartEdge> = edges.iter().filter(|e| e.faces.contains(&"Block z+".to_string()) && e.faces.contains(&"Bore side".to_string())).collect();
        assert!(!rim.is_empty());
        let len: f64 = rim.iter().map(|e| e.polyline.windows(2).map(|w| DVec3::from_array(w[0]).distance(DVec3::from_array(w[1]))).sum::<f64>()).sum();
        assert!((len - TAU * 3.0).abs() < 0.3, "{len}");
    }
}
