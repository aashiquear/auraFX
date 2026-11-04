//! Data axes grid: grid lines on the far faces of an object's bounding box, with tick labels.
//!
//! The lines are real 3D geometry (depth-tested, so the data hides them); the labels are
//! painted on top along the box edges that form the outline on screen.

use aurafx_data::Bounds;
use aurafx_render::tessellate::{MeshData, Vertex};
use aurafx_render::Camera;
use egui::{Color32, Pos2, Rect, Vec2};
use glam::DVec3;

/// About `target` "nice" tick values (1, 2, 5 × 10^k steps) inside [lo, hi].
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    let span = hi - lo;
    if !(span > 0.0) || !span.is_finite() {
        return vec![];
    }
    let raw = span / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    // Round to the step's decimal precision so labels read 0.03, not 0.030000000000000002.
    let scale = 10f64.powi((-step.log10().floor()).max(0.0) as i32);
    (first..=last).map(|i| ((i as f64 * step) * scale).round() / scale).map(|v| if v == 0.0 { 0.0 } else { v }).collect()
}

fn degenerate(b: &Bounds, a: usize) -> bool {
    let d = DVec3::from(b.max) - DVec3::from(b.min);
    d[a] <= d.max_element() * 1e-6
}

/// For each axis, whether the far face is the max side (seen from the camera).
pub fn back_sides(cam: &Camera, b: &Bounds) -> [bool; 3] {
    let c = (DVec3::from(b.min) + DVec3::from(b.max)) * 0.5;
    // An orthographic camera still has an eye position along its view direction.
    [0, 1, 2].map(|a| cam.eye[a] < c[a])
}

/// Grid line geometry for `b` with far faces chosen by `back`.
pub fn build_mesh(b: &Bounds, back: [bool; 3]) -> MeshData {
    let origin = b.min;
    let mut lines = Vec::new();
    let v = |p: [f64; 3]| Vertex {
        pos: [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32],
        scalar: 0.0,
        normal: [0.0, 0.0, 1.0],
        _pad: 0.0,
    };
    let ticks: Vec<Vec<f64>> = (0..3).map(|a| nice_ticks(b.min[a], b.max[a], 5)).collect();
    let flat: Vec<usize> = (0..3).filter(|&a| degenerate(b, a)).collect();
    let diag = (DVec3::from(b.max) - DVec3::from(b.min)).length();
    // 3-D data: the three far faces. Flat data: one face in its plane, nudged behind it. Lines: none.
    let faces: Vec<(usize, f64)> = match flat.len() {
        0 => (0..3).map(|a| (a, if back[a] { b.max[a] } else { b.min[a] })).collect(),
        1 => {
            let a = flat[0];
            vec![(a, if back[a] { b.max[a] + diag * 1e-3 } else { b.min[a] - diag * 1e-3 })]
        }
        _ => vec![],
    };
    for (a, fa) in faces {
        for (bb, cc) in [((a + 1) % 3, (a + 2) % 3), ((a + 2) % 3, (a + 1) % 3)] {
            // Lines of constant bb across the face (running along cc), including the borders.
            let mut vals = vec![b.min[bb], b.max[bb]];
            vals.extend(ticks[bb].iter().copied());
            for t in vals {
                let mut p0 = [0.0; 3];
                p0[a] = fa;
                p0[bb] = t;
                p0[cc] = b.min[cc];
                let mut p1 = p0;
                p1[cc] = b.max[cc];
                lines.push(v(p0));
                lines.push(v(p1));
            }
        }
    }
    MeshData { lines, bounds: *b, origin, ..Default::default() }
}

