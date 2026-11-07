//! CAM-SE (the spectral-element core of CESM's Community Atmosphere Model) NetCDF output, read
//! as ParaView's CAM reader does. The data file holds the columns (`lat`, `lon` on `ncol`,
//! degrees) and the variables; the quads joining the columns are in a second file
//! (`element_corners(ncorners, ncells)`, 1-based), found next to the data file or named with
//! the `connectivity` reader option.
//!
//! The grid lies on a lon/lat map: longitude and latitude in degrees, with quads that cross
//! the 0/360 seam moved to the side they stick out of least, on copies of their points. With
//! all levels of midpoint (`lev`) or interface (`ilev`) variables, each quad becomes a column
//! of hexahedra, level k at height `levels - k - 1`, and a point array gives each point's
//! level value. Reader options choose the variables (surface, midpoint or interface), one
//! level or all, and a sphere instead of the map.

use crate::{Loaded, ReaderOption, ReaderOptionKind};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use netcdf_reader::{NcFile, NcSliceInfo, NcSliceInfoElem, NcVariable};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SURFACE: &str = "Surface [time, ncol]";
const MIDPOINT: &str = "Midpoint layers [time, lev, ncol]";
const INTERFACE: &str = "Interface layers [time, ilev, ncol]";
const MAP: &str = "Lat/Lon map";
const SPHERE: &str = "Sphere";

/// Whether a NetCDF file is CAM-SE output: columns `ncol` with `lat` and `lon` on them.
pub fn is_cam(f: &NcFile) -> bool {
    f.dimension("ncol").is_ok() && ["lat", "lon"].iter().all(|n| f.variable(n).is_ok_and(|v| v.dimensions().len() == 1 && v.dimensions()[0].name == "ncol"))
}

pub fn is_cam_file(path: &Path) -> bool {
    NcFile::open(path).ok().is_some_and(|f| is_cam(&f))
}

/// Whether a file is a CAM-SE connectivity file (opened alone, it has nothing to show).
pub fn is_connectivity_file(path: &Path) -> bool {
    NcFile::open(path).ok().is_some_and(|f| is_connectivity(&f) && f.dimension("ncol").is_err())
}

fn is_connectivity(f: &NcFile) -> bool {
    f.dimension("ncells").is_ok() && f.variable("element_corners").is_ok()
}

/// The connectivity file: the `connectivity` option (relative to the data file's folder), or
/// the first NetCDF file beside it with `element_corners`.
fn connectivity(path: &Path) -> Result<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new("."));
    if let Some(c) = crate::option("connectivity").filter(|c| !c.trim().is_empty()) {
        let p = PathBuf::from(c.trim());
        return Ok(if p.is_absolute() { p } else { dir.join(p) });
    }
    let mut cands: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p != path && crate::netcdf_cf::is_netcdf_ext(p))
        .collect();
    cands.sort();
    cands.into_iter().find(|p| NcFile::open(p).ok().is_some_and(|f| is_connectivity(&f))).ok_or_else(|| {
        anyhow!(
            "{}: CAM-SE output needs its connectivity file (with element_corners); none was found beside it. Name it under Reader options > Connectivity file",
            path.display()
        )
    })
}

fn kinds(f: &NcFile) -> Vec<&'static str> {
    let mut k = Vec::new();
    if f.dimension("lev").is_ok() {
        k.push(MIDPOINT);
    }
    if f.dimension("ilev").is_ok() {
        k.push(INTERFACE);
    }
    k.push(SURFACE);
    k
}

pub fn options(path: &Path) -> Vec<ReaderOption> {
    let Ok(f) = NcFile::open(path) else { return Vec::new() };
    let kinds = kinds(&f);
    let max_lev = ["lev", "ilev"].iter().filter_map(|d| f.dimension(d).ok()).map(|d| d.size as i64).max().unwrap_or(1);
    vec![
        ReaderOption::new(
            "connectivity",
            "Connectivity file",
            ReaderOptionKind::Text,
            "",
            "The file with element_corners (the quads); empty finds it beside the data file",
        ),
        ReaderOption::new(
            "variables",
            "Variables",
            ReaderOptionKind::Choice(kinds.iter().map(|s| s.to_string()).collect()),
            kinds[0],
            "Surface variables, or those on midpoint or interface levels",
        ),
        ReaderOption::new("all_levels", "All levels", ReaderOptionKind::Bool, true, "Stack every level into hexahedra (off: only the level below)"),
        ReaderOption::new("level", "Level", ReaderOptionKind::Int { min: 0, max: (max_lev - 1).max(0) }, 0, "The level shown when not all levels are"),
        ReaderOption::new(
            "layout",
            "Layout",
            ReaderOptionKind::Choice(vec![MAP.into(), SPHERE.into()]),
            MAP,
            "A flat lon/lat map (as ParaView), or the sphere",
        ),
    ]
}

