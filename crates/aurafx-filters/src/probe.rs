//! Point location and probing: find the cell containing a point and interpolate point
//! data there. Used by Plot Over Line, Probe Location and Resample To Image.

use crate::simplex::{as_explicit, decompose, for_each_cell, parts, Simplex};
use anyhow::{anyhow, Result};
use aurafx_data::*;
use rayon::prelude::*;

/// Simplex-based locator with uniform binning.
pub struct Locator {
    pts: Vec<[f64; 3]>,
    /// (point ids, owning cell). Tets for volumes, triangles for surfaces.
    simplices: Vec<([u32; 4], u32, u8)>,
    bins: Vec<Vec<u32>>,
    dims: [usize; 3],
    lo: [f64; 3],
    size: [f64; 3],
    tol: f64,
}

pub struct Hit {
    pub cell: usize,
    pub ids: [u32; 4],
    pub w: [f64; 4],
    pub n: usize,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

impl Locator {
    pub fn new(ds: &DataSet) -> Result<(Self, std::borrow::Cow<'_, DataSet>)> {
        let ds = as_explicit(ds);
        let (points, ..) = parts(&ds).ok_or_else(|| anyhow!("probe: unsupported dataset type"))?;
        let pts: Vec<[f64; 3]> = (0..points.len()).map(|i| points.get(i)).collect();
        let mut tets = Vec::new();
        let mut tris = Vec::new();
        for_each_cell(&ds, |cid, ty, ids, faces| {
            decompose(ty, ids, faces, |s| match s {
                Simplex::Tet(t) => tets.push((t, cid as u32, 4u8)),
                Simplex::Tri(t) => tris.push(([t[0], t[1], t[2], 0], cid as u32, 3u8)),
                _ => {}
            })
        });
        let simplices = if !tets.is_empty() { tets } else { tris };
        let b = ds.bounds();
        let diag = b.diagonal().max(1e-30);
        let tol = diag * 1e-6;
        let n = simplices.len().max(1);
        let target = (n as f64 / 4.0).cbrt().ceil().max(1.0) as usize;
        let s = b.size();
        let maxs = s.iter().cloned().fold(0.0, f64::max).max(1e-30);
        let dims = [
            ((s[0] / maxs) * target as f64).ceil().max(1.0) as usize,
            ((s[1] / maxs) * target as f64).ceil().max(1.0) as usize,
            ((s[2] / maxs) * target as f64).ceil().max(1.0) as usize,
        ];
        let size = [s[0].max(tol) / dims[0] as f64, s[1].max(tol) / dims[1] as f64, s[2].max(tol) / dims[2] as f64];
        let mut loc = Self { pts, simplices, bins: vec![Vec::new(); dims[0] * dims[1] * dims[2]], dims, lo: b.min, size, tol };
        for (si, (ids, _, k)) in loc.simplices.iter().enumerate() {
            let mut mn = [f64::INFINITY; 3];
            let mut mx = [f64::NEG_INFINITY; 3];
            for &p in &ids[..*k as usize] {
                let q = loc.pts[p as usize];
                for a in 0..3 {
                    mn[a] = mn[a].min(q[a]);
                    mx[a] = mx[a].max(q[a]);
                }
            }
            let (i0, i1) = (loc.bin_index(mn, true), loc.bin_index(mx, true));
            for z in i0[2]..=i1[2] {
                for y in i0[1]..=i1[1] {
                    for x in i0[0]..=i1[0] {
                        let bi = x + loc.dims[0] * (y + loc.dims[1] * z);
                        loc.bins[bi].push(si as u32);
                    }
                }
            }
        }
        Ok((loc, ds))
    }

    fn bin_index(&self, p: [f64; 3], clamp: bool) -> [usize; 3] {
        let mut out = [0usize; 3];
        for a in 0..3 {
            let f = ((p[a] - self.lo[a]) / self.size[a]).floor();
            out[a] = if clamp { f.clamp(0.0, (self.dims[a] - 1) as f64) as usize } else { f as usize };
        }
        out
    }

