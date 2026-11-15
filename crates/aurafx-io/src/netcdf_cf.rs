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

/// Every grid variables are on, best first (more spatial dimensions, then more variables).
fn grids(f: &NcFile) -> Result<Vec<Grid>> {
    let vars = f.variables().map_err(|e| anyhow!("{e}"))?;
    let coord = |name: &str| vars.iter().find(|v| v.name() == name && v.dimensions().len() == 1);
    let mut bounds: Vec<String> = vars.iter().filter_map(|v| text(v, "bounds")).collect();
    // Auxiliary coordinates (2-D longitude/latitude named in `coordinates`) are not data.
    bounds.extend(vars.iter().filter_map(|v| text(v, "coordinates")).flat_map(|c| c.split_whitespace().map(String::from).collect::<Vec<_>>()));
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
    Ok(groups.into_iter().map(|(dims, time_dim, vars)| Grid { dims, time_dim, vars }).collect())
}

/// How a grid is named in the "Variables" option: its dimensions, as ParaView lists them.
fn grid_label(g: &Grid) -> String {
    format!("({})", g.dims.iter().map(|d| d.0.as_str()).collect::<Vec<_>>().join(", "))
}

/// The grid the "variables" reader option asks for, else the one most variables share.
fn choose_grid(f: &NcFile) -> Result<Grid> {
    let all = grids(f)?;
    let want = crate::option("variables");
    let k = want.and_then(|w| all.iter().position(|g| grid_label(g) == w)).unwrap_or(0);
    all.into_iter().nth(k).ok_or_else(|| anyhow!("no gridded variables"))
}

/// Reader options of a CF file: which grid's variables to read when there are several.
pub fn options(path: &Path) -> Vec<crate::ReaderOption> {
    let Ok(f) = NcFile::open(path) else { return Vec::new() };
    let Ok(all) = grids(&f) else { return Vec::new() };
    if all.len() < 2 {
        return Vec::new();
    }
    let labels: Vec<String> = all.iter().map(grid_label).collect();
    let help = all.iter().map(|g| format!("{}: {}", grid_label(g), g.vars.join(", "))).collect::<Vec<_>>().join("\n");
    vec![crate::ReaderOption::new(
        "variables",
        "Variables on",
        crate::ReaderOptionKind::Choice(labels.clone()),
        &labels[0],
        &format!("The variables of one grid are read at a time:\n{help}"),
    )]
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

/// 2-D longitude and latitude for the grid's last two dimensions, named in the variables'
/// `coordinates` attribute (POP's TLONG/TLAT, CICE, ROMS...): values in grid order.
fn lon_lat_2d(f: &NcFile, g: &Grid) -> Option<(Vec<f64>, Vec<f64>)> {
    if g.dims.len() < 2 {
        return None;
    }
    let want: Vec<&str> = g.dims[g.dims.len() - 2..].iter().map(|d| d.0.as_str()).collect();
    let vars = f.variables().ok()?;
    let names: Vec<String> = g
        .vars
        .iter()
        .filter_map(|n| vars.iter().find(|v| v.name() == n))
        .filter_map(|v| text(v, "coordinates"))
        .flat_map(|c| c.split_whitespace().map(String::from).collect::<Vec<_>>())
        .collect();
    let pick = |axis: Axis| {
        names
            .iter()
            .filter_map(|n| vars.iter().find(|v| v.name() == n))
            .find(|v| v.dimensions().iter().map(|d| d.name.as_str()).eq(want.iter().copied()) && axis_of(Some(v), "") == axis)
    };
    let (lo, la) = (pick(Axis::Lon)?, pick(Axis::Lat)?);
    Some((read_all(f, lo.name()).ok()?, read_all(f, la.name()).ok()?))
}

/// Radius of each level of a 3-D grid's first (vertical) dimension, as ParaView computes it:
/// the coordinate values (negated when `positive` is "down"), shifted so none is negative.
fn vertical_radius(f: &NcFile, g: &Grid, ndims: usize) -> Option<Vec<f64>> {
    if ndims != 3 {
        return None;
    }
    let (name, n, _) = &g.dims[0];
    let mut z = match f.variable(name) {
        Ok(v) if v.dimensions().len() == 1 => {
            let down = text(v, "positive").is_some_and(|p| p.to_ascii_lowercase().contains("down"));
            let vals = read_all(f, name).ok()?;
            if down {
                vals.into_iter().map(|x| -x).collect()
            } else {
                vals
            }
        }
        _ => (0..*n).map(|i| i as f64).collect::<Vec<_>>(),
    };
    let min = z.iter().copied().fold(f64::INFINITY, f64::min);
    if min < 0.0 {
        for x in &mut z {
            *x -= min;
        }
    }
    Some(z)
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
    let dependent = if rev.iter().any(|d| d.2 == Axis::Lon || d.2 == Axis::Lat) { None } else { lon_lat_2d(&f, &g) };
    // Cell data between the bounds when every axis has them.
    let cells = dependent.is_none() && cell_edges.iter().all(|e| e.is_some());
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
        let arr = Array::scalars_f64(name.clone(), vals).with_units(text(v, "units").unwrap_or_default());
        if cells {
            cell_data.insert(arr);
        } else {
            point_data.insert(arr);
        }
    }
    let ds = match (lon, lat) {
        _ if dependent.is_some() => {
            // 2-D longitude/latitude (a curvilinear grid): on the unit sphere, a third axis as
            // the radius, as ParaView's CF reader does.
            let (lo, la) = dependent.unwrap();
            let radius = vertical_radius(&f, &g, rev.len());
            let (nx, ny) = (n[0], n[1]);
            let nz = if rev.len() == 3 { n[2] } else { 1 };
            let mut pts = Vec::with_capacity(3 * nx * ny * nz);
            for k in 0..nz {
                let r = radius.as_ref().map(|r| r[k]).unwrap_or(1.0);
                for j in 0..ny {
                    for i in 0..nx {
                        let (o, a) = (lo[j * nx + i].to_radians(), la[j * nx + i].to_radians());
                        pts.extend_from_slice(&[r * a.cos() * o.cos(), r * a.cos() * o.sin(), r * a.sin()]);
                    }
                }
            }
            let extent = [0, nx as i64 - 1, 0, ny as i64 - 1, 0, nz as i64 - 1];
            DataSet::StructuredGrid(StructuredGrid { extent, points: Points::new_f64(pts), point_data, cell_data, field_data: Attributes::new() })
        }
        (Some(il), Some(ia)) => {
            // On a unit sphere; a vertical axis is the radius (scale 1, bias 0, as ParaView).
            let iv = (0..axes.len()).find(|&k| k != il && k != ia);
            // As ParaView: depths with positive="down" are negated, then all shifted so none
            // is below zero (the deepest level lies closest to the center).
            let mut axes = axes.clone();
            if let Some(v) = iv {
                let down =
                    vars.iter().find(|x| x.name() == rev[v].0).and_then(|x| text(x, "positive")).is_some_and(|p| p.to_ascii_lowercase().contains("down"));
                if down {
                    axes[v].iter_mut().for_each(|x| *x = -*x);
                }
                let min = axes[v].iter().copied().fold(f64::INFINITY, f64::min);
                if min < 0.0 {
                    axes[v].iter_mut().for_each(|x| *x -= min);
                }
            }
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
