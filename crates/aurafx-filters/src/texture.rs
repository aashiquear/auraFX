//! Texture coordinates (Texture Map to Plane / Cylinder / Sphere), Perlin noise and
//! spatio-temporal harmonics, computed as VTK's filters do so results match ParaView.

use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::f64::consts::PI;

/// Name VTK gives the texture coordinates these filters make.
pub const TCOORDS: &str = "Texture Coordinates";

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
/// Normalize in place, returning the old length (VTK's vtkMath::Normalize).
fn normalize(v: &mut [f64; 3]) -> f64 {
    let n = dot(*v, *v).sqrt();
    if n != 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    n
}
fn det3(c1: [f64; 3], c2: [f64; 3], c3: [f64; 3]) -> f64 {
    c1[0] * c2[1] * c3[2] - c1[0] * c2[2] * c3[1] - c2[0] * c1[1] * c3[2] + c2[0] * c1[2] * c3[1] + c3[0] * c1[1] * c2[2] - c3[0] * c1[2] * c2[1]
}

/// Copy of `ds` with `tc` (2 per point) as its texture coordinates.
fn with_tcoords(ds: &DataSet, tc: Vec<f32>) -> Result<DataSet> {
    let mut out = ds.clone();
    let pd = out.point_data_mut().ok_or_else(|| anyhow!("texture coordinates need a dataset with points"))?;
    pd.insert_active(AttributeKind::TCoords, Array::new(TCOORDS, 2, ArrayData::F32(tc)));
    Ok(out)
}

fn points_of(ds: &DataSet) -> Result<Points> {
    ds.points().ok_or_else(|| anyhow!("{} has no points", ds.type_name()))
}

/// Texture Map to Plane. With `automatic` (and `origin`/`point1` left at zero, as VTK
/// requires) the plane is fitted to the points by least squares and the coordinates span
/// 0..1 over the bounds; otherwise s and t are the projections on origin→point1 and
/// origin→point2 (1 at the points).
pub fn map_to_plane(ds: &DataSet, automatic: bool, origin: [f64; 3], p1: [f64; 3], p2: [f64; 3]) -> Result<DataSet> {
    let pts = points_of(ds)?;
    let n = pts.len();
    if n < 3 && automatic {
        bail!("not enough points for an automatic plane");
    }
    let mut tc = Vec::with_capacity(2 * n);
    if automatic && origin == [0.0; 3] && p1 == [0.0; 3] {
        let b = pts.bounds();
        let (lo, hi) = (b.min, b.max);
        let mut normal = plane_normal(&pts, lo, hi);
        normalize(&mut normal);
        let (mut dir, mut min_proj) = (0, 1.0);
        for i in 0..3 {
            let proj = normal[i].abs();
            if proj < min_proj {
                min_proj = proj;
                dir = i;
            }
        }
        let mut axis = [0.0; 3];
        axis[dir] = 1.0;
        let mut t_axis = cross(normal, axis);
        normalize(&mut t_axis);
        let s_axis = cross(t_axis, normal);
        let size = sub(hi, lo);
        let (s_sf, t_sf) = (1.0 / dot(s_axis, size), 1.0 / dot(t_axis, size));
        for i in 0..n {
            let d = sub(pts.get(i), lo);
            tc.push((dot(s_axis, d) * s_sf) as f32);
            tc.push((dot(t_axis, d) * t_sf) as f32);
        }
    } else {
        let (s_axis, t_axis) = (sub(p1, origin), sub(p2, origin));
        let (sd, td) = (dot(s_axis, s_axis), dot(t_axis, t_axis));
        if sd == 0.0 || td == 0.0 {
            bail!("the plane is not defined: set Origin, Point 1 and Point 2 apart (or turn Automatic on)");
        }
        for i in 0..n {
            let d = sub(pts.get(i), origin);
            tc.push((dot(s_axis, d) / sd) as f32);
            tc.push((dot(t_axis, d) / td) as f32);
        }
    }
    with_tcoords(ds, tc)
}

