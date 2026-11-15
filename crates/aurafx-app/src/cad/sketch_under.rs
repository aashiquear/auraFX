//! The part underneath the sketch: while a sketch is open the canvas shows the part as it is
//! seen looking down on the sketch plane, dimmed, with the grid and the sketch over it, so
//! that one sees where one draws (the Sketch tab's *Show part* toggle turns it off). The *From
//! the part* tool takes an edge of the part into the sketch: a straight edge as a line, a round
//! one as a circle or an arc, any other as an outline through its points.

use super::sketch::SketchEdit;
use crate::app::State;
use aurafx_cad::project::{fit_curve, flatten, to_plane, Fitted, SurfaceTriangles};
use egui::{Color32, Pos2};
use glam::{DMat3, DVec2, DVec3};
use std::collections::HashMap;
use std::sync::Arc;

/// A triangle or a border line of the part on the sketch plane.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
    /// Its corners and how light it is (0 to 1).
    Tri([DVec2; 3], f32),
    Seg(DVec2, DVec2),
}

/// An edge of the part that can be taken into the sketch.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The edge on the sketch plane.
    pub flat: Vec<DVec2>,
    /// How high above the sketch plane it reaches (the nearest to the eye wins a tie).
    pub height: f64,
    pub fitted: Fitted,
}

/// The part as the sketch canvas shows it.
pub struct Under {
    /// The build of the part and the sketch plane this was made for.
    source: Arc<aurafx_data::DataSet>,
    frame: (DMat3, DVec3),
    /// The faces that look at the eye and the borders between faces, the farthest first: drawn
    /// in this order the nearer ones cover the farther ones.
    items: Vec<Item>,
    /// The part's extent on the sketch plane.
    pub lo: DVec2,
    pub hi: DVec2,
    /// The edges that can be taken, and the revision of the part they were listed for.
    pub edges: Option<(u64, Vec<Candidate>)>,
}

fn same_frame(a: &(DMat3, DVec3), b: &(DMat3, DVec3)) -> bool {
    a.0.abs_diff_eq(b.0, 1e-12) && a.1.abs_diff_eq(b.1, 1e-9)
}

/// The faces and borders of a surface as seen looking down on a plane (against its normal),
/// the farthest first, and the extent of the surface on the plane.
fn seen_from_above(s: &SurfaceTriangles, frame: &(DMat3, DVec3)) -> (Vec<Item>, DVec2, DVec2) {
    let flat: Vec<(DVec2, f64)> = s.points.iter().map(|p| to_plane(frame, DVec3::from_array(*p))).collect();
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for (uv, _) in &flat {
        lo = lo.min(*uv);
        hi = hi.max(*uv);
    }
    let (hlo, hhi) = flat.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (_, h)| (a.min(*h), b.max(*h)));
    let size = (hi - lo).max_element().max(hhi - hlo);
    if s.tris.is_empty() || !size.is_finite() || size <= 0.0 {
        return (Vec::new(), DVec2::ZERO, DVec2::ZERO);
    }
    // Points at one place are one point, so that triangles of different faces meet.
    let q = 1e-7 * size;
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let weld: Vec<u32> = s
        .points
        .iter()
        .map(|p| {
            let n = ids.len() as u32;
            *ids.entry([(p[0] / q).round() as i64, (p[1] / q).round() as i64, (p[2] / q).round() as i64]).or_insert(n)
        })
        .collect();
    let light = DVec3::new(-0.35, 0.45, 0.82).normalize();
    let mut keyed: Vec<(f64, Item)> = Vec::with_capacity(s.tris.len());
    // For every side of a triangle: the faces on it and which way their triangles look
    // (1: at the eye, 0: seen from the side, -1: away).
    let mut sides: HashMap<(u32, u32), (Vec<(u32, i8)>, u32, u32)> = HashMap::new();
    for (t, f) in s.tris.iter().zip(&s.face) {
        let p: [(DVec2, f64); 3] = [flat[t[0] as usize], flat[t[1] as usize], flat[t[2] as usize]];
        let v = |k: usize| p[k].0.extend(p[k].1);
        let n = (v(1) - v(0)).cross(v(2) - v(0));
        let len = n.length();
        let facing = if len <= 0.0 || (n.z / len).abs() < 1e-6 {
            0
        } else if n.z > 0.0 {
            1
        } else {
            -1
        };
        if facing == 1 {
            // Lit from the upper left, and a little lighter the higher it is, so that a boss
            // stands out from the face it stands on.
            let h = (p[0].1 + p[1].1 + p[2].1) / 3.0;
            let rise = if hhi > hlo { (h - hlo) / (hhi - hlo) } else { 1.0 };
            let shade = (0.55 + 0.45 * (n / len).dot(light).max(0.0)) * (0.8 + 0.3 * rise);
            keyed.push((h, Item::Tri([p[0].0, p[1].0, p[2].0], shade as f32)));
        }
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let (wa, wb) = (weld[a as usize], weld[b as usize]);
            if wa != wb {
                sides.entry((wa.min(wb), wa.max(wb))).or_insert_with(|| (Vec::new(), a, b)).0.push((*f, facing));
            }
        }
    }
    for (on, a, b) in sides.values() {
        let border = on.len() == 1 || on.iter().any(|x| x.0 != on[0].0) || on.iter().any(|x| (x.1 == 1) != (on[0].1 == 1));
        let seen = on.iter().any(|x| x.1 >= 0);
        let (pa, pb) = (flat[*a as usize], flat[*b as usize]);
        if border && seen && pa.0.distance(pb.0) > 1e-6 * size {
            // Just above the faces it borders, so they do not cover it.
            keyed.push((pa.1.max(pb.1) + 1e-6 * size, Item::Seg(pa.0, pb.0)));
        }
    }
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    (keyed.into_iter().map(|(_, i)| i).collect(), lo, hi)
}

