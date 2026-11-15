//! Planes shown in the 3D view while one is being chosen or made: a filled, see-through
//! rectangle in the highlight color of the menus, sized to the part (or to the face it lies
//! on), light under the pointer and strong once chosen. The buttons and lists that take a plane
//! ask for one with [`hover`]; the New reference plane form and the face pickers draw theirs.

use crate::app::State;
use aurafx_cad::project::SurfaceTriangles;
use aurafx_cad::{PlaneFrame, SketchPlane};
use glam::{DVec2, DVec3};

/// A plane to show: where it is, how much of it (in its own x and y, from its origin), and a
/// few words to write at its corner.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanePreview {
    pub frame: PlaneFrame,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    /// Chosen (or about to be made) rather than under the pointer.
    pub strong: bool,
    pub label: String,
}

/// The part of a plane that covers a box (the part's bounds), with a margin: the box's corners
/// laid on the plane. Without a box, a square of 50 mm about the plane's origin.
pub fn extent_over(frame: &PlaneFrame, bounds: Option<([f64; 3], [f64; 3])>) -> ([f64; 2], [f64; 2]) {
    let Some((b0, b1)) = bounds.filter(|(a, b)| (0..3).all(|k| a[k].is_finite() && b[k].is_finite() && b[k] >= a[k])) else {
        return ([-25.0, -25.0], [25.0, 25.0]);
    };
    let r = frame.basis();
    let o = DVec3::from_array(frame.origin);
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for k in 0..8 {
        let c = DVec3::new(if k & 1 == 0 { b0[0] } else { b1[0] }, if k & 2 == 0 { b0[1] } else { b1[1] }, if k & 4 == 0 { b0[2] } else { b1[2] }) - o;
        let uv = DVec2::new(c.dot(r.x_axis), c.dot(r.y_axis));
        lo = lo.min(uv);
        hi = hi.max(uv);
    }
    let size = (0..3).map(|k| b1[k] - b0[k]).fold(0.0f64, f64::max);
    let margin = (0.15 * size).max(2.0);
    // A plane the box is seen end on from (a side of it) still shows as a strip of some width.
    let grow = |lo: f64, hi: f64| if hi - lo < 0.2 * size { ((lo + hi) / 2.0 - 0.1 * size, (lo + hi) / 2.0 + 0.1 * size) } else { (lo, hi) };
    let ((x0, x1), (y0, y1)) = (grow(lo.x, hi.x), grow(lo.y, hi.y));
    ([x0 - margin, y0 - margin], [x1 + margin, y1 + margin])
}

