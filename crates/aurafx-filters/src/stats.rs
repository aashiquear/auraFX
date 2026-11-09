//! Statistics over arrays: k-means clusters, principal components, means with covariance and
//! correlation, contingency tables of two arrays, quartiles (for box plots), and random arrays.

use crate::params::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

/// The values of the variables `names` ("T", "V" for a magnitude, "V[1]" for a component; empty:
/// every numeric array with one component) over points or cells, as columns.
pub fn variables(ds: &DataSet, assoc: Association, names: &[String]) -> Result<(Vec<String>, Vec<Vec<f64>>)> {
    let attrs = attrs_of(ds, assoc).ok_or_else(|| anyhow!("no {} arrays", if assoc == Association::Points { "point" } else { "cell" }))?;
    let wanted: Vec<String> = if names.iter().all(|n| n.trim().is_empty()) {
        attrs.iter().filter(|a| a.ncomp == 1 && a.scalar_type() != ScalarType::String && !a.name.starts_with("vtk")).map(|a| a.name.clone()).collect()
    } else {
        names.iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()
    };
    if wanted.is_empty() {
        bail!("no numeric arrays to use");
    }
    let mut cols = Vec::new();
    for w in &wanted {
        let (base, comp) = match w.strip_suffix(']').and_then(|s| s.split_once('[')) {
            Some((b, c)) => (b.to_string(), Some(c.parse::<usize>().map_err(|_| anyhow!("{w:?}: the component must be a number"))?)),
            None => (w.clone(), None),
        };
        let a = attrs.get(&base).ok_or_else(|| anyhow!("no array named {base:?}"))?;
        cols.push(
            (0..a.len())
                .map(|i| match comp {
                    Some(c) => a.get(i, c.min(a.ncomp - 1)),
                    None if a.ncomp == 1 => a.get(i, 0),
                    None => a.magnitude(i),
                })
                .collect(),
        );
    }
    Ok((wanted, cols))
}

fn attrs_of(ds: &DataSet, assoc: Association) -> Option<&Attributes> {
    match ds {
        DataSet::Table(t) => Some(&t.columns),
        d => match assoc {
            Association::Points => d.point_data(),
            Association::Cells => d.cell_data(),
        },
    }
}

fn attrs_mut(ds: &mut DataSet, assoc: Association) -> Option<&mut Attributes> {
    match ds {
        DataSet::Table(t) => Some(&mut t.columns),
        d => match assoc {
            Association::Points => d.point_data_mut(),
            Association::Cells => d.cell_data_mut(),
        },
    }
}

fn mean_std(v: &[f64]) -> (f64, f64) {
    let n = v.len().max(1) as f64;
    let m = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    (m, var.sqrt())
}

/// A small deterministic generator (xorshift64*), the same on every platform.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// k-means clustering of the rows of `cols` (k-means++ start, Lloyd iterations). Returns the
/// cluster of each row, the centers (k x variables) and the within-cluster sum of squares.
pub fn kmeans(cols: &[Vec<f64>], k: usize, max_iter: usize, seed: u64) -> (Vec<usize>, Vec<Vec<f64>>, f64) {
    let n = cols.first().map(|c| c.len()).unwrap_or(0);
    let d = cols.len();
    let k = k.clamp(1, n.max(1));
    let row = |i: usize| -> Vec<f64> { cols.iter().map(|c| c[i]).collect() };
    let dist2 = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f64>();
    if n == 0 {
        return (Vec::new(), Vec::new(), 0.0);
    }
    let mut rng = Rng::new(seed);
    let mut centers: Vec<Vec<f64>> = vec![row((rng.next_f64() * n as f64) as usize % n)];
    let mut dmin: Vec<f64> = (0..n).map(|i| dist2(&row(i), &centers[0])).collect();
    while centers.len() < k {
        let total: f64 = dmin.iter().sum();
        let pick = if total <= 0.0 {
            (rng.next_f64() * n as f64) as usize % n
        } else {
            let mut r = rng.next_f64() * total;
            let mut j = n - 1;
            for (i, &x) in dmin.iter().enumerate() {
                if r < x {
                    j = i;
                    break;
                }
                r -= x;
            }
            j
        };
        let c = row(pick);
        for (i, m) in dmin.iter_mut().enumerate() {
            *m = m.min(dist2(&row(i), &c));
        }
        centers.push(c);
    }
    let mut label = vec![0usize; n];
    for _ in 0..max_iter.max(1) {
        let mut changed = false;
        for (i, l) in label.iter_mut().enumerate() {
            let r = row(i);
            let best = (0..k).min_by(|&a, &b| dist2(&r, &centers[a]).total_cmp(&dist2(&r, &centers[b]))).unwrap();
            if best != *l {
                *l = best;
                changed = true;
            }
        }
        let mut sum = vec![vec![0.0; d]; k];
        let mut cnt = vec![0usize; k];
        for i in 0..n {
            cnt[label[i]] += 1;
            for (v, c) in cols.iter().enumerate() {
                sum[label[i]][v] += c[i];
            }
        }
        for c in 0..k {
            if cnt[c] > 0 {
                centers[c] = sum[c].iter().map(|s| s / cnt[c] as f64).collect();
            }
        }
        if !changed {
            break;
        }
    }
    let wss = (0..n).map(|i| dist2(&row(i), &centers[label[i]])).sum();
    (label, centers, wss)
}