    pub fn find(&self, p: [f64; 3]) -> Option<Hit> {
        for a in 0..3 {
            if p[a] < self.lo[a] - self.tol || p[a] > self.lo[a] + self.size[a] * self.dims[a] as f64 + self.tol {
                return None;
            }
        }
        let b = self.bin_index(p, true);
        let bi = b[0] + self.dims[0] * (b[1] + self.dims[1] * b[2]);
        for &si in &self.bins[bi] {
            let (ids, cell, k) = self.simplices[si as usize];
            if k == 4 {
                if let Some(w) = self.bary_tet(p, ids) {
                    return Some(Hit { cell: cell as usize, ids, w, n: 4 });
                }
            } else if let Some(w) = self.bary_tri(p, ids) {
                return Some(Hit { cell: cell as usize, ids, w: [w[0], w[1], w[2], 0.0], n: 3 });
            }
        }
        None
    }

    fn bary_tet(&self, p: [f64; 3], t: [u32; 4]) -> Option<[f64; 4]> {
        let a = self.pts[t[0] as usize];
        let (b, c, d) = (self.pts[t[1] as usize], self.pts[t[2] as usize], self.pts[t[3] as usize]);
        let (ab, ac, ad, ap) = (sub(b, a), sub(c, a), sub(d, a), sub(p, a));
        let vol = dot(ab, cross(ac, ad));
        if vol.abs() < 1e-300 {
            return None;
        }
        let w1 = dot(ap, cross(ac, ad)) / vol;
        let w2 = dot(ab, cross(ap, ad)) / vol;
        let w3 = dot(ab, cross(ac, ap)) / vol;
        let w0 = 1.0 - w1 - w2 - w3;
        let e = -1e-9;
        (w0 >= e && w1 >= e && w2 >= e && w3 >= e).then_some([w0, w1, w2, w3])
    }

