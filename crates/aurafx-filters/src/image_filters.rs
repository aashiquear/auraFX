//! Image filters on volumes (image data, a cell or point array): median, Gaussian smoothing,
//! Otsu thresholds (one or several) and binary morphology (erode, dilate, open, close), the
//! edges handled by repeating the outermost values. Also: setting a volume's origin and spacing,
//! and copying a per-feature array onto the cells of each feature.

use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use rayon::prelude::*;

/// The values of an array of an image and the grid they live on (cells or points).
fn grid_values(img: &ImageData, assoc: Association, name: &str) -> Result<([usize; 3], Vec<f64>)> {
    let (attrs, dims) = match assoc {
        Association::Cells => (&img.cell_data, img.dims().map(|d| (d - 1).max(1))),
        Association::Points => (&img.point_data, img.dims()),
    };
    let a = attrs.get(name).ok_or_else(|| anyhow!("no array named {name:?}"))?;
    if a.ncomp != 1 {
        bail!("{name} has {} components; these filters take one", a.ncomp);
    }
    Ok((dims, (0..a.len()).map(|i| a.get(i, 0)).collect()))
}

fn put(img: &mut ImageData, assoc: Association, arr: Array) {
    match assoc {
        Association::Cells => img.cell_data.insert(arr),
        Association::Points => img.point_data.insert(arr),
    };
}

fn idx(d: [usize; 3], i: i64, j: i64, k: i64) -> usize {
    let c = |v: i64, n: usize| v.clamp(0, n as i64 - 1) as usize;
    c(i, d[0]) + d[0] * (c(j, d[1]) + d[1] * c(k, d[2]))
}

/// Median over a box of half-size `r` (cells along each axis; 0 leaves an axis alone).
pub fn median(dims: [usize; 3], v: &[f64], r: [usize; 3]) -> Vec<f64> {
    let n = v.len();
    (0..n)
        .into_par_iter()
        .map(|c| {
            let (i, j, k) = ((c % dims[0]) as i64, ((c / dims[0]) % dims[1]) as i64, (c / (dims[0] * dims[1])) as i64);
            let mut w = Vec::with_capacity((2 * r[0] + 1) * (2 * r[1] + 1) * (2 * r[2] + 1));
            for dk in -(r[2] as i64)..=r[2] as i64 {
                for dj in -(r[1] as i64)..=r[1] as i64 {
                    for di in -(r[0] as i64)..=r[0] as i64 {
                        w.push(v[idx(dims, i + di, j + dj, k + dk)]);
                    }
                }
            }
            let m = w.len() / 2;
            w.select_nth_unstable_by(m, |a, b| a.total_cmp(b));
            w[m]
        })
        .collect()
}

/// Gaussian smoothing with standard deviation `sigma` (in cells, per axis), the kernel cut at
/// 4 sigma.
pub fn gaussian(dims: [usize; 3], v: &[f64], sigma: [f64; 3]) -> Vec<f64> {
    let mut cur = v.to_vec();
    for a in 0..3 {
        if sigma[a] <= 0.0 || dims[a] < 2 {
            continue;
        }
        let rad = (4.0 * sigma[a] + 0.5) as i64;
        let mut wts: Vec<f64> = (-rad..=rad).map(|x| (-0.5 * (x as f64 / sigma[a]).powi(2)).exp()).collect();
        let s: f64 = wts.iter().sum();
        wts.iter_mut().for_each(|w| *w /= s);
        let src = cur.clone();
        cur = (0..src.len())
            .into_par_iter()
            .map(|c| {
                let p = [(c % dims[0]) as i64, ((c / dims[0]) % dims[1]) as i64, (c / (dims[0] * dims[1])) as i64];
                let mut acc = 0.0;
                for (t, w) in (-rad..=rad).zip(&wts) {
                    let mut q = p;
                    q[a] += t;
                    acc += w * src[idx(dims, q[0], q[1], q[2])];
                }
                acc
            })
            .collect();
    }
    cur
}

