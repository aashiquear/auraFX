//! AMR filters: the dual grid of overlapping AMR data (cell centers joined into cells, the
//! levels stitched together without cracks) and contours of cell data on it.
//!
//! Each level's dual cells join 2×2×2 neighbouring cell centers. A dual cell with a corner in
//! a cell that a finer level covers is left to that finer level; a dual cell of a finer level
//! with corners outside its boxes takes those corners from the coarser cells there (their
//! centers and values), which makes it degenerate (a wedge, pyramid or tetrahedron) and closes
//! the gap between the levels. A contour of that grid is crack-free across levels.

use anyhow::{anyhow, bail, Result};
use aurafx_data::amr::{Amr, GHOST, MASKED};
use aurafx_data::*;
use std::collections::HashMap;

/// Where a level-index cell is found.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    /// A visible cell: (level, block, local cell index).
    Cell(usize, usize, usize),
    /// Covered by a finer level.
    Hidden,
    /// Outside the data.
    Outside,
}

struct Level {
    spacing: [f64; 3],
    boxes: Vec<([i64; 3], [i64; 3])>,
    hidden: Vec<Option<Vec<bool>>>,
    /// Boxes by bins of BIN cells.
    bins: HashMap<[i64; 3], Vec<usize>>,
}

const BIN: i64 = 16;

impl Level {
    fn find(&self, c: [i64; 3]) -> Option<(usize, usize)> {
        let key = c.map(|v| v.div_euclid(BIN));
        for &b in self.bins.get(&key)? {
            let (lo, hi) = self.boxes[b];
            if (0..3).all(|k| c[k] >= lo[k] && c[k] <= hi[k]) {
                let n = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1];
                let local = (c[0] - lo[0]) + n[0] * ((c[1] - lo[1]) + n[1] * (c[2] - lo[2]));
                return Some((b, local as usize));
            }
        }
        None
    }
}

fn levels_of(amr: &Amr) -> Vec<Level> {
    amr.levels
        .iter()
        .map(|lev| {
            let boxes: Vec<_> = lev.blocks.iter().map(|b| (b.lo, b.hi)).collect();
            let hidden = lev
                .blocks
                .iter()
                .map(|b| {
                    b.data.cell_data.get(GHOST).map(|a| (0..a.data.len()).map(|i| (a.data.get_f64(i) as i64 as u8) & MASKED != 0).collect())
                })
                .collect();
            let mut bins: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
            for (i, (lo, hi)) in boxes.iter().enumerate() {
                for bz in lo[2].div_euclid(BIN)..=hi[2].div_euclid(BIN) {
                    for by in lo[1].div_euclid(BIN)..=hi[1].div_euclid(BIN) {
                        for bx in lo[0].div_euclid(BIN)..=hi[0].div_euclid(BIN) {
                            bins.entry([bx, by, bz]).or_default().push(i);
                        }
                    }
                }
            }
            Level { spacing: lev.spacing, boxes, hidden, bins }
        })
        .collect()
}

