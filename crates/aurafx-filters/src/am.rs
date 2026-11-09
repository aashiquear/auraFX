//! Additive-manufacturing analyses of oriented volumes: SROD (each grain's misorientation from
//! the orientation it started with, by height above its base, with the grain's class), and
//! the density of geometrically necessary dislocations (Nye tensor of the lattice curvature,
//! or from a kernel average misorientation).

use crate::features::{image, Vox};
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use rayon::prelude::*;

fn ids_of(img: &ImageData, name: &str) -> Result<Vec<i64>> {
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("no cell array {name} (run Segment Features first)"))?;
    Ok((0..a.len()).map(|i| a.get(i, 0) as i64).collect())
}

/// The cubic (or other) symmetry equivalent of `b` nearest to `a`, sign-aligned with it.
fn nearest(a: Quat, b: Quat, laue: Laue) -> Quat {
    let mut best = (f64::NEG_INFINITY, b);
    for s in laue.operators() {
        let e = cx::mul(*s, b);
        let d = a[0] * e[0] + a[1] * e[1] + a[2] * e[2] + a[3] * e[3];
        if d.abs() > best.0 {
            best = (d.abs(), if d < 0.0 { e.map(|x| -x) } else { e });
        }
    }
    best.1
}

/// Disorientation angle in degrees, as 4 atan2(|a - b'|, |a + b'|) with b' the nearest
/// equivalent (exact near zero).
fn angle_deg(a: Quat, b: Quat, laue: Laue) -> f64 {
    let e = nearest(a, b, laue);
    let (mut dm, mut dp) = (0.0, 0.0);
    for k in 0..4 {
        dm += (a[k] - e[k]).powi(2);
        dp += (a[k] + e[k]).powi(2);
    }
    (4.0 * dm.sqrt().atan2(dp.sqrt())).to_degrees()
}

fn normalise(q: [f64; 4]) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-30);
    q.map(|x| x / n)
}

/// Percentile with linear interpolation (as numpy's default).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let x = p / 100.0 * (sorted.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x - x.floor());
    if i + 1 < sorted.len() {
        sorted[i] + f * (sorted[i + 1] - sorted[i])
    } else {
        sorted[i]
    }
}

pub struct SrodRules {
    pub layer_um: f64,
    pub plate_top: f64,
    pub interface_um: f64,
    pub min_volume: f64,
    pub reference_volume: f64,
    pub reference_sections: usize,
    pub min_bin_volume: f64,
}

pub const CLASSES: [&str; 4] = ["plate", "epitaxial", "interface", "originating"];

/// One bin of one grain.
pub struct SrodBin {
    pub feature: i64,
    pub bin: usize,
    pub height: f64,
    pub n_vox: usize,
    pub srod: f64,
    pub stats: [f64; 6],
    pub class: usize,
}

pub struct Srod {
    /// Per cell: SROD in degrees (NaN outside the grains analysed).
    pub field: Vec<f64>,
    /// Per feature id: (class or -1, base, top, bins, first-bin SROD, largest bin SROD, rate per bin, mean voxel SROD).
    pub grains: Vec<(i32, f64, f64, usize, f64, f64, f64, f64)>,
    pub bins: Vec<SrodBin>,
}

