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
pub fn interference_built(a: &crate::kernel::Evaluated, pa: &crate::document::Placement, b: &crate::kernel::Evaluated, pb: &crate::document::Placement, resolution: f64) -> f64 {
    use glam::DMat3;
    let (Some(da), Some(db)) = (a.distance.as_ref(), b.distance.as_ref()) else { return 0.0 };
    let place = |pl: &crate::document::Placement| {
        let r = DMat3::from_quat(glam::DQuat::from_euler(glam::EulerRot::XYZ, pl.rotate_deg[0].to_radians(), pl.rotate_deg[1].to_radians(), pl.rotate_deg[2].to_radians()));
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