/// Covariance matrix of the columns.
pub fn covariance(cols: &[Vec<f64>]) -> (Vec<f64>, Vec<Vec<f64>>) {
    let d = cols.len();
    let n = cols.first().map(|c| c.len()).unwrap_or(0);
    let means: Vec<f64> = cols.iter().map(|c| c.iter().sum::<f64>() / n.max(1) as f64).collect();
    let mut cov = vec![vec![0.0; d]; d];
    for a in 0..d {
        for b in a..d {
            let s: f64 = (0..n).map(|i| (cols[a][i] - means[a]) * (cols[b][i] - means[b])).sum::<f64>() / (n as f64 - 1.0).max(1.0);
            cov[a][b] = s;
            cov[b][a] = s;
        }
    }
    (means, cov)
}

/// Eigenvalues (largest first) and unit eigenvectors (as rows) of a symmetric matrix (Jacobi).
pub fn symmetric_eigen(m: &[Vec<f64>]) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = m.len();
    let mut a: Vec<Vec<f64>> = m.to_vec();
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for _ in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        if off < 1e-30 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&x, &y| a[y][y].total_cmp(&a[x][x]));
    let vals = order.iter().map(|&i| a[i][i]).collect();
    let vecs = order
        .iter()
        .map(|&i| {
            let mut e: Vec<f64> = (0..n).map(|k| v[k][i]).collect();
            // A fixed sign: the largest component positive.
            let big = e.iter().copied().fold(0.0f64, |m, x| if x.abs() > m.abs() { x } else { m });
            if big < 0.0 {
                e.iter_mut().for_each(|x| *x = -*x);
            }
            e
        })
        .collect();
    (vals, vecs)
}

fn standardised(cols: &[Vec<f64>], on: bool) -> Vec<Vec<f64>> {
    if !on {
        return cols.to_vec();
    }
    cols.iter()
        .map(|c| {
            let (m, s) = mean_std(c);
            let s = if s > 0.0 { s } else { 1.0 };
            c.iter().map(|x| (x - m) / s).collect()
        })
        .collect()
}

