//! Picking and screen-space selection in the 3D view.

use aurafx_data::DataSet;
use aurafx_filters::Association;
use aurafx_pipeline::NodeId;
use aurafx_render::{Camera, MeshData};
use egui::{Pos2, Rect};
use glam::{DMat4, DVec3, DVec4};
use rayon::prelude::*;
use std::sync::Arc;

pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

fn view_proj(cam: &Camera, rect: Rect) -> DMat4 {
    let aspect = (rect.width() / rect.height().max(1.0)) as f64;
    cam.proj_matrix(aspect) * cam.view_matrix()
}

pub fn screen_ray(cam: &Camera, rect: Rect, pos: Pos2) -> Ray {
    let inv = view_proj(cam, rect).inverse();
    let x = ((pos.x - rect.left()) / rect.width()) as f64 * 2.0 - 1.0;
    let y = 1.0 - ((pos.y - rect.top()) / rect.height()) as f64 * 2.0;
    let near = inv * DVec4::new(x, y, 0.0, 1.0);
    let far = inv * DVec4::new(x, y, 1.0, 1.0);
    let n = near.truncate() / near.w;
    let f = far.truncate() / far.w;
    Ray { origin: n, dir: (f - n).normalize_or_zero() }
}

/// Project world points to screen positions (None when behind the camera).
pub fn project_many(cam: &Camera, rect: Rect, pts: &[[f64; 3]]) -> Vec<Option<Pos2>> {
    let m = view_proj(cam, rect);
    pts.par_iter()
        .map(|p| {
            let c = m * DVec4::new(p[0], p[1], p[2], 1.0);
            if c.w <= 0.0 {
                return None;
            }
            let ndc = c.truncate() / c.w;
            Some(Pos2::new(rect.left() + ((ndc.x + 1.0) * 0.5) as f32 * rect.width(), rect.top() + ((1.0 - ndc.y) * 0.5) as f32 * rect.height()))
        })
        .collect()
}

pub fn project(cam: &Camera, rect: Rect, p: DVec3) -> Option<Pos2> {
    project_many(cam, rect, &[p.to_array()]).pop().flatten()
}

pub struct Hit {
    pub node: NodeId,
    pub world: DVec3,
    pub t: f64,
}

/// Closest intersection of `ray` with the rendered triangles / point sprites.
pub fn raycast(ray: &Ray, meshes: &[(NodeId, Arc<MeshData>)], point_radius: f64) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for (id, m) in meshes {
        let o = ray.origin - DVec3::from_array(m.origin);
        let (ox, oy, oz) = (o.x as f32, o.y as f32, o.z as f32);
        let (dx, dy, dz) = (ray.dir.x as f32, ray.dir.y as f32, ray.dir.z as f32);
        let t_tri = m
            .tris
            .par_chunks_exact(3)
            .filter_map(|t| {
                // Möller–Trumbore.
                let (a, b, c) = (t[0].pos, t[1].pos, t[2].pos);
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let p = [dy * e2[2] - dz * e2[1], dz * e2[0] - dx * e2[2], dx * e2[1] - dy * e2[0]];
                let det = e1[0] * p[0] + e1[1] * p[1] + e1[2] * p[2];
                if det.abs() < 1e-12 {
                    return None;
                }
                let inv = 1.0 / det;
                let s = [ox - a[0], oy - a[1], oz - a[2]];
                let u = (s[0] * p[0] + s[1] * p[1] + s[2] * p[2]) * inv;
                if !(0.0..=1.0).contains(&u) {
                    return None;
                }
                let q = [s[1] * e1[2] - s[2] * e1[1], s[2] * e1[0] - s[0] * e1[2], s[0] * e1[1] - s[1] * e1[0]];
                let v = (dx * q[0] + dy * q[1] + dz * q[2]) * inv;
                if v < 0.0 || u + v > 1.0 {
                    return None;
                }
                let t = (e2[0] * q[0] + e2[1] * q[1] + e2[2] * q[2]) * inv;
                (t > 0.0).then_some(t as f64)
            })
            .reduce_with(f64::min);
        let r2 = (point_radius * point_radius) as f32;
        let t_pt = m
            .points
            .par_iter()
            .filter_map(|v| {
                let w = [v.pos[0] - ox, v.pos[1] - oy, v.pos[2] - oz];
                let t = w[0] * dx + w[1] * dy + w[2] * dz;
                if t <= 0.0 {
                    return None;
                }
                let c = [w[0] - t * dx, w[1] - t * dy, w[2] - t * dz];
                ((c[0] * c[0] + c[1] * c[1] + c[2] * c[2]) <= r2).then_some(t as f64)
            })
            .reduce_with(f64::min);
        let t = match (t_tri, t_pt) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(t) = t {
            if best.as_ref().map(|b| t < b.t).unwrap_or(true) {
                best = Some(Hit { node: *id, world: ray.origin + ray.dir * t, t });
            }
        }
    }
    best
}

