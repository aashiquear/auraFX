//! Overlapping AMR (adaptive mesh refinement) data, held as a multi-block dataset: one block per
//! level ("Level 0", "Level 1", ...), each holding that level's boxes as image data. Coarse
//! cells covered by a finer box are marked in the `vtkGhostType` cell array (REFINEDCELL), as
//! VTK's `vtkOverlappingAMR` does, so filters and drawing skip them and each place in space is
//! counted once.

use crate::{Array, ArrayData, Attributes, DataSet, ImageData, MultiBlock};

/// The ghost array of VTK (cell and point flags).
pub const GHOST: &str = "vtkGhostType";
/// A cell covered by finer cells.
pub const REFINED_CELL: u8 = 8;
/// A cell whose values must be ignored.
pub const HIDDEN_CELL: u8 = 32;
/// The cells VTK treats as not there (vtkStructuredData::IsCellVisible).
pub const MASKED: u8 = REFINED_CELL | HIDDEN_CELL;
/// Field data of the root block marking an AMR dataset.
pub const AMR_MARKER: &str = "vtkAMRDataType";

/// Which cells are hidden (refined or blanked), or None when no cell is.
pub fn hidden_cells(cell_data: &Attributes) -> Option<Vec<bool>> {
    let a = cell_data.get(GHOST)?;
    let hidden: Vec<bool> = match &a.data {
        ArrayData::U8(v) => v.iter().map(|&g| g & MASKED != 0).collect(),
        d => (0..d.len()).map(|i| (d.get_f64(i) as i64 as u8) & MASKED != 0).collect(),
    };
    hidden.iter().any(|&h| h).then_some(hidden)
}

/// Whether a dataset has hidden cells.
pub fn has_hidden_cells(ds: &DataSet) -> bool {
    ds.cell_data().and_then(hidden_cells).is_some()
}

/// One box of a level: its cells (inclusive index range in the level's index space, measured
/// from the AMR origin) and its data.
#[derive(Debug, Clone)]
pub struct AmrBlock {
    pub lo: [i64; 3],
    pub hi: [i64; 3],
    pub data: ImageData,
}

#[derive(Debug, Clone, Default)]
pub struct AmrLevel {
    pub spacing: [f64; 3],
    pub blocks: Vec<AmrBlock>,
}

/// An overlapping AMR hierarchy.
#[derive(Debug, Clone, Default)]
pub struct Amr {
    pub origin: [f64; 3],
    pub levels: Vec<AmrLevel>,
}

/// Number of cells along each axis of an image (an axis with one point counts as one cell
/// layer for indexing, as in 2-D AMR).
fn cells_per_axis(img: &ImageData) -> [i64; 3] {
    let d = img.dims();
    [((d[0] as i64) - 1).max(1), ((d[1] as i64) - 1).max(1), ((d[2] as i64) - 1).max(1)]
}

impl AmrBlock {
    /// A box for `data` on a level with `spacing`, placed relative to `origin`.
    pub fn locate(origin: [f64; 3], spacing: [f64; 3], data: ImageData) -> Self {
        let c = cells_per_axis(&data);
        let o = data.origin;
        let mut lo = [0i64; 3];
        for k in 0..3 {
            let first = o[k] + data.extent[2 * k] as f64 * data.spacing[k];
            lo[k] = if spacing[k] > 0.0 { ((first - origin[k]) / spacing[k]).round() as i64 } else { 0 };
        }
        let hi = [lo[0] + c[0] - 1, lo[1] + c[1] - 1, lo[2] + c[2] - 1];
        AmrBlock { lo, hi, data }
    }
}

impl Amr {
    /// Refinement ratio between level `l` and `l + 1` along each axis.
    pub fn ratio(&self, l: usize) -> [i64; 3] {
        let (a, b) = (self.levels[l].spacing, self.levels[l + 1].spacing);
        std::array::from_fn(|k| if b[k] > 0.0 { (a[k] / b[k]).round().max(1.0) as i64 } else { 1 })
    }