/// Otsu thresholds: `levels` thresholds that best split the histogram (`bins` bins over the
/// values' range) into levels + 1 classes (largest between-class variance), searched exhaustively.
pub fn otsu(v: &[f64], levels: usize, bins: usize) -> Vec<f64> {
    let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    if !(hi > lo) {
        return vec![lo; levels];
    }
    let bins = bins.max(2);
    let w = (hi - lo) / bins as f64;
    let mut h = vec![0.0; bins];
    for &x in v.iter().filter(|x| x.is_finite()) {
        h[(((x - lo) / w) as usize).min(bins - 1)] += 1.0;
    }
    let total: f64 = h.iter().sum();
    // Prefix sums of weight and first moment (bin centres).
    let mut p0 = vec![0.0; bins + 1];
    let mut p1 = vec![0.0; bins + 1];
    for b in 0..bins {
        let c = lo + (b as f64 + 0.5) * w;
        p0[b + 1] = p0[b] + h[b] / total;
        p1[b + 1] = p1[b] + h[b] / total * c;
    }
    let class = |a: usize, b: usize| -> f64 {
        let w0 = p0[b] - p0[a];
        if w0 <= 0.0 {
            0.0
        } else {
            let m = p1[b] - p1[a];
            m * m / w0
        }
    };
    let levels = levels.clamp(1, 4).min(bins - 1);
    // Exhaustive search over increasing cut indices.
    let mut best = (f64::NEG_INFINITY, vec![0usize; levels]);
    let mut cuts: Vec<usize> = (1..=levels).collect();
    loop {
        let mut score = 0.0;
        let mut prev = 0;
        for &c in &cuts {
            score += class(prev, c);
            prev = c;
        }
        score += class(prev, bins);
        if score > best.0 {
            best = (score, cuts.clone());
        }
        // Next combination.
        let mut k = levels;
        loop {
            if k == 0 {
                return best.1.iter().map(|&c| lo + c as f64 * w).collect();
            }
            k -= 1;
            if cuts[k] < bins - (levels - k) {
                cuts[k] += 1;
                for m in k + 1..levels {
                    cuts[m] = cuts[m - 1] + 1;
                }
                break;
            }
        }
    }
}

/// Binary morphology on `v != 0` with a ball of radius `r` cells (6-connected for r = 1):
/// erode, dilate, open (erode then dilate) or close (dilate then erode).
pub fn morphology(dims: [usize; 3], v: &[f64], op: &str, r: usize) -> Result<Vec<f64>> {
    let r = r.max(1) as i64;
    let offs: Vec<[i64; 3]> = (-r..=r)
        .flat_map(|k| (-r..=r).flat_map(move |j| (-r..=r).map(move |i| [i, j, k])))
        .filter(|o| o[0] * o[0] + o[1] * o[1] + o[2] * o[2] <= r * r)
        .filter(|o| (0..3).all(|a| dims[a] > 1 || o[a] == 0))
        .collect();
    let pass = |src: &[bool], erode: bool| -> Vec<bool> {
        (0..src.len())
            .into_par_iter()
            .map(|c| {
                let p = [(c % dims[0]) as i64, ((c / dims[0]) % dims[1]) as i64, (c / (dims[0] * dims[1])) as i64];
                let inside = |o: &[i64; 3]| (0..3).all(|a| (0..dims[a] as i64).contains(&(p[a] + o[a])));
                if erode {
                    // Outside the grid counts as set (edges are not eaten).
                    offs.iter().all(|o| !inside(o) || src[idx(dims, p[0] + o[0], p[1] + o[1], p[2] + o[2])])
                } else {
                    offs.iter().any(|o| inside(o) && src[idx(dims, p[0] + o[0], p[1] + o[1], p[2] + o[2])])
                }
            })
            .collect()
    };
    let b: Vec<bool> = v.iter().map(|x| *x != 0.0).collect();
    let out = match op {
        "Erode" => pass(&b, true),
        "Dilate" => pass(&b, false),
        "Open" => pass(&pass(&b, true), false),
        "Close" => pass(&pass(&b, false), true),
        other => bail!("unknown operation {other}"),
    };
    Ok(out.into_iter().map(|x| x as u8 as f64).collect())
}