/// The part of a plane a face of the part covers, with a small margin (`None`: no such face).
pub fn extent_of_face(surface: &SurfaceTriangles, face: &str, frame: &PlaneFrame) -> Option<([f64; 2], [f64; 2])> {
    let id = surface.names.iter().position(|n| n == face)? as u32;
    let r = frame.basis();
    let o = DVec3::from_array(frame.origin);
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for (t, _) in surface.tris.iter().zip(&surface.face).filter(|(_, f)| **f == id) {
        for i in t {
            let c = DVec3::from_array(surface.points[*i as usize]) - o;
            let uv = DVec2::new(c.dot(r.x_axis), c.dot(r.y_axis));
            lo = lo.min(uv);
            hi = hi.max(uv);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let margin = (0.12 * (hi - lo).max_element()).max(1.0);
    Some(((lo - margin).to_array(), (hi + margin).to_array()))
}

/// The bounds of the part being edited, as last built.
pub fn part_bounds(state: &State) -> Option<([f64; 3], [f64; 3])> {
    let s = state.session.lock();
    let e = s.cad_edit.as_ref()?;
    if e.active().features.is_empty() && e.active().components.is_empty() {
        return None;
    }
    e.summary.as_ref().map(|s| s.bounds)
}

/// The triangles of the part being edited, as shown (read once for every build of the part).
pub(super) fn part_surface(state: &State) -> Option<std::sync::Arc<SurfaceTriangles>> {
    let ds = {
        let s = state.session.lock();
        let node = s.cad_edit.as_ref()?.node;
        s.nodes.get(&node)?.output.clone()?
    };
    let mut kept = state.cad.surface.lock();
    if let Some((k, s)) = kept.as_ref() {
        if std::sync::Arc::ptr_eq(k, &ds) {
            return Some(s.clone());
        }
    }
    let made = std::sync::Arc::new(aurafx_cad::project::surface_triangles(&ds)?);
    *kept = Some((ds, made.clone()));
    Some(made)
}

/// A plane sized to the part.
pub fn over_part(state: &State, frame: PlaneFrame, label: impl Into<String>, strong: bool) -> PlanePreview {
    let (lo, hi) = extent_over(&frame, part_bounds(state));
    PlanePreview { frame, lo, hi, strong, label: label.into() }
}

/// A principal plane (moved along its normal), sized to the part.
pub fn principal(state: &State, plane: SketchPlane, offset: f64, label: impl Into<String>) -> PlanePreview {
    over_part(state, PlaneFrame::from_principal(plane, offset, 0.0, true), label, false)
}

/// The plane of a flat face of the part, as large as the face.
pub fn of_face(state: &State, name: &str, point: [f64; 3], direction: [f64; 3], strong: bool) -> Option<PlanePreview> {
    let frame = PlaneFrame::new(point, direction, None)?;
    let (lo, hi) = part_surface(state).and_then(|s| extent_of_face(&s, name, &frame)).unwrap_or_else(|| extent_over(&frame, part_bounds(state)));
    Some(PlanePreview { frame, lo, hi, strong, label: name.to_string() })
}

/// Show this plane in the 3D views for this frame: call it while the pointer is on the button
/// or the row that would take the plane.
pub fn hover(state: &mut State, ctx: &egui::Context, p: PlanePreview) {
    state.cad.plane_hover = Some((ctx.cumulative_frame_nr(), p));
    ctx.request_repaint();
}

/// [`hover`] when the pointer is on the widget.
pub fn hover_if(state: &mut State, r: &egui::Response, p: impl FnOnce(&State) -> Option<PlanePreview>) {
    if r.hovered() {
        if let Some(p) = p(state) {
            hover(state, &r.ctx, p);
        }
    }
}

/// Draw one plane over a 3D view.
pub fn draw_plane(painter: &egui::Painter, cam: &aurafx_render::Camera, rect: egui::Rect, p: &PlanePreview) {
    let r = p.frame.basis();
    let o = DVec3::from_array(p.frame.origin);
    let at = |u: f64, v: f64| crate::pick::project(cam, rect, o + r.x_axis * u + r.y_axis * v);
    let corners = [at(p.lo[0], p.lo[1]), at(p.hi[0], p.lo[1]), at(p.hi[0], p.hi[1]), at(p.lo[0], p.hi[1])];
    let accent = crate::theme::ACCENT;
    let fill = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), if p.strong { 115 } else { 60 });
    let line = egui::Stroke::new(if p.strong { 2.5 } else { 1.5 }, if p.strong { accent } else { accent.gamma_multiply(0.8) });
    if corners.iter().all(|c| c.is_some()) {
        let pts: Vec<egui::Pos2> = corners.iter().flatten().copied().collect();
        painter.add(egui::Shape::convex_polygon(pts, fill, line));
    } else {
        // Partly behind the eye: the sides that can be drawn.
        for k in 0..4 {
            if let (Some(a), Some(b)) = (corners[k], corners[(k + 1) % 4]) {
                painter.line_segment([a, b], line);
            }
        }
    }
    // A few lines across it, so its slant reads, and its normal from the middle.
    let faint = egui::Stroke::new(1.0, accent.gamma_multiply(if p.strong { 0.55 } else { 0.35 }));
    for k in 1..4 {
        let t = k as f64 / 4.0;
        let (u, v) = (p.lo[0] + (p.hi[0] - p.lo[0]) * t, p.lo[1] + (p.hi[1] - p.lo[1]) * t);
        if let (Some(a), Some(b)) = (at(u, p.lo[1]), at(u, p.hi[1])) {
            painter.line_segment([a, b], faint);
        }
        if let (Some(a), Some(b)) = (at(p.lo[0], v), at(p.hi[0], v)) {
            painter.line_segment([a, b], faint);
        }
    }
    let mid = ((p.lo[0] + p.hi[0]) / 2.0, (p.lo[1] + p.hi[1]) / 2.0);
    let len = 0.18 * (p.hi[0] - p.lo[0]).max(p.hi[1] - p.lo[1]);
    let c3 = o + r.x_axis * mid.0 + r.y_axis * mid.1;
    if let (Some(a), Some(b)) = (crate::pick::project(cam, rect, c3), crate::pick::project(cam, rect, c3 + r.z_axis * len)) {
        if (b - a).length() > 6.0 {
            painter.arrow(a, b - a, egui::Stroke::new(if p.strong { 2.0 } else { 1.5 }, accent));
        }
        painter.circle_filled(a, 2.5, accent);
    }
    if !p.label.is_empty() {
        if let Some(c) = corners[3] {
            let font = egui::FontId::proportional(12.5);
            let galley = painter.layout_no_wrap(p.label.clone(), font, egui::Color32::WHITE);
            let pos = c + egui::vec2(4.0, -galley.size().y - 5.0);
            painter.rect_filled(egui::Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(4.0, 1.5)), 3.0, accent);
            painter.galley(pos, galley, egui::Color32::WHITE);
        }
    }
}

