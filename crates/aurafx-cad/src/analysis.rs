//! Ported from EZ-YX-craft (`ezyx-cad`, MIT OR Apache-2.0), the user's own project.
//!
//! Evaluate tools: mass properties, interference detection and planar sections.

use crate::document::Document;
use crate::kernel::sdf::Sdf;
use crate::mesh::TriMesh;
use glam::DVec3;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Mass properties of a closed solid (mm, g).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MassProperties {
    pub volume_mm3: f64,
    pub area_mm2: f64,
    pub centroid: [f64; 3],
    pub bbox_min: [f64; 3],
    pub bbox_max: [f64; 3],
    pub mass_g: f64,
    pub density_g_cm3: f64,
}

impl MassProperties {
    /// From a closed, outward-oriented triangle mesh (divergence theorem, f64 accumulation).
    pub fn from_mesh(m: &TriMesh, density_g_cm3: f64) -> Self {
        let mut vol = 0.0;
        let mut area = 0.0;
        let mut c = DVec3::ZERO;
        for i in 0..m.indices.len() {
            let [a, b, d] = m.triangle(i).map(|v| v.as_dvec3());
            let v6 = a.dot(b.cross(d)); // 6 × signed tetra volume with the origin
            vol += v6;
            c += (a + b + d) * v6; // tetra centroid (a+b+d+0)/4, weighted
            area += (b - a).cross(d - a).length() * 0.5;
        }
        let volume = vol / 6.0;
        let centroid = if vol.abs() > 1e-12 { c / (4.0 * vol) } else { DVec3::ZERO };
        let (lo, hi) = if m.positions.is_empty() { (DVec3::ZERO, DVec3::ZERO) } else { (m.bounds().0.as_dvec3(), m.bounds().1.as_dvec3()) };
        Self {
            volume_mm3: volume,
            area_mm2: area,
            centroid: centroid.to_array(),
            bbox_min: lo.to_array(),
            bbox_max: hi.to_array(),
            mass_g: volume / 1000.0 * density_g_cm3,
            density_g_cm3,
        }
    }
}

impl Document {
    /// Mass properties of the meshed part at `resolution` mm.
    pub fn mass_properties(&self, resolution: f64, density_g_cm3: f64) -> MassProperties {
        MassProperties::from_mesh(&self.mesh(resolution), density_g_cm3)
    }

    /// Cross-section area (mm²) of the part at height `z`, sampled on an XY grid of `cell` mm
    /// with anti-aliased coverage.
    pub fn section_area_at_z(&self, z: f64, cell: f64) -> f64 {
        self.sdf().map(|s| section_area(&s, z, cell)).unwrap_or(0.0)
    }
}

/// Cross-section area of an SDF at height `z`.
pub fn section_area(s: &Sdf, z: f64, cell: f64) -> f64 {
    let (lo, hi) = s.bounds();
    if z < lo.z || z > hi.z {
        return 0.0;
    }
    let cell = cell.max(1e-3);
    let nx = (((hi.x - lo.x) / cell).ceil() as usize + 2).min(20_000);
    let ny = (((hi.y - lo.y) / cell).ceil() as usize + 2).min(20_000);
    let cov: f64 = (0..ny)
        .into_par_iter()
        .map(|j| {
            let y = lo.y - cell * 0.5 + (j as f64 + 0.5) * cell;
            (0..nx)
                .map(|i| {
                    let x = lo.x - cell * 0.5 + (i as f64 + 0.5) * cell;
                    (0.5 - s.eval(DVec3::new(x, y, z)) / cell).clamp(0.0, 1.0)
                })
                .sum::<f64>()
        })
        .sum();
    cov * cell * cell
}

/// Volume (mm³) where both solids overlap, by grid sampling at `resolution` mm.
pub fn interference_volume(a: &Document, b: &Document, resolution: f64) -> f64 {
    match (a.sdf(), b.sdf()) {
        (Some(a), Some(b)) => interference_sdf(&a, &b, resolution),
        _ => 0.0,
    }
}

