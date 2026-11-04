//! Threshold: keep cells whose scalar lies within [lo, hi].

use crate::params::Association;
use crate::simplex::{as_explicit, for_each_cell, parts};
use anyhow::{anyhow, Result};
use aurafx_data::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointMode {
    All,
    Any,
}

pub fn threshold(ds: &DataSet, assoc: Association, array: &str, comp: Option<usize>, lo: f64, hi: f64, mode: PointMode, invert: bool) -> Result<UnstructuredGrid> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("threshold: unsupported dataset"))?;
    let arr = match assoc {
        Association::Points => pd.get(array),
        Association::Cells => cd.get(array),
    }
    .ok_or_else(|| anyhow!("threshold: no {:?} array {array:?}", assoc))?;
    let val = |i: usize| match comp {
        Some(c) => arr.get(i, c),
        None => arr.magnitude(i),
    };
    let inside = |v: f64| (v >= lo && v <= hi) != invert;
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    for_each_cell(&ds, |cid, ty, pts, _| {
        let keep = match assoc {
            Association::Cells => inside(val(cid)),
            Association::Points => match mode {
                PointMode::All => pts.iter().all(|&p| inside(val(p as usize))),
                PointMode::Any => pts.iter().any(|&p| inside(val(p as usize))),
            },
        };
        if keep {
            cells.push(ty, pts);
            parents.push(cid);
        }
    });
    let (points, point_data) = crate::interp::compact_points(points, pd, &mut cells.connectivity);
    Ok(UnstructuredGrid { points, cells, point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}
