//! NetCDF following the CF conventions (`.nc`, `.nc4`, `.cdf`; classic and NetCDF-4): the
//! variables sharing the most common grid become arrays of one grid. Longitude/latitude grids
//! lie on a unit sphere as in ParaView (cell data between the `bounds` edges when present);
//! other grids are rectilinear. `scale_factor`/`add_offset` are applied and fill values become
//! NaN. The time coordinate makes time steps.

use crate::Loaded;
use anyhow::{anyhow, Context, Result};
use aurafx_data::*;
use netcdf_reader::{NcFile, NcVariable};
use std::path::Path;
use std::sync::Arc;

fn text(v: &NcVariable, a: &str) -> Option<String> {
    v.attribute(a).and_then(|x| x.value.as_string()).map(|s| s.trim_end_matches('\0').trim().to_string())
}

fn number(v: &NcVariable, a: &str) -> Option<f64> {
    v.attribute(a).and_then(|x| x.value.as_f64())
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Axis {
    Lon,
    Lat,
    Vertical,
    Time,
    Other,
}

fn axis_of(v: Option<&NcVariable>, dim: &str) -> Axis {
    let Some(v) = v else { return if dim.eq_ignore_ascii_case("time") { Axis::Time } else { Axis::Other } };
    let units = text(v, "units").unwrap_or_default().to_ascii_lowercase();
    let sn = text(v, "standard_name").unwrap_or_default().to_ascii_lowercase();
    let ax = text(v, "axis").unwrap_or_default().to_ascii_uppercase();
    if units.contains("since") || sn == "time" || ax == "T" {
        Axis::Time
    } else if units.starts_with("degrees_east") || units == "degree_east" || units == "degree_e" || sn == "longitude" || ax == "X" && units.starts_with("deg") {
        Axis::Lon
    } else if units.starts_with("degrees_north") || units == "degree_north" || units == "degree_n" || sn == "latitude" || ax == "Y" && units.starts_with("deg")
    {
        Axis::Lat
    } else if ax == "Z" || v.attribute("positive").is_some() {
        Axis::Vertical
    } else {
        Axis::Other
    }
}

struct Grid {
    /// Spatial dimensions as in the variables (slowest first), and their axes.
    dims: Vec<(String, usize, Axis)>,
    time_dim: Option<String>,
    vars: Vec<String>,
}

/// The grid most variables share (more spatial dimensions first).
fn choose_grid(f: &NcFile) -> Result<Grid> {
    let vars = f.variables().map_err(|e| anyhow!("{e}"))?;
    let coord = |name: &str| vars.iter().find(|v| v.name() == name && v.dimensions().len() == 1);
    let bounds: Vec<String> = vars.iter().filter_map(|v| text(v, "bounds")).collect();
    let mut groups: Vec<(Vec<(String, usize, Axis)>, Option<String>, Vec<String>)> = Vec::new();
    for v in vars {
        if v.is_coordinate_variable() || bounds.contains(&v.name().to_string()) || v.dimensions().is_empty() {
            continue;
        }
        if matches!(v.dtype(), netcdf_reader::NcType::Char | netcdf_reader::NcType::String) {
            continue;
        }
        let mut spatial = Vec::new();
        let mut time = None;
        for d in v.dimensions() {
            let ax = axis_of(coord(&d.name), &d.name);
            if ax == Axis::Time || (d.is_unlimited && time.is_none() && ax != Axis::Lon && ax != Axis::Lat) {
                time = Some(d.name.clone());
            } else {
                spatial.push((d.name.clone(), d.size as usize, ax));
            }
        }
        if spatial.is_empty() || spatial.len() > 3 {
            continue;
        }
        match groups.iter_mut().find(|g| g.0.iter().map(|d| &d.0).eq(spatial.iter().map(|d| &d.0))) {
            Some(g) => {
                g.2.push(v.name().to_string());
                g.1 = g.1.clone().or(time);
            }
            None => groups.push((spatial, time, vec![v.name().to_string()])),
        }
    }
    groups.sort_by_key(|g| (std::cmp::Reverse(g.0.len()), std::cmp::Reverse(g.2.len())));
    let (dims, time_dim, vars) = groups.into_iter().next().ok_or_else(|| anyhow!("no gridded variables"))?;
    Ok(Grid { dims, time_dim, vars })
}

fn read_all(f: &NcFile, name: &str) -> Result<Vec<f64>> {
    Ok(f.read_variable_as_f64(name).map_err(|e| anyhow!("reading {name}: {e}"))?.into_iter().collect())
}

/// Cell edges from a coordinate's `bounds` variable ((n, 2) values), if it has one.
fn edges(f: &NcFile, v: &NcVariable) -> Option<Vec<f64>> {
    let b = text(v, "bounds")?;
    let vals = read_all(f, &b).ok()?;
    let n = v.dimensions()[0].size as usize;
    if vals.len() != 2 * n {
        return None;
    }
    let mut e: Vec<f64> = (0..n).map(|i| vals[2 * i]).collect();
    e.push(vals[2 * n - 1]);
    Some(e)
}

/// Values of a variable at a time step, unpacked, fill values as NaN.
fn values(f: &NcFile, v: &NcVariable, time_dim: Option<&str>, step: usize) -> Result<Vec<f64>> {
    let name = v.name();
    let raw: Vec<f64> = match v.dimensions().iter().position(|d| Some(d.name.as_str()) == time_dim) {
        Some(k) => {
            let mut sel = netcdf_reader::NcSliceInfo::all(v.dimensions().len());
            sel.selections[k] = netcdf_reader::NcSliceInfoElem::Index(step as u64);
            f.read_variable_slice_as_f64(name, &sel).map_err(|e| anyhow!("reading {name}: {e}"))?.into_iter().collect()
        }
        None => read_all(f, name)?,
    };
    let fill: Vec<f64> = ["_FillValue", "missing_value"].iter().filter_map(|a| number(v, a)).collect();
    let (scale, offset) = (number(v, "scale_factor").unwrap_or(1.0), number(v, "add_offset").unwrap_or(0.0));
    Ok(raw
        .into_iter()
        .map(|x| if fill.iter().any(|f| x == *f || (f.abs() > 1e30 && (x - f).abs() <= f.abs() * 1e-6)) { f64::NAN } else { x * scale + offset })
        .collect())
}

fn read_grid(path: &Path, step: usize) -> Result<DataSet> {
    let f = NcFile::open(path).map_err(|e| anyhow!("NetCDF file {}: {e}", path.display()))?;
    let g = choose_grid(&f)?;
    let vars = f.variables().map_err(|e| anyhow!("{e}"))?;
    let coord = |name: &str| vars.iter().find(|v| v.name() == name && v.dimensions().len() == 1);
    // Axes x fastest: the variables' last dimension first.
    let rev: Vec<&(String, usize, Axis)> = g.dims.iter().rev().collect();
    let mut centers: Vec<Vec<f64>> = Vec::new();
    let mut cell_edges: Vec<Option<Vec<f64>>> = Vec::new();
    for (name, n, _) in &rev {
        match coord(name) {
            Some(c) => {
                centers.push(read_all(&f, name)?);
                cell_edges.push(edges(&f, c));
            }
            None => {
                centers.push((0..*n).map(|i| i as f64).collect());
                cell_edges.push(None);
            }
        }
    }
    // Cell data between the bounds when every axis has them.
    let cells = cell_edges.iter().all(|e| e.is_some());
    let axes: Vec<Vec<f64>> = if cells { cell_edges.into_iter().map(|e| e.unwrap()).collect() } else { centers };
    let mut n = [1usize; 3];
    for (k, a) in axes.iter().enumerate() {
        n[k] = a.len();
    }
    let extent = [0, n[0] as i64 - 1, 0, n[1] as i64 - 1, 0, n[2] as i64 - 1];
    let lon = rev.iter().position(|d| d.2 == Axis::Lon);
    let lat = rev.iter().position(|d| d.2 == Axis::Lat);
    let mut point_data = Attributes::new();
    let mut cell_data = Attributes::new();
    for name in &g.vars {
        let Some(v) = vars.iter().find(|v| v.name() == name) else { continue };
        let vals = values(&f, v, g.time_dim.as_deref(), step).with_context(|| format!("variable {name}"))?;
        let arr = Array::scalars_f64(name.clone(), vals);
        if cells {
            cell_data.insert(arr);
        } else {
            point_data.insert(arr);
        }
    }
    let ds = match (lon, lat) {
        (Some(il), Some(ia)) => {
            // On a unit sphere; a vertical axis is the radius (scale 1, bias 0, as ParaView).
            let iv = (0..axes.len()).find(|&k| k != il && k != ia);
            let mut pts = Vec::with_capacity(3 * n[0] * n[1] * n[2]);
            for k in 0..n[2] {
                for j in 0..n[1] {
                    for i in 0..n[0] {
                        let idx = [i, j, k];
                        let (lo, la) = (axes[il][idx[il]].to_radians(), axes[ia][idx[ia]].to_radians());
                        let r = iv.map(|v| axes[v][idx[v]]).unwrap_or(1.0);
                        pts.extend_from_slice(&[r * la.cos() * lo.cos(), r * la.cos() * lo.sin(), r * la.sin()]);
                    }
                }
            }
            DataSet::StructuredGrid(StructuredGrid { extent, points: Points::new_f64(pts), point_data, cell_data, field_data: Attributes::new() })
        }
        _ => {
            let get = |k: usize| axes.get(k).cloned().unwrap_or(vec![0.0]);
            DataSet::RectilinearGrid(RectilinearGrid { extent, x: get(0), y: get(1), z: get(2), point_data, cell_data, field_data: Attributes::new() })
        }
    };
    Ok(ds)
}

/// Time values (the time coordinate's numbers, as ParaView shows them).
fn times(path: &Path) -> Result<Vec<f64>> {
    let f = NcFile::open(path).map_err(|e| anyhow!("NetCDF file {}: {e}", path.display()))?;
    let g = choose_grid(&f)?;
    let Some(t) = g.time_dim else { return Ok(Vec::new()) };
    let n = f.dimension(&t).map(|d| d.size as usize).unwrap_or(0);
    Ok(match f.variable(&t) {
        Ok(_) => read_all(&f, &t)?,
        Err(_) => (0..n).map(|i| i as f64).collect(),
    })
}

pub fn open(path: &Path) -> Result<Loaded> {
    let t = times(path)?;
    if t.len() > 1 {
        return Ok(Loaded::Series(TimeSeries {
            times: t.clone(),
            steps: (0..t.len()).map(|i| TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i }).collect(),
        }));
    }
    Ok(Loaded::Static(Arc::new(read_grid(path, 0)?)))
}

pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    read_grid(path, step)
}

pub fn is_netcdf_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "nc" | "nc4" | "cdf" | "netcdf"))
}

/// Exodus files are NetCDF too; they have a `num_nodes` dimension.
pub fn is_exodus(path: &Path) -> bool {
    NcFile::open(path).ok().is_some_and(|f| f.dimension("num_nodes").is_ok() || f.dimension("num_dim").is_ok())
}
