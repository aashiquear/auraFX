//! Operations on voxel microstructures (image data with cell arrays): Crop, Change
//! Resolution and Align Sections.
//!
//! Per-feature arrays are field arrays with one row per feature id (row 0 for id 0), as the
//! `.dream3d` reader gives them. Filters that drop features renumber the feature ids from 1 and
//! keep those rows in the new order.

use crate::crystal::{orientation_array, Repr};
use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use rayon::prelude::*;

pub(crate) fn image(ds: &DataSet) -> Result<&ImageData> {
    match ds {
        DataSet::ImageData(i) => Ok(i),
        _ => bail!("this filter works on image data (voxel volumes and EBSD maps)"),
    }
}

/// Cells along each axis; a flat axis (one point) holds one layer of cells.
fn dims(img: &ImageData) -> [usize; 3] {
    let e = img.extent;
    [(e[1] - e[0]).max(1) as usize, (e[3] - e[2]).max(1) as usize, (e[5] - e[4]).max(1) as usize]
}

fn flat(img: &ImageData) -> [bool; 3] {
    let e = img.extent;
    [e[1] == e[0], e[3] == e[2], e[5] == e[4]]
}

/// The number of rows of the feature table: the longest field array that has at least one row
/// per feature id, unless it is the phase (ensemble) table.
fn feature_rows(img: &ImageData, max_id: usize) -> Option<usize> {
    let phases = img.field_data.get("CrystalStructures").map(|a| a.len());
    img.field_data.iter().map(|a| a.len()).filter(|n| *n > max_id && Some(*n) != phases).max()
}

/// Renumber the feature ids in `img` (cell array `name`) from 1 in order of first appearance
/// of the remaining ids' old values, and keep the matching rows of per-feature field arrays.
pub fn renumber_features(img: &mut ImageData, name: &str, old_rows: Option<usize>) {
    let Some(f) = img.cell_data.get(name) else { return };
    let ids: Vec<i64> = (0..f.len()).map(|k| f.get(k, 0) as i64).collect();
    let max = ids.iter().copied().max().unwrap_or(0).max(0) as usize;
    let mut present = vec![false; max + 1];
    for i in &ids {
        if *i > 0 {
            present[*i as usize] = true;
        }
    }
    let mut new_id = vec![0i32; max + 1];
    let mut rows = vec![0usize];
    for (old, p) in present.iter().enumerate() {
        if *p {
            new_id[old] = rows.len() as i32;
            rows.push(old);
        }
    }
    img.cell_data.insert(Array::new(name, 1, ArrayData::I32(ids.iter().map(|i| if *i > 0 { new_id[*i as usize] } else { 0 }).collect())));
    if let Some(n) = old_rows {
        let names: Vec<String> = img.field_data.iter().filter(|a| a.len() == n).map(|a| a.name.clone()).collect();
        for nm in names {
            let a = img.field_data.get(&nm).unwrap();
            let rows: Vec<usize> = rows.iter().copied().filter(|r| *r < n).collect();
            let g = a.gather(&rows);
            img.field_data.insert(g);
        }
    }
}

fn max_feature(img: &ImageData, name: &str) -> Option<usize> {
    let f = img.cell_data.get(name)?;
    Some((0..f.len()).map(|k| f.get(k, 0).max(0.0) as usize).max().unwrap_or(0))
}

/// The cells of `img` with indices in `lo..hi` (per axis) as a new image at the same place.
fn subvolume(img: &ImageData, lo: [usize; 3], hi: [usize; 3]) -> ImageData {
    let d = dims(img);
    let n: [usize; 3] = std::array::from_fn(|k| hi[k] - lo[k]);
    let mut idx = Vec::with_capacity(n[0] * n[1] * n[2]);
    for k in lo[2]..hi[2] {
        for j in lo[1]..hi[1] {
            for i in lo[0]..hi[0] {
                idx.push((k * d[1] + j) * d[0] + i);
            }
        }
    }
    let fl = flat(img);
    let pd: [usize; 3] = std::array::from_fn(|k| if fl[k] { 1 } else { d[k] + 1 });
    let mut pidx = Vec::new();
    if !img.point_data.is_empty() {
        for k in lo[2]..=hi[2].min(pd[2] - 1) {
            for j in lo[1]..=hi[1].min(pd[1] - 1) {
                for i in lo[0]..=hi[0].min(pd[0] - 1) {
                    pidx.push((k * pd[1] + j) * pd[0] + i);
                }
            }
        }
    }
    let e = img.extent;
    ImageData {
        extent: std::array::from_fn(|m| {
            let k = m / 2;
            let v = if m % 2 == 0 || fl[k] { lo[k] } else { hi[k] };
            e[2 * k] + v as i64
        }),
        origin: img.origin,
        spacing: img.spacing,
        direction: img.direction,
        point_data: if pidx.is_empty() { Attributes::new() } else { img.point_data.gather(&pidx) },
        cell_data: img.cell_data.gather(&idx),
        field_data: img.field_data.clone(),
    }
}