/// The dual grid of AMR data: a point at the center of every visible cell used (with the
/// cells' arrays as point data), hexahedra (quads in 2-D) between them.
pub fn dual_grid(ds: &DataSet) -> Result<UnstructuredGrid> {
    let amr = Amr::from_dataset(ds).ok_or_else(|| anyhow!("this filter needs AMR data (levels of image boxes)"))?;
    if amr.levels.is_empty() {
        bail!("the AMR data has no levels");
    }
    let levels = levels_of(&amr);
    let ratios: Vec<[i64; 3]> = (0..amr.levels.len().saturating_sub(1)).map(|l| amr.ratio(l)).collect();
    // Planar data: one cell layer along an axis everywhere.
    let flat: [bool; 3] = std::array::from_fn(|k| amr.levels.iter().all(|l| l.blocks.iter().all(|b| b.hi[k] == b.lo[k])));
    let resolve = |l: usize, c: [i64; 3]| -> Hit {
        let mut l = l;
        let mut c = c;
        loop {
            if let Some((b, i)) = levels[l].find(c) {
                if levels[l].hidden[b].as_ref().is_some_and(|h| h.get(i).copied().unwrap_or(false)) {
                    return Hit::Hidden;
                }
                return Hit::Cell(l, b, i);
            }
            if l == 0 {
                return Hit::Outside;
            }
            l -= 1;
            c = std::array::from_fn(|k| c[k].div_euclid(ratios[l][k]));
        }
    };
    // Arrays copied to the points (all cell arrays but the ghost flags, from the first block).
    let first = &amr.levels[0].blocks.first().ok_or_else(|| anyhow!("the AMR data has no blocks"))?.data;
    let names: Vec<(String, usize)> = first.cell_data.iter().filter(|a| a.name != GHOST && a.scalar_type() != ScalarType::String).map(|a| (a.name.clone(), a.ncomp)).collect();
    let mut ids: HashMap<(usize, usize, usize), u32> = HashMap::new();
    let mut pts: Vec<f64> = Vec::new();
    let mut vals: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    let mut point_id = |l: usize, b: usize, i: usize, pts: &mut Vec<f64>, vals: &mut Vec<Vec<f64>>| -> u32 {
        *ids.entry((l, b, i)).or_insert_with(|| {
            let blk = &amr.levels[l].blocks[b];
            let (lo, hi) = (blk.lo, blk.hi);
            let n = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1];
            let ii = i as i64;
            let local = [ii % n[0], (ii / n[0]) % n[1], ii / (n[0] * n[1])];
            for k in 0..3 {
                let s = levels[l].spacing[k];
                let o = blk.data.origin[k] + blk.data.extent[2 * k] as f64 * blk.data.spacing[k];
                pts.push(if flat[k] { o } else { o + (local[k] as f64 + 0.5) * s });
            }
            for (v, (name, nc)) in vals.iter_mut().zip(&names) {
                match blk.data.cell_data.get(name) {
                    Some(a) => (0..*nc).for_each(|c| v.push(a.get(i, c))),
                    None => v.extend(std::iter::repeat_n(f64::NAN, *nc)),
                }
            }
            (pts.len() / 3 - 1) as u32
        })
    };
    let span = |k: usize| if flat[k] { 0 } else { 1 };
    let (sx, sy, sz) = (span(0), span(1), span(2));
    let corners: Vec<[i64; 3]> = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]]
        .into_iter()
        .filter(|c| (c[0] <= sx) && (c[1] <= sy) && (c[2] <= sz))
        .collect();
    let cell_type = match corners.len() {
        8 => CellType::Hexahedron,
        4 => CellType::Quad,
        2 => CellType::Line,
        _ => bail!("AMR data must be 1-, 2- or 3-dimensional"),
    };
    let mut cells = Cells::new();
    for (l, lev) in levels.iter().enumerate() {
        for (b, (lo, hi)) in lev.boxes.iter().enumerate() {
            // Dual cells with a corner in this box: origins from one cell before it.
            let from = [lo[0] - sx, lo[1] - sy, lo[2] - sz];
            for z in from[2]..=hi[2] {
                for y in from[1]..=hi[1] {
                    for x in from[0]..=hi[0] {
                        let mut hits = [Hit::Outside; 8];
                        let mut owner = None;
                        let mut skip = false;
                        for (k, d) in corners.iter().enumerate() {
                            let c = [x + d[0], y + d[1], z + d[2]];
                            let h = resolve(l, c);
                            match h {
                                Hit::Hidden | Hit::Outside => {
                                    skip = true;
                                    break;
                                }
                                Hit::Cell(hl, hb, _) if hl == l && owner.is_none() => owner = Some(hb),
                                _ => {}
                            }
                            hits[k] = h;
                        }
                        // Each dual cell once: by the box of its first corner on this level.
                        if skip || owner != Some(b) {
                            continue;
                        }
                        let mut pid = [0u32; 8];
                        for k in 0..corners.len() {
                            if let Hit::Cell(hl, hb, hi) = hits[k] {
                                pid[k] = point_id(hl, hb, hi, &mut pts, &mut vals);
                            }
                        }
                        let ids = &pid[..corners.len()];
                        let mut uniq: Vec<u32> = ids.to_vec();
                        uniq.sort_unstable();
                        uniq.dedup();
                        // Collapsed below a solid (a hexahedron needs 4 distinct corners, a quad 3).
                        let need = match corners.len() {
                            8 => 4,
                            4 => 3,
                            _ => 2,
                        };
                        if uniq.len() < need {
                            continue;
                        }
                        cells.push(cell_type, ids);
                    }
                }
            }
        }
    }
    let mut point_data = Attributes::new();
    for ((name, nc), v) in names.iter().zip(vals) {
        point_data.insert(Array::new(name.clone(), *nc, ArrayData::F64(v)));
    }
    Ok(UnstructuredGrid { points: Points::new_f64(pts), cells, point_data, cell_data: Attributes::new(), field_data: Attributes::new() })
}