/// vtkTextureMapToPlane::ComputeNormal: the thinnest bounding direction when the points are
/// (nearly) flat, else the least-squares plane z = ax + by + c.
fn plane_normal(pts: &Points, lo: [f64; 3], hi: [f64; 3]) -> [f64; 3] {
    let length = dot(sub(hi, lo), sub(hi, lo)).sqrt();
    let (mut dir, mut w) = (0, length);
    for i in 0..3 {
        if hi[i] - lo[i] < w {
            dir = i;
            w = hi[i] - lo[i];
        }
    }
    let mut normal = [0.0; 3];
    normal[dir] = 1.0;
    if w <= length * 1e-3 {
        return normal;
    }
    let (mut v, mut c1, mut c2, mut c3) = ([0.0; 3], [0.0; 3], [0.0; 3], [0.0; 3]);
    for i in 0..pts.len() {
        let x = pts.get(i);
        v[0] += x[0] * x[2];
        v[1] += x[1] * x[2];
        v[2] += x[2];
        c1[0] += x[0] * x[0];
        c1[1] += x[0] * x[1];
        c1[2] += x[0];
        c2[0] += x[0] * x[1];
        c2[1] += x[1] * x[1];
        c2[2] += x[1];
        c3[0] += x[0];
        c3[1] += x[1];
    }
    c3[2] = pts.len() as f64;
    let det = det3(c1, c2, c3);
    if det <= 1e-3 {
        return normal;
    }
    [det3(v, c2, c3) / det, det3(c1, v, c3) / det, -1.0]
}

/// vtkMath::JacobiN for a symmetric 3×3 matrix: eigenvalues in decreasing order and the
/// eigenvectors as columns of `v`, each flipped so most of its components are non-negative.
fn jacobi3(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let n = 3;
    let mut v = [[0.0; 3]; 3];
    let (mut w, mut b, mut z) = ([0.0; 3], [0.0; 3], [0.0; 3]);
    for ip in 0..n {
        v[ip][ip] = 1.0;
        b[ip] = a[ip][ip];
        w[ip] = a[ip][ip];
    }
    fn rot(m: &mut [[f64; 3]; 3], (i, j): (usize, usize), (k, l): (usize, usize), s: f64, tau: f64) {
        let (g, h) = (m[i][j], m[k][l]);
        m[i][j] = g - s * (h + g * tau);
        m[k][l] = h + s * (g - h * tau);
    }
    for i in 0..20 {
        let mut sm = 0.0;
        for ip in 0..n - 1 {
            for iq in ip + 1..n {
                sm += a[ip][iq].abs();
            }
        }
        if sm == 0.0 {
            break;
        }
        let tresh = if i < 3 { 0.2 * sm / (n * n) as f64 } else { 0.0 };
        for ip in 0..n - 1 {
            for iq in ip + 1..n {
                let g = 100.0 * a[ip][iq].abs();
                if i > 3 && w[ip].abs() + g == w[ip].abs() && w[iq].abs() + g == w[iq].abs() {
                    a[ip][iq] = 0.0;
                } else if a[ip][iq].abs() > tresh {
                    let h = w[iq] - w[ip];
                    let t = if h.abs() + g == h.abs() {
                        a[ip][iq] / h
                    } else {
                        let theta = 0.5 * h / a[ip][iq];
                        let t = 1.0 / (theta.abs() + (1.0 + theta * theta).sqrt());
                        if theta < 0.0 {
                            -t
                        } else {
                            t
                        }
                    };
                    let c = 1.0 / (1.0 + t * t).sqrt();
                    let s = t * c;
                    let tau = s / (1.0 + c);
                    let h = t * a[ip][iq];
                    z[ip] -= h;
                    z[iq] += h;
                    w[ip] -= h;
                    w[iq] += h;
                    a[ip][iq] = 0.0;
                    for j in 0..ip {
                        rot(&mut a, (j, ip), (j, iq), s, tau);
                    }
                    for j in ip + 1..iq {
                        rot(&mut a, (ip, j), (j, iq), s, tau);
                    }
                    for j in iq + 1..n {
                        rot(&mut a, (ip, j), (iq, j), s, tau);
                    }
                    for j in 0..n {
                        rot(&mut v, (j, ip), (j, iq), s, tau);
                    }
                }
            }
        }
        for ip in 0..n {
            b[ip] += z[ip];
            w[ip] = b[ip];
            z[ip] = 0.0;
        }
    }
    for j in 0..n - 1 {
        let mut k = j;
        let mut tmp = w[k];
        for i in j + 1..n {
            if w[i] >= tmp {
                k = i;
                tmp = w[k];
            }
        }
        if k != j {
            w[k] = w[j];
            w[j] = tmp;
            for row in v.iter_mut() {
                row.swap(j, k);
            }
        }
    }
    for j in 0..n {
        let pos = (0..n).filter(|&i| v[i][j] >= 0.0).count();
        if pos < 2 {
            for row in v.iter_mut() {
                row[j] = -row[j];
            }
        }
    }
    (w, v)
}

