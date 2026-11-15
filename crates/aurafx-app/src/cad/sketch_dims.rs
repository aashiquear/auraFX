//! Dimensions on the sketch canvas: drawn with extension lines, a dimension line and arrows (an
//! arc for angles, a leader for radii and diameters), the label where the user dragged it, and
//! the label found under the cursor for dragging and double-click editing.

use super::num_text;
use aurafx_cad::sketch::{Constraint, Dim, Entity, Sketch};
use glam::DVec2;

/// How one dimension is drawn (sketch units).
#[derive(Debug, Clone)]
pub struct DimGeom {
    /// The constraint's index.
    pub index: usize,
    pub label: DVec2,
    pub text: String,
    /// Plain lines (extension lines, leaders).
    pub lines: Vec<(DVec2, DVec2)>,
    /// Lines with an arrow at the first end (pointing at it) and, when the flag is set, at both.
    pub arrows: Vec<(DVec2, DVec2, bool)>,
    /// An arc about a centre with a radius, from one angle to another (radians, counter-clockwise).
    pub arc: Option<(DVec2, f64, f64, f64)>,
}

fn line_pts(sk: &Sketch, e: usize) -> Option<(DVec2, DVec2)> {
    match sk.entities.get(e)? {
        Entity::Line { a, b, .. } => Some((sk.point(*a), sk.point(*b))),
        _ => None,
    }
}

fn circle_of(sk: &Sketch, e: usize) -> Option<(DVec2, f64)> {
    match sk.entities.get(e)? {
        Entity::Circle { center, radius, .. } => Some((sk.point(*center), *radius)),
        Entity::Arc { center, start, .. } => Some((sk.point(*center), sk.point(*start).distance(sk.point(*center)))),
        _ => None,
    }
}

pub fn dim_text(d: &Dim, prefix: &str, suffix: &str) -> String {
    format!("{prefix}{}{suffix}{}", num_text(d.value), d.expr.as_ref().map(|x| format!(" = {x}")).unwrap_or_default())
}

/// A linear dimension between `a` and `b` measured along `dir` (unit), its label at `label`.
fn linear(index: usize, a: DVec2, b: DVec2, dir: DVec2, label: DVec2, text: String) -> DimGeom {
    let n = dir.perp();
    // The dimension line runs along dir at the label's offset across it.
    let off_a = (label - a).dot(n);
    let off_b = (label - b).dot(n);
    let (pa, pb) = (a + n * off_a, b + n * off_b);
    let lines = vec![(a, pa), (b, pb)];
    let mut g = DimGeom { index, label, text, lines, arrows: vec![(pa, pb, true)], arc: None };
    // An extension of the dimension line out to the label when the label is outside the ends.
    let t = (label - pa).dot(dir);
    let len = (pb - pa).dot(dir);
    if t < 0.0 || t > len {
        let end = if t < 0.0 { pa } else { pb };
        g.lines.push((end, pa + dir * t));
    }
    g
}