    fn bary_tri(&self, p: [f64; 3], t: [u32; 4]) -> Option<[f64; 3]> {
        let a = self.pts[t[0] as usize];
        let (b, c) = (self.pts[t[1] as usize], self.pts[t[2] as usize]);
        let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
        let n = cross(ab, ac);
        let nn = dot(n, n);
        if nn < 1e-300 {
            return None;
        }
        // Distance from plane.
        if (dot(ap, n) / nn.sqrt()).abs() > self.tol * 100.0 {
            return None;
        }
        let w1 = dot(cross(ap, ac), n) / nn;
        let w2 = dot(cross(ab, ap), n) / nn;
        let w0 = 1.0 - w1 - w2;
        let e = -1e-9;
        (w0 >= e && w1 >= e && w2 >= e).then_some([w0, w1, w2])
    }
}

/// Probe `ds` at `points`. Output point data holds interpolated point arrays, cell arrays of
/// the containing cell, and `vtkValidPointMask` (1 where the point was inside the data).
pub fn probe_points(ds: &DataSet, points: &[[f64; 3]]) -> Result<Attributes> {
    // Multi-block: probe each leaf, first hit wins.
    if let DataSet::MultiBlock(mb) = ds {
        let mut acc: Option<Attributes> = None;
        for (_, leaf) in mb.leaves() {
            let a = probe_points(leaf, points)?;
            acc = Some(match acc {
                None => a,
                Some(mut prev) => {
                    let mask_prev = prev.get("vtkValidPointMask").cloned();
                    let mask_new = a.get("vtkValidPointMask").cloned();
                    if let (Some(mp), Some(mn)) = (mask_prev, mask_new) {
                        for i in 0..points.len() {
                            if mp.get(i, 0) == 0.0 && mn.get(i, 0) == 1.0 {
                                for arr in a.iter() {
                                    if let Some(dst) = prev.get_mut(&arr.name) {
                                        if dst.ncomp == arr.ncomp {
                                            if let ArrayData::F64(v) = &mut dst.data {
                                                for c in 0..arr.ncomp {
                                                    v[i * arr.ncomp + c] = arr.get(i, c);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    prev
                }
            });
        }
        return Ok(acc.unwrap_or_default());
    }
    let (loc, ds) = Locator::new(ds)?;
    let (_, pd, cd, _) = parts(&ds).ok_or_else(|| anyhow!("probe: unsupported dataset"))?;
    let hits: Vec<Option<Hit>> = points.par_iter().map(|&p| loc.find(p)).collect();
    let mut out = Attributes::new();
    for arr in pd.iter().filter(|a| a.scalar_type() != ScalarType::String) {
        let nc = arr.ncomp;
        let mut v = vec![f64::NAN; points.len() * nc];
        for (i, h) in hits.iter().enumerate() {
            if let Some(h) = h {
                for c in 0..nc {
                    let mut s = 0.0;
                    for k in 0..h.n {
                        s += h.w[k] * arr.get(h.ids[k] as usize, c);
                    }
                    v[i * nc + c] = s;
                }
            }
        }
        let mut a = Array::new(arr.name.clone(), nc, ArrayData::F64(v));
        a.component_names = arr.component_names.clone();
        out.insert(a);
    }
    for arr in cd.iter().filter(|a| a.scalar_type() != ScalarType::String) {
        if out.get(&arr.name).is_some() {
            continue;
        }
        let nc = arr.ncomp;
        let mut v = vec![f64::NAN; points.len() * nc];
        for (i, h) in hits.iter().enumerate() {
            if let Some(h) = h {
                for c in 0..nc {
                    v[i * nc + c] = arr.get(h.cell, c);
                }
            }
        }
        out.insert(Array::new(arr.name.clone(), nc, ArrayData::F64(v)));
    }
    out.insert(Array::new("vtkValidPointMask", 1, ArrayData::F64(hits.iter().map(|h| if h.is_some() { 1.0 } else { 0.0 }).collect())));
    if let Some(s) = pd.active(AttributeKind::Scalars) {
        out.set_active(AttributeKind::Scalars, &s.name.clone());
    }
    Ok(out)
}

/// Sample along a line: returns a polyline with `arc_length` and probed arrays.
pub fn plot_over_line(ds: &DataSet, p1: [f64; 3], p2: [f64; 3], resolution: usize) -> Result<PolyData> {
    let n = resolution.max(1) + 1;
    let pts: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let t = i as f64 / (n - 1) as f64;
            [p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1]), p1[2] + t * (p2[2] - p1[2])]
        })
        .collect();
    let len = dot(sub(p2, p1), sub(p2, p1)).sqrt();
    let mut attrs = probe_points(ds, &pts)?;
    attrs.insert(Array::new("arc_length", 1, ArrayData::F64((0..n).map(|i| len * i as f64 / (n - 1) as f64).collect())));
    let mut lines = Cells::new();
    lines.push(CellType::PolyLine, &(0..n as u32).collect::<Vec<_>>());
    Ok(PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), lines, point_data: attrs, ..Default::default() })
}

/// Resample onto a uniform grid of `dims` points spanning the data bounds.
pub fn resample_to_image(ds: &DataSet, dims: [usize; 3]) -> Result<ImageData> {
    let b = ds.bounds();
    if b.is_empty() {
        return Err(anyhow!("resample: empty dataset"));
    }
    let d = [dims[0].max(1), dims[1].max(1), dims[2].max(1)];
    let s = b.size();
    let spacing = [
        if d[0] > 1 { s[0] / (d[0] - 1) as f64 } else { 1.0 },
        if d[1] > 1 { s[1] / (d[1] - 1) as f64 } else { 1.0 },
        if d[2] > 1 { s[2] / (d[2] - 1) as f64 } else { 1.0 },
    ];
    let mut pts = Vec::with_capacity(d[0] * d[1] * d[2]);
    for k in 0..d[2] {
        for j in 0..d[1] {
            for i in 0..d[0] {
                pts.push([b.min[0] + i as f64 * spacing[0], b.min[1] + j as f64 * spacing[1], b.min[2] + k as f64 * spacing[2]]);
            }
        }
    }
    let attrs = probe_points(ds, &pts)?;
    Ok(ImageData { extent: [0, d[0] as i64 - 1, 0, d[1] as i64 - 1, 0, d[2] as i64 - 1], origin: b.min, spacing, point_data: attrs, ..Default::default() })
}
