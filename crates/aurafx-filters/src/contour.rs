//! Contour (isosurface / isoline) by one or more values of a point scalar.

use crate::attributes::cell_to_point;
use crate::interp::ScalarFn;
use crate::params::Association;
use crate::simplex::parts;
use crate::slice::cut_at_zero;
use anyhow::{anyhow, Result};
use aurafx_data::*;

pub fn contour(ds: &DataSet, assoc: Association, array: &str, comp: Option<usize>, values: &[f64]) -> Result<PolyData> {
    let ds = crate::higher_order::as_linear(ds);
    let ds: std::borrow::Cow<DataSet> = if assoc == Association::Cells { std::borrow::Cow::Owned(cell_to_point(&ds, &[array.to_string()])?) } else { ds };
    let (_, pd, ..) = parts(&ds).ok_or_else(|| anyhow!("contour: unsupported dataset"))?;
    let arr = pd.get(array).ok_or_else(|| anyhow!("contour: no point array named {array:?}"))?;
    let mut out: Option<PolyData> = None;
    for &iso in values {
        let f = ScalarFn::Array { arr, comp, offset: iso };
        let mut piece = cut_at_zero(&ds, &f, true)?;
        // Mark the contour value as a cell array for convenience.
        let nc = piece.n_cells();
        piece.cell_data.insert(Array::scalars_f64("IsoValue", vec![iso; nc]));
        match &mut out {
            None => out = Some(piece),
            Some(acc) => {
                let off = acc.points.len() as u32;
                acc.points.append(&piece.points);
                acc.verts.append(&piece.verts, off);
                acc.lines.append(&piece.lines, off);
                acc.polys.append(&piece.polys, off);
                for a in piece.point_data.iter() {
                    if let Some(dst) = acc.point_data.get_mut(&a.name) {
                        let _ = dst.data.append(&a.data);
                    }
                }
                // Cell data only kept when there is a single iso value (ordering would otherwise interleave).
                acc.cell_data = Attributes::new();
            }
        }
    }
    Ok(out.unwrap_or_default())
}