/// Crop: keep the cells whose centres lie in the box `min`..`max` (sample coordinates; NaN
/// leaves that side open). With `renumber`, feature ids are renumbered from 1 and per-feature
/// field arrays keep only the remaining features.
pub fn crop(ds: &DataSet, min: [f64; 3], max: [f64; 3], renumber: bool, feature_ids: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let d = dims(img);
    let mut lo = [0usize; 3];
    let mut hi = d;
    for k in 0..3 {
        // Cell i's centre: origin + (extent start + i + 0.5) * spacing.
        let c0 = img.origin[k] + (img.extent[2 * k] as f64 + 0.5) * img.spacing[k];
        let h = img.spacing[k];
        let (a, b) = if h >= 0.0 { (min[k], max[k]) } else { (max[k], min[k]) };
        if a.is_finite() {
            lo[k] = ((a - c0) / h).ceil().clamp(0.0, d[k] as f64) as usize;
        }
        if b.is_finite() {
            hi[k] = (((b - c0) / h).floor() + 1.0).clamp(0.0, d[k] as f64) as usize;
        }
        if hi[k] <= lo[k] {
            bail!("the box holds no cell along {}", ["x", "y", "z"][k]);
        }
    }
    let rows = max_feature(img, feature_ids).and_then(|m| feature_rows(img, m));
    let mut out = subvolume(img, lo, hi);
    if renumber {
        renumber_features(&mut out, feature_ids, rows);
    }
    Ok(DataSet::ImageData(out))
}

/// Change Resolution: resample the cell arrays onto cells of `spacing`, each new cell taking
/// the value of the old cell holding its centre (nearest neighbour, so orientations, phases
/// and ids stay valid). The volume keeps its place and size (rounded to whole new cells).
pub fn change_resolution(ds: &DataSet, spacing: [f64; 3], renumber: bool, feature_ids: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let d = dims(img);
    if spacing.iter().any(|s| !(*s > 0.0)) {
        bail!("the new spacing must be positive");
    }
    let old: [f64; 3] = img.spacing.map(|s| s.abs());
    let n: [usize; 3] = std::array::from_fn(|k| if flat(img)[k] { 1 } else { ((d[k] as f64 * old[k] / spacing[k]).round() as usize).max(1) });
    let src = |k: usize, i: usize| (((i as f64 + 0.5) * spacing[k] / old[k]).floor() as usize).min(d[k].saturating_sub(1));
    let mut idx = Vec::with_capacity(n[0] * n[1] * n[2]);
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                idx.push((src(2, k) * d[1] + src(1, j)) * d[0] + src(0, i));
            }
        }
    }
    let rows = max_feature(img, feature_ids).and_then(|m| feature_rows(img, m));
    let start: [f64; 3] = std::array::from_fn(|k| img.origin[k] + img.extent[2 * k] as f64 * img.spacing[k]);
    let sign: [f64; 3] = img.spacing.map(|s| if s < 0.0 { -1.0 } else { 1.0 });
    let fl = flat(img);
    let mut out = ImageData {
        extent: [0, if fl[0] { 0 } else { n[0] as i64 }, 0, if fl[1] { 0 } else { n[1] as i64 }, 0, if fl[2] { 0 } else { n[2] as i64 }],
        origin: start,
        spacing: std::array::from_fn(|k| spacing[k] * sign[k]),
        direction: img.direction,
        cell_data: img.cell_data.gather(&idx),
        field_data: img.field_data.clone(),
        ..Default::default()
    };
    if renumber {
        renumber_features(&mut out, feature_ids, rows);
    }
    Ok(DataSet::ImageData(out))
}