/// Values of every array at `p`: interpolated inside cells, else at the nearest point.
pub fn values_at(ds: &DataSet, p: DVec3) -> Vec<(String, String)> {
    let fmt = |a: &aurafx_data::Array, i: usize| -> String {
        if a.ncomp == 1 {
            aurafx_data::info::g(a.get(i, 0))
        } else {
            format!("({})", (0..a.ncomp.min(9)).map(|c| aurafx_data::info::g(a.get(i, c))).collect::<Vec<_>>().join(", "))
        }
    };
    if let Ok(attrs) = aurafx_filters::probe::probe_points(ds, &[p.to_array()]) {
        if attrs.get("vtkValidPointMask").map(|m| m.get(0, 0) == 1.0).unwrap_or(false) {
            return attrs.iter().filter(|a| a.name != "vtkValidPointMask").map(|a| (a.name.clone(), fmt(a, 0))).collect();
        }
    }
    // Nearest point fallback (points, lines, glyphs ...).
    let mut best: Option<(f64, aurafx_data::DataSet, usize)> = None;
    for (leaf, _, _) in aurafx_filters::selection::flat_leaves(ds) {
        if let Some(pts) = leaf.points() {
            for i in 0..pts.len() {
                let q = pts.get(i);
                let d = (q[0] - p.x).powi(2) + (q[1] - p.y).powi(2) + (q[2] - p.z).powi(2);
                if best.as_ref().map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, leaf.clone(), i));
                }
            }
        }
    }
    match best {
        Some((_, leaf, i)) => {
            let mut v = vec![("point id".to_string(), i.to_string())];
            if let Some(pd) = leaf.point_data() {
                v.extend(pd.iter().filter(|a| a.scalar_type() != aurafx_data::ScalarType::String).map(|a| (a.name.clone(), fmt(a, i))));
            }
            v
        }
        None => vec![],
    }
}

/// Flattened ids of cells (centers) or points of `ds` whose projection lies inside `sel`.
pub fn select_in_rect(cam: &Camera, rect: Rect, sel: Rect, ds: &DataSet, assoc: Association) -> Vec<usize> {
    let mut out = Vec::new();
    for (leaf, c0, p0) in aurafx_filters::selection::flat_leaves(ds) {
        let (pts, base): (Vec<[f64; 3]>, usize) = match assoc {
            Association::Points => match leaf.points() {
                Some(p) => ((0..p.len()).map(|i| p.get(i)).collect(), p0),
                None => continue,
            },
            Association::Cells => match aurafx_filters::attributes::cell_centers(&leaf) {
                Ok(c) => ((0..c.points.len()).map(|i| c.points.get(i)).collect(), c0),
                Err(_) => continue,
            },
        };
        for (i, sp) in project_many(cam, rect, &pts).into_iter().enumerate() {
            if let Some(sp) = sp {
                if sel.contains(sp) {
                    out.push(base + i);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_selection_and_ray() {
        let img = aurafx_filters::sources::wavelet([-5, 5, -5, 5, -5, 5], 255.0, 0.5);
        let ds = DataSet::ImageData(img);
        let mut cam = Camera::default();
        cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &ds.bounds());
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        // Left half of the screen = cells with x < 0 (all z layers, looking down -z).
        let left = Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 600.0));
        let ids = select_in_rect(&cam, rect, left, &ds, Association::Cells);
        assert_eq!(ids.len(), 500, "half of 1000 cells");
        let all = select_in_rect(&cam, rect, rect, &ds, Association::Points);
        assert_eq!(all.len(), 1331);
        // A ray through the screen center hits the top face (z = 5) of the box surface.
        let d = aurafx_pipeline::DisplayProps::default();
        let mesh = Arc::new(aurafx_render::tessellate::tessellate(&ds, &d));
        let ray = screen_ray(&cam, rect, rect.center());
        let hit = raycast(&ray, &[(1, mesh)], 0.01).expect("hit");
        assert!((hit.world.z - 5.0).abs() < 1e-3, "{:?}", hit.world);
        assert!(hit.world.x.abs() < 1e-3 && hit.world.y.abs() < 1e-3);
        let vals = values_at(&ds, hit.world);
        assert!(vals.iter().any(|(k, _)| k == "RTData"));
    }
}