/// Paint tick labels and axis titles for `b`.
pub fn draw_labels(p: &egui::Painter, cam: &Camera, rect: Rect, b: &Bounds, names: [&str; 3], color: Color32) {
    let corners: Vec<DVec3> = (0..8)
        .map(|i| {
            DVec3::new(if i & 1 == 0 { b.min[0] } else { b.max[0] }, if i & 2 == 0 { b.min[1] } else { b.max[1] }, if i & 4 == 0 { b.min[2] } else { b.max[2] })
        })
        .collect();
    let proj: Vec<Pos2> = corners.iter().filter_map(|c| crate::pick::project(cam, rect, *c)).collect();
    if proj.len() != 8 {
        return;
    }
    let center = proj.iter().fold(Vec2::ZERO, |s, q| s + q.to_vec2()) / 8.0;
    let font = egui::FontId::proportional(12.0);
    for a in 0..3 {
        if degenerate(b, a) {
            continue;
        }
        let (bb, cc) = ((a + 1) % 3, (a + 2) % 3);
        // The edge along `a` whose screen midpoint is farthest from the box center lies on the outline.
        let mut best: Option<(f32, DVec3, DVec3)> = None;
        for (sb, sc) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut p0 = DVec3::ZERO;
            p0[bb] = if sb { b.max[bb] } else { b.min[bb] };
            p0[cc] = if sc { b.max[cc] } else { b.min[cc] };
            let mut p1 = p0;
            p0[a] = b.min[a];
            p1[a] = b.max[a];
            let (Some(s0), Some(s1)) = (crate::pick::project(cam, rect, p0), crate::pick::project(cam, rect, p1)) else { continue };
            // Edges seen end-on carry no readable labels.
            if (s1 - s0).length() < 30.0 {
                continue;
            }
            let mid = (s0.to_vec2() + s1.to_vec2()) * 0.5;
            let d = (mid - center).length();
            if best.map(|(bd, ..)| d > bd).unwrap_or(true) {
                best = Some((d, p0, p1));
            }
        }
        let Some((_, p0, p1)) = best else { continue };
        let (Some(s0), Some(s1)) = (crate::pick::project(cam, rect, p0), crate::pick::project(cam, rect, p1)) else { continue };
        let mid = (s0.to_vec2() + s1.to_vec2()) * 0.5;
        let out = (mid - center).normalized();
        let out = if out.is_finite() { out } else { Vec2::new(0.0, 1.0) };
        let anchor = egui::Align2([
            if out.x > 0.35 {
                egui::Align::Min
            } else if out.x < -0.35 {
                egui::Align::Max
            } else {
                egui::Align::Center
            },
            if out.y > 0.35 {
                egui::Align::Min
            } else if out.y < -0.35 {
                egui::Align::Max
            } else {
                egui::Align::Center
            },
        ]);
        let ticks = nice_ticks(b.min[a], b.max[a], 5);
        let px_per_tick = (s1 - s0).length() / ticks.len().max(1) as f32;
        let stride = if px_per_tick < 45.0 { 2 } else { 1 };
        for (i, t) in ticks.iter().enumerate() {
            if i % stride != 0 {
                continue;
            }
            let mut w = p0;
            w[a] = *t;
            if let Some(s) = crate::pick::project(cam, rect, w) {
                p.line_segment([s, s + out * 5.0], egui::Stroke::new(1.0, color));
                p.text(s + out * 8.0, anchor, aurafx_data::info::g(*t), font.clone(), color);
            }
        }
        p.text(Pos2::new(mid.x, mid.y) + out * 34.0, anchor, names[a], egui::FontId::proportional(14.0), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round() {
        assert_eq!(nice_ticks(0.0, 10.0, 5), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        assert_eq!(nice_ticks(-0.013, 0.051, 5), vec![0.0, 0.02, 0.04]);
        let t = nice_ticks(0.0, 0.1, 10);
        assert_eq!(t[3], 0.03, "no floating-point residue: {t:?}");
        assert_eq!(nice_ticks(-7.0, 7.0, 5), vec![-5.0, 0.0, 5.0]);
        assert!(nice_ticks(1.0, 1.0, 5).is_empty());
    }

    #[test]
    fn grid_skips_flat_axis() {
        let b = Bounds::new([0.0, 0.0, 0.0], [10.0, 5.0, 0.0]);
        let m = build_mesh(&b, [true, true, true]);
        // One face, in the data plane, just behind it as seen from a camera on the -z side.
        assert!(!m.lines.is_empty());
        assert!(m.lines.iter().all(|v| v.pos[2] > 0.0 && v.pos[2] < 0.05));
    }
}