pub const ALIGN_MISORIENTATION: &str = "Misorientation";
pub const ALIGN_FEATURES: &str = "Feature ids";
pub const ALIGN_SHIFTS: &str = "Given shifts";

/// What two cells are compared by when aligning sections.
enum Compare {
    /// Orientation and crystal class per cell; cells match when their disorientation is below
    /// the tolerance (cosine of half the angle).
    Orientation(Vec<Option<(Quat, Laue)>>, f64),
    Ids(Vec<i64>),
}

impl Compare {
    fn valid(&self, c: usize) -> bool {
        match self {
            Compare::Orientation(o, _) => o[c].is_some(),
            Compare::Ids(v) => v[c] > 0,
        }
    }
    fn differ(&self, a: usize, b: usize) -> bool {
        match self {
            Compare::Orientation(o, w) => {
                let (Some((qa, la)), Some((qb, lb))) = (o[a], o[b]) else { return true };
                if la != lb {
                    return true;
                }
                let d = cx::mul(qb, cx::conj(qa));
                !la.operators().iter().any(|s| (s[0] * d[0] - s[1] * d[1] - s[2] * d[2] - s[3] * d[3]).abs() >= *w)
            }
            Compare::Ids(v) => v[a] != v[b],
        }
    }
}

/// The fraction of overlapping valid cell pairs that differ when the upper of two slices
/// (cells `plane..2 plane` of `cmp`, the lower being `0..plane`) is moved by (`dx`, `dy`) cells.
fn mismatch(cmp: &Compare, mask: Option<&[bool]>, nx: usize, ny: usize, dx: i64, dy: i64) -> f64 {
    let (mut bad, mut n) = (0usize, 0usize);
    let plane = nx * ny;
    for j in 0..ny as i64 {
        let sj = j - dy;
        if sj < 0 || sj >= ny as i64 {
            continue;
        }
        for i in 0..nx as i64 {
            let si = i - dx;
            if si < 0 || si >= nx as i64 {
                continue;
            }
            let a = (j as usize) * nx + i as usize;
            let b = plane + (sj as usize) * nx + si as usize;
            if !cmp.valid(a) || !cmp.valid(b) || mask.is_some_and(|m| !m[a] || !m[b]) {
                continue;
            }
            n += 1;
            bad += cmp.differ(a, b) as usize;
        }
    }
    // Too little overlap is never a good match.
    if n < (plane / 10).max(1) {
        return f64::INFINITY;
    }
    bad as f64 / n as f64
}

/// The shift of slice `k` against `k - 1` with the fewest differing cells, searched from no
/// shift by steps to the best neighbouring shift (at most `max_shift` cells each way).
fn best_shift(cmp: &Compare, mask: Option<&[bool]>, nx: usize, ny: usize, max_shift: i64) -> (i64, i64) {
    let mut cur = (0i64, 0i64);
    let mut best = mismatch(cmp, mask, nx, ny, 0, 0);
    loop {
        let mut next = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = (cur.0 + dx, cur.1 + dy);
                if (dx, dy) == (0, 0) || c.0.abs() > max_shift || c.1.abs() > max_shift {
                    continue;
                }
                let m = mismatch(cmp, mask, nx, ny, c.0, c.1);
                if m < best - 1e-12 {
                    best = m;
                    next = Some(c);
                }
            }
        }
        match next {
            Some(c) => cur = c,
            None => return cur,
        }
    }
}

