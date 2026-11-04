//! Unique edges of a dataset (for wireframe) and feature edges of a surface.

use crate::simplex::{as_explicit, for_each_cell, parts};
use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::collections::HashSet;

pub fn extract_edges(ds: &DataSet) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, pd, _, fd) = parts(&ds).ok_or_else(|| anyhow!("edges: unsupported dataset"))?;
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut lines = Cells::new();
    let mut add = |a: u32, b: u32, lines: &mut Cells| {
        let k = if a < b { (a, b) } else { (b, a) };
        if seen.insert(k) {
            lines.push(CellType::Line, &[a, b]);
        }
    };
    for_each_cell(&ds, |_, ty, pts, faces| {
        let e = ty.edges();
        if !e.is_empty() {
            for ed in e {
                add(pts[ed[0]], pts[ed[1]], &mut lines);
            }
        } else {
            match ty.dimension() {
                1 => {
                    for w in pts.windows(2) {
                        add(w[0], w[1], &mut lines);
                    }
                }
                2 => {
                    for k in 0..pts.len() {
                        add(pts[k], pts[(k + 1) % pts.len()], &mut lines);
                    }
                }
                3 => {
                    if let Some(fs) = faces {
                        for f in fs {
                            for k in 0..f.len() {
                                add(f[k], f[(k + 1) % f.len()], &mut lines);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });
    let (points, point_data) = crate::interp::compact_points(points, pd, &mut lines.connectivity);
    Ok(PolyData { points, lines, point_data, field_data: fd.clone(), ..Default::default() })
}

/// Feature edges: boundary edges (used by one polygon) and sharp edges (dihedral angle > `angle_deg`).
pub fn feature_edges(surface: &PolyData, angle_deg: f64, boundary: bool, sharp: bool) -> Result<PolyData> {
    use std::collections::HashMap;
    let pts = &surface.points;
    let normal = |c: &[u32]| -> [f64; 3] {
        if c.len() < 3 {
            return [0.0, 0.0, 1.0];
        }
        let (a, b, d) = (pts.get(c[0] as usize), pts.get(c[1] as usize), pts.get(c[2] as usize));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
        crate::slice::normalize([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]])
    };
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    let mut normals = Vec::with_capacity(surface.polys.len());
    for i in 0..surface.polys.len() {
        let c = surface.polys.cell(i);
        normals.push(normal(c));
        for k in 0..c.len() {
            let (a, b) = (c[k], c[(k + 1) % c.len()]);
            let key = if a < b { (a, b) } else { (b, a) };
            edge_faces.entry(key).or_default().push(i);
        }
    }
    let cos_thresh = angle_deg.to_radians().cos();
    let mut lines = Cells::new();
    for ((a, b), faces) in edge_faces {
        let is_boundary = faces.len() == 1;
        let is_sharp = faces.len() == 2 && {
            let (n0, n1) = (normals[faces[0]], normals[faces[1]]);
            n0[0] * n1[0] + n0[1] * n1[1] + n0[2] * n1[2] < cos_thresh
        };
        if (boundary && is_boundary) || (sharp && is_sharp) {
            lines.push(CellType::Line, &[a, b]);
        }
    }
    let (points, point_data) = crate::interp::compact_points(pts, &surface.point_data, &mut lines.connectivity);
    Ok(PolyData { points, lines, point_data, ..Default::default() })
}