/// SROD of every grain of an oriented volume (z the build direction; heights from the plate
/// top): its reference orientation is the mean of its lowest sections that hold the reference
/// volume (at most `reference_sections` of them); every cell's SROD is its disorientation from
/// that reference; bins of one layer above the grain's lowest section give the SROD of their
/// mean orientation and the spread of their cells; the grain's rate is the slope of the bin
/// SROD over its bins after the first.
pub fn srod(ds: &DataSet, ids_name: &str, rules: &SrodRules) -> Result<Srod> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let o =
        crate::features::Orients::of(ds, None, crate::crystal::FROM_PHASES, None).or_else(|_| crate::features::Orients::of(ds, None, "Cubic m-3m", None))?;
    let laue = o.laue.iter().find(|l| **l != 255).and_then(|l| Laue::from_code(*l as u32)).ok_or_else(|| anyhow!("no indexed orientations"))?;
    let (dx, dy, dz) = (v.sp[0], v.sp[1], v.sp[2]);
    let vox = dx * dy * dz;
    let n = v.n;
    let zc: Vec<f64> = (0..n[2]).map(|k| v.corner[2] + (k as f64 + 0.5) * dz - rules.plate_top).collect();
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let q: Vec<Option<Quat>> = (0..ids.len()).map(|c| if ids[c] > 0 { o.get(c).map(|x| x.0) } else { None }).collect();
    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); nf];
    for (c, f) in ids.iter().enumerate() {
        if *f > 0 && q[c].is_some() {
            cells[*f as usize].push(c);
        }
    }
    let rnd = |x: f64| (x.round() as usize).max(1);
    let (min_vox, ref_vox, bin_vox) = (rnd(rules.min_volume / vox), rnd(rules.reference_volume / vox), rnd(rules.min_bin_volume / vox));
    let nxy = n[0] * n[1];
    let mut field = vec![f64::NAN; ids.len()];
    let mut grains = vec![(-1, f64::NAN, f64::NAN, 0usize, f64::NAN, f64::NAN, f64::NAN, f64::NAN); nf];
    let per: Vec<_> = (1..nf)
        .into_par_iter()
        .filter(|f| cells[*f].len() >= min_vox)
        .map(|f| {
            let cs = &cells[f];
            let kz: Vec<usize> = cs.iter().map(|c| c / nxy).collect();
            let (zmin, zmax) = (*kz.iter().min().unwrap(), *kz.iter().max().unwrap());
            let s = rules.reference_sections.max(1);
            let mut occ = vec![0usize; s];
            for k in &kz {
                if k - zmin < s {
                    occ[k - zmin] += 1;
                }
            }
            for i in 1..s {
                occ[i] += occ[i - 1];
            }
            let kk = if occ[s - 1] >= ref_vox { occ.iter().position(|x| *x >= ref_vox).unwrap() } else { s - 1 };
            let base: Vec<usize> = cs.iter().zip(&kz).filter(|(_, k)| **k <= zmin + kk).map(|(c, _)| *c).collect();
            let q0 = q[base[0]].unwrap();
            let mut sum = [0.0f64; 4];
            for c in &base {
                let e = nearest(q0, q[*c].unwrap(), laue);
                for k in 0..4 {
                    sum[k] += e[k];
                }
            }
            let reference = normalise(sum);
            let (zb, zt) = (zc[zmin], zc[zmax]);
            let class = if zt <= 0.0 {
                0
            } else if zb <= 0.0 {
                1
            } else if zb < rules.interface_um {
                2
            } else {
                3
            };
            // Cells: SROD and the equivalent nearest the reference; bins above the base.
            let mut vals = Vec::with_capacity(cs.len());
            let mut bins: std::collections::BTreeMap<usize, (Vec<f64>, [f64; 4])> = Default::default();
            for (c, k) in cs.iter().zip(&kz) {
                let g = q[*c].unwrap();
                let e = nearest(reference, g, laue);
                let a = angle_deg(reference, g, laue);
                vals.push((*c, a));
                let b = ((zc[*k] - zb) / rules.layer_um + 1e-9).floor().max(0.0) as usize;
                let entry = bins.entry(b).or_insert((Vec::new(), [0.0; 4]));
                entry.0.push(a);
                for i in 0..4 {
                    entry.1[i] += e[i];
                }
            }
            let mut out_bins = Vec::new();
            for (b, (mut a, qs)) in bins {
                if a.len() < bin_vox {
                    continue;
                }
                let s = angle_deg(reference, normalise(qs), laue);
                let mean = a.iter().sum::<f64>() / a.len() as f64;
                a.sort_by(|x, y| x.total_cmp(y));
                out_bins.push(SrodBin {
                    feature: f as i64,
                    bin: b,
                    height: zb + (b as f64 + 0.5) * rules.layer_um,
                    n_vox: a.len(),
                    srod: s,
                    stats: [mean, percentile(&a, 10.0), percentile(&a, 25.0), percentile(&a, 50.0), percentile(&a, 75.0), percentile(&a, 90.0)],
                    class,
                });
            }
            let nb = out_bins.len();
            let first = out_bins.iter().find(|b| b.bin == 0).map(|b| b.srod).unwrap_or(f64::NAN);
            let max = out_bins.iter().map(|b| b.srod).fold(0.0, f64::max);
            // Rate: least-squares slope of SROD on the bin index over bins >= 1 (at least 3).
            let pts: Vec<(f64, f64)> = out_bins.iter().filter(|b| b.bin >= 1).map(|b| (b.bin as f64, b.srod)).collect();
            let rate = if nb >= 3 && pts.len() >= 3 {
                let m = pts.len() as f64;
                let (sx, sy) = (pts.iter().map(|p| p.0).sum::<f64>() / m, pts.iter().map(|p| p.1).sum::<f64>() / m);
                let sxy: f64 = pts.iter().map(|p| (p.0 - sx) * (p.1 - sy)).sum();
                let sxx: f64 = pts.iter().map(|p| (p.0 - sx) * (p.0 - sx)).sum();
                sxy / sxx
            } else {
                f64::NAN
            };
            let vmean = vals.iter().map(|x| x.1).sum::<f64>() / vals.len() as f64;
            (f, (class as i32, zb, zt, nb, first, max, rate, vmean), vals, out_bins)
        })
        .collect();
    let mut all_bins = Vec::new();
    for (f, g, vals, b) in per {
        grains[f] = g;
        for (c, a) in vals {
            field[c] = a;
        }
        all_bins.extend(b);
    }
    all_bins.sort_by(|a, b| (a.feature, a.bin).cmp(&(b.feature, b.bin)));
    Ok(Srod { field, grains, bins: all_bins })
}