/// Align Sections: move each z slice of a volume in x and y, by whole cells, so that it
/// matches the slice below. Cells uncovered by a move are filled with zeros. The shifts (cells,
/// accumulated from the first slice) are kept as the field array `SectionShifts` (x, y per
/// slice).
#[allow(clippy::too_many_arguments)]
pub fn align_sections(
    ds: &DataSet,
    method: &str,
    orientations: Option<(Association, String)>,
    tolerance_deg: f64,
    structure: &str,
    phases: Option<&str>,
    feature_ids: &str,
    mask: Option<&str>,
    max_shift: i64,
    given: &[f64],
) -> Result<DataSet> {
    let img = image(ds)?;
    let [nx, ny, nz] = dims(img);
    if nz < 2 {
        bail!("the volume has a single slice: nothing to align");
    }
    let mask: Option<Vec<bool>> = match mask.filter(|m| !m.is_empty()) {
        Some(m) => {
            let a = img.cell_data.get(m).ok_or_else(|| anyhow!("no mask array {m}"))?;
            Some((0..a.len()).map(|k| a.get(k, 0) != 0.0).collect())
        }
        None => None,
    };
    let pair: Vec<(i64, i64)> = if method == ALIGN_SHIFTS {
        if given.len() != 2 * nz && given.len() != 2 * (nz - 1) {
            bail!("give 2 numbers (x, y cells) per slice, or per slice after the first: {} slices", nz);
        }
        let off = if given.len() == 2 * nz { 2 } else { 0 };
        std::iter::once((0, 0)).chain((0..nz - 1).map(|k| (given[off + 2 * k].round() as i64, given[off + 2 * k + 1].round() as i64))).collect()
    } else {
        let plane = nx * ny;
        // What each pair of slices is compared by, decoded for those two slices only.
        enum Src<'a> {
            Orient(&'a Array, Repr, Vec<Option<Laue>>, f64),
            Ids(&'a Array),
        }
        let src = if method == ALIGN_FEATURES {
            Src::Ids(img.cell_data.get(feature_ids).ok_or_else(|| anyhow!("no feature id array {feature_ids}"))?)
        } else {
            let (assoc, a) = orientation_array(ds, orientations)?;
            if assoc != Association::Cells {
                bail!("the orientations must be cell data");
            }
            let repr = Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
            let cls = crate::crystal::classes_of(ds, assoc, a.len(), structure, phases)?;
            Src::Orient(a, repr, cls, (tolerance_deg.to_radians() * 0.5).cos())
        };
        let shift_of = |k: usize| {
            let cells = (k - 1) * plane..(k + 1) * plane;
            let cmp = match &src {
                Src::Ids(f) => Compare::Ids(cells.map(|c| f.get(c, 0) as i64).collect()),
                Src::Orient(a, repr, cls, w) => Compare::Orientation(cells.map(|c| cls[c].map(|l| (repr.decode_values(a, c), l))).collect(), *w),
            };
            let m: Option<Vec<bool>> = mask.as_ref().map(|m| m[(k - 1) * plane..(k + 1) * plane].to_vec());
            best_shift(&cmp, m.as_deref(), nx, ny, max_shift)
        };
        std::iter::once((0, 0)).chain((1..nz).into_par_iter().map(shift_of).collect::<Vec<_>>()).collect()
    };
    // Accumulate and move every slice.
    let mut total = vec![(0i64, 0i64); nz];
    for k in 1..nz {
        total[k] = (total[k - 1].0 + pair[k].0, total[k - 1].1 + pair[k].1);
    }
    let plane = nx * ny;
    let n = nz * plane;
    // The source cell of each cell; `n` (one past the end) where the move uncovers it.
    let idx: Vec<usize> = (0..n)
        .into_par_iter()
        .map(|c| {
            let (k, j, i) = (c / plane, (c % plane) / nx, c % nx);
            let (si, sj) = (i as i64 - total[k].0, j as i64 - total[k].1);
            if si >= 0 && sj >= 0 && (si as usize) < nx && (sj as usize) < ny {
                k * plane + sj as usize * nx + si as usize
            } else {
                n
            }
        })
        .collect();
    let mut cells = Attributes::new();
    for a in img.cell_data.iter() {
        // Uncovered cells take an extra zero tuple appended to the array.
        let mut data = a.data.clone();
        let g = if data.append(&ArrayData::zeros(a.scalar_type(), a.ncomp)).is_ok() {
            data.gather(a.ncomp, &idx)
        } else {
            drop(data);
            a.data.gather(a.ncomp, &idx.iter().map(|c| if *c == n { 0 } else { *c }).collect::<Vec<_>>())
        };
        cells.insert(Array::new(a.name.clone(), a.ncomp, g));
    }
    let mut out = ImageData {
        extent: img.extent,
        origin: img.origin,
        spacing: img.spacing,
        direction: img.direction,
        cell_data: cells,
        field_data: img.field_data.clone(),
        ..Default::default()
    };
    out.field_data.insert(Array::new("SectionShifts", 2, ArrayData::I32(total.iter().flat_map(|(x, y)| [*x as i32, *y as i32]).collect())));
    Ok(DataSet::ImageData(out))
}