/// K-Means filter: `ClusterId` and `ClusterDistance` on the data; field data with the centers
/// (in the variables' own units) and the within-cluster sum of squares.
pub fn kmeans_filter(ds: &DataSet, assoc: Association, names: &[String], k: usize, max_iter: usize, standardise: bool, seed: u64) -> Result<DataSet> {
    let (vars, cols) = variables(ds, assoc, names)?;
    let use_cols = standardised(&cols, standardise);
    let (label, _, wss) = kmeans(&use_cols, k, max_iter, seed);
    let k = k.clamp(1, label.len().max(1));
    let mut out = ds.clone();
    // Centers and distances in the variables' units.
    let mut centers = vec![vec![0.0; vars.len()]; k];
    let mut cnt = vec![0usize; k];
    for (i, &l) in label.iter().enumerate() {
        cnt[l] += 1;
        for v in 0..vars.len() {
            centers[l][v] += cols[v][i];
        }
    }
    for c in 0..k {
        for v in 0..vars.len() {
            centers[c][v] /= cnt[c].max(1) as f64;
        }
    }
    let dist: Vec<f64> = label.iter().enumerate().map(|(i, &l)| (0..vars.len()).map(|v| (cols[v][i] - centers[l][v]).powi(2)).sum::<f64>().sqrt()).collect();
    let a = attrs_mut(&mut out, assoc).ok_or_else(|| anyhow!("cannot add arrays here"))?;
    a.insert(Array::new("ClusterId", 1, ArrayData::I64(label.iter().map(|&l| l as i64).collect())));
    a.insert(Array::scalars_f64("ClusterDistance", dist));
    let fd = out.field_data_mut_any();
    for (v, name) in vars.iter().enumerate() {
        fd.insert(Array::scalars_f64(format!("ClusterCenter {name}"), centers.iter().map(|c| c[v]).collect()));
    }
    fd.insert(Array::scalars_f64("ClusterSize", cnt.iter().map(|&c| c as f64).collect()));
    fd.insert(Array::scalars_f64("WithinClusterSumOfSquares", vec![wss]));
    Ok(out)
}

/// Principal Component Analysis filter: the projections `PCA 0`, `PCA 1`... on the data; field
/// data with the eigenvalues, the explained fraction of the variance, and the components.
pub fn pca_filter(ds: &DataSet, assoc: Association, names: &[String], keep: usize, standardise: bool) -> Result<DataSet> {
    let (vars, cols) = variables(ds, assoc, names)?;
    let cols = standardised(&cols, standardise);
    let (means, cov) = covariance(&cols);
    let (vals, vecs) = symmetric_eigen(&cov);
    let keep = if keep == 0 { vars.len() } else { keep.min(vars.len()) };
    let n = cols[0].len();
    let mut out = ds.clone();
    let a = attrs_mut(&mut out, assoc).ok_or_else(|| anyhow!("cannot add arrays here"))?;
    for (k, e) in vecs.iter().take(keep).enumerate() {
        a.insert(Array::scalars_f64(format!("PCA {k}"), (0..n).map(|i| (0..vars.len()).map(|v| (cols[v][i] - means[v]) * e[v]).sum()).collect()));
    }
    let total: f64 = vals.iter().map(|x| x.max(0.0)).sum();
    let fd = out.field_data_mut_any();
    fd.insert(Array::scalars_f64("PCAEigenvalues", vals.clone()));
    fd.insert(Array::scalars_f64("PCAExplainedVariance", vals.iter().map(|x| if total > 0.0 { x.max(0.0) / total } else { 0.0 }).collect()));
    for (v, name) in vars.iter().enumerate() {
        fd.insert(Array::scalars_f64(format!("PCAComponent {name}"), vecs.iter().map(|e| e[v]).collect()));
    }
    Ok(out)
}

/// Multicorrelative statistics: a table with each variable's mean and its rows of the
/// covariance and (Pearson) correlation matrices.
pub fn multicorrelative(ds: &DataSet, assoc: Association, names: &[String]) -> Result<Table> {
    let (vars, cols) = variables(ds, assoc, names)?;
    let (means, cov) = covariance(&cols);
    let d = vars.len();
    let mut t = Table::default();
    t.columns.insert(Array::new("Variable", 1, ArrayData::String(vars.clone())));
    t.columns.insert(Array::scalars_f64("Mean", means));
    for (b, name) in vars.iter().enumerate() {
        t.columns.insert(Array::scalars_f64(format!("Cov {name}"), (0..d).map(|a| cov[a][b]).collect()));
    }
    for (b, name) in vars.iter().enumerate() {
        t.columns.insert(Array::scalars_f64(
            format!("Corr {name}"),
            (0..d)
                .map(|a| {
                    let s = (cov[a][a] * cov[b][b]).sqrt();
                    if s > 0.0 {
                        cov[a][b] / s
                    } else {
                        0.0
                    }
                })
                .collect(),
        ));
    }
    Ok(t)
}

