//! 3D widgets: handles in the view that edit the active filter's plane, sphere, box, line or
//! point, and the directions of custom lights.

use crate::app::State;
use crate::pick::{project, screen_ray};
use aurafx_filters::ParamValue;
use aurafx_pipeline::NodeId;
use aurafx_render::Camera;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use glam::DVec3;

const COLOR: Color32 = Color32::from_rgb(255, 170, 60);
/// How close (points) the pointer must be to grab a handle.
const GRAB: f32 = 14.0;

#[derive(Debug, Clone)]
pub enum Kind {
    /// Origin and unit normal (Slice, Clip).
    Plane {
        origin: DVec3,
        normal: DVec3,
        size: f64,
    },
    /// Center and radius, with the parameter names.
    Sphere {
        center: DVec3,
        radius: f64,
        center_key: &'static str,
        radius_key: &'static str,
    },
    /// Box corners; `source` boxes store center and size instead.
    Box {
        lo: DVec3,
        hi: DVec3,
        source: bool,
    },
    /// Two end points.
    Line {
        p1: DVec3,
        p2: DVec3,
    },
    Point {
        p: DVec3,
        key: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct Widget {
    pub node: NodeId,
    pub kind: Kind,
    /// Size of the data around (for handle lengths).
    pub scale: f64,
}

fn or(v: [f64; 3], d: [f64; 3]) -> DVec3 {
    DVec3::from_array(if v.iter().any(|x| x.is_nan()) { d } else { v })
}

/// The widget of the active node, if its filter has one.
pub fn active(state: &State) -> Option<Widget> {
    let s = state.session.lock();
    let n = s.active_node()?;
    if !n.visible {
        return None;
    }
    let def = n.def()?;
    let data = match n.input.and_then(|i| s.nodes.get(&i)) {
        Some(input) => input.output.clone(),
        None => n.output.clone(),
    }?;
    let b = data.bounds();
    if b.is_empty() {
        return None;
    }
    let p = &n.params;
    let (lo, hi, c) = (b.min, b.max, b.center());
    let at = |f: f64| [lo[0] + f * (hi[0] - lo[0]), lo[1] + f * (hi[1] - lo[1]), lo[2] + f * (hi[2] - lo[2])];
    let diag = b.diagonal().max(1e-12);
    let kind = match (def.id, p.str("type").as_str(), p.str("seed_type").as_str()) {
        ("slice" | "clip", "Plane", _) => {
            let nrm = DVec3::from_array(p.vec3("normal", [1.0, 0.0, 0.0])).normalize_or_zero();
            Kind::Plane { origin: or(p.vec3("origin", [f64::NAN; 3]), c), normal: if nrm == DVec3::ZERO { DVec3::X } else { nrm }, size: 0.35 * diag }
        }
        ("slice" | "clip", "Sphere", _) => {
            Kind::Sphere { center: or(p.vec3("origin", [f64::NAN; 3]), c), radius: p.f64("radius", 1.0), center_key: "origin", radius_key: "radius" }
        }
        ("clip", "Box", _) => {
            let s = b.size();
            let dlo = [c[0] - 0.25 * s[0], c[1] - 0.25 * s[1], c[2] - 0.25 * s[2]];
            let dhi = [c[0] + 0.25 * s[0], c[1] + 0.25 * s[1], c[2] + 0.25 * s[2]];
            let (l, h) = (p.vec3("box_min", [f64::NAN; 3]), p.vec3("box_max", [f64::NAN; 3]));
            let unset = l.iter().chain(&h).any(|x| x.is_nan());
            Kind::Box { lo: DVec3::from_array(if unset { dlo } else { l }), hi: DVec3::from_array(if unset { dhi } else { h }), source: false }
        }
        ("stream_tracer", _, "Point Cloud") => {
            let r = p.f64("radius", f64::NAN);
            Kind::Sphere {
                center: or(p.vec3("center", [f64::NAN; 3]), c),
                radius: if r.is_nan() { 0.1 * diag } else { r },
                center_key: "center",
                radius_key: "radius",
            }
        }
        ("stream_tracer", _, "Line") | ("plot_over_line", _, _) => {
            Kind::Line { p1: or(p.vec3("point1", [f64::NAN; 3]), lo), p2: or(p.vec3("point2", [f64::NAN; 3]), hi) }
        }
        ("particle_tracer" | "particle_path" | "streak_line", _, _) if n.extra_inputs.is_empty() => {
            Kind::Line { p1: or(p.vec3("point1", [f64::NAN; 3]), at(0.25)), p2: or(p.vec3("point2", [f64::NAN; 3]), at(0.75)) }
        }
        ("probe_location", _, _) => Kind::Point { p: or(p.vec3("location", [f64::NAN; 3]), c), key: "location" },
        ("sphere", _, _) => {
            Kind::Sphere { center: DVec3::from_array(p.vec3("center", [0.0; 3])), radius: p.f64("radius", 0.5), center_key: "center", radius_key: "radius" }
        }
        ("line", _, _) => Kind::Line { p1: DVec3::from_array(p.vec3("point1", [-0.5, 0.0, 0.0])), p2: DVec3::from_array(p.vec3("point2", [0.5, 0.0, 0.0])) },
        ("box", _, _) => {
            let (ctr, size) = (DVec3::from_array(p.vec3("center", [0.0; 3])), DVec3::from_array(p.vec3("size", [1.0; 3])));
            Kind::Box { lo: ctr - size * 0.5, hi: ctr + size * 0.5, source: true }
        }
        _ => return None,
    };
    Some(Widget { node: n.id, kind, scale: diag })
}

/// Handle positions; the index identifies the handle while dragging.
pub fn handles(w: &Widget, cam: &Camera) -> Vec<DVec3> {
    match &w.kind {
        Kind::Plane { origin, normal, size } => vec![*origin, *origin + *normal * *size * 0.8],
        Kind::Sphere { center, radius, .. } => vec![*center, *center + cam.right() * *radius],
        Kind::Box { lo, hi, .. } => {
            let c = (*lo + *hi) * 0.5;
            let mut v = vec![c];
            for axis in 0..3 {
                for side in [*lo, *hi] {
                    let mut p = c;
                    p[axis] = side[axis];
                    v.push(p);
                }
            }
            v
        }
        Kind::Line { p1, p2 } => vec![*p1, *p2, (*p1 + *p2) * 0.5],
        Kind::Point { p, .. } => vec![*p],
    }
}

/// The handle under the pointer.
pub fn grab(w: &Widget, cam: &Camera, rect: Rect, pos: Pos2) -> Option<usize> {
    handles(w, cam)
        .iter()
        .enumerate()
        .filter_map(|(i, h)| project(cam, rect, *h).map(|s| (i, s.distance(pos))))
        .filter(|(_, d)| *d < GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Where the pointer is on the plane through `at` facing the camera.
fn on_screen_plane(cam: &Camera, rect: Rect, pos: Pos2, at: DVec3) -> Option<DVec3> {
    let ray = screen_ray(cam, rect, pos);
    let f = cam.forward();
    let denom = ray.dir.dot(f);
    (denom.abs() > 1e-12).then(|| ray.origin + ray.dir * ((at - ray.origin).dot(f) / denom))
}

/// Movement along `axis` for a pointer movement `drag` (screen points).
fn along(cam: &Camera, rect: Rect, at: DVec3, axis: DVec3, len: f64, drag: Vec2) -> f64 {
    match (project(cam, rect, at), project(cam, rect, at + axis * len)) {
        (Some(a), Some(b)) => {
            let s = b - a;
            let l2 = s.length_sq();
            if l2 > 1.0 {
                (drag.dot(s) / l2) as f64 * len
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

/// Apply a drag of handle `h` to the widget's node.
pub fn drag(state: &State, w: &Widget, h: usize, cam: &Camera, rect: Rect, pos: Pos2, delta: Vec2) {
    let set = drag_params(w, h, cam, rect, pos, delta);
    let mut s = state.session.lock();
    for (k, v) in set {
        s.set_param(w.node, k, v);
    }
}

/// The parameter changes for dragging handle `h` to `pos` (pointer moved by `delta`).
pub fn drag_params(w: &Widget, h: usize, cam: &Camera, rect: Rect, pos: Pos2, delta: Vec2) -> Vec<(&'static str, ParamValue)> {
    let hs = handles(w, cam);
    let Some(&at) = hs.get(h) else { return Vec::new() };
    let v3 = |x: DVec3| ParamValue::Vec3(x.to_array());
    let mut set: Vec<(&'static str, ParamValue)> = Vec::new();
    match &w.kind {
        Kind::Plane { origin, normal, size } => {
            if h == 1 {
                if let Some(tip) = on_screen_plane(cam, rect, pos, at) {
                    let nn = (tip - *origin).normalize_or_zero();
                    if nn != DVec3::ZERO {
                        set.push(("normal", v3(nn)));
                    }
                }
            } else {
                let shift = along(cam, rect, *origin, *normal, *size, delta);
                set.push(("origin", v3(*origin + *normal * shift)));
            }
        }
        Kind::Sphere { center, center_key, radius_key, .. } => {
            let Some(p) = on_screen_plane(cam, rect, pos, at) else { return set };
            if h == 0 {
                set.push((center_key, v3(p)));
            } else {
                set.push((radius_key, ParamValue::Float((p - *center).length().max(1e-9 * w.scale))));
            }
        }
        Kind::Box { lo, hi, source } => {
            let (mut lo, mut hi) = (*lo, *hi);
            if h == 0 {
                let Some(p) = on_screen_plane(cam, rect, pos, at) else { return set };
                let d = p - at;
                lo += d;
                hi += d;
            } else {
                let axis = (h - 1) / 2;
                let mut dir = DVec3::ZERO;
                dir[axis] = 1.0;
                let shift = along(cam, rect, at, dir, 0.25 * w.scale, delta);
                let min_size = 1e-6 * w.scale;
                if (h - 1) % 2 == 0 {
                    lo[axis] = (lo[axis] + shift).min(hi[axis] - min_size);
                } else {
                    hi[axis] = (hi[axis] + shift).max(lo[axis] + min_size);
                }
            }
            if *source {
                set.push(("center", v3((lo + hi) * 0.5)));
                set.push(("size", v3(hi - lo)));
            } else {
                set.push(("box_min", v3(lo)));
                set.push(("box_max", v3(hi)));
            }
        }
        Kind::Line { p1, p2 } => {
            let Some(p) = on_screen_plane(cam, rect, pos, at) else { return set };
            match h {
                0 => set.push(("point1", v3(p))),
                1 => set.push(("point2", v3(p))),
                _ => {
                    let d = p - at;
                    set.push(("point1", v3(*p1 + d)));
                    set.push(("point2", v3(*p2 + d)));
                }
            }
        }
        Kind::Point { key, .. } => {
            if let Some(p) = on_screen_plane(cam, rect, pos, at) {
                set.push((key, v3(p)));
            }
        }
    }
    set
}

fn handle(p: &egui::Painter, at: Pos2, active: bool, fill: Color32) {
    p.circle(at, if active { 9.0 } else { 7.0 }, fill, Stroke::new(1.5, Color32::WHITE));
}

fn polyline(p: &egui::Painter, cam: &Camera, rect: Rect, pts: &[DVec3], closed: bool, stroke: Stroke) {
    let s: Vec<Option<Pos2>> = pts.iter().map(|x| project(cam, rect, *x)).collect();
    let n = s.len();
    for i in 0..if closed { n } else { n.saturating_sub(1) } {
        if let (Some(a), Some(b)) = (s[i], s[(i + 1) % n]) {
            p.line_segment([a, b], stroke);
        }
    }
}

pub fn draw(p: &egui::Painter, cam: &Camera, rect: Rect, w: &Widget, active: Option<usize>) {
    let stroke = Stroke::new(1.5, COLOR);
    let hs = handles(w, cam);
    let screen: Vec<Option<Pos2>> = hs.iter().map(|h| project(cam, rect, *h)).collect();
    match &w.kind {
        Kind::Plane { origin, normal, size } => {
            let up = if normal.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
            let u = normal.cross(up).normalize();
            let v = normal.cross(u);
            let corners: Vec<DVec3> =
                [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(a, b)| *origin + u * (a * size) + v * (b * size)).collect();
            let pts: Vec<Pos2> = corners.iter().filter_map(|c| project(cam, rect, *c)).collect();
            if pts.len() == 4 {
                p.add(egui::Shape::convex_polygon(pts, COLOR.linear_multiply(0.08), stroke));
            }
            if let (Some(a), Some(b)) = (screen[0], screen[1]) {
                p.line_segment([a, b], Stroke::new(2.5, COLOR));
            }
        }
        Kind::Sphere { center, radius, .. } => {
            // Three great circles.
            for (a, b) in [(DVec3::X, DVec3::Y), (DVec3::Y, DVec3::Z), (DVec3::Z, DVec3::X)] {
                let ring: Vec<DVec3> = (0..48)
                    .map(|k| {
                        let t = k as f64 / 48.0 * std::f64::consts::TAU;
                        *center + (a * t.cos() + b * t.sin()) * *radius
                    })
                    .collect();
                polyline(p, cam, rect, &ring, true, stroke);
            }
            // The outline as seen from the camera.
            let (r, u) = (cam.right(), cam.right().cross(cam.forward()).normalize_or_zero());
            let ring: Vec<DVec3> = (0..64)
                .map(|k| {
                    let t = k as f64 / 64.0 * std::f64::consts::TAU;
                    *center + (r * t.cos() + u * t.sin()) * *radius
                })
                .collect();
            polyline(p, cam, rect, &ring, true, Stroke::new(2.0, COLOR));
            if let (Some(a), Some(b)) = (screen[0], screen[1]) {
                p.line_segment([a, b], Stroke::new(1.0, COLOR.linear_multiply(0.7)));
            }
        }
        Kind::Box { lo, hi, .. } => {
            let c = |i: usize| DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                polyline(p, cam, rect, &[c(a), c(b)], false, stroke);
            }
        }
        Kind::Line { p1, p2 } => polyline(p, cam, rect, &[*p1, *p2], false, Stroke::new(2.5, COLOR)),
        Kind::Point { p: at, .. } => {
            let s = 0.04 * w.scale;
            for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
                polyline(p, cam, rect, &[*at - axis * s, *at + axis * s], false, Stroke::new(2.0, COLOR));
            }
        }
    }
    for (i, s) in screen.iter().enumerate() {
        if let Some(s) = s {
            // Secondary handles (box faces, line middle, plane origin) are dimmer.
            let main = match &w.kind {
                Kind::Plane { .. } => i == 1,
                Kind::Box { .. } => i > 0,
                Kind::Line { .. } => i < 2,
                _ => true,
            };
            handle(p, *s, active == Some(i), if main { COLOR } else { COLOR.linear_multiply(0.6) });
        }
    }
}

// ---- light widgets ----

/// World direction of a light (camera-relative lights follow the view).
pub fn light_world(l: &aurafx_pipeline::Light, cam: &Camera) -> DVec3 {
    let d = DVec3::new(l.direction[0] as f64, l.direction[1] as f64, l.direction[2] as f64);
    let (r, f) = (cam.right(), (cam.eye - cam.target).normalize_or_zero());
    let u = r.cross(-f).normalize_or_zero();
    let w = if l.camera_relative { r * d.x + u * d.y + f * d.z } else { d };
    w.normalize_or_zero()
}

fn light_from_world(w: DVec3, camera_relative: bool, cam: &Camera) -> [f32; 3] {
    let (r, f) = (cam.right(), (cam.eye - cam.target).normalize_or_zero());
    let u = r.cross(-f).normalize_or_zero();
    let d = if camera_relative { DVec3::new(w.dot(r), w.dot(u), w.dot(f)) } else { w };
    [d.x as f32, d.y as f32, d.z as f32]
}

/// Handles of the custom lights around the visible objects: (center, radius, handle per light).
pub fn light_handles(lights: &[aurafx_pipeline::Light], cam: &Camera, bounds: &aurafx_data::Bounds) -> (DVec3, f64, Vec<DVec3>) {
    let c = DVec3::from_array(bounds.center());
    let r = 0.6 * bounds.diagonal().max(1e-12);
    (c, r, lights.iter().map(|l| c + light_world(l, cam) * r).collect())
}

pub fn draw_lights(p: &egui::Painter, cam: &Camera, rect: Rect, lights: &[aurafx_pipeline::Light], bounds: &aurafx_data::Bounds, active: Option<usize>) {
    let (c, _, hs) = light_handles(lights, cam, bounds);
    let Some(sc) = project(cam, rect, c) else { return };
    for (i, (h, l)) in hs.iter().zip(lights).enumerate() {
        let Some(sh) = project(cam, rect, *h) else { continue };
        let col = Color32::from_rgb(
            (l.color[0].clamp(0.0, 1.0) * 255.0) as u8,
            (l.color[1].clamp(0.0, 1.0) * 255.0) as u8,
            (l.color[2].clamp(0.0, 1.0) * 255.0) as u8,
        );
        p.line_segment([sc, sh], Stroke::new(1.0, Color32::from_rgb(255, 210, 80).linear_multiply(0.7)));
        // A small sun: rays around a disc.
        for k in 0..8 {
            let a = k as f32 / 8.0 * std::f32::consts::TAU;
            let d = Vec2::new(a.cos(), a.sin());
            p.line_segment([sh + d * 10.0, sh + d * 14.0], Stroke::new(1.5, Color32::from_rgb(255, 210, 80)));
        }
        handle(p, sh, active == Some(i), col);
        p.text(sh + Vec2::new(12.0, -12.0), egui::Align2::LEFT_BOTTOM, format!("{}", i + 1), egui::FontId::proportional(11.0), Color32::from_rgb(255, 210, 80));
    }
}

pub fn grab_light(lights: &[aurafx_pipeline::Light], cam: &Camera, rect: Rect, bounds: &aurafx_data::Bounds, pos: Pos2) -> Option<usize> {
    let (_, _, hs) = light_handles(lights, cam, bounds);
    hs.iter()
        .enumerate()
        .filter_map(|(i, h)| project(cam, rect, *h).map(|s| (i, s.distance(pos))))
        .filter(|(_, d)| *d < GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Point light `k` at the pointer: the direction from the center to where the pointer meets the
/// sphere of handles (or its nearest point to the pointer ray).
pub fn drag_light(lights: &mut [aurafx_pipeline::Light], k: usize, cam: &Camera, rect: Rect, bounds: &aurafx_data::Bounds, pos: Pos2) {
    let (c, r, _) = light_handles(lights, cam, bounds);
    let ray = screen_ray(cam, rect, pos);
    let oc = ray.origin - c;
    let b = oc.dot(ray.dir);
    let disc = b * b - (oc.length_squared() - r * r);
    let hit = if disc >= 0.0 { ray.origin + ray.dir * (-b - disc.sqrt()) } else { ray.origin + ray.dir * (-b) };
    let w = (hit - c).normalize_or_zero();
    if w != DVec3::ZERO {
        if let Some(l) = lights.get_mut(k) {
            l.direction = light_from_world(w, l.camera_relative, cam);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> (Camera, Rect) {
        let b = aurafx_data::Bounds { min: [-1.0; 3], max: [1.0; 3] };
        let mut cam = Camera::default();
        cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &b);
        cam.update_clip(&b);
        (cam, Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)))
    }

    fn vec3(set: &[(&str, ParamValue)], k: &str) -> DVec3 {
        match set.iter().find(|x| x.0 == k).map(|x| &x.1) {
            Some(ParamValue::Vec3(v)) => DVec3::from_array(*v),
            other => panic!("{k}: {other:?}"),
        }
    }

    /// Drag handle `h` by `d` screen points.
    fn drag_by(w: &Widget, h: usize, cam: &Camera, rect: Rect, d: Vec2) -> Vec<(&'static str, ParamValue)> {
        let at = project(cam, rect, handles(w, cam)[h]).unwrap();
        drag_params(w, h, cam, rect, at + d, d)
    }

    #[test]
    fn handles_follow_the_pointer() {
        let (cam, rect) = view();
        let px = cam.world_per_pixel(rect.height() as f64);
        // Box: the +x face moves right by the pointer movement; the center moves the box.
        let b = Widget { node: 1, kind: Kind::Box { lo: DVec3::splat(-0.5), hi: DVec3::splat(0.5), source: false }, scale: 2.0 };
        assert_eq!(grab(&b, &cam, rect, project(&cam, rect, DVec3::new(0.5, 0.0, 0.0)).unwrap()), Some(2));
        let set = drag_by(&b, 2, &cam, rect, Vec2::new(40.0, 0.0));
        let hi = vec3(&set, "box_max");
        assert!((hi.x - (0.5 + 40.0 * px)).abs() < 0.02, "face follows: {hi}");
        assert_eq!(vec3(&set, "box_min"), DVec3::splat(-0.5));
        let set = drag_by(&b, 0, &cam, rect, Vec2::new(0.0, -30.0));
        assert!((vec3(&set, "box_min").y - (-0.5 + 30.0 * px)).abs() < 1e-6 && (vec3(&set, "box_max").y - (0.5 + 30.0 * px)).abs() < 1e-6, "box moves up");
        // A face can't pass the opposite one.
        let set = drag_by(&b, 2, &cam, rect, Vec2::new(-4000.0, 0.0));
        assert!(vec3(&set, "box_max").x > -0.5);
        // Sphere: the rim handle sets the radius to the pointer's distance from the center.
        let s = Widget { node: 1, kind: Kind::Sphere { center: DVec3::ZERO, radius: 0.5, center_key: "origin", radius_key: "radius" }, scale: 2.0 };
        let set = drag_by(&s, 1, &cam, rect, Vec2::new(50.0, 0.0));
        let ParamValue::Float(r) = set[0].1 else { panic!() };
        assert!((r - (0.5 + 50.0 * px)).abs() < 0.01, "radius {r}");
        // Line: an end point follows the pointer; the middle moves both.
        let l = Widget { node: 1, kind: Kind::Line { p1: DVec3::new(-0.5, 0.0, 0.0), p2: DVec3::new(0.5, 0.0, 0.0) }, scale: 2.0 };
        let set = drag_by(&l, 1, &cam, rect, Vec2::new(0.0, 60.0));
        assert!((vec3(&set, "point2") - DVec3::new(0.5, -60.0 * px, 0.0)).length() < 1e-6);
        let set = drag_by(&l, 2, &cam, rect, Vec2::new(20.0, 0.0));
        assert!((vec3(&set, "point1").x + 0.5 - 20.0 * px).abs() < 1e-6 && (vec3(&set, "point2").x - 0.5 - 20.0 * px).abs() < 1e-6);
        // Plane: the tip tilts the normal towards the pointer.
        let p = Widget { node: 1, kind: Kind::Plane { origin: DVec3::ZERO, normal: DVec3::X, size: 1.0 }, scale: 2.0 };
        let set = drag_by(&p, 1, &cam, rect, Vec2::new(-1.0, -400.0));
        let n = vec3(&set, "normal");
        assert!(n.y > 0.8 && (n.length() - 1.0).abs() < 1e-9, "normal {n}");
    }

    #[test]
    fn light_handles_aim_the_light() {
        let (cam, rect) = view();
        let b = aurafx_data::Bounds { min: [-1.0; 3], max: [1.0; 3] };
        for camera_relative in [true, false] {
            let mut lights = vec![aurafx_pipeline::Light { camera_relative, ..Default::default() }];
            // Point at a spot on the near side of the handle sphere: the light aims there.
            let want = DVec3::new(0.3, 0.8, 0.5).normalize();
            let (c, r, _) = light_handles(&lights, &cam, &b);
            let top = project(&cam, rect, c + want * r).unwrap();
            assert_eq!(grab_light(&lights, &cam, rect, &b, project(&cam, rect, light_handles(&lights, &cam, &b).2[0]).unwrap()), Some(0));
            drag_light(&mut lights, 0, &cam, rect, &b, top);
            let w = light_world(&lights[0], &cam);
            assert!((w - want).length() < 1e-3, "light aims at the pointer: {w} (camera_relative {camera_relative})");
        }
    }
}