/// Apply an image filter to a volume's array and add the result (as `result`).
pub fn run(
    ds: &DataSet,
    assoc: Association,
    name: &str,
    result: &str,
    op: &dyn Fn([usize; 3], &[f64]) -> Result<(ArrayData, Option<Array>)>,
) -> Result<DataSet> {
    let DataSet::ImageData(img) = ds else { bail!("this filter works on image data (a volume or a picture)") };
    let (dims, v) = grid_values(img, assoc, name)?;
    let (data, extra) = op(dims, &v)?;
    let mut out = img.clone();
    put(&mut out, assoc, Array::new(result, 1, data));
    if let Some(e) = extra {
        out.field_data.insert(e);
    }
    Ok(DataSet::ImageData(out))
}

/// Set Origin and Spacing of image data (the cells keep their values).
pub fn set_origin_spacing(ds: &DataSet, origin: Option<[f64; 3]>, spacing: Option<[f64; 3]>) -> Result<DataSet> {
    let DataSet::ImageData(img) = ds else { bail!("this filter works on image data") };
    let mut out = img.clone();
    if let Some(o) = origin {
        // The origin is where the first cell's corner goes.
        out.origin = std::array::from_fn(|a| o[a] - out.extent[2 * a] as f64 * spacing.unwrap_or(out.spacing)[a]);
    }
    if let Some(s) = spacing {
        out.spacing = s;
    }
    Ok(DataSet::ImageData(out))
}

/// Copy Feature Array to Cells: every cell takes its feature's row of a per-feature field array.
pub fn copy_feature_array(ds: &DataSet, ids_name: &str, array: &str, result: &str) -> Result<DataSet> {
    let DataSet::ImageData(img) = ds else { bail!("this filter works on image data") };
    let ids = img.cell_data.get(ids_name).ok_or_else(|| anyhow!("no cell array {ids_name}"))?;
    let f = img.field_data.get(array).ok_or_else(|| anyhow!("no field (per-feature) array {array}"))?;
    let rows: Vec<usize> = (0..ids.len()).map(|c| (ids.get(c, 0).max(0.0) as usize).min(f.len().saturating_sub(1))).collect();
    let mut g = f.gather(&rows);
    g.name = if result.is_empty() { array.to_string() } else { result.to_string() };
    let mut out = img.clone();
    out.cell_data.insert(g);
    Ok(DataSet::ImageData(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// testdata/images/filters_reference.json from scripts/image_filters_reference.py (scipy).
    fn reference() -> Option<serde_json::Value> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/images/filters_reference.json");
        serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
    }

    fn vals(v: &serde_json::Value) -> Vec<f64> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    #[test]
    fn match_scipy() {
        let Some(r) = reference() else { return };
        let dims = [r["nx"].as_u64().unwrap() as usize, r["ny"].as_u64().unwrap() as usize, r["nz"].as_u64().unwrap() as usize];
        let input = vals(&r["input"]);
        let close = |a: &[f64], b: &[f64], tol: f64, what: &str| {
            assert_eq!(a.len(), b.len());
            for (k, (x, y)) in a.iter().zip(b).enumerate() {
                assert!((x - y).abs() <= tol, "{what} at {k}: {x} vs {y}");
            }
        };
        close(&median(dims, &input, [1, 1, 1]), &vals(&r["median"]), 0.0, "median");
        close(&gaussian(dims, &input, [1.0; 3]), &vals(&r["gaussian"]), 1e-9, "gaussian");
        let mask: Vec<f64> = vals(&r["mask"]);
        for op in ["Erode", "Dilate", "Open", "Close"] {
            close(&morphology(dims, &mask, op, 1).unwrap(), &vals(&r[op.to_lowercase().as_str()]), 0.0, op);
        }
        let t = otsu(&input, 1, 128);
        assert!((t[0] - r["otsu"].as_f64().unwrap()).abs() < 1e-9, "{t:?}");
        let t2 = otsu(&input, 2, 64);
        let e2 = vals(&r["otsu2"]);
        assert!((t2[0] - e2[0]).abs() < 1e-9 && (t2[1] - e2[1]).abs() < 1e-9, "{t2:?} {e2:?}");
    }
}