/// Contingency table of two arrays (their values as categories; real values are put in
/// `bins` bins first): one row per pair with its count, joint probability and the
/// conditional probabilities; chi-square, its degrees of freedom and the mutual information
/// as field data.
pub fn contingency(ds: &DataSet, assoc: Association, x: &str, y: &str, bins: usize) -> Result<Table> {
    let (_, cols) = variables(ds, assoc, &[x.to_string(), y.to_string()])?;
    let cat = |v: &[f64]| -> Vec<f64> {
        let integral = v.iter().all(|x| x.fract() == 0.0);
        let distinct = {
            let mut u: Vec<i64> = v.iter().filter(|x| x.is_finite()).map(|x| x.to_bits() as i64).collect();
            u.sort_unstable();
            u.dedup();
            u.len()
        };
        if integral || distinct <= bins.max(1) {
            return v.to_vec();
        }
        let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
        let w = ((hi - lo) / bins as f64).max(1e-300);
        v.iter().map(|&x| lo + (((x - lo) / w).floor().min(bins as f64 - 1.0) + 0.5) * w).collect()
    };
    let (cx, cy) = (cat(&cols[0]), cat(&cols[1]));
    let mut counts: std::collections::BTreeMap<(u64, u64), usize> = Default::default();
    let key = |v: f64| if v >= 0.0 { v.to_bits() ^ (1 << 63) } else { !v.to_bits() };
    let unkey = |k: u64| if k >> 63 == 1 { f64::from_bits(k ^ (1 << 63)) } else { f64::from_bits(!k) };
    let mut n = 0usize;
    for (a, b) in cx.iter().zip(&cy) {
        if a.is_finite() && b.is_finite() {
            *counts.entry((key(*a), key(*b))).or_default() += 1;
            n += 1;
        }
    }
    if n == 0 {
        bail!("no values");
    }
    let mut px: std::collections::BTreeMap<u64, usize> = Default::default();
    let mut py: std::collections::BTreeMap<u64, usize> = Default::default();
    for (&(a, b), &c) in &counts {
        *px.entry(a).or_default() += c;
        *py.entry(b).or_default() += c;
    }
    let nf = n as f64;
    let (mut chi2, mut mi) = (0.0, 0.0);
    for &a in px.keys() {
        for &b in py.keys() {
            let o = *counts.get(&(a, b)).unwrap_or(&0) as f64;
            let e = px[&a] as f64 * py[&b] as f64 / nf;
            chi2 += (o - e).powi(2) / e;
            if o > 0.0 {
                mi += o / nf * (o * nf / (px[&a] as f64 * py[&b] as f64)).ln();
            }
        }
    }
    let mut t = Table::default();
    let rows: Vec<((u64, u64), usize)> = counts.into_iter().collect();
    t.columns.insert(Array::scalars_f64(x, rows.iter().map(|r| unkey(r.0 .0)).collect()));
    t.columns.insert(Array::scalars_f64(if x == y { format!("{y} (2)") } else { y.to_string() }, rows.iter().map(|r| unkey(r.0 .1)).collect()));
    t.columns.insert(Array::scalars_f64("Count", rows.iter().map(|r| r.1 as f64).collect()));
    t.columns.insert(Array::scalars_f64("P", rows.iter().map(|r| r.1 as f64 / nf).collect()));
    t.columns.insert(Array::scalars_f64(format!("P({y} | {x})"), rows.iter().map(|r| r.1 as f64 / px[&r.0 .0] as f64).collect()));
    t.columns.insert(Array::scalars_f64(format!("P({x} | {y})"), rows.iter().map(|r| r.1 as f64 / py[&r.0 .1] as f64).collect()));
    t.field_data.insert(Array::scalars_f64("ChiSquare", vec![chi2]));
    t.field_data.insert(Array::scalars_f64("DegreesOfFreedom", vec![((px.len() - 1) * (py.len() - 1)) as f64]));
    t.field_data.insert(Array::scalars_f64("MutualInformation", vec![mi]));
    Ok(t)
}

/// Quartiles of a sorted sample (linear interpolation between order statistics).
pub fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let h = (sorted.len() - 1) as f64 * q.clamp(0.0, 1.0);
    let (lo, hi) = (h.floor() as usize, h.ceil() as usize);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

