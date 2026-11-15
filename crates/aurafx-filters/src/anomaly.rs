//! Anomaly and change helpers: points or cells whose value stands out from the rest (robust
//! z-score, z-score, or the interquartile fences), and the places where a series (a table column,
//! such as a value over time) changes its level.

use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

pub const ROBUST: &str = "Robust z-score (median, MAD)";
pub const ZSCORE: &str = "z-score (mean, standard deviation)";
pub const IQR: &str = "Interquartile fences";

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let x = q * (sorted.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x - x.floor());
    if i + 1 < sorted.len() {
        sorted[i] * (1.0 - f) + sorted[i + 1] * f
    } else {
        sorted[i]
    }
}

/// Scores of values (a vector's magnitude for several components) and whether each is an outlier:
/// robust z = 0.6745 (x - median) / MAD, or z = (x - mean) / sd, beyond `threshold`; or outside
/// [Q1 - k IQR, Q3 + k IQR] with k = `threshold` (the score is the distance past the fence in IQRs).
pub fn outlier_scores(values: &[f64], method: &str, threshold: f64) -> (Vec<f64>, Vec<bool>) {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let mut s = finite.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    let score: Vec<f64> = match method {
        ZSCORE => {
            let n = finite.len().max(1) as f64;
            let m = finite.iter().sum::<f64>() / n;
            let sd = (finite.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
            values.iter().map(|x| if sd > 0.0 { (x - m) / sd } else { 0.0 }).collect()
        }
        IQR => {
            let (q1, q3) = (percentile(&s, 0.25), percentile(&s, 0.75));
            let iqr = (q3 - q1).max(1e-300);
            values
                .iter()
                .map(|x| {
                    if *x < q1 {
                        (x - q1) / iqr
                    } else if *x > q3 {
                        (x - q3) / iqr
                    } else {
                        0.0
                    }
                })
                .collect()
        }
        _ => {
            let med = percentile(&s, 0.5);
            let mut dev: Vec<f64> = finite.iter().map(|x| (x - med).abs()).collect();
            dev.sort_by(|a, b| a.total_cmp(b));
            let mad = percentile(&dev, 0.5);
            values.iter().map(|x| if mad > 0.0 { 0.6745 * (x - med) / mad } else { 0.0 }).collect()
        }
    };
    let flag = score.iter().zip(values).map(|(z, x)| !x.is_finite() || z.abs() > threshold).collect();
    (score, flag)
}

/// Find Outliers: an array's points or cells that stand out, as `OutlierScore` and `Outlier`
/// (1 or 0) arrays with the count in the field data `Outliers` (all blocks together).
pub fn find_outliers(ds: &DataSet, assoc: Association, name: &str, method: &str, threshold: f64) -> Result<DataSet> {
    let mut leaves = Vec::new();
    for (leaf, _, _) in crate::selection::flat_leaves(ds) {
        let attrs = if assoc == Association::Points { leaf.point_data() } else { leaf.cell_data() };
        if let Some(a) = attrs.and_then(|a| a.get(name)) {
            leaves.push(a.clone());
        }
    }
    if leaves.is_empty() {
        bail!("no {} array {name}", if assoc == Association::Points { "point" } else { "cell" });
    }
    let value = |a: &Array, i: usize| -> f64 {
        if a.ncomp == 1 {
            a.get(i, 0)
        } else {
            (0..a.ncomp).map(|c| a.get(i, c).powi(2)).sum::<f64>().sqrt()
        }
    };
    let all: Vec<f64> = leaves.iter().flat_map(|a| (0..a.len()).map(move |i| value(a, i))).collect();
    let (score, flag) = outlier_scores(&all, method, threshold);
    let count = flag.iter().filter(|f| **f).count();
    let k = std::cell::Cell::new(0usize);
    let out = crate::registry::per_leaf(ds, &|leaf| {
        let mut d = leaf.clone();
        let attrs = if assoc == Association::Points { d.point_data_mut() } else { d.cell_data_mut() };
        if let Some(attrs) = attrs {
            if let Some(n) = attrs.get(name).map(|a| a.len()) {
                let s0 = k.get();
                attrs.insert(Array::scalars_f64("OutlierScore", score[s0..s0 + n].to_vec()));
                attrs.insert(Array::new("Outlier", 1, ArrayData::U8(flag[s0..s0 + n].iter().map(|f| *f as u8).collect())));
                k.set(s0 + n);
            }
        }
        Ok(d)
    })?;
    let mut out = out;
    out.field_data_mut_any().insert(Array::new("Outliers", 1, ArrayData::I64(vec![count as i64])));
    Ok(out)
}

/// The places where a series changes level: binary segmentation on the mean (a split is kept when
/// it lowers the squared error by more than `penalty` times the series' noise variance, estimated
/// from successive differences), segments at least `min_size` long.
pub fn change_points(y: &[f64], penalty: f64, min_size: usize) -> Vec<usize> {
    let n = y.len();
    if n < 2 * min_size.max(1) {
        return Vec::new();
    }
    // Noise: the median absolute successive difference (robust to the jumps themselves).
    let mut d: Vec<f64> = y.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    d.sort_by(|a, b| a.total_cmp(b));
    let sigma = percentile(&d, 0.5) / (0.6745 * 2f64.sqrt());
    let var = (sigma * sigma).max(1e-300);
    let mut cum = vec![0.0; n + 1];
    let mut cum2 = vec![0.0; n + 1];
    for i in 0..n {
        cum[i + 1] = cum[i] + y[i];
        cum2[i + 1] = cum2[i] + y[i] * y[i];
    }
    let sse = |a: usize, b: usize| {
        let m = (b - a) as f64;
        let s = cum[b] - cum[a];
        (cum2[b] - cum2[a]) - s * s / m
    };
    let mut cps = Vec::new();
    let mut stack = vec![(0usize, n)];
    let ms = min_size.max(1);
    while let Some((a, b)) = stack.pop() {
        if b - a < 2 * ms {
            continue;
        }
        let whole = sse(a, b);
        let best = (a + ms..=b - ms).map(|t| (t, sse(a, t) + sse(t, b))).min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((t, e)) = best {
            if whole - e > penalty * var {
                cps.push(t);
                stack.push((a, t));
                stack.push((t, b));
            }
        }
    }
    cps.sort_unstable();
    cps
}

/// Find Change Points on a table column (a value over time from Plot Data Over Time, say): the
/// table with `ChangePoint` (1 on the first row of each new level) and `SegmentMean` columns,
/// and the change rows (and their x values, when an x column is given) as field data.
pub fn find_change_points(ds: &DataSet, column: &str, x_column: &str, penalty: f64, min_size: usize) -> Result<DataSet> {
    let DataSet::Table(t) = ds else { bail!("Find Change Points takes a table (Plot Data Over Time, Plot Over Line, a CSV file)") };
    let c = t.columns.get(column).ok_or_else(|| anyhow!("no column {column}"))?;
    let y: Vec<f64> = (0..c.len()).map(|i| c.get(i, 0)).collect();
    let cps = change_points(&y, penalty, min_size);
    let mut flag = vec![0u8; y.len()];
    let mut mean = vec![0.0; y.len()];
    let mut edges = vec![0];
    edges.extend(cps.iter().copied());
    edges.push(y.len());
    for w in edges.windows(2) {
        let m = y[w[0]..w[1]].iter().sum::<f64>() / (w[1] - w[0]).max(1) as f64;
        for v in mean.iter_mut().take(w[1]).skip(w[0]) {
            *v = m;
        }
    }
    for &p in &cps {
        flag[p] = 1;
    }
    let mut out = t.clone();
    out.columns.insert(Array::new("ChangePoint", 1, ArrayData::U8(flag)));
    out.columns.insert(Array::scalars_f64("SegmentMean", mean));
    out.field_data.insert(Array::new("ChangeRows", 1, ArrayData::I64(cps.iter().map(|x| *x as i64).collect())));
    if let Some(x) = t.columns.get(x_column) {
        out.field_data.insert(Array::scalars_f64("ChangeAt", cps.iter().map(|&p| x.get(p, 0)).collect()));
    }
    Ok(DataSet::Table(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outliers_by_each_method() {
        let mut v: Vec<f64> = (0..200).map(|i| ((i * 37) % 101) as f64 / 100.0).collect();
        v[17] = 40.0;
        v[90] = -35.0;
        for m in [ROBUST, ZSCORE, IQR] {
            let th = if m == IQR { 3.0 } else { 3.5 };
            let (_, f) = outlier_scores(&v, m, th);
            let hits: Vec<usize> = (0..v.len()).filter(|&i| f[i]).collect();
            assert_eq!(hits, vec![17, 90], "{m}");
        }
    }

    #[test]
    fn change_points_of_steps_in_noise() {
        // Levels 0, 5, 2 with small seeded noise; changes at 40 and 75.
        let mut s = 7u64;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % 1000) as f64 / 1000.0 - 0.5
        };
        let y: Vec<f64> = (0..120)
            .map(|i| {
                (if i < 40 {
                    0.0
                } else if i < 75 {
                    5.0
                } else {
                    2.0
                }) + 0.3 * rnd()
            })
            .collect();
        assert_eq!(change_points(&y, 25.0, 3), vec![40, 75]);
        // A flat noisy series has none.
        let flat: Vec<f64> = (0..120).map(|_| 0.3 * rnd()).collect();
        assert!(change_points(&flat, 25.0, 3).is_empty());
    }
}