/// Overlap volume of two SDFs (cell-centre sampling inside the intersection of their bounds).
pub fn interference_sdf(a: &Sdf, b: &Sdf, resolution: f64) -> f64 {
    let (a0, a1) = a.bounds();
    let (b0, b1) = b.bounds();
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    if lo.x >= hi.x || lo.y >= hi.y || lo.z >= hi.z {
        return 0.0;
    }
    let h = resolution.max(1e-3);
    let n = ((hi - lo) / h).ceil().as_uvec3().min(glam::UVec3::splat(4000));
    let count: usize = (0..n.z)
        .into_par_iter()
        .map(|k| {
            let mut c = 0;
            for j in 0..n.y {
                for i in 0..n.x {
                    let p = lo + (DVec3::new(i as f64, j as f64, k as f64) + 0.5) * h;
                    if a.eval(p) < 0.0 && b.eval(p) < 0.0 {
                        c += 1;
                    }
                }
            }
            c
        })
        .sum();
    count as f64 * h * h * h
}

/// The overlap volume of two *built* parts (either kernel) each at its own placement, by sampling
/// both distance fields on a grid of cell `resolution` mm over the overlap of their bounds.
pub fn interference_built(
    a: &crate::kernel::Evaluated,
    pa: &crate::document::Placement,
    b: &crate::kernel::Evaluated,
    pb: &crate::document::Placement,
    resolution: f64,
) -> f64 {
    use glam::DMat3;
    let (Some(da), Some(db)) = (a.distance.as_ref(), b.distance.as_ref()) else { return 0.0 };
    let place = |pl: &crate::document::Placement| {
        let r = DMat3::from_quat(glam::DQuat::from_euler(
            glam::EulerRot::XYZ,
            pl.rotate_deg[0].to_radians(),
            pl.rotate_deg[1].to_radians(),
            pl.rotate_deg[2].to_radians(),
        ));
        (r, DVec3::from_array(pl.translate))
    };
    let corners = |e: &crate::kernel::Evaluated, pl: &crate::document::Placement| {
        let (r, t) = place(pl);
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for bd in &e.bodies {
            let (l, h) = (DVec3::from_array(bd.bounds.0), DVec3::from_array(bd.bounds.1));
            for i in 0..8 {
                let c = DVec3::new(if i & 1 == 0 { l.x } else { h.x }, if i & 2 == 0 { l.y } else { h.y }, if i & 4 == 0 { l.z } else { h.z });
                let w = t + r * c;
                lo = lo.min(w);
                hi = hi.max(w);
            }
        }
        (lo, hi)
    };
    let ((a0, a1), (b0, b1)) = (corners(a, pa), corners(b, pb));
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    if lo.x >= hi.x || lo.y >= hi.y || lo.z >= hi.z {
        return 0.0;
    }
    let h = resolution.max(1e-3);
    let n = ((hi - lo) / h).ceil().as_uvec3().min(glam::UVec3::splat(600));
    let ((ra, ta), (rb, tb)) = (place(pa), place(pb));
    let (ia, ib) = (ra.transpose(), rb.transpose());
    let count: usize = (0..n.z)
        .into_par_iter()
        .map(|k| {
            let mut c = 0;
            for j in 0..n.y {
                for i in 0..n.x {
                    let w = lo + (DVec3::new(i as f64, j as f64, k as f64) + 0.5) * h;
                    if da((ia * (w - ta)).to_array()) < 0.0 && db((ib * (w - tb)).to_array()) < 0.0 {
                        c += 1;
                    }
                }
            }
            c
        })
        .sum();
    count as f64 * h * h * h
}

/// What lies between two flat faces of a built part: the distance between their planes when they are
/// parallel, the angle between them, and the distance between their centers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceMeasure {
    pub parallel: bool,
    pub angle_deg: f64,
    /// Along the faces' common normal when parallel, else the distance between the centers.
    pub distance: f64,
    pub center_distance: f64,
}

