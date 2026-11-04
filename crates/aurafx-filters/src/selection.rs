//! Selections: extract cells/points by global index, or by a boolean expression ("find data").

use crate::params::Association;
use crate::simplex::{as_explicit, for_each_cell, parts};
use anyhow::{anyhow, Result};
use aurafx_data::*;

/// Leaves of a dataset in a fixed order, each with its cell/point index offset into the
/// "flattened" numbering used by selections.
pub fn flat_leaves(ds: &DataSet) -> Vec<(DataSet, usize, usize)> {
    let leaves: Vec<DataSet> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|(_, d)| d.clone()).collect(),
        d => vec![d.clone()],
    };
    let mut out = Vec::new();
    let (mut c0, mut p0) = (0usize, 0usize);
    for l in leaves {
        let (nc, np) = (l.n_cells(), l.n_points());
        out.push((l, c0, p0));
        c0 += nc;
        p0 += np;
    }
    out
}

fn extract_cells_leaf(ds: &DataSet, ids: &[usize]) -> Result<UnstructuredGrid> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("selection: unsupported dataset"))?;
    let want: std::collections::HashSet<usize> = ids.iter().copied().collect();
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    for_each_cell(&ds, |cid, ty, pts, _| {
        if want.contains(&cid) {
            cells.push(ty, pts);
            parents.push(cid);
        }
    });
    let (points, point_data) = crate::interp::compact_points(points, pd, &mut cells.connectivity);
    let mut cell_data = cd.gather(&parents);
    cell_data.insert(Array::new("vtkOriginalCellIds", 1, ArrayData::I64(parents.iter().map(|&i| i as i64).collect())));
    Ok(UnstructuredGrid { points, cells, point_data, cell_data, field_data: fd.clone() })
}

fn extract_points_leaf(ds: &DataSet, ids: &[usize]) -> Result<PolyData> {
    let pts = ds.points().ok_or_else(|| anyhow!("selection: dataset has no points"))?;
    let pd = ds.point_data().cloned().unwrap_or_default();
    let ids: Vec<usize> = ids.iter().copied().filter(|&i| i < pts.len()).collect();
    let mut out = PolyData::from_points(pts.gather(&ids));
    out.point_data = pd.gather(&ids);
    out.point_data.insert(Array::new("vtkOriginalPointIds", 1, ArrayData::I64(ids.iter().map(|&i| i as i64).collect())));
    Ok(out)
}

/// Extract by flattened ids. Returns a single dataset (multi-block when the input had several leaves).
pub fn extract_ids(ds: &DataSet, assoc: Association, ids: &[usize]) -> Result<DataSet> {
    let leaves = flat_leaves(ds);
    let mut blocks = Vec::new();
    for (i, (leaf, c0, p0)) in leaves.iter().enumerate() {
        let (base, n) = match assoc {
            Association::Cells => (*c0, leaf.n_cells()),
            Association::Points => (*p0, leaf.n_points()),
        };
        let local: Vec<usize> = ids.iter().filter(|&&g| g >= base && g < base + n).map(|&g| g - base).collect();
        if local.is_empty() {
            continue;
        }
        let d = match assoc {
            Association::Cells => DataSet::UnstructuredGrid(extract_cells_leaf(leaf, &local)?),
            Association::Points => DataSet::PolyData(extract_points_leaf(leaf, &local)?),
        };
        blocks.push((format!("block {i}"), Some(d)));
    }
    Ok(match blocks.len() {
        0 => match assoc {
            Association::Cells => DataSet::UnstructuredGrid(UnstructuredGrid::default()),
            Association::Points => DataSet::PolyData(PolyData::default()),
        },
        1 => blocks.pop().unwrap().1.unwrap(),
        _ => DataSet::MultiBlock(MultiBlock { blocks, field_data: Attributes::new() }),
    })
}

/// Flattened ids of cells/points where `expression` (calculator syntax) is non-zero.
pub fn find_ids(ds: &DataSet, assoc: Association, expression: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for (leaf, c0, p0) in flat_leaves(ds) {
        // Cell queries may use point arrays: average them onto cells (cell arrays win on name clashes).
        let leaf = if assoc == Association::Cells {
            match crate::attributes::point_to_cell(&leaf, &[]) {
                Ok(mut conv) => {
                    if let (Some(orig), Some(cd)) = (leaf.cell_data(), conv.cell_data_mut()) {
                        for a in orig.iter() {
                            cd.insert(a.clone());
                        }
                    }
                    conv
                }
                Err(_) => leaf,
            }
        } else {
            leaf
        };
        let with = crate::calculator::calculator(&leaf, assoc, expression, "__selected")?;
        let attrs = match assoc {
            Association::Points => with.point_data(),
            Association::Cells => with.cell_data(),
        };
        let a = attrs.and_then(|a| a.get("__selected")).ok_or_else(|| anyhow!("expression produced no values"))?;
        let base = if assoc == Association::Cells { c0 } else { p0 };
        for i in 0..a.len() {
            let v = a.magnitude(i);
            if v != 0.0 && v.is_finite() {
                out.push(base + i);
            }
        }
    }
    Ok(out)
}