pub const OUT_VOLUME: &str = "Volume";
pub const OUT_BINS: &str = "Bins";
pub const OUT_HEIGHT: &str = "By height";

/// The SROD filter's outputs.
pub fn srod_filter(ds: &DataSet, ids_name: &str, rules: &SrodRules, output: &str) -> Result<DataSet> {
    let r = srod(ds, ids_name, rules)?;
    let col = |n: &str, v: Vec<f64>| Array::scalars_f64(n, v);
    match output {
        OUT_BINS => {
            let mut t = Table::default();
            let b = &r.bins;
            t.columns.insert(Array::new("Feature", 1, ArrayData::I32(b.iter().map(|x| x.feature as i32).collect())));
            t.columns.insert(Array::new("Bin", 1, ArrayData::I32(b.iter().map(|x| x.bin as i32).collect())));
            t.columns.insert(Array::new("Class", 1, ArrayData::String(b.iter().map(|x| CLASSES[x.class].to_string()).collect())));
            t.columns.insert(col("Height", b.iter().map(|x| x.height).collect()));
            t.columns.insert(col("SROD", b.iter().map(|x| x.srod).collect()));
            t.columns.insert(col("Cells", b.iter().map(|x| x.n_vox as f64).collect()));
            for (k, n) in ["CellMean", "P10", "P25", "P50", "P75", "P90"].iter().enumerate() {
                t.columns.insert(col(n, b.iter().map(|x| x.stats[k]).collect()));
            }
            Ok(DataSet::Table(t))
        }
        OUT_HEIGHT => {
            // Mean bin SROD by height above the plate (one layer per row) and class.
            let top = r.bins.iter().map(|b| b.height).fold(0.0, f64::max);
            let nh = ((top / rules.layer_um).floor() as usize + 1).max(1);
            let mut sum = vec![[0.0; 4]; nh];
            let mut cnt = vec![[0usize; 4]; nh];
            for b in &r.bins {
                if b.height < 0.0 {
                    continue;
                }
                let h = ((b.height / rules.layer_um) as usize).min(nh - 1);
                sum[h][b.class] += b.srod;
                cnt[h][b.class] += 1;
            }
            let mut t = Table::default();
            t.columns.insert(col("bin_center", (0..nh).map(|h| (h as f64 + 0.5) * rules.layer_um).collect()));
            for (k, name) in CLASSES.iter().enumerate().skip(1) {
                t.columns.insert(col(&format!("SROD {name}"), (0..nh).map(|h| if cnt[h][k] > 0 { sum[h][k] / cnt[h][k] as f64 } else { f64::NAN }).collect()));
                t.columns.insert(col(&format!("Grains {name}"), (0..nh).map(|h| cnt[h][k] as f64).collect()));
            }
            Ok(DataSet::Table(t))
        }
        _ => {
            let mut out = image(ds)?.clone();
            out.cell_data.insert(col("SROD", r.field));
            let g = &r.grains;
            out.field_data.insert(Array::new("GrainClass", 1, ArrayData::I32(g.iter().map(|x| x.0).collect())));
            out.field_data.insert(col("GrainBase", g.iter().map(|x| x.1).collect()));
            out.field_data.insert(col("GrainTop", g.iter().map(|x| x.2).collect()));
            out.field_data.insert(col("SRODBins", g.iter().map(|x| x.3 as f64).collect()));
            out.field_data.insert(col("FirstBinSROD", g.iter().map(|x| x.4).collect()));
            out.field_data.insert(col("MaxBinSROD", g.iter().map(|x| x.5).collect()));
            out.field_data.insert(col("SRODRate", g.iter().map(|x| x.6).collect()));
            out.field_data.insert(col("MeanCellSROD", g.iter().map(|x| x.7).collect()));
            let rates: Vec<f64> = g.iter().zip(0..).filter(|(x, _)| x.0 == 3 && x.6.is_finite()).map(|(x, _)| x.6).collect();
            let mut sorted = rates.clone();
            sorted.sort_by(|a, b| a.total_cmp(b));
            out.field_data.insert(col("MedianOriginatingRate", vec![percentile(&sorted, 50.0)]));
            Ok(DataSet::ImageData(out))
        }
    }
}