/// vtkLine::DistanceToLine's parametric coordinate and closest point (clamped to the segment).
fn line_param(x: [f64; 3], p1: [f64; 3], p2: [f64; 3]) -> (f64, [f64; 3]) {
    let p21 = sub(p2, p1);
    let num = dot(p21, sub(x, p1));
    if num == 0.0 {
        return (0.0, p1);
    }
    let denom = dot(p21, p21);
    if denom < (1e-5 * num).abs() {
        return if num > 0.0 { (1.0e299, p2) } else { (-1.0e299, p1) };
    }
    let t = num / denom;
    if t < 0.0 {
        (t, p1)
    } else if t > 1.0 {
        (t, p2)
    } else {
        (t, [p1[0] + t * p21[0], p1[1] + t * p21[1], p1[2] + t * p21[2]])
    }
}

/// The axis of the oriented bounding box's longest side through the middle of its cross
/// section (vtkOBBTree::ComputeOBB as vtkTextureMapToCylinder uses it).
fn obb_axis(pts: &Points) -> ([f64; 3], [f64; 3]) {
    let n = pts.len() as f64;
    let mut mean = [0.0; 3];
    for i in 0..pts.len() {
        let x = pts.get(i);
        (0..3).for_each(|k| mean[k] += x[k]);
    }
    mean.iter_mut().for_each(|m| *m /= n);
    let mut a = [[0.0; 3]; 3];
    for i in 0..pts.len() {
        let xp = sub(pts.get(i), mean);
        for r in 0..3 {
            for c in 0..3 {
                a[r][c] += xp[r] * xp[c];
            }
        }
    }
    a.iter_mut().flatten().for_each(|x| *x /= n);
    let (_, v) = jacobi3(a);
    let axes: [[f64; 3]; 3] = std::array::from_fn(|c| [v[0][c], v[1][c], v[2][c]]);
    let ends: [[f64; 3]; 3] = std::array::from_fn(|c| [mean[0] + axes[c][0], mean[1] + axes[c][1], mean[2] + axes[c][2]]);
    let (mut tmin, mut tmax) = ([f64::MAX; 3], [-f64::MAX; 3]);
    for i in 0..pts.len() {
        let x = pts.get(i);
        for c in 0..3 {
            let (t, _) = line_param(x, mean, ends[c]);
            tmin[c] = tmin[c].min(t);
            tmax[c] = tmax[c].max(t);
        }
    }
    let mut corner = [0.0; 3];
    let (mut max, mut mid, mut min) = ([0.0; 3], [0.0; 3], [0.0; 3]);
    for i in 0..3 {
        corner[i] = mean[i] + tmin[0] * axes[0][i] + tmin[1] * axes[1][i] + tmin[2] * axes[2][i];
        max[i] = (tmax[0] - tmin[0]) * axes[0][i];
        mid[i] = (tmax[1] - tmin[1]) * axes[1][i];
        min[i] = (tmax[2] - tmin[2]) * axes[2][i];
    }
    let (mut p1, mut p2) = ([0.0; 3], [0.0; 3]);
    for i in 0..3 {
        let l = (mid[i] + min[i]) / 2.0;
        p1[i] = corner[i] + l;
        p2[i] = corner[i] + max[i] + l;
    }
    (p1, p2)
}

/// Texture Map to Cylinder: t along the axis (0 at point 1, 1 at point 2), s around it. The
/// axis is the longest side of the points' oriented bounding box when `automatic`. With
/// `prevent_seam` s runs 0→1→0 around the axis so a texture repeats without a seam.
pub fn map_to_cylinder(ds: &DataSet, automatic: bool, p1: [f64; 3], p2: [f64; 3], prevent_seam: bool) -> Result<DataSet> {
    let pts = points_of(ds)?;
    if pts.is_empty() {
        bail!("no points to map");
    }
    let (p1, p2) = if automatic { obb_axis(&pts) } else { (p1, p2) };
    let axis = sub(p2, p1);
    if dot(axis, axis) == 0.0 {
        bail!("the cylinder axis is not defined: set Point 1 and Point 2 apart");
    }
    let mut vp = cross(axis, [1.0, 0.0, 0.0]);
    if dot(vp, vp) == 0.0 {
        vp = cross(axis, [0.0, 1.0, 0.0]);
    }
    let mut vec = cross(vp, axis);
    if normalize(&mut vec) == 0.0 {
        bail!("the cylinder axis is not defined");
    }
    let mut tc = Vec::with_capacity(2 * pts.len());
    for i in 0..pts.len() {
        let x = pts.get(i);
        let (t, closest) = line_param(x, p1, p2);
        let mut v = sub(x, closest);
        normalize(&mut v);
        let theta_x = dot(v, vec).clamp(-1.0, 1.0).acos();
        let theta_y = dot(axis, cross(vec, v));
        let s = if prevent_seam {
            theta_x / PI
        } else {
            let s = theta_x / (2.0 * PI);
            if theta_y < 0.0 {
                1.0 - s
            } else {
                s
            }
        };
        tc.push(s as f32);
        tc.push(t as f32);
    }
    with_tcoords(ds, tc)
}

