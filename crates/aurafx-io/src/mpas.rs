//! MPAS (Model for Prediction Across Scales) NetCDF output, read as ParaView's MPAS reader does
//! by default: the dual grid, whose points are the MPAS cell centers (`xCell`, `yCell`,
//! `zCell`) and whose cells are the triangles (or quads) joining the cells around each MPAS
//! vertex (`cellsOnVertex`). Variables on `nCells` become point data and variables on
//! `nVertices` cell data, with the first index of any other dimension (the top vertical
//! level). Time steps are numbered 0, 1, 2... as in ParaView; `xtime` goes to field data.
//!
//! Two differences from ParaView, which keeps every MPAS vertex as a cell: triangles that
//! reach outside the mesh (boundary vertices) and, with `maxLevelCell`, triangles touching a
//! land cell are left out instead of being collapsed onto an extra point at the origin.

use crate::Loaded;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use netcdf_reader::{NcFile, NcSliceInfo, NcSliceInfoElem, NcVariable};
use std::path::Path;
use std::sync::Arc;

/// Whether a NetCDF file is MPAS output (it has the dual-grid connectivity and cell centers).
pub fn is_mpas(f: &NcFile) -> bool {
    ["nCells", "nVertices", "vertexDegree"].iter().all(|d| f.dimension(d).is_ok())
        && ["xCell", "yCell", "zCell", "cellsOnVertex"].iter().all(|v| f.variable(v).is_ok())
}

pub fn is_mpas_file(path: &Path) -> bool {
    NcFile::open(path).ok().is_some_and(|f| is_mpas(&f))
}

fn read_all(f: &NcFile, name: &str) -> Result<Vec<f64>> {
    Ok(f.read_variable_as_f64(name).map_err(|e| anyhow!("reading {name}: {e}"))?.into_iter().collect())
}

/// Values of a variable on `nCells` or `nVertices` at a time step: the first index of every
/// other dimension.
fn values(f: &NcFile, v: &NcVariable, step: usize) -> Result<Vec<f64>> {
    let dims = v.dimensions();
    let mut sel = NcSliceInfo::all(dims.len());
    for (k, d) in dims.iter().enumerate() {
        match d.name.as_str() {
            "nCells" | "nVertices" => {}
            "Time" => sel.selections[k] = NcSliceInfoElem::Index(step as u64),
            _ => sel.selections[k] = NcSliceInfoElem::Index(0),
        }
    }
    Ok(f.read_variable_slice_as_f64(v.name(), &sel).map_err(|e| anyhow!("reading {}: {e}", v.name()))?.into_iter().collect())
}

/// Which mesh dimension a variable lives on: `[Time,] nCells|nVertices, ...`.
fn location(v: &NcVariable) -> Option<&'static str> {
    let dims = v.dimensions();
    let first = if dims.first().is_some_and(|d| d.name == "Time") { dims.get(1) } else { dims.first() };
    match first.map(|d| d.name.as_str()) {
        Some("nCells") => Some("nCells"),
        Some("nVertices") => Some("nVertices"),
        _ => None,
    }
}

fn steps_in(f: &NcFile) -> usize {
    f.dimension("Time").map(|d| d.size as usize).unwrap_or(0)
}

fn read_grid(path: &Path, step: usize) -> Result<DataSet> {
    let f = NcFile::open(path).map_err(|e| anyhow!("NetCDF file {}: {e}", path.display()))?;
    if !is_mpas(&f) {
        bail!("{} is not MPAS output (no cellsOnVertex/xCell)", path.display());
    }
    let dim = |n: &str| f.dimension(n).map(|d| d.size as usize).unwrap_or(0);
    let (np, degree, nverts) = (dim("nCells"), dim("vertexDegree"), dim("nVertices"));
    let ty = match degree {
        3 => CellType::Triangle,
        4 => CellType::Quad,
        d => bail!("MPAS vertexDegree {d}: only triangle (3) and quad (4) dual grids are supported"),
    };
    let (x, y, z) = (read_all(&f, "xCell")?, read_all(&f, "yCell")?, read_all(&f, "zCell")?);
    if x.len() != np || y.len() != np || z.len() != np {
        bail!("xCell/yCell/zCell do not have nCells values");
    }
    let mut pts = Vec::with_capacity(3 * np);
    for i in 0..np {
        pts.extend_from_slice(&[x[i], y[i], z[i]]);
    }
    let conn = read_all(&f, "cellsOnVertex")?;
    if conn.len() != nverts * degree {
        bail!("cellsOnVertex does not have nVertices x vertexDegree values");
    }
    // Land cells (the top level is below the sea floor) hide the triangles that touch them.
    let max_level = f.variable("maxLevelCell").ok().and_then(|_| read_all(&f, "maxLevelCell").ok()).filter(|m| m.len() == np);
    let mut cells = Cells::new();
    let mut kept = Vec::with_capacity(nverts);
    let mut ids = vec![0u32; degree];
    'verts: for v in 0..nverts {
        for k in 0..degree {
            let c = conn[v * degree + k];
            if c < 1.0 || c > np as f64 {
                continue 'verts;
            }
            ids[k] = c as u32 - 1;
            if max_level.as_ref().is_some_and(|m| m[ids[k] as usize] < 1.0) {
                continue 'verts;
            }
        }
        cells.push(ty, &ids);
        kept.push(v);
    }
    let mut pd = Attributes::new();
    let mut cd = Attributes::new();
    let vars = f.variables().map_err(|e| anyhow!("{e}"))?;
    for v in vars {
        let Some(loc) = location(v) else { continue };
        let Ok(vals) = values(&f, v, step) else { continue };
        if loc == "nCells" && vals.len() == np {
            pd.insert(Array::scalars_f64(v.name().to_string(), vals));
        } else if loc == "nVertices" && vals.len() == nverts {
            cd.insert(Array::scalars_f64(v.name().to_string(), kept.iter().map(|&i| vals[i]).collect()));
        }
    }
    let mut fd = Attributes::new();
    if let Ok(t) = f.read_variable_as_strings("xtime") {
        if let Some(s) = t.get(step.min(t.len().saturating_sub(1))) {
            fd.insert(Array::new("xtime", 1, ArrayData::String(vec![s.trim_end_matches('\0').trim().to_string()])));
        }
    }
    Ok(DataSet::UnstructuredGrid(UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: pd, cell_data: cd, field_data: fd }))
}

pub fn open(path: &Path) -> Result<Loaded> {
    let f = NcFile::open(path).map_err(|e| anyhow!("NetCDF file {}: {e}", path.display()))?;
    let n = steps_in(&f);
    if n > 1 {
        return Ok(Loaded::Series(TimeSeries {
            times: (0..n).map(|i| i as f64).collect(),
            steps: (0..n).map(|i| TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i }).collect(),
        }));
    }
    Ok(Loaded::Static(Arc::new(read_grid(path, 0)?)))
}

pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    read_grid(path, step)
}