/// Contours of a cell array of AMR data on its dual grid (no cracks between levels; the
/// other cell arrays are interpolated onto the surface).
pub fn amr_contour(ds: &DataSet, array: &str, comp: Option<usize>, values: &[f64]) -> Result<PolyData> {
    let ug = dual_grid(ds)?;
    if ug.point_data.get(array).is_none() {
        bail!("no cell array {array:?} in the AMR data");
    }
    let mut pd = crate::contour::contour(&DataSet::UnstructuredGrid(ug), crate::params::Association::Points, array, comp, values)?;
    // Pieces collapsed in degenerate dual cells (segments of one point, triangles without
    // area) are dropped.
    let distinct = |c: &[u32]| {
        let mut u: Vec<u32> = c.to_vec();
        u.sort_unstable();
        u.dedup();
        u.len()
    };
    let mut keep: Vec<usize> = (0..pd.verts.len()).collect();
    let (mut lines, mut polys) = (Cells::new(), Cells::new());
    let nv = pd.verts.len();
    for i in 0..pd.lines.len() {
        if distinct(pd.lines.cell(i)) >= 2 {
            lines.push(pd.lines.types[i], pd.lines.cell(i));
            keep.push(nv + i);
        }
    }
    let base = nv + pd.lines.len();
    for i in 0..pd.polys.len() {
        if distinct(pd.polys.cell(i)) >= 3 {
            polys.push(pd.polys.types[i], pd.polys.cell(i));
            keep.push(base + i);
        }
    }
    if lines.len() != pd.lines.len() || polys.len() != pd.polys.len() {
        pd.cell_data = pd.cell_data.gather(&keep);
        pd.lines = lines;
        pd.polys = polys;
    }
    Ok(pd)
}

/// One box of an AMR level as VTK's AMR information sees it: closed bounds and cell counts.
struct BoxInfo<'a> {
    lo: [f64; 3],
    hi: [f64; 3],
    h: [f64; 3],
    cells: [usize; 3],
    img: &'a ImageData,
}

impl BoxInfo<'_> {
    /// VTK's vtkAMRInformation::FindCell: the cell holding `q` (closed bounds; a point on the
    /// upper face goes to the last cell).
    fn find(&self, q: [f64; 3]) -> Option<usize> {
        if (0..3).any(|k| q[k] < self.lo[k] || q[k] > self.hi[k]) {
            return None;
        }
        let mut ijk = [0usize; 3];
        for k in 0..3 {
            if self.cells[k] <= 1 && self.h[k] == 0.0 {
                continue;
            }
            let loc = ((q[k] - self.lo[k]) / self.h[k]).floor();
            ijk[k] = if loc < 0.0 { 0 } else { (loc as usize).min(self.cells[k] - 1) };
        }
        Some(ijk[0] + self.cells[0] * (ijk[1] + self.cells[1] * ijk[2]))
    }
}

