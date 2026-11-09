//! Point sets: points sampled on a surface (spread by area), points kept inside a box, and the
//! rigid alignment of one point set onto another (iterative closest point).

use anyhow::{anyhow, bail, Result};
use aurafx_data::kdtree::KdTree;
use aurafx_data::*;

fn all_points(ds: &DataSet) -> Result<Vec<[f64; 3]>> {
    let mut out = Vec::new();
    let leaves: Vec<&DataSet> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|l| l.1).collect(),
        d => vec![d],
    };
    for l in leaves {
        if let Some(p) = l.points() {
            out.extend((0..p.len()).map(|i| p.get(i)));
        }
    }
    if out.is_empty() {
        bail!("no points");
    }
    Ok(out)
}

fn cloud(pts: Vec<[f64; 3]>) -> PolyData {
    let mut pd = PolyData::from_points(Points::new_f64(pts.iter().flatten().copied().collect()));
    for i in 0..pts.len() as u32 {
        pd.verts.push(CellType::Vertex, &[i]);
    }
    pd
}

/// Point Sample Triangle Geometry: `n` points spread over the triangles of a surface, each
/// triangle getting points in proportion to its area (seeded).
pub fn sample_surface(ds: &DataSet, n: usize, seed: u64) -> Result<PolyData> {
    let DataSet::PolyData(pd) = ds else { bail!("Point Sample Triangle Geometry takes a surface (Extract Surface first)") };
    let mut tris: Vec<[[f64; 3]; 3]> = Vec::new();
    for (_, c) in pd.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            tris.push([pd.points.get(c[0] as usize), pd.points.get(c[k] as usize), pd.points.get(c[k + 1] as usize)]);
        }
    }
    if tris.is_empty() {
        bail!("the surface has no polygons");
    }
    let area = |t: &[[f64; 3]; 3]| {
        let (u, v) = ([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
        let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    };
    let mut cum = Vec::with_capacity(tris.len());
    let mut s = 0.0;
    for t in &tris {
        s += area(t);
        cum.push(s);
    }
    let mut rng = crate::stats::Rng::new(seed);
    let pts: Vec<[f64; 3]> = (0..n)
        .map(|_| {
            let x = rng.next_f64() * s;
            let t = &tris[cum.partition_point(|&c| c < x).min(tris.len() - 1)];
            let (mut a, mut b) = (rng.next_f64(), rng.next_f64());
            if a + b > 1.0 {
                a = 1.0 - a;
                b = 1.0 - b;
            }
            std::array::from_fn(|k| t[0][k] + a * (t[1][k] - t[0][k]) + b * (t[2][k] - t[0][k]))
        })
        .collect();
    Ok(cloud(pts))
}

/// Crop points: the points inside a box (their point arrays kept).
pub fn crop_points(ds: &DataSet, lo: [f64; 3], hi: [f64; 3]) -> Result<PolyData> {
    let p = ds.points().ok_or_else(|| anyhow!("no points"))?;
    let keep: Vec<usize> = (0..p.len())
        .filter(|&i| {
            let q = p.get(i);
            (0..3).all(|a| q[a] >= lo[a] && q[a] <= hi[a])
        })
        .collect();
    let mut out = cloud(keep.iter().map(|&i| p.get(i)).collect());
    if let Some(pd) = ds.point_data() {
        out.point_data = pd.gather(&keep);
    }
    Ok(out)
}

/// Iterative Closest Point: the rigid motion (rotation and translation) that brings the `moving`
/// points onto the `target` points: each round pairs every moving point with its nearest target
/// point and solves for the best rigid motion of the pairs (Horn's quaternion method), until it
/// stops improving or `iterations` rounds. Returns the moved points (with their arrays), the
/// 4 x 4 transform (row by row) and the RMS distance as field data.
pub fn icp(moving: &DataSet, target: &DataSet, iterations: usize) -> Result<DataSet> {
    let tgt = all_points(target)?;
    let mut cur = all_points(moving)?;
    let tree = KdTree::new(&tgt);
    let mut total_r = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut total_t = [0.0; 3];
    let mut prev = f64::INFINITY;
    let mut rms = 0.0;
    for _ in 0..iterations.max(1) {
        let pairs: Vec<[f64; 3]> = cur.iter().map(|&p| tgt[tree.nearest_one(p).unwrap().0]).collect();
        rms = (cur.iter().zip(&pairs).map(|(a, b)| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>()).sum::<f64>() / cur.len() as f64).sqrt();
        if prev - rms < 1e-12 * (1.0 + rms) {
            break;
        }
        prev = rms;
        let (r, t) = best_rigid(&cur, &pairs);
        for p in cur.iter_mut() {
            *p = std::array::from_fn(|i| r[i][0] * p[0] + r[i][1] * p[1] + r[i][2] * p[2] + t[i]);
        }
        // Compose: total = (r, t) after (total_r, total_t).
        let nr: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| r[i][k] * total_r[k][j]).sum()));
        let nt: [f64; 3] = std::array::from_fn(|i| (0..3).map(|k| r[i][k] * total_t[k]).sum::<f64>() + t[i]);
        total_r = nr;
        total_t = nt;
    }
    let mut out = cloud(cur);
    if let Some(pd) = moving.point_data() {
        out.point_data = pd.clone();
    }
    let m: Vec<f64> = (0..4)
        .flat_map(|i| (0..4).map(move |j| (i, j)))
        .map(|(i, j)| {
            if i < 3 && j < 3 {
                total_r[i][j]
            } else if i < 3 {
                total_t[i]
            } else if j == 3 {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    out.field_data.insert(Array::scalars_f64("Transform", m));
    out.field_data.insert(Array::scalars_f64("RMSDistance", vec![rms]));
    Ok(DataSet::PolyData(out))
}

/// The rotation and translation that best map `a` onto `b` (least squares; Horn 1987).
fn best_rigid(a: &[[f64; 3]], b: &[[f64; 3]]) -> ([[f64; 3]; 3], [f64; 3]) {
    let n = a.len() as f64;
    let ca: [f64; 3] = std::array::from_fn(|k| a.iter().map(|p| p[k]).sum::<f64>() / n);
    let cb: [f64; 3] = std::array::from_fn(|k| b.iter().map(|p| p[k]).sum::<f64>() / n);
    let mut s = [[0.0; 3]; 3];
    for (p, q) in a.iter().zip(b) {
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += (p[i] - ca[i]) * (q[j] - cb[j]);
            }
        }
    }
    let (sxx, sxy, sxz, syx, syy, syz, szx, szy, szz) = (s[0][0], s[0][1], s[0][2], s[1][0], s[1][1], s[1][2], s[2][0], s[2][1], s[2][2]);
    let nmat = [
        [sxx + syy + szz, syz - szy, szx - sxz, sxy - syx],
        [syz - szy, sxx - syy - szz, sxy + syx, szx + sxz],
        [szx - sxz, sxy + syx, -sxx + syy - szz, syz + szy],
        [sxy - syx, szx + sxz, syz + szy, -sxx - syy + szz],
    ];
    // The eigenvector of the largest eigenvalue, by power iteration on N + c I.
    let shift = nmat.iter().flatten().map(|x| x.abs()).sum::<f64>() + 1.0;
    let mut q = [1.0, 0.0, 0.0, 0.0];
    for _ in 0..500 {
        let mut nq = [0.0; 4];
        for i in 0..4 {
            nq[i] = (0..4).map(|j| nmat[i][j] * q[j]).sum::<f64>() + shift * q[i];
        }
        let l = nq.iter().map(|x| x * x).sum::<f64>().sqrt();
        q = nq.map(|x| x / l);
    }
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    let r = [
        [w * w + x * x - y * y - z * z, 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), w * w - x * x + y * y - z * z, 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), w * w - x * x - y * y + z * z],
    ];
    let t: [f64; 3] = std::array::from_fn(|i| cb[i] - (0..3).map(|k| r[i][k] * ca[k]).sum::<f64>());
    (r, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icp_recovers_a_rigid_motion() {
        // An asymmetric cloud, turned 10 degrees about z and moved; ICP brings it back.
        let mut rng = crate::stats::Rng::new(5);
        let pts: Vec<[f64; 3]> = (0..400).map(|_| [rng.next_f64() * 3.0, rng.next_f64() * 2.0, rng.next_f64()]).collect();
        let (s, c) = 10f64.to_radians().sin_cos();
        let moved: Vec<[f64; 3]> = pts.iter().map(|p| [c * p[0] - s * p[1] + 0.1, s * p[0] + c * p[1] - 0.05, p[2] + 0.02]).collect();
        let out = icp(&DataSet::PolyData(cloud(moved)), &DataSet::PolyData(cloud(pts.clone())), 100).unwrap();
        assert!(out.field_data().get("RMSDistance").unwrap().get(0, 0) < 1e-6);
        let DataSet::PolyData(o) = &out else { panic!() };
        assert!((0..400).all(|i| (0..3).all(|k| (o.points.get(i)[k] - pts[i][k]).abs() < 1e-6)));
    }

    #[test]
    fn samples_and_crops() {
        // A unit square of two triangles: points spread evenly.
        let mut sq = PolyData::default();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]] {
            sq.points.push(p);
        }
        sq.polys.push(CellType::Quad, &[0, 1, 2, 3]);
        let s = sample_surface(&DataSet::PolyData(sq), 4000, 3).unwrap();
        let left = (0..s.points.len()).filter(|&i| s.points.get(i)[0] < 0.5).count();
        assert!((left as f64 / 4000.0 - 0.5).abs() < 0.03, "{left}");
        let c = crop_points(&DataSet::PolyData(s), [0.0; 3], [0.5, 0.5, 1.0]).unwrap();
        assert!((c.points.len() as f64 / 4000.0 - 0.25).abs() < 0.03);
    }
}