// ------------------------------------------------------------------ GND

/// Nye-tensor GND density of every cell inside a grain: the least-squares lattice curvature
/// K (rotation vectors to the same-grain cells of a `kernel`-wide neighbourhood against their
/// offsets, with an intercept), Nye's tensor alpha = K^T - tr(K) I, and rho = sum |alpha_ij| / b
/// (1/m^2; lengths in micrometres). Cells with fewer than `min_neighbours` neighbours or whose
/// neighbours do not span three directions get NaN. On a 2-D map (one layer of cells) the
/// curvature across the map is unknown: the five components of alpha a map gives (the three in
/// the map's normal column, the two off-diagonal ones in the plane) are summed, a lower bound.
pub fn nye_gnd(ds: &DataSet, ids_name: &str, burgers_m: f64, kernel: usize, min_neighbours: usize) -> Result<(Vec<f64>, Vec<f64>)> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let o =
        crate::features::Orients::of(ds, None, crate::crystal::FROM_PHASES, None).or_else(|_| crate::features::Orients::of(ds, None, "Cubic m-3m", None))?;
    let laue = o.laue.iter().find(|l| **l != 255).and_then(|l| Laue::from_code(*l as u32)).ok_or_else(|| anyhow!("no indexed orientations"))?;
    if kernel < 3 || kernel % 2 == 0 {
        bail!("the kernel must be an odd number of cells, 3 or more");
    }
    let r = (kernel / 2) as i64;
    let n = v.n;
    let sp = v.sp;
    // A flat axis (a 2-D map): offsets have no component along it.
    let flat: Option<usize> = (0..3).find(|&a| n[a] == 1);
    let eig_tol = 0.5 * sp.iter().cloned().fold(f64::INFINITY, f64::min).powi(2);
    let q: Vec<Option<Quat>> = (0..ids.len()).map(|c| if ids[c] > 0 { o.get(c).map(|x| x.0) } else { None }).collect();
    let scale = 1e6 / burgers_m;
    let res: Vec<(f64, f64)> = (0..ids.len())
        .into_par_iter()
        .map(|c| {
            let (Some(qc), f) = (q[c], ids[c]) else { return (f64::NAN, f64::NAN) };
            if f <= 0 {
                return (f64::NAN, f64::NAN);
            }
            let p = v.ijk(c);
            let (mut a, mut b) = ([[0.0f64; 3]; 3], [[0.0f64; 3]; 3]);
            let (mut sx, mut st) = ([0.0f64; 3], [0.0f64; 3]);
            let mut cnt = 0usize;
            for dzz in -r..=r {
                for dyy in -r..=r {
                    for dxx in -r..=r {
                        if (dxx, dyy, dzz) == (0, 0, 0) {
                            continue;
                        }
                        let w = [p[0] as i64 + dxx, p[1] as i64 + dyy, p[2] as i64 + dzz];
                        if (0..3).any(|k| w[k] < 0 || w[k] >= n[k] as i64) {
                            continue;
                        }
                        let d = w[0] as usize + n[0] * (w[1] as usize + n[1] * w[2] as usize);
                        if ids[d] != f {
                            continue;
                        }
                        let Some(qn) = q[d] else { continue };
                        let e = nearest(qc, qn, laue);
                        // Rotation vector of conj(qc) e (sample frame).
                        let mut dq = cx::mul(cx::conj(qc), e);
                        if dq[0] < 0.0 {
                            dq = dq.map(|x| -x);
                        }
                        let vv = [dq[1], dq[2], dq[3]];
                        let nv = (vv[0] * vv[0] + vv[1] * vv[1] + vv[2] * vv[2]).sqrt();
                        let ang = 2.0 * nv.atan2(dq[0]);
                        let th = vv.map(|x| x * ang / nv.max(1e-300));
                        let x = [dxx as f64 * sp[0], dyy as f64 * sp[1], dzz as f64 * sp[2]];
                        for i in 0..3 {
                            for j in 0..3 {
                                a[i][j] += x[i] * x[j];
                                b[i][j] += th[i] * x[j];
                            }
                            sx[i] += x[i];
                            st[i] += th[i];
                        }
                        cnt += 1;
                    }
                }
            }
            if cnt < min_neighbours {
                return (f64::NAN, f64::NAN);
            }
            let n1 = cnt as f64 + 1.0;
            let xm = sx.map(|x| x / n1);
            let tm = st.map(|x| x / n1);
            let mut ac: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| a[i][j] - n1 * xm[i] * xm[j]));
            let bc: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| b[i][j] - n1 * tm[i] * xm[j]));
            if let Some(fa) = flat {
                // Solve in the plane: a unit entry on the flat axis keeps A invertible, its column of
                // K comes out zero and is not used below.
                ac[fa][fa] = 1.0;
            }
            let (ev, _) = crate::features::sym_eigen(ac);
            if ev.iter().cloned().fold(f64::INFINITY, f64::min) <= eig_tol {
                return (f64::NAN, f64::NAN);
            }
            let inv = match inv3(ac) {
                Some(m) => m,
                None => return (f64::NAN, f64::NAN),
            };
            // K = B A^-1.
            let k: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|l| bc[i][l] * inv[l][j]).sum()));
            let tr = k[0][0] + k[1][1] + k[2][2];
            let mut l1 = 0.0;
            let mut fro = 0.0;
            for i in 0..3 {
                for j in 0..3 {
                    // alpha_ij = K_ji - delta_ij tr(K); on a map K's column of the flat axis is unknown.
                    let al = match flat {
                        Some(fa) if i == j && i == fa => -(0..3).filter(|&m| m != fa).map(|m| k[m][m]).sum::<f64>(),
                        Some(fa) if i == j || i == fa => continue,
                        _ => k[j][i] - if i == j { tr } else { 0.0 },
                    };
                    l1 += al.abs();
                    fro += al * al;
                }
            }
            (l1 * scale, fro.sqrt() * scale)
        })
        .collect();
    Ok((res.iter().map(|x| x.0).collect(), res.iter().map(|x| x.1).collect()))
}