/// Texture Map to Sphere: t from the +z pole (0) to the -z pole (1), s around z. The center is
/// the mean of the points unless given.
pub fn map_to_sphere(ds: &DataSet, center: Option<[f64; 3]>, prevent_seam: bool) -> Result<DataSet> {
    let pts = points_of(ds)?;
    if pts.is_empty() {
        bail!("no points to map");
    }
    let c = center.unwrap_or_else(|| {
        let mut c = [0.0; 3];
        for i in 0..pts.len() {
            let x = pts.get(i);
            (0..3).for_each(|k| c[k] += x[k]);
        }
        c.map(|v| v / pts.len() as f64)
    });
    let mut tc = Vec::with_capacity(2 * pts.len());
    for i in 0..pts.len() {
        let x = pts.get(i);
        let rho = dot(sub(x, c), sub(x, c)).sqrt();
        let mut phi = 0.0;
        let t = if rho != 0.0 {
            let diff = x[2] - c[2];
            if diff.abs() > rho {
                if diff > 0.0 {
                    0.0
                } else {
                    1.0
                }
            } else {
                phi = (diff / rho).acos();
                phi / PI
            }
        } else {
            0.0
        };
        let r = rho * phi.sin();
        let (theta_x, theta_y) = if r != 0.0 {
            let dx = x[0] - c[0];
            let tx = if dx.abs() > r {
                if dx > 0.0 {
                    0.0
                } else {
                    PI
                }
            } else {
                (dx / r).acos()
            };
            let dy = x[1] - c[1];
            let ty = if dy.abs() > r {
                if dy > 0.0 {
                    PI / 2.0
                } else {
                    -PI / 2.0
                }
            } else {
                (dy / r).asin()
            };
            (tx, ty)
        } else {
            (0.0, 0.0)
        };
        let s = if prevent_seam {
            theta_x / PI
        } else {
            let s = theta_x / (2.0 * PI);
            if theta_y < 0.0 {
                1.0 - s
            } else {
                s
            }
        };
        tc.push(s as f32);
        tc.push(t as f32);
    }
    with_tcoords(ds, tc)
}

// ------------------------------------------------------------------ Perlin noise

fn hermite(p0: f64, p1: f64, r0: f64, r1: f64, t: f64) -> f64 {
    let tt = t * t;
    p0 * ((2.0 * t - 3.0) * tt + 1.0) + p1 * (-2.0 * t + 3.0) * tt + r0 * ((t - 2.0) * t + 1.0) * t + r1 * (t - 1.0) * tt
}

fn frand(s: u32) -> f64 {
    let s = (s << 13) ^ s;
    let s = s.wrapping_mul(s.wrapping_mul(s).wrapping_mul(15731).wrapping_add(789221)).wrapping_add(1376312589) & 0x7fff_ffff;
    1.0 - s as f64 / 1073741824.0
}

fn rand3abcd(x: i32, y: i32, z: i32) -> [f64; 4] {
    let h = |a: i32, b: i32, c: i32| frand(a.wrapping_mul(x).wrapping_add(b.wrapping_mul(y)).wrapping_add(c.wrapping_mul(z)) as u32);
    [h(67, 59, 71), h(73, 79, 83), h(89, 97, 101), h(103, 107, 109)]
}

fn interpolate(i: usize, n: usize, xlim: &[[i32; 2]; 3], xarg: &[f64; 3]) -> [f64; 4] {
    if n == 0 {
        return rand3abcd(xlim[0][i & 1], xlim[1][(i >> 1) & 1], xlim[2][i >> 2]);
    }
    let n = n - 1;
    let f0 = interpolate(i, n, xlim, xarg);
    let f1 = interpolate(i | (1 << n), n, xlim, xarg);
    let a = xarg[n];
    [(1.0 - a) * f0[0] + a * f1[0], (1.0 - a) * f0[1] + a * f1[1], (1.0 - a) * f0[2] + a * f1[2], hermite(f0[3], f1[3], f0[n], f1[n], a)]
}