/// The edges of a part that can be taken into a sketch on a plane: each laid on the plane with
/// what it becomes there. Edges seen end on are left out.
pub fn candidates(edges: &[aurafx_cad::project::PartEdge], frame: &(DMat3, DVec3)) -> Vec<Candidate> {
    edges
        .iter()
        .filter_map(|e| {
            let flat = flatten(frame, &e.polyline);
            let fitted = fit_curve(&flat, 1e-4)?;
            let height = e.polyline.iter().map(|p| to_plane(frame, DVec3::from_array(*p)).1).fold(f64::NEG_INFINITY, f64::max);
            Some(Candidate { flat, height, fitted })
        })
        .collect()
}

/// The candidate nearest to a point of the sketch plane, no farther than `reach` from it (of
/// two as near, the one higher above the plane: the one the eye sees).
pub fn nearest(cands: &[Candidate], at: DVec2, reach: f64) -> Option<usize> {
    let dist = |c: &Candidate| -> f64 {
        c.flat
            .windows(2)
            .map(|w| {
                let ab = w[1] - w[0];
                let t = ((at - w[0]).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
                (w[0] + ab * t).distance(at)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, c) in cands.iter().enumerate() {
        let d = dist(c);
        if d > reach {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, bd, bh)) => d < bd - 1e-6 * reach.min(1.0) || ((d - bd).abs() <= 1e-6 * reach.min(1.0) && c.height > bh),
        };
        if better {
            best = Some((i, d, c.height));
        }
    }
    best.map(|(i, _, _)| i)
}

/// Bring what the canvas shows of the part up to date: the faces and borders when the part
/// was built again or the sketch plane moved, and the edges that can be taken when `edges` is
/// asked for (the *From the part* tool) and the part changed since they were listed.
pub fn refresh(state: &mut State, edges: bool) {
    let Some(frame) = state.cad.sketch.as_ref().map(|e| e.sketch.placement_frame()) else { return };
    let (ds, rev) = {
        let s = state.session.lock();
        let Some(e) = s.cad_edit.as_ref() else { return };
        (s.nodes.get(&e.node).and_then(|n| n.output.clone()), e.revision)
    };
    let current = state
        .cad
        .sketch
        .as_ref()
        .and_then(|e| e.under.as_ref())
        .is_some_and(|u| ds.as_ref().is_some_and(|d| Arc::ptr_eq(d, &u.source)) && same_frame(&u.frame, &frame));
    if !current {
        let made = ds.and_then(|ds| {
            let surface = super::preview::part_surface(state)?;
            let (items, lo, hi) = seen_from_above(&surface, &frame);
            (!items.is_empty()).then_some(Under { source: ds, frame, items, lo, hi, edges: None })
        });
        if let Some(e) = state.cad.sketch.as_mut() {
            e.under = made;
        }
    }
    if !edges {
        return;
    }
    let listed = state.cad.sketch.as_ref().and_then(|e| e.under.as_ref()).is_none_or(|u| u.edges.as_ref().is_some_and(|(r, _)| *r == rev));
    if listed {
        return;
    }
    let doc = state.session.lock().cad_edit.as_ref().map(|e| e.document_to_build());
    let found = doc.map(|d| aurafx_cad::project::part_edges(&d).map(|v| candidates(&v, &frame)));
    match found {
        Some(Ok(c)) => {
            if c.is_empty() {
                state.toast("The part has no edge that can be laid on this sketch plane.".to_string(), true);
            }
            if let Some(u) = state.cad.sketch.as_mut().and_then(|e| e.under.as_mut()) {
                u.edges = Some((rev, c));
            }
        }
        Some(Err(err)) => {
            state.toast(format!("The edges of the part cannot be listed: {err}"), true);
            if let Some(u) = state.cad.sketch.as_mut().and_then(|e| e.under.as_mut()) {
                u.edges = Some((rev, Vec::new()));
            }
        }
        None => {}
    }
}

fn mix(a: Color32, b: [f32; 3], t: f32) -> Color32 {
    let m = |x: u8, y: f32| (x as f32 * (1.0 - t) + y * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b[0]), m(a.g(), b[1]), m(a.b(), b[2]))
}