    /// Mark the cells of each level covered by the next level's boxes (REFINEDCELL), keeping
    /// other ghost flags.
    pub fn blank(&mut self) {
        for l in 0..self.levels.len() {
            let ratio = if l + 1 < self.levels.len() { Some(self.ratio(l)) } else { None };
            let fine: Vec<([i64; 3], [i64; 3])> = match ratio {
                Some(r) => self.levels[l + 1]
                    .blocks
                    .iter()
                    .map(|b| (std::array::from_fn(|k| b.lo[k].div_euclid(r[k])), std::array::from_fn(|k| b.hi[k].div_euclid(r[k]))))
                    .collect(),
                None => Vec::new(),
            };
            for blk in &mut self.levels[l].blocks {
                let c = cells_per_axis(&blk.data);
                let n = (c[0] * c[1] * c[2]) as usize;
                let mut g: Vec<u8> = match blk.data.cell_data.get(GHOST) {
                    Some(a) if a.data.len() == n => (0..n).map(|i| (a.data.get_f64(i) as i64 as u8) & !REFINED_CELL).collect(),
                    _ => vec![0; n],
                };
                for (flo, fhi) in &fine {
                    let lo: [i64; 3] = std::array::from_fn(|k| flo[k].max(blk.lo[k]));
                    let hi: [i64; 3] = std::array::from_fn(|k| fhi[k].min(blk.hi[k]));
                    if (0..3).any(|k| lo[k] > hi[k]) {
                        continue;
                    }
                    for z in lo[2]..=hi[2] {
                        for y in lo[1]..=hi[1] {
                            for x in lo[0]..=hi[0] {
                                let (i, j, kk) = (x - blk.lo[0], y - blk.lo[1], z - blk.lo[2]);
                                g[(i + c[0] * (j + c[1] * kk)) as usize] |= REFINED_CELL;
                            }
                        }
                    }
                }
                blk.data.cell_data.insert(Array::new(GHOST, 1, ArrayData::U8(g)));
            }
        }
    }

    /// The multi-block form: "Level n" blocks holding "Block m" images.
    pub fn into_dataset(self) -> DataSet {
        let mut field_data = Attributes::new();
        field_data.insert(Array::new(AMR_MARKER, 1, ArrayData::String(vec!["Overlapping AMR".into()])));
        field_data.insert(Array::new("vtkAMROrigin", 3, ArrayData::F64(self.origin.to_vec())));
        let blocks = self
            .levels
            .into_iter()
            .enumerate()
            .map(|(l, lev)| {
                let mut fd = Attributes::new();
                fd.insert(Array::new("vtkAMRSpacing", 3, ArrayData::F64(lev.spacing.to_vec())));
                let kids = lev.blocks.into_iter().enumerate().map(|(b, blk)| (format!("Block {b}"), Some(DataSet::ImageData(blk.data)))).collect();
                (format!("Level {l}"), Some(DataSet::MultiBlock(MultiBlock { blocks: kids, field_data: fd })))
            })
            .collect();
        DataSet::MultiBlock(MultiBlock { blocks, field_data })
    }

    /// Back from the multi-block form (None when `ds` is not AMR).
    pub fn from_dataset(ds: &DataSet) -> Option<Amr> {
        let DataSet::MultiBlock(mb) = ds else { return None };
        mb.field_data.get(AMR_MARKER)?;
        let origin = mb.field_data.get("vtkAMROrigin").map(|a| [a.get(0, 0), a.get(0, 1), a.get(0, 2)]).unwrap_or([0.0; 3]);
        let mut levels = Vec::new();
        for (_, lev) in &mb.blocks {
            let Some(DataSet::MultiBlock(lev)) = lev else { return None };
            let first = lev.blocks.iter().find_map(|(_, b)| match b {
                Some(DataSet::ImageData(i)) => Some(i.spacing),
                _ => None,
            });
            let spacing = lev.field_data.get("vtkAMRSpacing").map(|a| [a.get(0, 0), a.get(0, 1), a.get(0, 2)]).or(first).unwrap_or([1.0; 3]);
            let blocks = lev
                .blocks
                .iter()
                .filter_map(|(_, b)| match b {
                    Some(DataSet::ImageData(i)) => Some(AmrBlock::locate(origin, spacing, i.clone())),
                    _ => None,
                })
                .collect();
            levels.push(AmrLevel { spacing, blocks });
        }
        Some(Amr { origin, levels })
    }
}

/// Whether a dataset is AMR data in the multi-block form.
pub fn is_amr(ds: &DataSet) -> bool {
    matches!(ds, DataSet::MultiBlock(mb) if mb.field_data.get(AMR_MARKER).is_some())
}
