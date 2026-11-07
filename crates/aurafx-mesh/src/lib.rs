//! Geometry kernel for auraFX's mesh generator: implicit solids with named faces, CSG, 2-D
//! outlines extruded, revolved or swept, closed triangle surfaces as signed distance fields,
//! and the geometry command language (`model`).

pub mod bodymesh;
pub mod hex;
pub mod improve;
pub mod inclusions;
pub mod model;
pub mod outline;
pub mod overlay;
pub mod quality;
pub mod shape;
pub mod specimens;
pub mod structured;
pub mod stuffing;
pub mod tet;
pub mod trimesh;

pub use model::{Body, History, Model};
pub use outline::Outline;
pub use shape::{FaceId, Shape, V3};
pub use trimesh::TriMesh;

use rayon::prelude::*;

/// Volume and surface area of a solid, integrated over cells of size `h` (a smoothed step
/// for the volume, a smoothed surface delta times the distance gradient for the area). Errors
/// shrink with h squared for smooth surfaces.
pub fn measure(shape: &Shape, h: f64) -> (f64, f64) {
    let (lo, hi) = shape.bounds();
    let lo: V3 = std::array::from_fn(|k| lo[k] - 2.0 * h);
    let n: [usize; 3] = std::array::from_fn(|k| ((hi[k] + 2.0 * h - lo[k]) / h).ceil() as usize);
    let cell = h * h * h;
    (0..n[2])
        .into_par_iter()
        .map(|k| {
            let (mut v, mut a) = (0.0, 0.0);
            for j in 0..n[1] {
                for i in 0..n[0] {
                    let p = [lo[0] + (i as f64 + 0.5) * h, lo[1] + (j as f64 + 0.5) * h, lo[2] + (k as f64 + 0.5) * h];
                    let d = shape.eval(p).0;
                    v += (0.5 - d / h).clamp(0.0, 1.0);
                    if d.abs() < h {
                        // The larger of the one-sided gradients: both are unit length for an
                        // exact distance, even on the kinks along edges.
                        let e = h * 1e-3;
                        let (mut gf, mut gb) = (0.0, 0.0);
                        for c in 0..3 {
                            let mut q = p;
                            let mut r = p;
                            q[c] += e;
                            r[c] -= e;
                            gf += ((shape.eval(q).0 - d) / e).powi(2);
                            gb += ((d - shape.eval(r).0) / e).powi(2);
                        }
                        a += (1.0 - d.abs() / h) / h * gf.max(gb).sqrt();
                    }
                }
            }
            (v * cell, a * cell)
        })
        .reduce(|| (0.0, 0.0), |x, y| (x.0 + y.0, x.1 + y.1))
}