pub fn measure_faces(ev: &crate::kernel::Evaluated, a: &str, b: &str) -> Option<FaceMeasure> {
    let (fa, fb) = (ev.face_by_name(a)?, ev.face_by_name(b)?);
    let (na, nb) = (DVec3::from_array(fa.direction), DVec3::from_array(fb.direction));
    let (ca, cb) = (DVec3::from_array(fa.centroid), DVec3::from_array(fb.centroid));
    let cos = na.dot(nb).clamp(-1.0, 1.0);
    let angle = cos.acos().to_degrees();
    let parallel = fa.kind == crate::FaceKind::Plane && fb.kind == crate::FaceKind::Plane && (cos.abs() - 1.0).abs() < 1e-3;
    let cd = ca.distance(cb);
    Some(FaceMeasure { parallel, angle_deg: angle, distance: if parallel { (cb - ca).dot(na).abs() } else { cd }, center_distance: cd })
}

/// The geometry of a face that a sketch or a mate can use: a flat face's plane, or the axis of a
/// round one (a hole, a pin, a cone, a fillet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceGeometry {
    /// A flat face: `point` is on it and `dir` is its outward normal.
    pub flat: bool,
    /// A face of revolution: `point` is on its axis and `dir` is along it.
    pub round: bool,
    pub point: [f64; 3],
    pub dir: [f64; 3],
}

/// Eigenvalues (ascending) and eigenvectors of a symmetric 3 x 3 matrix (Jacobi rotations).
fn eigen_sym3(mut a: [[f64; 3]; 3]) -> ([f64; 3], [DVec3; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let (mut p, mut q, mut big) = (0, 1, 0.0f64);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            if a[i][j].abs() > big {
                (p, q, big) = (i, j, a[i][j].abs());
            }
        }
        if big < 1e-15 {
            break;
        }
        let theta = 0.5 * (2.0 * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (s, c) = theta.sin_cos();
        let mut r = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        r[p][p] = c;
        r[q][q] = c;
        r[p][q] = s;
        r[q][p] = -s;
        let mul = |x: &[[f64; 3]; 3], y: &[[f64; 3]; 3]| -> [[f64; 3]; 3] {
            std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| x[i][k] * y[k][j]).sum()))
        };
        let rt: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| r[j][i]));
        a = mul(&mul(&rt, &a), &r);
        v = mul(&v, &r);
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|i, j| a[*i][*i].total_cmp(&a[*j][*j]));
    (order.map(|i| a[i][i]), order.map(|i| DVec3::new(v[0][i], v[1][i], v[2][i])))
}