fn read_all(f: &NcFile, name: &str) -> Result<Vec<f64>> {
    Ok(f.read_variable_as_f64(name).map_err(|e| anyhow!("reading {name}: {e}"))?.into_iter().collect())
}

/// Whether a quad's corners turn counter-clockwise on the map (ParaView's test, on the first
/// three corners and then the next three): CAM-SE cells are clockwise unless they cross the seam.
fn inverted(c: &[[f64; 2]; 4]) -> bool {
    let up = |a: [f64; 2], b: [f64; 2], d: [f64; 2]| (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]) > 0.0;
    up(c[0], c[1], c[2]) || up(c[1], c[2], c[3])
}

fn read_grid(path: &Path, step: usize) -> Result<DataSet> {
    let f = NcFile::open(path).map_err(|e| anyhow!("NetCDF file {}: {e}", path.display()))?;
    let cpath = connectivity(path)?;
    let cf = NcFile::open(&cpath).map_err(|e| anyhow!("connectivity file {}: {e}", cpath.display()))?;
    if !is_connectivity(&cf) {
        bail!("{} has no element_corners", cpath.display());
    }
    let (lon, lat) = (read_all(&f, "lon")?, read_all(&f, "lat")?);
    let np = lon.len();
    let ncells = cf.dimension("ncells").map(|d| d.size as usize).unwrap_or(0);
    let corners = read_all(&cf, "element_corners").with_context(|| cpath.display().to_string())?;
    if corners.len() != 4 * ncells {
        bail!("element_corners must be (4, ncells)");
    }
    let sphere = crate::option("layout").is_some_and(|l| l == SPHERE);

    // One level of points: the columns, plus copies moved by 360 degrees for seam cells.
    let mut pts: Vec<[f64; 2]> = (0..np).map(|i| [lon[i], lat[i]]).collect();
    let mut src: Vec<usize> = (0..np).collect();
    let mut seen: std::collections::HashMap<(u64, u64), usize> = std::collections::HashMap::new();
    for (i, p) in pts.iter().enumerate() {
        seen.entry((p[0].to_bits(), p[1].to_bits())).or_insert(i);
    }
    let mut quads: Vec<[usize; 4]> = Vec::with_capacity(ncells);
    for c in 0..ncells {
        let mut ids = [0usize; 4];
        for j in 0..4 {
            let v = corners[j * ncells + c];
            if v < 1.0 || v > np as f64 {
                bail!("element_corners refers to column {v} of {np}");
            }
            ids[j] = v as usize - 1;
        }
        let mut co = [[0.0; 2]; 4];
        for j in 0..4 {
            co[j] = pts[ids[j]];
        }
        if !sphere && inverted(&co) {
            // Anchor on the side the cell sticks out of least, as ParaView does.
            let (mut delta, mut left) = (0.0, false);
            for p in &co {
                let (r, l) = (360.0 - p[0], p[0]);
                if r.abs() < f64::EPSILON || l.abs() < f64::EPSILON || r == l {
                    continue;
                }
                if r < l {
                    if r > delta {
                        delta = r;
                        left = false;
                    }
                } else if l > delta {
                    delta = l;
                    left = true;
                }
            }
            for j in 0..4 {
                let x = co[j][0];
                let moved = if left && 360.0 - x < x {
                    x - 360.0
                } else if !left && x < 360.0 - x {
                    x + 360.0
                } else {
                    continue;
                };
                let q = [moved, co[j][1]];
                let id = *seen.entry((q[0].to_bits(), q[1].to_bits())).or_insert_with(|| {
                    pts.push(q);
                    src.push(src[ids[j]]);
                    pts.len() - 1
                });
                ids[j] = id;
            }
        }
        quads.push(ids);
    }

    // Variables of the chosen kind, and the levels shown.
    let kinds = kinds(&f);
    let kind = crate::option("variables").filter(|k| kinds.contains(&k.as_str())).unwrap_or(kinds[0].to_string());
    let lev_dim = match kind.as_str() {
        MIDPOINT => Some("lev"),
        INTERFACE => Some("ilev"),
        _ => None,
    };
    let nlev = lev_dim.and_then(|d| f.dimension(d).ok()).map(|d| d.size as usize).unwrap_or(1);
    let all = lev_dim.is_some() && crate::option_bool("all_levels", true);
    let levels: Vec<usize> = if all { (0..nlev).collect() } else { vec![(crate::option_f64("level", 0.0).max(0.0) as usize).min(nlev - 1)] };
    let npl = pts.len();

    let mut out = Vec::with_capacity(3 * npl * levels.len());
    for &l in &levels {
        let z = if all { (nlev - l - 1) as f64 } else { 0.0 };
        for p in &pts {
            if sphere {
                let r = 1.0 + 0.02 * z;
                let (o, a) = (p[0].to_radians(), p[1].to_radians());
                out.extend_from_slice(&[r * a.cos() * o.cos(), r * a.cos() * o.sin(), r * a.sin()]);
            } else {
                out.extend_from_slice(&[p[0], p[1], z]);
            }
        }
    }
    let mut cells = Cells::new();
    for q in &quads {
        if all {
            for l in 0..nlev.saturating_sub(1) {
                let ring: Vec<u32> = q.iter().map(|&i| (i + l * npl) as u32).chain(q.iter().map(|&i| (i + (l + 1) * npl) as u32)).collect();
                cells.push(CellType::Hexahedron, &ring);
            }
        } else {
            cells.push(CellType::Quad, &q.iter().map(|&i| i as u32).collect::<Vec<_>>());
        }
    }

    // Point data: each variable on this kind's dimensions, at every point of every level.
    let mut pd = Attributes::new();
    let want: Vec<&str> = match lev_dim {
        Some(d) => vec!["time", d, "ncol"],
        None => vec!["time", "ncol"],
    };
    for v in f.variables().map_err(|e| anyhow!("{e}"))? {
        if !v.dimensions().iter().map(|d| d.name.as_str()).eq(want.iter().copied()) {
            continue;
        }
        let Ok(per_level) = levels.iter().map(|&l| slice(&f, v, step, lev_dim.map(|_| l))).collect::<Result<Vec<_>>>() else { continue };
        if per_level.iter().any(|x| x.len() != np) {
            continue;
        }
        let vals: Vec<f64> = per_level.iter().flat_map(|x| src.iter().map(move |&s| x[s])).collect();
        pd.insert(Array::scalars_f64(v.name().to_string(), vals));
    }
    if let Some(d) = lev_dim {
        if let Ok(lv) = read_all(&f, d) {
            pd.insert(Array::scalars_f64(
                d.to_string(),
                levels.iter().flat_map(|&l| std::iter::repeat_n(lv.get(l).copied().unwrap_or(l as f64), npl)).collect(),
            ));
        }
    }
    Ok(DataSet::UnstructuredGrid(UnstructuredGrid {
        points: Points::new_f64(out),
        cells,
        point_data: pd,
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    }))
}

/// One time step (and level) of a variable on `[time, [lev,] ncol]`.
fn slice(f: &NcFile, v: &NcVariable, step: usize, level: Option<usize>) -> Result<Vec<f64>> {
    let mut sel = NcSliceInfo::all(v.dimensions().len());
    sel.selections[0] = NcSliceInfoElem::Index(step as u64);
    if let Some(l) = level {
        sel.selections[1] = NcSliceInfoElem::Index(l as u64);
    }
    Ok(f.read_variable_slice_as_f64(v.name(), &sel).map_err(|e| anyhow!("reading {}: {e}", v.name()))?.into_iter().collect())
}

fn times(f: &NcFile) -> Vec<f64> {
    let n = f.dimension("time").map(|d| d.size as usize).unwrap_or(0);
    match read_all(f, "time") {
        Ok(t) if t.len() == n => t,
        _ => (0..n).map(|i| i as f64).collect(),
    }
}

pub fn open(path: &Path) -> Result<Loaded> {
    let f = NcFile::open(path).map_err(|e| anyhow!("NetCDF file {}: {e}", path.display()))?;
    let t = times(&f);
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