/// The planes to show over a 3D view now: the one a button or a row under the pointer would
/// take, and the one the New reference plane form would make with the plane it starts from.
pub fn draw_overlay(painter: &egui::Painter, cam: &aurafx_render::Camera, rect: egui::Rect, state: &State) {
    if !super::editing(state) {
        return;
    }
    let now = painter.ctx().cumulative_frame_nr();
    if let Some((frame, p)) = &state.cad.plane_hover {
        // Asked for in this frame or the one before (the panels may be drawn after the view).
        if now.saturating_sub(*frame) <= 1 {
            draw_plane(painter, cam, rect, p);
        }
    }
    if let Some(p) = &state.cad.plane_demo {
        draw_plane(painter, cam, rect, p);
    }
    for p in super::planes::form_previews(state) {
        draw_plane(painter, cam, rect, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_is_shown_as_large_as_the_part_or_the_face_it_lies_on() {
        // A 40 x 30 x 8 plate centered on the origin, its top at z = 8.
        let bounds = Some(([-20.0, -15.0, 0.0], [20.0, 15.0, 8.0]));
        let top = PlaneFrame::from_principal(SketchPlane::XY, 0.0, 0.0, true);
        let (lo, hi) = extent_over(&top, bounds);
        assert!(lo[0] < -20.0 && hi[0] > 20.0 && lo[1] < -15.0 && hi[1] > 15.0, "{lo:?} {hi:?}");
        assert!(hi[0] < 30.0 && hi[1] < 25.0, "not much larger than the part: {hi:?}");
        // The front plane sees the plate from its side: 40 wide, and a strip rather than a line.
        let front = PlaneFrame::from_principal(SketchPlane::XZ, 0.0, 0.0, true);
        let (lo, hi) = extent_over(&front, bounds);
        assert!(lo[0] < -20.0 && hi[0] > 20.0 && hi[1] - lo[1] >= 8.0, "{lo:?} {hi:?}");
        // A part away from the origin: the plane is shown where the part is.
        let (lo, hi) = extent_over(&top, Some(([100.0, 100.0, 0.0], [110.0, 110.0, 5.0])));
        assert!(lo[0] > 90.0 && hi[0] < 120.0, "{lo:?} {hi:?}");
        // No part yet: a square about the origin.
        assert_eq!(extent_over(&top, None), ([-25.0, -25.0], [25.0, 25.0]));
        // A face: two triangles of the plate's top; the plane covers them and little more.
        let s = SurfaceTriangles {
            points: vec![[-20.0, -15.0, 8.0], [20.0, -15.0, 8.0], [20.0, 15.0, 8.0], [-20.0, 15.0, 8.0], [0.0, 0.0, 0.0]],
            tris: vec![[0, 1, 2], [0, 2, 3], [0, 1, 4]],
            face: vec![1, 1, 0],
            names: vec!["Plate side".into(), "Plate z+".into()],
        };
        let f = PlaneFrame::new([0.0, 0.0, 8.0], [0.0, 0.0, 1.0], None).unwrap();
        let (lo, hi) = extent_of_face(&s, "Plate z+", &f).unwrap();
        assert!(lo[0] < -20.0 && lo[0] > -26.0 && hi[1] > 15.0 && hi[1] < 21.0, "{lo:?} {hi:?}");
        assert!(extent_of_face(&s, "No such face", &f).is_none());
    }
}