/// Tell a face's plane or axis from its triangles, whichever kernel built it (both name the
/// same faces). None: the part has no face of that name.
pub fn face_geometry(ev: &crate::kernel::Evaluated, name: &str) -> Option<FaceGeometry> {
    use aurafx_data::DataSet;
    let id = ev.faces().find(|(_, f)| f.name == name)?.1.face_id as i32;
    let DataSet::PolyData(pd) = &ev.surface else { return None };
    let ids = pd.cell_data.get("FaceId")?;
    // Area, unit normal and center of every triangle of the face.
    let mut tris: Vec<(f64, DVec3, DVec3)> = Vec::new();
    for (ci, (_, c)) in pd.polys.iter().enumerate() {
        if c.len() != 3 || ids.get(ci, 0) as i32 != id {
            continue;
        }
        let p: Vec<DVec3> = c.iter().map(|i| DVec3::from_array(pd.points.get(*i as usize))).collect();
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        let a = 0.5 * n.length();
        if a > 0.0 {
            tris.push((a, n.normalize(), (p[0] + p[1] + p[2]) / 3.0));
        }
    }
    let total: f64 = tris.iter().map(|t| t.0).sum();
    if total <= 0.0 {
        return None;
    }
    // Flat: nearly all of its area looks one way. The triangles along its edges may lean (the
    // distance-field kernel cuts corners), so the plane is taken from those that do not.
    // The direction most of its area looks along (triangles on the rim may lean, and may be the
    // largest): the fullest of the buckets the normals fall in.
    let mut buckets: std::collections::HashMap<[i64; 3], (f64, DVec3)> = std::collections::HashMap::new();
    for (a, n, _) in &tris {
        let e = buckets.entry(n.to_array().map(|x| (x * 2e4).round() as i64)).or_insert((0.0, DVec3::ZERO));
        e.0 += a;
        e.1 += *n * *a;
    }
    let n0 = buckets.values().max_by(|a, b| a.0.total_cmp(&b.0)).map(|b| b.1.normalize_or(DVec3::Z)).unwrap_or(DVec3::Z);
    let same: Vec<&(f64, DVec3, DVec3)> = tris.iter().filter(|t| t.1.dot(n0) > 1.0 - 1e-8).collect();
    let area_same: f64 = same.iter().map(|t| t.0).sum();
    // What does not look that way must be the rim (within a couple of triangles of the plane).
    let c0 = same.iter().fold(DVec3::ZERO, |s, t| s + t.2 * t.0) / area_same.max(1e-300);
    let reach = 3.0 * tris.iter().map(|t| t.0).fold(0.0, f64::max).sqrt();
    let rim_only = tris.iter().filter(|t| t.1.dot(n0) <= 1.0 - 1e-6).all(|t| (t.2 - c0).dot(n0).abs() < reach);
    if area_same > 0.35 * total && rim_only {
        let mut n = same.iter().fold(DVec3::ZERO, |s, t| s + t.1 * t.0).normalize_or(n0);
        // A face along an axis is exactly along it.
        for k in 0..3 {
            if n[k].abs() < 1e-6 {
                n[k] = 0.0;
            } else if (n[k].abs() - 1.0).abs() < 1e-6 {
                n[k] = n[k].signum();
            }
        }
        let n = n.normalize_or(n0);
        // The plane's height: the one half of that area is below (a few triangles of the rim sit a
        // little inside the plane), rounded to a nanometre. The point: the center of all of it, put on the plane.
        let mut heights: Vec<(f64, f64)> = same.iter().map(|t| (t.2.dot(n), t.0)).collect();
        heights.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut below = 0.0;
        let mut h = heights[heights.len() / 2].0;
        for (z, a) in &heights {
            below += a;
            if below >= 0.5 * area_same {
                h = *z;
                break;
            }
        }
        let h = (h * 1e6).round() / 1e6;
        let all = tris.iter().fold(DVec3::ZERO, |s, t| s + t.2 * t.0) / total;
        let point = all + n * (h - all.dot(n));
        return Some(FaceGeometry { flat: true, round: false, point: point.to_array(), dir: n.to_array() });
    }
    // Round: the normals of a face of revolution all make one angle with its axis, so about
    // their mean they do not vary along it; and every normal's line meets the axis.
    let spread = |set: &[&(f64, DVec3, DVec3)]| -> ([f64; 3], [DVec3; 3], DVec3) {
        let area: f64 = set.iter().map(|t| t.0).sum::<f64>().max(1e-300);
        let mean = set.iter().fold(DVec3::ZERO, |s, t| s + t.1 * t.0) / area;
        let mut cov = [[0.0; 3]; 3];
        for (a, n, _) in set.iter().map(|t| **t) {
            let d = n - mean;
            for i in 0..3 {
                for j in 0..3 {
                    cov[i][j] += a * d[i] * d[j] / area;
                }
            }
        }
        let (val, vec) = eigen_sym3(cov);
        (val, vec, mean)
    };
    let all: Vec<&(f64, DVec3, DVec3)> = tris.iter().collect();
    let (_, vec, mean) = spread(&all);
    let center = (tris.iter().fold(DVec3::ZERO, |s, t| s + t.2 * t.0) / total).to_array();
    let neither = Some(FaceGeometry { flat: false, round: false, point: center, dir: mean.normalize_or(DVec3::Z).to_array() });
    // The triangles along the face's rim lean out of that pattern (the distance-field kernel
    // cuts corners): they are left out, by how far their angle with the axis is from most.
    let first = vec[0].normalize();
    let mut tilt: Vec<(f64, f64)> = tris.iter().map(|t| (t.1.dot(first), t.0)).collect();
    tilt.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut below, mut median) = (0.0, 0.0);
    for (s, a) in &tilt {
        below += a;
        if below >= 0.5 * total {
            median = *s;
            break;
        }
    }
    let kept: Vec<&(f64, DVec3, DVec3)> = tris.iter().filter(|t| (t.1.dot(first) - median).abs() < 0.08).collect();
    let kept_area: f64 = kept.iter().map(|t| t.0).sum();
    if kept_area < 0.4 * total || kept.len() < 6 {
        return neither;
    }
    let (val, vec, _) = spread(&kept);
    if val[0] > 0.01 * val[2].max(1e-12) || val[1] < 0.01 * val[2].max(1e-12) {
        return neither;
    }
    let mut axis = vec[0].normalize();
    // One of the two directions, always the same: along the larger coordinate.
    let k = (0..3).max_by(|i, j| axis[*i].abs().total_cmp(&axis[*j].abs())).unwrap_or(2);
    if axis[k] < 0.0 {
        axis = -axis;
    }
    for k in 0..3 {
        if axis[k].abs() < 1e-4 {
            axis[k] = 0.0;
        }
    }
    let axis = axis.normalize();
    // The point nearest to all the normals' lines.
    let (mut m, mut rhs) = ([0.0; 9], [0.0; 3]);
    for (a, n, c) in kept.iter().map(|t| **t) {
        for i in 0..3 {
            for j in 0..3 {
                let e = (if i == j { 1.0 } else { 0.0 }) - n[i] * n[j];
                m[i * 3 + j] += a * e;
                rhs[i] += a * e * c[j];
            }
        }
    }
    if !crate::sketch::solve_dense(&mut m, &mut rhs, 3) {
        return None;
    }
    Some(FaceGeometry { flat: false, round: true, point: rhs, dir: axis.to_array() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;

    fn boxp(size: [f64; 3], at: [f64; 3]) -> Document {
        Document::new("b").add(Feature::new("b", BooleanOp::Add, Shape::Box { size, corner_radius: 0.0 }).at(at[0], at[1], at[2]))
    }

    #[test]
    fn box_mass_properties() {
        let p = boxp([20.0, 10.0, 6.0], [5.0, -3.0, 3.0]);
        let mp = p.mass_properties(0.5, 0.7);
        let v = 1200.0;
        let a = 2.0 * (200.0 + 120.0 + 60.0);
        assert!((mp.volume_mm3 - v).abs() / v < 0.03, "{}", mp.volume_mm3);
        assert!((mp.area_mm2 - a).abs() / a < 0.05, "{}", mp.area_mm2);
        assert!((mp.centroid[0] - 5.0).abs() < 0.1 && (mp.centroid[1] + 3.0).abs() < 0.1 && (mp.centroid[2] - 3.0).abs() < 0.1, "{:?}", mp.centroid);
        assert!((mp.mass_g - v / 1000.0 * 0.7).abs() < 0.05);
        assert!((mp.bbox_max[0] - 15.0).abs() < 0.3 && (mp.bbox_min[2]).abs() < 0.3);
        // Exact on an analytic mesh.
        let cube = MassProperties::from_mesh(&TriMesh::cube(glam::Vec3::ZERO, glam::Vec3::new(2.0, 3.0, 4.0)), 1.0);
        assert!((cube.volume_mm3 - 24.0).abs() < 1e-4 && (cube.area_mm2 - 52.0).abs() < 1e-4);
        assert!((cube.centroid[1] - 1.5).abs() < 1e-5);
    }

    #[test]
    fn interference_of_overlapping_boxes() {
        let a = boxp([20.0, 20.0, 20.0], [0.0, 0.0, 0.0]);
        let b = boxp([20.0, 20.0, 20.0], [12.0, 6.0, 0.0]);
        let v = interference_volume(&a, &b, 0.5);
        let expect = 8.0 * 14.0 * 20.0;
        assert!((v - expect).abs() / expect < 0.08, "{v} vs {expect}");
        let far = boxp([5.0, 5.0, 5.0], [100.0, 0.0, 0.0]);
        assert_eq!(interference_volume(&a, &far, 0.5), 0.0);
    }

    #[test]
    fn section_area_of_cylinder_and_cone() {
        let c = Document::new("c").add(Feature::new("c", BooleanOp::Add, Shape::Cylinder { radius: 10.0, height: 20.0 }));
        let a = c.section_area_at_z(5.0, 0.25);
        let e = std::f64::consts::PI * 100.0;
        assert!((a - e).abs() / e < 0.01, "{a}");
        assert_eq!(c.section_area_at_z(30.0, 0.25), 0.0);
    }
}