impl Under {
    /// Draw the part under the grid: its faces dimmed toward the canvas color and its borders
    /// as thin lines.
    pub fn draw(&self, painter: &egui::Painter, to_s: &dyn Fn(DVec2) -> Pos2, bg: Color32, dark: bool) {
        let body: [f32; 3] = if dark { [150.0, 170.0, 205.0] } else { [118.0, 146.0, 188.0] };
        let line = mix(bg, if dark { [235.0; 3] } else { [20.0, 30.0, 45.0] }, 0.75);
        let mut mesh = egui::Mesh::default();
        for item in &self.items {
            match item {
                Item::Tri(p, shade) => {
                    let k = 0.35 + 0.75 * shade;
                    let col = mix(bg, body.map(|c| (c * k).min(255.0)), if dark { 0.6 } else { 0.72 });
                    let i = mesh.vertices.len() as u32;
                    for q in p {
                        mesh.colored_vertex(to_s(*q), col);
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                }
                Item::Seg(a, b) => {
                    let (a, b) = (to_s(*a), to_s(*b));
                    let d = b - a;
                    let len = d.length();
                    if len < 0.05 {
                        continue;
                    }
                    let n = egui::vec2(-d.y, d.x) / len * 0.75;
                    let i = mesh.vertices.len() as u32;
                    for q in [a + n, b + n, b - n, a - n] {
                        mesh.colored_vertex(q, line);
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                    mesh.add_triangle(i, i + 2, i + 3);
                }
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }

    /// The edges that can be taken (empty until they were listed).
    pub fn candidates(&self) -> &[Candidate] {
        self.edges.as_ref().map(|(_, c)| c.as_slice()).unwrap_or(&[])
    }
}

/// Take a candidate edge into the sketch. Returns what it became in a few words (`None`: it
/// is there already).
pub fn take(e: &mut SketchEdit, index: usize) -> Option<String> {
    let c = e.under.as_ref()?.candidates().get(index)?.clone();
    if e.sketch.add_fitted(&c.fitted, false).is_empty() {
        return None;
    }
    Some(match &c.fitted {
        Fitted::Line { a, b } => format!("a line of {} mm", super::num_text(a.distance(*b))),
        Fitted::Circle { radius, .. } => format!("a circle of radius {} mm", super::num_text(*radius)),
        Fitted::Arc { center, start, .. } => format!("an arc of radius {} mm", super::num_text(start.distance(*center))),
        Fitted::Polyline { pts, closed } => format!("{} outline through {} points", if *closed { "a closed" } else { "an open" }, pts.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block() -> aurafx_cad::Document {
        let mut d = aurafx_cad::Document::new("block");
        aurafx_cad::edit::apply_op(&mut d, "add", &json!({"kind": "box", "name": "Block", "size": [20, 20, 20]})).unwrap();
        aurafx_cad::edit::apply_op(&mut d, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 6, "depth": 20, "at": [0, 0, 10]})).unwrap();
        d
    }

    #[test]
    fn the_part_under_a_sketch_is_its_top_faces_and_their_borders_the_nearest_last() {
        let d = block();
        let ev = aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &d, 48, true).unwrap();
        let s = aurafx_cad::project::surface_triangles(&ev.surface).unwrap();
        // Looking down on the top of the block (z = 10).
        let frame = (DMat3::IDENTITY, DVec3::new(0.0, 0.0, 10.0));
        let (items, lo, hi) = seen_from_above(&s, &frame);
        assert!((lo - DVec2::splat(-10.0)).length() < 1e-6 && (hi - DVec2::splat(10.0)).length() < 1e-6, "{lo} {hi}");
        // The faces that look up cover the top less the bore; the sides and the bottom are not drawn.
        let area: f64 = items
            .iter()
            .map(|i| match i {
                Item::Tri(p, _) => 0.5 * (p[1] - p[0]).perp_dot(p[2] - p[0]).abs(),
                Item::Seg(..) => 0.0,
            })
            .sum();
        assert!((area - (400.0 - std::f64::consts::PI * 9.0)).abs() < 0.5, "{area}");
        // The outline of the top and the rim of the bore are among the borders.
        let on_outline = |p: DVec2| (p.x.abs() - 10.0).abs() < 1e-6 || (p.y.abs() - 10.0).abs() < 1e-6;
        let segs: Vec<(DVec2, DVec2)> = items.iter().filter_map(|i| if let Item::Seg(a, b) = i { Some((*a, *b)) } else { None }).collect();
        assert!(segs.iter().any(|(a, b)| on_outline(*a) && on_outline(*b)));
        assert!(segs.iter().any(|(a, b)| (a.length() - 3.0).abs() < 1e-3 && (b.length() - 3.0).abs() < 1e-3));
        // From the side (the front plane): the square front, and behind it the half of the bore's
        // wall that looks at the eye (6 wide, 20 high), drawn first so that the front covers it.
        let front = aurafx_cad::SketchPlane::XZ.frame(0.0);
        let (side, ..) = seen_from_above(&s, &front);
        let area: f64 = side.iter().map(|i| if let Item::Tri(p, _) = i { 0.5 * (p[1] - p[0]).perp_dot(p[2] - p[0]).abs() } else { 0.0 }).sum();
        assert!((area - 520.0).abs() < 0.5, "{area}");
        let first_front = side.iter().position(|i| matches!(i, Item::Tri(p, _) if p.iter().any(|q| q.x.abs() > 9.0))).unwrap();
        let last_wall = side.iter().rposition(|i| matches!(i, Item::Tri(p, _) if p.iter().all(|q| q.x.abs() < 3.1))).unwrap();
        assert!(last_wall < first_front, "the wall of the bore is drawn before the front that hides it");
    }

    #[test]
    fn the_edge_under_the_cursor_is_taken_as_a_circle_or_a_line() {
        let d = block();
        let frame = (DMat3::IDENTITY, DVec3::new(0.0, 0.0, 10.0));
        let cands = candidates(&aurafx_cad::project::part_edges(&d).unwrap(), &frame);
        // The vertical edges of the block are seen end on: not offered.
        assert!(cands.iter().all(|c| c.flat.first() != c.flat.last() || c.flat.len() > 2));
        // Near the rim of the bore: a circle (or an arc of it) of radius 3, the one at the top.
        let i = nearest(&cands, DVec2::new(3.2, 0.1), 1.0).expect("the rim");
        assert!((cands[i].height).abs() < 1e-6, "the rim at the top, not the one at the bottom: {}", cands[i].height);
        match &cands[i].fitted {
            Fitted::Circle { center, radius } => assert!(center.length() < 1e-6 && (radius - 3.0).abs() < 1e-6),
            Fitted::Arc { center, start, .. } => assert!(center.length() < 1e-6 && (start.length() - 3.0).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
        // Near the +x side of the top: the line along it; far from everything: nothing.
        let j = nearest(&cands, DVec2::new(9.8, 4.0), 1.0).expect("the side");
        assert!(matches!(&cands[j].fitted, Fitted::Line { a, b } if (a.x - 10.0).abs() < 1e-6 && (b.x - 10.0).abs() < 1e-6 && (a.y - b.y).abs() > 19.9));
        assert_eq!(nearest(&cands, DVec2::new(6.5, 6.5), 1.0), None);
        // Taken into a sketch on the top of the block, twice: the second time adds nothing.
        let mut e = SketchEdit::new(None, aurafx_cad::Sketch::new(aurafx_cad::SketchPlane::XY).on(aurafx_cad::SketchPlane::XY, 10.0));
        let ev = aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &d, 48, true).unwrap();
        e.under = Some(Under { source: Arc::new(ev.surface), frame, items: Vec::new(), lo: DVec2::ZERO, hi: DVec2::ZERO, edges: Some((0, cands)) });
        assert!(take(&mut e, j).unwrap().starts_with("a line of 20"));
        assert_eq!(take(&mut e, j), None);
        assert_eq!(e.sketch.entities.len(), 1);
    }
}