fn inv3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-300 {
        return None;
    }
    let c = |i: usize, j: usize| {
        let (r0, r1) = ((i + 1) % 3, (i + 2) % 3);
        let (c0, c1) = ((j + 1) % 3, (j + 2) % 3);
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    };
    Some(std::array::from_fn(|i| std::array::from_fn(|j| c(j, i) / det)))
}

pub const GND_NYE: &str = "Nye tensor";
pub const GND_KAM: &str = "From KAM";

/// GND Density: per cell (1/m^2), by the Nye tensor or from a KAM array (degrees):
/// rho = KAM (rad) / (step b).
pub fn gnd_density(ds: &DataSet, ids_name: &str, method: &str, burgers_m: f64, kernel: usize, kam: &str, step: f64) -> Result<DataSet> {
    let img = image(ds)?;
    let rho: Vec<f64> = if method == GND_KAM {
        let a = img.cell_data.get(kam).ok_or_else(|| anyhow!("no cell array {kam} (run Kernel Average Misorientation first)"))?;
        let d = if step > 0.0 { step } else { img.spacing.iter().map(|x| x.abs()).fold(f64::INFINITY, f64::min) };
        (0..a.len()).map(|i| a.get(i, 0).to_radians() / (d * 1e-6 * burgers_m)).collect()
    } else {
        nye_gnd(ds, ids_name, burgers_m, kernel, 6)?.0
    };
    let mut sorted: Vec<f64> = rho.iter().copied().filter(|x| x.is_finite()).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mean = if sorted.is_empty() { f64::NAN } else { sorted.iter().sum::<f64>() / sorted.len() as f64 };
    let mut out = img.clone();
    out.cell_data.insert(Array::scalars_f64("GND", rho));
    out.field_data.insert(Array::scalars_f64("MeanGND", vec![mean]));
    out.field_data.insert(Array::scalars_f64("MedianGND", vec![percentile(&sorted, 50.0)]));
    out.field_data.insert(Array::scalars_f64("P90GND", vec![percentile(&sorted, 90.0)]));
    Ok(DataSet::ImageData(out))
}