/// Compute Quartiles: one row per variable with Minimum, Q1, Median, Q3, Maximum, the
/// interquartile range and the whisker ends (1.5 IQR, clipped to the data) for box plots.
pub fn quartiles(ds: &DataSet, assoc: Association, names: &[String]) -> Result<Table> {
    let (vars, cols) = variables(ds, assoc, names)?;
    let mut rows: [Vec<f64>; 8] = Default::default();
    for c in &cols {
        let mut v: Vec<f64> = c.iter().copied().filter(|x| x.is_finite()).collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let (q1, q2, q3) = (quantile(&v, 0.25), quantile(&v, 0.5), quantile(&v, 0.75));
        let iqr = q3 - q1;
        let lo = v.iter().copied().find(|&x| x >= q1 - 1.5 * iqr).unwrap_or(f64::NAN);
        let hi = v.iter().rev().copied().find(|&x| x <= q3 + 1.5 * iqr).unwrap_or(f64::NAN);
        for (k, x) in [v.first().copied().unwrap_or(f64::NAN), q1, q2, q3, v.last().copied().unwrap_or(f64::NAN), iqr, lo, hi].into_iter().enumerate() {
            rows[k].push(x);
        }
    }
    let mut t = Table::default();
    t.columns.insert(Array::new("Variable", 1, ArrayData::String(vars)));
    for (k, n) in ["Minimum", "Q1", "Median", "Q3", "Maximum", "IQR", "LowerWhisker", "UpperWhisker"].iter().enumerate() {
        t.columns.insert(Array::scalars_f64(*n, std::mem::take(&mut rows[k])));
    }
    Ok(t)
}

