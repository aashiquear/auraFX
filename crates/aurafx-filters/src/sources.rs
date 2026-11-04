//! Procedural sources: sphere, cone, cylinder, box, plane, line, point source, wavelet.

use aurafx_data::*;
use std::f64::consts::{PI, TAU};

pub fn sphere(center: [f64; 3], radius: f64, theta_res: usize, phi_res: usize) -> PolyData {
    let (n, m) = (theta_res.max(3), phi_res.max(3));
    let mut pts = Vec::new();
    pts.extend_from_slice(&[center[0], center[1], center[2] + radius]);
    for j in 1..m - 1 {
        let phi = j as f64 / (m - 1) as f64 * PI;
        for i in 0..n {
            let th = i as f64 / n as f64 * TAU;
            pts.extend_from_slice(&[center[0] + radius * phi.sin() * th.cos(), center[1] + radius * phi.sin() * th.sin(), center[2] + radius * phi.cos()]);
        }
    }
    pts.extend_from_slice(&[center[0], center[1], center[2] - radius]);
    let south = (pts.len() / 3 - 1) as u32;
    let mut polys = Cells::new();
    let ring = |j: usize, i: usize| (1 + (j - 1) * n + (i % n)) as u32;
    for i in 0..n {
        polys.push(CellType::Triangle, &[0, ring(1, i), ring(1, i + 1)]);
    }
    for j in 1..m - 2 {
        for i in 0..n {
            polys.push(CellType::Quad, &[ring(j, i), ring(j + 1, i), ring(j + 1, i + 1), ring(j, i + 1)]);
        }
    }
    for i in 0..n {
        polys.push(CellType::Triangle, &[south, ring(m - 2, i + 1), ring(m - 2, i)]);
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    let n_pts = pd.points.len();
    let mut normals = Vec::with_capacity(3 * n_pts);
    for i in 0..n_pts {
        let p = pd.points.get(i);
        normals.extend_from_slice(&[(p[0] - center[0]) / radius, (p[1] - center[1]) / radius, (p[2] - center[2]) / radius]);
    }
    pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(normals)));
    pd
}

pub fn cone(center: [f64; 3], height: f64, radius: f64, res: usize, direction: [f64; 3]) -> PolyData {
    let n = res.max(3);
    let dir = crate::slice::normalize(direction);
    let up = if dir[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let z = crate::slice::normalize([dir[1] * up[2] - dir[2] * up[1], dir[2] * up[0] - dir[0] * up[2], dir[0] * up[1] - dir[1] * up[0]]);
    let y = [z[1] * dir[2] - z[2] * dir[1], z[2] * dir[0] - z[0] * dir[2], z[0] * dir[1] - z[1] * dir[0]];
    let mut pts = Vec::new();
    let tip = [center[0] + 0.5 * height * dir[0], center[1] + 0.5 * height * dir[1], center[2] + 0.5 * height * dir[2]];
    let base = [center[0] - 0.5 * height * dir[0], center[1] - 0.5 * height * dir[1], center[2] - 0.5 * height * dir[2]];
    pts.extend_from_slice(&tip);
    pts.extend_from_slice(&base);
    for i in 0..n {
        let a = i as f64 / n as f64 * TAU;
        pts.extend_from_slice(&[
            base[0] + radius * (a.cos() * y[0] + a.sin() * z[0]),
            base[1] + radius * (a.cos() * y[1] + a.sin() * z[1]),
            base[2] + radius * (a.cos() * y[2] + a.sin() * z[2]),
        ]);
    }
    let mut polys = Cells::new();
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        polys.push(CellType::Triangle, &[0, 2 + i, 2 + j]);
        polys.push(CellType::Triangle, &[1, 2 + j, 2 + i]);
    }
    PolyData { points: Points::new_f64(pts), polys, ..Default::default() }
}

pub fn cylinder(center: [f64; 3], height: f64, radius: f64, res: usize) -> PolyData {
    let n = res.max(3);
    let mut pts = Vec::new();
    for i in 0..n {
        let a = i as f64 / n as f64 * TAU;
        pts.extend_from_slice(&[center[0] + radius * a.cos(), center[1] - 0.5 * height, center[2] + radius * a.sin()]);
        pts.extend_from_slice(&[center[0] + radius * a.cos(), center[1] + 0.5 * height, center[2] + radius * a.sin()]);
    }
    let mut polys = Cells::new();
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        polys.push(CellType::Quad, &[2 * i, 2 * j, 2 * j + 1, 2 * i + 1]);
    }
    let bottom: Vec<u32> = (0..n as u32).rev().map(|i| 2 * i).collect();
    let top: Vec<u32> = (0..n as u32).map(|i| 2 * i + 1).collect();
    polys.push(CellType::Polygon, &bottom);
    polys.push(CellType::Polygon, &top);
    PolyData { points: Points::new_f64(pts), polys, ..Default::default() }
}

