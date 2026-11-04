//! Glyphs: place an arrow / line / sphere / cube at every point, oriented and scaled by arrays.

use anyhow::{anyhow, Result};
use aurafx_data::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphShape {
    Arrow,
    Line,
    Sphere,
    Cube,
}

impl GlyphShape {
    pub fn parse(s: &str) -> GlyphShape {
        match s.to_ascii_lowercase().as_str() {
            "line" => GlyphShape::Line,
            "sphere" => GlyphShape::Sphere,
            "cube" | "box" => GlyphShape::Cube,
            _ => GlyphShape::Arrow,
        }
    }
}

/// Unit glyph geometry pointing along +X with length 1.
fn unit_glyph(shape: GlyphShape, res: usize) -> (Vec<[f64; 3]>, Vec<[u32; 3]>, Vec<[u32; 2]>) {
    let mut pts = Vec::new();
    let mut tris = Vec::new();
    let mut lines = Vec::new();
    match shape {
        GlyphShape::Line => {
            pts.push([0.0, 0.0, 0.0]);
            pts.push([1.0, 0.0, 0.0]);
            lines.push([0, 1]);
        }
        GlyphShape::Arrow => {
            let (shaft_r, tip_r, tip_len) = (0.03, 0.1, 0.35);
            let n = res.max(6);
            // Shaft cylinder.
            for i in 0..n {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                pts.push([0.0, shaft_r * a.cos(), shaft_r * a.sin()]);
                pts.push([1.0 - tip_len, shaft_r * a.cos(), shaft_r * a.sin()]);
            }
            for i in 0..n as u32 {
                let j = (i + 1) % n as u32;
                tris.push([2 * i, 2 * j, 2 * i + 1]);
                tris.push([2 * j, 2 * j + 1, 2 * i + 1]);
            }
            let base = pts.len() as u32;
            for i in 0..n {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                pts.push([1.0 - tip_len, tip_r * a.cos(), tip_r * a.sin()]);
            }
            let tip = pts.len() as u32;
            pts.push([1.0, 0.0, 0.0]);
            let center = pts.len() as u32;
            pts.push([1.0 - tip_len, 0.0, 0.0]);
            for i in 0..n as u32 {
                let j = (i + 1) % n as u32;
                tris.push([base + i, base + j, tip]);
                tris.push([base + j, base + i, center]);
            }
        }
        GlyphShape::Sphere => {
            let n = res.max(6);
            let m = n / 2 + 1;
            for j in 0..=m {
                let phi = j as f64 / m as f64 * std::f64::consts::PI;
                for i in 0..n {
                    let th = i as f64 / n as f64 * std::f64::consts::TAU;
                    pts.push([0.5 * phi.sin() * th.cos(), 0.5 * phi.sin() * th.sin(), 0.5 * phi.cos()]);
                }
            }
            for j in 0..m as u32 {
                for i in 0..n as u32 {
                    let i2 = (i + 1) % n as u32;
                    let (a, b, c, d) = (j * n as u32 + i, j * n as u32 + i2, (j + 1) * n as u32 + i, (j + 1) * n as u32 + i2);
                    tris.push([a, c, b]);
                    tris.push([b, c, d]);
                }
            }
        }
        GlyphShape::Cube => {
            for k in 0..8 {
                pts.push([if k & 1 != 0 { 0.5 } else { -0.5 }, if k & 2 != 0 { 0.5 } else { -0.5 }, if k & 4 != 0 { 0.5 } else { -0.5 }]);
            }
            for f in CellType::Voxel.faces() {
                tris.push([f[0] as u32, f[1] as u32, f[2] as u32]);
                tris.push([f[0] as u32, f[2] as u32, f[3] as u32]);
            }
        }
    }
    (pts, tris, lines)
}

pub struct GlyphOptions {
    pub shape: GlyphShape,
    pub orient_array: Option<String>,
    pub scale_array: Option<String>,
    pub scale_factor: f64,
    pub resolution: usize,
    pub stride: usize,
    pub max_glyphs: usize,
}

pub fn glyph(ds: &DataSet, opt: &GlyphOptions) -> Result<PolyData> {
    let points = ds.points().ok_or_else(|| anyhow!("glyph: dataset has no points"))?;
    let pd = ds.point_data().ok_or_else(|| anyhow!("glyph: dataset has no point data"))?;
    let orient = opt.orient_array.as_deref().and_then(|n| pd.get(n)).filter(|a| a.ncomp >= 3);
    let scale = opt.scale_array.as_deref().and_then(|n| pd.get(n));
    let (gp, gt, gl) = unit_glyph(opt.shape, opt.resolution);
    let n = points.len();
    let stride = opt.stride.max(1).max(n.div_ceil(opt.max_glyphs.max(1)));
    let mut out_pts: Vec<f64> = Vec::new();
    let mut polys = Cells::new();
    let mut lines = Cells::new();
    let mut parents: Vec<usize> = Vec::new();
    for i in (0..n).step_by(stride) {
        let p = points.get(i);
        let dir = orient.map(|a| [a.get(i, 0), a.get(i, 1), a.get(i, 2)]).unwrap_or([1.0, 0.0, 0.0]);
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let s = opt.scale_factor
            * match scale {
                Some(a) => a.magnitude(i),
                None => if orient.is_some() { len } else { 1.0 },
            };
        if s == 0.0 || !s.is_finite() {
            continue;
        }
        // Orthonormal frame with x along dir.
        let x = if len > 0.0 { [dir[0] / len, dir[1] / len, dir[2] / len] } else { [1.0, 0.0, 0.0] };
        let up = if x[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let z = crate::slice::normalize([x[1] * up[2] - x[2] * up[1], x[2] * up[0] - x[0] * up[2], x[0] * up[1] - x[1] * up[0]]);
        let y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
        let base = (out_pts.len() / 3) as u32;
        for g in &gp {
            out_pts.extend_from_slice(&[
                p[0] + s * (g[0] * x[0] + g[1] * y[0] + g[2] * z[0]),
                p[1] + s * (g[0] * x[1] + g[1] * y[1] + g[2] * z[1]),
                p[2] + s * (g[0] * x[2] + g[1] * y[2] + g[2] * z[2]),
            ]);
        }
        for t in &gt {
            polys.push(CellType::Triangle, &[base + t[0], base + t[1], base + t[2]]);
            parents.push(i);
        }
        for l in &gl {
            lines.push(CellType::Line, &[base + l[0], base + l[1]]);
            parents.push(i);
        }
    }
    // Copy point data of the source point onto every glyph vertex (useful for coloring).
    let per_vertex: Vec<usize> = (0..n).step_by(stride).flat_map(|i| std::iter::repeat_n(i, gp.len())).collect();
    let point_data = pd.gather(&per_vertex[..(out_pts.len() / 3).min(per_vertex.len())]);
    let _ = parents;
    Ok(PolyData { points: Points::new_f64(out_pts), lines, polys, point_data, ..Default::default() })
}