/// The default place of a dimension's label (before the user's offset), `gap` sketch units out.
pub fn geometry(sk: &Sketch, index: usize, c: &Constraint, gap: f64) -> Option<DimGeom> {
    let d = c.clone();
    let at = |dm: &Dim| dm.at.map(DVec2::from_array).unwrap_or(DVec2::ZERO);
    Some(match &d {
        Constraint::Length(e, dm) => {
            let (a, b) = line_pts(sk, *e)?;
            let dir = (b - a).normalize_or(DVec2::X);
            let lab = (a + b) * 0.5 + dir.perp() * gap + at(dm);
            linear(index, a, b, dir, lab, dim_text(dm, "", ""))
        }
        Constraint::Distance(p, q, dm) => {
            let (a, b) = (sk.point(*p), sk.point(*q));
            let dir = (b - a).normalize_or(DVec2::X);
            let lab = (a + b) * 0.5 + dir.perp() * gap + at(dm);
            linear(index, a, b, dir, lab, dim_text(dm, "", ""))
        }
        Constraint::HorizontalDistance(p, q, dm) => {
            let (a, b) = (sk.point(*p), sk.point(*q));
            let lab = DVec2::new((a.x + b.x) * 0.5, a.y.max(b.y) + gap) + at(dm);
            linear(index, a, b, DVec2::X, lab, dim_text(dm, "", ""))
        }
        Constraint::VerticalDistance(p, q, dm) => {
            let (a, b) = (sk.point(*p), sk.point(*q));
            let lab = DVec2::new(a.x.max(b.x) + gap, (a.y + b.y) * 0.5) + at(dm);
            linear(index, a, b, DVec2::Y, lab, dim_text(dm, "", ""))
        }
        Constraint::PointLineDistance(p, e, dm) => {
            let (la, lb) = line_pts(sk, *e)?;
            let pp = sk.point(*p);
            let u = (lb - la).normalize_or(DVec2::X);
            let foot = la + u * (pp - la).dot(u);
            let dir = (pp - foot).normalize_or(u.perp());
            let lab = (pp + foot) * 0.5 + dir.perp() * gap + at(dm);
            let mut g = linear(index, foot, pp, dir, lab, dim_text(dm, "", ""));
            // The line itself extended to the foot when the foot is beyond its ends.
            let t = (foot - la).dot(u);
            if t < 0.0 || t > (lb - la).length() {
                g.lines.push((if t < 0.0 { la } else { lb }, foot));
            }
            g
        }
        Constraint::Radius(e, dm) | Constraint::Diameter(e, dm) => {
            let (cen, r) = circle_of(sk, *e)?;
            let lab = cen + DVec2::new(1.0, 1.0).normalize() * (r + gap) + at(dm);
            let dir = (lab - cen).normalize_or(DVec2::X);
            let rim = cen + dir * r;
            let diameter = matches!(d, Constraint::Diameter(..));
            let mut g = DimGeom { index, label: lab, text: dim_text(dm, if diameter { "⌀" } else { "R" }, ""), lines: vec![], arrows: vec![], arc: None };
            if diameter {
                g.arrows.push((rim, cen - dir * r, true));
            } else {
                g.arrows.push((rim, cen, false));
            }
            if (lab - cen).length() > r {
                g.lines.push((rim, lab));
            }
            g
        }
        Constraint::Angle(e1, e2, dm) => {
            let (a, b) = line_pts(sk, *e1)?;
            let (c, dd) = line_pts(sk, *e2)?;
            let (u, v) = ((b - a).normalize_or(DVec2::X), (dd - c).normalize_or(DVec2::Y));
            let den = u.perp_dot(v);
            let vertex = if den.abs() < 1e-12 { (a + c) * 0.5 } else { a + u * ((c - a).perp_dot(v) / den) };
            // Point each line away from the vertex (towards its far end).
            let away =
                |p: DVec2, q: DVec2, w: DVec2| if (p - vertex).length() + (q - vertex).length() > 0.0 && (p + q - vertex * 2.0).dot(w) < 0.0 { -w } else { w };
            let (u, v) = (away(a, b, u), away(c, dd, v));
            let bis = (u + v).normalize_or(u.perp());
            let r0 = ((a.distance(b).min(c.distance(dd))) * 0.4).max(gap * 2.0);
            let lab = vertex + bis * (r0 + gap * 0.5) + at(dm);
            let r = (lab - vertex).length().max(1e-9);
            let (mut a0, mut a1) = (u.y.atan2(u.x), v.y.atan2(v.x));
            if (a1 - a0).rem_euclid(std::f64::consts::TAU) > std::f64::consts::PI {
                std::mem::swap(&mut a0, &mut a1);
            }
            DimGeom { index, label: lab, text: dim_text(dm, "", "°"), lines: vec![], arrows: vec![], arc: Some((vertex, r, a0, a1)) }
        }
        _ => return None,
    })
}

/// Every dimension of the sketch.
pub fn all(sk: &Sketch, gap: f64) -> Vec<DimGeom> {
    sk.constraints.iter().enumerate().filter_map(|(i, c)| geometry(sk, i, c, gap)).collect()
}

/// The label's box on screen (centred on the label point), for a text of `chars` characters.
pub fn label_rect(center: egui::Pos2, chars: usize) -> egui::Rect {
    egui::Rect::from_center_size(center, egui::vec2(7.2 * chars as f32 + 8.0, 18.0))
}

