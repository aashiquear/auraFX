//! Threshold: keep cells whose scalar lies within [lo, hi].

use crate::params::Association;
use crate::simplex::{as_explicit, parts};
use anyhow::{anyhow, Result};
use aurafx_data::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointMode {
    All,
    Any,
}

pub fn threshold(
    ds: &DataSet,
    assoc: Association,
    array: &str,
    comp: Option<usize>,
    lo: f64,
    hi: f64,
    mode: PointMode,
    invert: bool,
) -> Result<UnstructuredGrid> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("threshold: unsupported dataset"))?;
    // A name given without its association is looked up on the other side too.
    let on = |a: Association| match a {
        Association::Points => pd.get(array).map(|x| (a, x)),
        Association::Cells => cd.get(array).map(|x| (a, x)),
    };
    let other = if assoc == Association::Points { Association::Cells } else { Association::Points };
    let (assoc, arr) = on(assoc).or_else(|| on(other)).ok_or_else(|| anyhow!("threshold: no array {array:?}"))?;
    let val = |i: usize| match comp {
        Some(c) => arr.get(i, c),
        None => arr.magnitude(i),
    };
    let inside = |v: f64| (v >= lo && v <= hi) != invert;
    use rayon::prelude::*;
    // Point (or cell) test results, computed once in parallel.
    let n_vals = match assoc {
        Association::Cells => ds.n_cells(),
        Association::Points => points.len(),
    };
    let ok: Vec<bool> = (0..n_vals).into_par_iter().with_min_len(8192).map(|i| inside(val(i))).collect();
    let chunks = crate::interp::par_cell_chunks(
        &ds,
        || (Cells::new(), Vec::<usize>::new()),
        |(cells, parents), cid, ty, pts, _| {
            let keep = match assoc {
                Association::Cells => ok[cid],
                Association::Points => match mode {
                    PointMode::All => pts.iter().all(|&p| ok[p as usize]),
                    PointMode::Any => pts.iter().any(|&p| ok[p as usize]),
                },
            };
            if keep {
                cells.push(ty, pts);
                parents.push(cid);
            }
        },
    );
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    for (c, p) in chunks {
        cells.append(&c, 0);
        parents.extend(p);
    }
    let (points, point_data) = crate::interp::compact_points(points, pd, &mut cells.connectivity);
    Ok(UnstructuredGrid { points, cells, point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}