/// ParaView's Resample AMR (vtkAMRResampleFilter with demand-driven mode off): a uniform grid of
/// `samples` points over `region` (the data's bounds when None), clipped to the AMR domain,
/// with each cell array of the AMR data taken from the finest cell holding the point (point
/// data), or holding the grid cell's center when `to_nodes` is false (cell data). Points no
/// box holds are marked hidden.
pub fn resample_amr(ds: &DataSet, samples: [usize; 3], region: Option<([f64; 3], [f64; 3])>, to_nodes: bool) -> Result<ImageData> {
    let amr = aurafx_data::amr::Amr::from_dataset(ds).ok_or_else(|| anyhow!("Resample AMR needs AMR data (levels of boxes)"))?;
    let levels: Vec<Vec<BoxInfo>> = amr
        .levels
        .iter()
        .map(|l| {
            l.blocks
                .iter()
                .map(|b| {
                    let img = &b.data;
                    let d = img.dims();
                    let cells: [usize; 3] = std::array::from_fn(|k| d[k].saturating_sub(1).max(1));
                    let lo: [f64; 3] = std::array::from_fn(|k| img.origin[k] + img.extent[2 * k] as f64 * img.spacing[k]);
                    let hi: [f64; 3] = std::array::from_fn(|k| lo[k] + if d[k] > 1 { (d[k] - 1) as f64 * img.spacing[k] } else { 0.0 });
                    let h: [f64; 3] = std::array::from_fn(|k| if d[k] > 1 { img.spacing[k] } else { 0.0 });
                    BoxInfo { lo, hi, h, cells, img }
                })
                .collect()
        })
        .collect();
    let bounds = ds.bounds();
    if bounds.is_empty() || levels.is_empty() {
        bail!("the AMR data is empty");
    }
    let (dmin, dmax) = (bounds.min, bounds.max);
    let (rmin, rmax) = region.unwrap_or((dmin, dmax));
    if (0..3).any(|k| rmax[k] < dmin[k] || rmin[k] > dmax[k]) {
        bail!("the region does not meet the AMR data");
    }
    // Snap the region to the domain and drop the samples that fell outside (as VTK does).
    let gmin: [f64; 3] = std::array::from_fn(|k| rmin[k].max(dmin[k]));
    let gmax: [f64; 3] = std::array::from_fn(|k| rmax[k].min(dmax[k]));
    let mut n = [0usize; 3];
    for k in 0..3 {
        let req = samples[k].max(1);
        let rh = (rmax[k] - rmin[k]) / (req as f64 - 1.0);
        n[k] = req;
        if rmin[k] < dmin[k] || rmax[k] > dmax[k] {
            let start = if gmin[k] - rmin[k] > 0.0 { ((gmin[k] - rmin[k]) / rh + 1.0) as usize } else { 0 };
            let end = (((gmax[k] - rmin[k]) / rh + 1.0) as usize).min(req);
            let new_n = end.saturating_sub(start) + 1;
            if new_n <= req {
                n[k] = new_n;
            }
        }
        n[k] = n[k].max(2);
    }
    let h: [f64; 3] = std::array::from_fn(|k| (gmax[k] - gmin[k]) / (n[k] as f64 - 1.0));
    let mut out = ImageData { origin: gmin, spacing: h, extent: [0, n[0] as i64 - 1, 0, n[1] as i64 - 1, 0, n[2] as i64 - 1], ..Default::default() };
    // Arrays of the first box (VTK's reference grid), without the ghost flags.
    let reference = levels.iter().flatten().next().unwrap().img;
    let names: Vec<(String, usize, ScalarType)> = reference.cell_data.iter().filter(|a| a.name != aurafx_data::amr::GHOST).map(|a| (a.name.clone(), a.ncomp, a.scalar_type())).collect();
    let queries: Vec<[f64; 3]> = if to_nodes {
        (0..n[2]).flat_map(|k| (0..n[1]).flat_map(move |j| (0..n[0]).map(move |i| [i, j, k]))).map(|[i, j, k]| [gmin[0] + i as f64 * h[0], gmin[1] + j as f64 * h[1], gmin[2] + k as f64 * h[2]]).collect()
    } else {
        let c: [usize; 3] = std::array::from_fn(|k| (n[k] - 1).max(1));
        (0..c[2])
            .flat_map(|k| (0..c[1]).flat_map(move |j| (0..c[0]).map(move |i| [i, j, k])))
            .map(|[i, j, k]| std::array::from_fn(|a| gmin[a] + ([i, j, k][a] as f64 + if n[a] > 1 { 0.5 } else { 0.0 }) * h[a]))
            .collect()
    };
    // (level, box, cell) of the donor of every query point.
    let mut donors: Vec<Option<(usize, usize, usize)>> = Vec::with_capacity(queries.len());
    if to_nodes {
        // VTK's ProbeGridPointInAMR: start from the previous point's box, then go up while a
        // finer level holds the point (or down when the previous level no longer does).
        let search = |q: [f64; 3], l: usize| -> Option<(usize, usize)> { levels[l].iter().enumerate().find_map(|(b, bx)| bx.find(q).map(|c| (b, c))) };
        let mut cache: Option<(usize, usize)> = None;
        for &q in &queries {
            let mut current: Option<(usize, usize, usize)> = None;
            let mut start = 0;
            let mut descend_from = None;
            if let Some((dl, db)) = cache {
                let hit = levels[dl][db].find(q).map(|c| (db, c)).or_else(|| search(q, dl));
                match hit {
                    Some((b, c)) => {
                        current = Some((dl, b, c));
                        start = dl + 1;
                    }
                    None if dl == 0 => {
                        donors.push(None);
                        cache = None;
                        continue;
                    }
                    None => descend_from = Some(dl),
                }
            }
            let found = if let Some(top) = descend_from {
                (0..top).rev().find_map(|l| search(q, l).map(|(b, c)| (l, b, c)))
            } else {
                let mut best = current;
                for l in start..levels.len() {
                    match search(q, l) {
                        Some((b, c)) => best = Some((l, b, c)),
                        None => break,
                    }
                }
                best
            };
            cache = found.map(|(l, b, _)| (l, b));
            donors.push(found);
        }
    } else {
        // Cell centers: the last box (finest level, last in order) holding the center.
        for &q in &queries {
            let mut d = None;
            for (l, lev) in levels.iter().enumerate() {
                for (b, bx) in lev.iter().enumerate() {
                    if let Some(c) = bx.find(q) {
                        d = Some((l, b, c));
                    }
                }
            }
            donors.push(d);
        }
    }
    let mut attrs = Attributes::new();
    for (name, nc, ty) in &names {
        let mut v = vec![0.0; donors.len() * nc];
        for (i, d) in donors.iter().enumerate() {
            if let Some((l, b, c)) = d {
                if let Some(a) = levels[*l][*b].img.cell_data.get(name) {
                    for k in 0..*nc {
                        v[i * nc + k] = a.get(*c, k);
                    }
                }
            }
        }
        attrs.insert(Array::new(name.clone(), *nc, ArrayData::from_f64(*ty, &v)));
    }
    if donors.iter().any(|d| d.is_none()) {
        // Points (cells) outside every box: hidden.
        let flag = if to_nodes { 2u8 } else { 32u8 };
        attrs.insert(Array::new(aurafx_data::amr::GHOST, 1, ArrayData::U8(donors.iter().map(|d| if d.is_none() { flag } else { 0 }).collect())));
    }
    if to_nodes {
        out.point_data = attrs;
    } else {
        out.cell_data = attrs;
    }
    Ok(out)
}