pub fn cube(center: [f64; 3], size: [f64; 3]) -> PolyData {
    let mut pts = Vec::new();
    for k in 0..8 {
        pts.extend_from_slice(&[
            center[0] + if k & 1 != 0 { 0.5 } else { -0.5 } * size[0],
            center[1] + if k & 2 != 0 { 0.5 } else { -0.5 } * size[1],
            center[2] + if k & 4 != 0 { 0.5 } else { -0.5 } * size[2],
        ]);
    }
    let mut polys = Cells::new();
    for f in CellType::Voxel.faces() {
        polys.push(CellType::Quad, &[f[0] as u32, f[1] as u32, f[2] as u32, f[3] as u32]);
    }
    PolyData { points: Points::new_f64(pts), polys, ..Default::default() }
}

pub fn plane(origin: [f64; 3], p1: [f64; 3], p2: [f64; 3], xres: usize, yres: usize) -> PolyData {
    let (nx, ny) = (xres.max(1) + 1, yres.max(1) + 1);
    let mut pts = Vec::with_capacity(3 * nx * ny);
    let mut tc = Vec::with_capacity(2 * nx * ny);
    for j in 0..ny {
        let v = j as f64 / (ny - 1) as f64;
        for i in 0..nx {
            let u = i as f64 / (nx - 1) as f64;
            pts.extend_from_slice(&[
                origin[0] + u * (p1[0] - origin[0]) + v * (p2[0] - origin[0]),
                origin[1] + u * (p1[1] - origin[1]) + v * (p2[1] - origin[1]),
                origin[2] + u * (p1[2] - origin[2]) + v * (p2[2] - origin[2]),
            ]);
            tc.extend_from_slice(&[u, v]);
        }
    }
    let mut polys = Cells::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let a = (j * nx + i) as u32;
            polys.push(CellType::Quad, &[a, a + 1, a + nx as u32 + 1, a + nx as u32]);
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    pd.point_data.insert_active(AttributeKind::TCoords, Array::new("TextureCoordinates", 2, ArrayData::F64(tc)));
    pd
}

pub fn line(p1: [f64; 3], p2: [f64; 3], res: usize) -> PolyData {
    let n = res.max(1) + 1;
    let mut pts = Vec::with_capacity(3 * n);
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        pts.extend_from_slice(&[p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1]), p1[2] + t * (p2[2] - p1[2])]);
    }
    let mut lines = Cells::new();
    lines.push(CellType::PolyLine, &(0..n as u32).collect::<Vec<_>>());
    PolyData { points: Points::new_f64(pts), lines, ..Default::default() }
}

pub fn point_source(center: [f64; 3], radius: f64, n: usize, seed: u64) -> PolyData {
    // Simple deterministic LCG so results are reproducible.
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut rnd = || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 11) as f64) / ((1u64 << 53) as f64)
    };
    let mut pts = Vec::with_capacity(3 * n);
    for _ in 0..n {
        let r = radius * rnd().cbrt();
        let th = TAU * rnd();
        let ph = (2.0 * rnd() - 1.0).acos();
        pts.extend_from_slice(&[center[0] + r * ph.sin() * th.cos(), center[1] + r * ph.sin() * th.sin(), center[2] + r * ph.cos()]);
    }
    PolyData::from_points(Points::new_f64(pts))
}

/// The classic VTK "Wavelet" synthetic volume (RTData), identical to vtkRTAnalyticSource.
pub fn wavelet(extent: [i64; 6], maximum: f64, std_dev: f64) -> ImageData {
    let mut img = ImageData { extent, ..Default::default() };
    let d = img.dims();
    let mut v = Vec::with_capacity(d[0] * d[1] * d[2]);
    let (xf, yf, zf, xm, ym, zm) = (60.0, 30.0, 40.0, 10.0, 18.0, 5.0);
    let scale = |a: i64, b: i64| if b > a { 1.0 / (b - a) as f64 } else { 1.0 };
    let (xs, ys, zs) = (scale(extent[0], extent[1]), scale(extent[2], extent[3]), scale(extent[4], extent[5]));
    let temp2 = 1.0 / (2.0 * std_dev * std_dev);
    for k in 0..d[2] {
        let z = -((extent[4] + k as i64) as f64) * zs;
        for j in 0..d[1] {
            let y = -((extent[2] + j as i64) as f64) * ys;
            for i in 0..d[0] {
                let x = -((extent[0] + i as i64) as f64) * xs;
                let sum = x * x + y * y + z * z;
                let val = maximum * (-sum * temp2).exp() + xm * (xf * x).sin() + ym * (yf * y).sin() + zm * (zf * z).cos();
                v.push(val as f32);
            }
        }
    }
    img.point_data.insert_active(AttributeKind::Scalars, Array::new("RTData", 1, ArrayData::F32(v)));
    img
}