/// VTK's Perlin noise (Greg Ward's version from Graphics Gems II) at `x`.
pub fn perlin(x: [f64; 3], frequency: [f64; 3], phase: [f64; 3], amplitude: f64) -> f64 {
    let xd: [f64; 3] = std::array::from_fn(|k| x[k] * frequency[k] - phase[k] * 2.0);
    let lo: [i32; 3] = std::array::from_fn(|k| xd[k].floor() as i32);
    let xlim = [[lo[0], lo[0] + 1], [lo[1], lo[1] + 1], [lo[2], lo[2] + 1]];
    let xarg: [f64; 3] = std::array::from_fn(|k| xd[k] - lo[k] as f64);
    interpolate(0, 3, &xlim, &xarg)[3] * amplitude
}

/// Perlin Noise filter: the noise at every point as a new (active) point array.
pub fn perlin_noise(ds: &DataSet, frequency: [f64; 3], phase: [f64; 3], amplitude: f64, name: &str) -> Result<DataSet> {
    let pts = points_of(ds)?;
    let v: Vec<f32> = (0..pts.len()).map(|i| perlin(pts.get(i), frequency, phase, amplitude) as f32).collect();
    let mut out = ds.clone();
    out.point_data_mut()
        .ok_or_else(|| anyhow!("Perlin noise needs a dataset with points"))?
        .insert_active(AttributeKind::Scalars, Array::new(if name.is_empty() { "PerlinNoise" } else { name }, 1, ArrayData::F32(v)));
    Ok(out)
}

// ------------------------------------------------------------------ harmonics

/// ParaView's default harmonics: (amplitude, temporal frequency, wave vector x y z, phase).
pub const DEFAULT_HARMONICS: [f64; 24] =
    [1.0, 1.0, 0.6283, 0.6283, 0.6283, 0.0, 3.0, 1.0, 0.6283, 0.0, 0.0, 1.5708, 2.0, 2.0, 0.0, 0.6283, 0.0, 3.1416, 1.0, 3.0, 0.0, 0.0, 0.6283, 4.7124];
/// ParaView's default time steps of the harmonics source (one period in 20 steps).
pub const DEFAULT_TIMES: [f64; 20] = [
    0.0, 0.3141, 0.6283, 0.9425, 1.2566, 1.5708, 1.885, 2.1991, 2.5133, 2.8274, 3.1416, 3.4558, 3.7699, 4.0841, 4.3982, 4.7124, 5.0266, 5.3407, 5.6549, 5.969,
];
pub const HARMONICS_NAME: &str = "SpatioTemporalHarmonics";

/// Sum of amplitude·sin(frequency·t + k·x + phase) over the harmonics (6 numbers each).
pub fn harmonics_value(h: &[f64], x: [f64; 3], t: f64) -> f64 {
    h.chunks_exact(6).map(|c| c[0] * (c[1] * t + dot(x, [c[2], c[3], c[4]]) + c[5]).sin()).sum()
}

pub fn check_harmonics(h: &[f64]) -> Result<()> {
    if h.is_empty() || h.len() % 6 != 0 {
        bail!("harmonics take 6 numbers each (amplitude, temporal frequency, wave vector x y z, phase); got {}", h.len());
    }
    Ok(())
}

/// Spatio Temporal Harmonics filter: the sum of the harmonics at every point at time `t`.
pub fn harmonics(ds: &DataSet, h: &[f64], t: f64) -> Result<DataSet> {
    check_harmonics(h)?;
    let pts = points_of(ds)?;
    let v: Vec<f64> = (0..pts.len()).map(|i| harmonics_value(h, pts.get(i), t)).collect();
    let mut out = ds.clone();
    out.point_data_mut()
        .ok_or_else(|| anyhow!("harmonics need a dataset with points"))?
        .insert_active(AttributeKind::Scalars, Array::new(HARMONICS_NAME, 1, ArrayData::F64(v)));
    Ok(out)
}

/// Spatio Temporal Harmonics source: an image (spacing 1) over `extent` with the harmonics.
pub fn harmonics_grid(extent: [i64; 6], h: &[f64], t: f64) -> Result<DataSet> {
    check_harmonics(h)?;
    if extent[1] < extent[0] || extent[3] < extent[2] || extent[5] < extent[4] {
        bail!("the extent is empty");
    }
    let img = ImageData { extent, ..Default::default() };
    harmonics(&DataSet::ImageData(img), h, t)
}
