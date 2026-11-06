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