/// Draw one dimension.
pub fn draw(painter: &egui::Painter, g: &DimGeom, to_s: &dyn Fn(DVec2) -> egui::Pos2, col: egui::Color32, bg: egui::Color32, highlight: bool) {
    let st = egui::Stroke::new(if highlight { 1.6 } else { 1.0 }, col);
    for (a, b) in &g.lines {
        painter.line_segment([to_s(*a), to_s(*b)], egui::Stroke::new(0.8, col.gamma_multiply(0.8)));
    }
    let arrow_head = |tip: egui::Pos2, from: egui::Pos2| {
        let d = (tip - from).normalized();
        if !d.x.is_finite() {
            return;
        }
        let n = egui::vec2(-d.y, d.x);
        painter.add(egui::Shape::convex_polygon(vec![tip, tip - d * 9.0 + n * 3.5, tip - d * 9.0 - n * 3.5], col, egui::Stroke::NONE));
    };
    for (a, b, both) in &g.arrows {
        let (sa, sb) = (to_s(*a), to_s(*b));
        painter.line_segment([sa, sb], st);
        arrow_head(sa, sb);
        if *both {
            arrow_head(sb, sa);
        }
    }
    if let Some((c, r, a0, a1)) = g.arc {
        let span = (a1 - a0).rem_euclid(std::f64::consts::TAU);
        let n = 40;
        let pts: Vec<egui::Pos2> = (0..=n).map(|k| to_s(c + DVec2::from_angle(a0 + span * k as f64 / n as f64) * r)).collect();
        painter.add(egui::Shape::line(pts.clone(), st));
        if pts.len() > 2 {
            arrow_head(pts[0], pts[1]);
            arrow_head(pts[n], pts[n - 1]);
        }
    }
    let p = to_s(g.label);
    let rect = label_rect(p, g.text.chars().count());
    painter.rect_filled(rect, 3.0, bg.gamma_multiply(0.85));
    if highlight {
        painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Outside);
    }
    painter.text(p, egui::Align2::CENTER_CENTER, &g.text, egui::FontId::proportional(12.5), col);
}

/// The dimension whose label is under the cursor.
pub fn under(dims: &[DimGeom], to_s: &dyn Fn(DVec2) -> egui::Pos2, cur: egui::Pos2) -> Option<usize> {
    dims.iter().rev().find(|g| label_rect(to_s(g.label), g.text.chars().count()).contains(cur)).map(|g| g.index)
}

/// Move a dimension's label by `delta` (sketch units).
pub fn move_label(sk: &mut Sketch, index: usize, delta: DVec2) {
    if let Some(d) = sk.constraints.get_mut(index).and_then(|c| c.dim_mut()) {
        let at = d.at.map(DVec2::from_array).unwrap_or(DVec2::ZERO) + delta;
        d.at = Some(at.to_array());
    }
}

/// Set a dimension from typed text: a number, or an expression of the part's dimensions. The
/// label keeps its place.
pub fn set_from_text(sk: &mut Sketch, index: usize, text: &str) -> bool {
    let Some(d) = sk.constraints.get_mut(index).and_then(|c| c.dim_mut()) else { return false };
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let at = d.at;
    *d = match t.parse::<f64>() {
        Ok(x) => Dim::new(x),
        Err(_) => Dim::expr(t),
    };
    d.at = at;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_move_and_dimensions_take_typed_values() {
        let mut sk = Sketch::default();
        let a = sk.add_point(0.0, 0.0);
        let b = sk.add_point(10.0, 0.0);
        let l = sk.add_line(a, b);
        let c = sk.add_point(0.0, 6.0);
        sk.constrain(Constraint::Length(l, Dim::new(10.0)));
        sk.constrain(Constraint::PointLineDistance(c, l, Dim::new(6.0)));
        let g = all(&sk, 2.0);
        assert_eq!(g.len(), 2);
        // The length's label sits 2 above the middle; the dimension line runs at that height.
        assert_eq!(g[0].label, DVec2::new(5.0, 2.0));
        assert_eq!(g[0].arrows[0].0, DVec2::new(0.0, 2.0));
        move_label(&mut sk, 0, DVec2::new(0.0, 3.0));
        assert_eq!(all(&sk, 2.0)[0].label, DVec2::new(5.0, 5.0));
        // Typed values: a number, then an expression; the label keeps its place.
        assert!(set_from_text(&mut sk, 0, "12.5"));
        assert_eq!(sk.constraints[0].dim_mut().unwrap().value, 12.5);
        assert!(set_from_text(&mut sk, 0, "width / 2"));
        let d = sk.constraints[0].dim_mut().unwrap().clone();
        assert_eq!((d.expr.as_deref(), d.at), (Some("width / 2"), Some([0.0, 3.0])));
        // The point-to-line distance is measured from the foot (0, 0) to the point (0, 6).
        assert_eq!(g[1].arrows[0].0.x, g[1].arrows[0].1.x);
        // The solver holds the point 6 from the line.
        sk.constraints[0] = Constraint::Length(l, Dim::new(10.0));
        sk.points[c].y = 3.0;
        sk.solve();
        let (pa, pb, pc) = (sk.point(a), sk.point(b), sk.point(c));
        let dist = (pb - pa).normalize().perp_dot(pc - pa).abs();
        assert!((dist - 6.0).abs() < 1e-6, "{dist}");
    }
}
