//! Cell <-> point data conversion, array extraction and cell centers.

use crate::simplex::{as_explicit, for_each_cell, parts};
use anyhow::{anyhow, Result};
use aurafx_data::*;

/// Average cell arrays onto points. Empty `names` means all arrays.
pub fn cell_to_point(input: &DataSet, names: &[String]) -> Result<DataSet> {
    let ds = as_explicit(input);
    let ds = &*ds;
    let (points, _, cd, _) = parts(ds).ok_or_else(|| anyhow!("cell_to_point: unsupported dataset"))?;
    let n = points.len();
    let mut new_arrays = Vec::new();
    let mut counts = vec![0u32; n];
    let mut counted = false;
    for arr in cd.iter() {
        if !names.is_empty() && !names.contains(&arr.name) {
            continue;
        }
        if arr.scalar_type() == ScalarType::String {
            continue;
        }
        let nc = arr.ncomp;
        let mut sum = vec![0.0f64; n * nc];
        for_each_cell(ds, |cid, _, pts, _| {
            for &p in pts {
                let p = p as usize;
                for c in 0..nc {
                    sum[p * nc + c] += arr.get(cid, c);
                }
                if !counted {
                    counts[p] += 1;
                }
            }
        });
        counted = true;
        for i in 0..n {
            let k = counts[i].max(1) as f64;
            for c in 0..nc {
                sum[i * nc + c] /= k;
            }
        }
        let ty = if arr.scalar_type().is_float() { arr.scalar_type() } else { ScalarType::F64 };
        let mut a = Array::new(arr.name.clone(), nc, ArrayData::from_f64(ty, &sum));
        a.component_names = arr.component_names.clone();
        new_arrays.push(a);
    }
    // Structured inputs stay structured (cell and point order match the explicit copy).
    let mut out = input.clone();
    let pd = out.point_data_mut().ok_or_else(|| anyhow!("cell_to_point: unsupported dataset"))?;
    for a in new_arrays {
        if pd.scalars().is_none() && a.ncomp == 1 {
            pd.insert_active(AttributeKind::Scalars, a);
        } else {
            pd.insert(a);
        }
    }
    Ok(out)
}

/// Average point arrays onto cells.
pub fn point_to_cell(input: &DataSet, names: &[String]) -> Result<DataSet> {
    let ds = as_explicit(input);
    let ds = &*ds;
    let (_, pd, _, _) = parts(ds).ok_or_else(|| anyhow!("point_to_cell: unsupported dataset"))?;
    let ncells = ds.n_cells();
    let mut new_arrays = Vec::new();
    for arr in pd.iter() {
        if !names.is_empty() && !names.contains(&arr.name) {
            continue;
        }
        if arr.scalar_type() == ScalarType::String {
            continue;
        }
        let nc = arr.ncomp;
        let mut out = vec![0.0f64; ncells * nc];
        for_each_cell(ds, |cid, _, pts, _| {
            let k = pts.len().max(1) as f64;
            for &p in pts {
                for c in 0..nc {
                    out[cid * nc + c] += arr.get(p as usize, c) / k;
                }
            }
        });
        let ty = if arr.scalar_type().is_float() { arr.scalar_type() } else { ScalarType::F64 };
        let mut a = Array::new(arr.name.clone(), nc, ArrayData::from_f64(ty, &out));
        a.component_names = arr.component_names.clone();
        new_arrays.push(a);
    }
    let mut out = input.clone();
    let cd = out.cell_data_mut().ok_or_else(|| anyhow!("point_to_cell: unsupported dataset"))?;
    for a in new_arrays {
        if cd.scalars().is_none() && a.ncomp == 1 {
            cd.insert_active(AttributeKind::Scalars, a);
        } else {
            cd.insert(a);
        }
    }
    Ok(out)
}

/// Cell centers as a point cloud carrying the cell data as point data.
pub fn cell_centers(ds: &DataSet) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, _, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("cell_centers: unsupported dataset"))?;
    let mut centers = Vec::with_capacity(ds.n_cells() * 3);
    for_each_cell(&ds, |_, _, pts, _| {
        let mut c = [0.0; 3];
        for &p in pts {
            let q = points.get(p as usize);
            c[0] += q[0];
            c[1] += q[1];
            c[2] += q[2];
        }
        let k = pts.len().max(1) as f64;
        centers.extend_from_slice(&[c[0] / k, c[1] / k, c[2] / k]);
    });
    let mut pd = PolyData::from_points(Points::new_f64(centers));
    pd.point_data = cd.clone();
    pd.field_data = fd.clone();
    Ok(pd)
}

/// Keep only the listed arrays (point and cell).
pub fn pass_arrays(ds: &DataSet, point_names: &[String], cell_names: &[String]) -> DataSet {
    let mut out = ds.clone();
    if let Some(pd) = out.point_data_mut() {
        pd.retain(|a| point_names.contains(&a.name));
    }
    if let Some(cd) = out.cell_data_mut() {
        cd.retain(|a| cell_names.contains(&a.name));
    }
    out
}