/// Random Attributes: a random scalar or vector array on the points or cells, uniform in a range.
pub fn random_attributes(ds: &DataSet, assoc: Association, name: &str, components: usize, lo: f64, hi: f64, seed: u64) -> Result<DataSet> {
    let mut out = ds.clone();
    let n = match (&out, assoc) {
        (DataSet::Table(t), _) => t.n_rows(),
        (d, Association::Points) => d.n_points(),
        (d, Association::Cells) => d.n_cells(),
    };
    let nc = components.max(1);
    let mut rng = Rng::new(seed);
    let v: Vec<f64> = (0..n * nc).map(|_| lo + (hi - lo) * rng.next_f64()).collect();
    let a = attrs_mut(&mut out, assoc).ok_or_else(|| anyhow!("cannot add arrays here"))?;
    a.insert(Array::new(name, nc, ArrayData::F64(v)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(cols: &[(&str, Vec<f64>)]) -> DataSet {
        let mut t = Table::default();
        for (n, v) in cols {
            t.columns.insert(Array::scalars_f64(*n, v.clone()));
        }
        DataSet::Table(t)
    }

    fn normal(rng: &mut Rng) -> f64 {
        // Box-Muller.
        let (u, v) = (rng.next_f64().max(1e-300), rng.next_f64());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }

    #[test]
    fn kmeans_finds_three_blobs() {
        let mut rng = Rng::new(3);
        let centres = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let (mut x, mut y, mut truth) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..600 {
            let c = centres[i % 3];
            x.push(c[0] + normal(&mut rng));
            y.push(c[1] + normal(&mut rng));
            truth.push(i % 3);
        }
        let ds = table(&[("x", x), ("y", y)]);
        let out = kmeans_filter(&ds, Association::Points, &[], 3, 100, false, 1).unwrap();
        let DataSet::Table(t) = &out else { panic!() };
        let id = t.columns.get("ClusterId").unwrap();
        // The same partition as the truth (cluster numbers may differ).
        let mut map = [usize::MAX; 3];
        for (i, &tr) in truth.iter().enumerate() {
            let c = id.get(i, 0) as usize;
            if map[tr] == usize::MAX {
                map[tr] = c;
            }
            assert_eq!(map[tr], c);
        }
        let cx = t.field_data.get("ClusterCenter x").unwrap();
        let mut xs: Vec<f64> = (0..3).map(|k| cx.get(k, 0)).collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        assert!(xs[0].abs() < 0.3 && xs[1].abs() < 0.3 && (xs[2] - 10.0).abs() < 0.3, "{xs:?}");
    }

    #[test]
    fn pca_of_a_stretched_cloud() {
        // Variance 4 along (1, 1)/sqrt2 and 0.25 across it.
        let mut rng = Rng::new(9);
        let (mut x, mut y) = (Vec::new(), Vec::new());
        for _ in 0..20000 {
            let (a, b) = (2.0 * normal(&mut rng), 0.5 * normal(&mut rng));
            let s = std::f64::consts::FRAC_1_SQRT_2;
            x.push(s * (a - b) + 5.0);
            y.push(s * (a + b) - 1.0);
        }
        let out = pca_filter(&table(&[("x", x), ("y", y)]), Association::Points, &[], 0, false).unwrap();
        let fd = out.field_data();
        let ev = fd.get("PCAEigenvalues").unwrap();
        assert!((ev.get(0, 0) - 4.0).abs() < 0.12 && (ev.get(1, 0) - 0.25).abs() < 0.01, "{} {}", ev.get(0, 0), ev.get(1, 0));
        let cx = fd.get("PCAComponent x").unwrap().get(0, 0);
        let cy = fd.get("PCAComponent y").unwrap().get(0, 0);
        assert!((cx - cy).abs() < 0.02 && (cx - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02);
        let DataSet::Table(t) = &out else { panic!() };
        let (m, s) = mean_std(&t.columns.get("PCA 0").unwrap().data.to_f64_vec());
        assert!(m.abs() < 1e-9 && (s - 2.0).abs() < 0.05);
    }

    #[test]
    fn moments_contingency_quartiles() {
        let ds = table(&[("a", vec![1.0, 2.0, 3.0, 4.0]), ("b", vec![2.0, 4.0, 6.0, 8.0]), ("c", vec![4.0, 3.0, 2.0, 1.0])]);
        let t = multicorrelative(&ds, Association::Points, &[]).unwrap();
        let cov_a = t.columns.get("Cov a").unwrap();
        assert!((cov_a.get(0, 0) - 5.0 / 3.0).abs() < 1e-12 && (cov_a.get(1, 0) - 10.0 / 3.0).abs() < 1e-12);
        let corr_c = t.columns.get("Corr c").unwrap();
        assert!((corr_c.get(0, 0) + 1.0).abs() < 1e-12 && (corr_c.get(1, 0) + 1.0).abs() < 1e-12);
        // Two independent fair categories: chi-square 0, mutual information 0.
        let x: Vec<f64> = (0..400).map(|i| (i % 2) as f64).collect();
        let y: Vec<f64> = (0..400).map(|i| ((i / 2) % 2) as f64).collect();
        let c = contingency(&table(&[("x", x.clone()), ("y", y)]), Association::Points, "x", "y", 10).unwrap();
        assert_eq!(c.n_rows(), 4);
        assert!(c.field_data.get("ChiSquare").unwrap().get(0, 0).abs() < 1e-12);
        assert!(c.columns.get("P").unwrap().get(0, 0) == 0.25);
        // Identical categories: mutual information ln 2.
        let c = contingency(&table(&[("x", x.clone()), ("y", x)]), Association::Points, "x", "y", 10).unwrap();
        assert!((c.field_data.get("MutualInformation").unwrap().get(0, 0) - 2f64.ln()).abs() < 1e-12);
        let q = quartiles(&table(&[("v", (1..=9).map(|i| i as f64).chain([100.0]).collect())]), Association::Points, &[]).unwrap();
        let get = |n: &str| q.columns.get(n).unwrap().get(0, 0);
        assert_eq!((get("Minimum"), get("Median"), get("Maximum")), (1.0, 5.5, 100.0));
        assert_eq!((get("Q1"), get("Q3")), (3.25, 7.75));
        assert_eq!(get("UpperWhisker"), 9.0, "100 is an outlier");
        let r = random_attributes(&ds, Association::Points, "R", 3, -1.0, 1.0, 5).unwrap();
        let DataSet::Table(rt) = &r else { panic!() };
        let ra = rt.columns.get("R").unwrap();
        assert_eq!((ra.len(), ra.ncomp), (4, 3));
        assert!((0..4).all(|i| (0..3).all(|c| ra.get(i, c).abs() <= 1.0)));
    }
}
