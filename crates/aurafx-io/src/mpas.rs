//! MPAS (Model for Prediction Across Scales) NetCDF output, read as ParaView's MPAS reader does:
//! the dual grid, whose points are the MPAS cell centers and whose cells are the triangles (or
//! quads) joining the cells around each MPAS vertex (`cellsOnVertex`). Variables on `nCells`
//! become point data and variables on `nVertices` cell data. Time steps are numbered 0, 1,
//! 2... as in ParaView; `xtime` goes to field data.
//!
//! Reader options (as ParaView's): the vertical level shown (`vertical_level`), all levels
//! stacked into wedges or hexahedra (`multilayer`, `layer_thickness`, `atmosphere` for layers
//! going up), and a flat longitude/latitude map (`layout`, `center_lon`) where cells crossing
//! the map's edge are drawn on both sides, as ParaView does.
//!
//! Two differences from ParaView, which keeps every MPAS vertex as a cell: triangles that
//! reach outside the mesh (boundary vertices) and, with `maxLevelCell`, triangles (or layers)
//! below the sea floor are left out instead of being collapsed onto an extra point at the
//! origin.

use crate::{Loaded, ReaderOption, ReaderOptionKind};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use netcdf_reader::{NcFile, NcSliceInfo, NcSliceInfoElem, NcVariable};
use std::path::Path;
use std::sync::Arc;

const VERTICAL: &str = "nVertLevels";
const SPHERE: &str = "Sphere";
const LATLON: &str = "Lat/Lon map";

/// Whether a NetCDF file is MPAS output (it has the dual-grid connectivity and cell centers).
pub fn is_mpas(f: &NcFile) -> bool {
    ["nCells", "nVertices", "vertexDegree"].iter().all(|d| f.dimension(d).is_ok())
        && ["xCell", "yCell", "zCell", "cellsOnVertex"].iter().all(|v| f.variable(v).is_ok())
}

pub fn is_mpas_file(path: &Path) -> bool {
    NcFile::open(path).ok().is_some_and(|f| is_mpas(&f))
}

fn on_sphere(f: &NcFile) -> bool {
    // ParaView assumes a sphere when the attribute is missing.
    f.global_attribute("on_a_sphere")
        .ok()
        .and_then(|a| a.value.as_string())
        .map(|s| s.trim_end_matches('\0').trim().eq_ignore_ascii_case("YES"))
        .unwrap_or(true)
}

fn levels(f: &NcFile) -> usize {
    f.dimension(VERTICAL).map(|d| d.size as usize).unwrap_or(0)
}

/// The options offered for an MPAS file.
pub fn options(path: &Path) -> Vec<ReaderOption> {
    let Ok(f) = NcFile::open(path) else { return Vec::new() };
    let nlev = levels(&f);
    let mut o = Vec::new();
    if nlev > 1 {
        o.push(ReaderOption::new(
            "vertical_level",
            "Vertical level",
            ReaderOptionKind::Int { min: 0, max: nlev as i64 - 1 },
            0,
            "Which level of 3-D variables to show (0 is the top level of the ocean, the bottom of the atmosphere)",
        ));
        o.push(ReaderOption::new("multilayer", "All levels (multilayer)", ReaderOptionKind::Bool, false, "Stack every level into a 3-D shell of wedges"));
        o.push(ReaderOption::new(
            "layer_thickness",
            "Layer thickness",
            ReaderOptionKind::Float { min: 0.0, max: 1e9 },
            0,
            "Distance between levels in the multilayer view (meters on the sphere, degrees on the map); 0 picks one",
        ));
        o.push(ReaderOption::new("atmosphere", "Layers go up (atmosphere)", ReaderOptionKind::Bool, false, "Stack levels outwards instead of downwards"));
    }
    if on_sphere(&f) {
        o.push(ReaderOption::new(
            "layout",
            "Layout",
            ReaderOptionKind::Choice(vec![SPHERE.into(), LATLON.into()]),
            SPHERE,
            "On the sphere, or flat on a longitude/latitude map in degrees",
        ));
        o.push(ReaderOption::new("center_lon", "Map center longitude", ReaderOptionKind::Float { min: 0.0, max: 360.0 }, 180, "The longitude in the middle of the lon/lat map"));
    }
    o
}

struct Opts {
    level: usize,
    multilayer: bool,
    thickness: f64,
    up: bool,
    latlon: bool,
    center_lon: f64,
}

fn opts(nlev: usize, sphere: bool) -> Opts {
    Opts {
        level: (crate::option_f64("vertical_level", 0.0).max(0.0) as usize).min(nlev.saturating_sub(1)),
        multilayer: nlev > 0 && crate::option_bool("multilayer", false),
        thickness: crate::option_f64("layer_thickness", 0.0),
        up: crate::option_bool("atmosphere", false),
        latlon: sphere && crate::option("layout").is_some_and(|l| l == LATLON),
        center_lon: crate::option_f64("center_lon", 180.0),
    }
}

fn read_all(f: &NcFile, name: &str) -> Result<Vec<f64>> {
    Ok(f.read_variable_as_f64(name).map_err(|e| anyhow!("reading {name}: {e}"))?.into_iter().collect())
}

/// A variable at a time step: `n` positions by `nlev` levels (1 when it has no vertical
/// dimension, or when only `level` is read). Other dimensions take their first index.
struct Values {
    data: Vec<f64>,
    nlev: usize,
    /// The vertical dimension comes before the position dimension.
    level_major: bool,
    n: usize,
}

impl Values {
    fn at(&self, p: usize, l: usize) -> f64 {
        let l = l.min(self.nlev - 1);
        if self.level_major {
            self.data[l * self.n + p]
        } else {
            self.data[p * self.nlev + l]
        }
    }
}

fn values(f: &NcFile, v: &NcVariable, step: usize, level: Option<usize>, n: usize) -> Result<Values> {
    let dims = v.dimensions();
    let mut sel = NcSliceInfo::all(dims.len());
    let mut nlev = 1;
    let (mut pos_dim, mut lev_dim) = (0, usize::MAX);
    for (k, d) in dims.iter().enumerate() {
        match d.name.as_str() {
            "nCells" | "nVertices" => pos_dim = k,
            "Time" => sel.selections[k] = NcSliceInfoElem::Index(step as u64),
            name if name.starts_with(VERTICAL) => {
                lev_dim = k;
                match level {
                    Some(l) => sel.selections[k] = NcSliceInfoElem::Index(l.min(d.size as usize - 1) as u64),
                    None => nlev = d.size as usize,
                }
            }
            _ => sel.selections[k] = NcSliceInfoElem::Index(0),
        }
    }
    let data: Vec<f64> = f.read_variable_slice_as_f64(v.name(), &sel).map_err(|e| anyhow!("reading {}: {e}", v.name()))?.into_iter().collect();
    if data.len() != n * nlev {
        bail!("{}: {} values, expected {}", v.name(), data.len(), n * nlev);
    }
    Ok(Values { data, nlev, level_major: lev_dim < pos_dim, n })
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
    let nlev = levels(&f);
    let sphere = on_sphere(&f);
    let o = opts(nlev, sphere);
    if degree != 3 && degree != 4 {
        bail!("MPAS vertexDegree {degree}: only triangle (3) and quad (4) dual grids are supported");
    }

    // Base points (one per MPAS cell), as x, y, z.
    let mut px: Vec<[f64; 3]> = if o.latlon {
        let (lon, lat) = (read_all(&f, "lonCell")?, read_all(&f, "latCell")?);
        if lon.len() != np || lat.len() != np {
            bail!("lonCell/latCell do not have nCells values");
        }
        // Degrees, shifted so the map is centered on center_lon (longitudes in [c-180, c+180)).
        let c = o.center_lon;
        (0..np)
            .map(|i| {
                let mut x = lon[i].to_degrees();
                x = (x - (c - 180.0)).rem_euclid(360.0) + (c - 180.0);
                [x, lat[i].to_degrees(), 0.0]
            })
            .collect()
    } else {
        let (x, y, z) = (read_all(&f, "xCell")?, read_all(&f, "yCell")?, read_all(&f, "zCell")?);
        if x.len() != np || y.len() != np || z.len() != np {
            bail!("xCell/yCell/zCell do not have nCells values");
        }
        (0..np).map(|i| [x[i], y[i], z[i]]).collect()
    };
    // Which MPAS cell each base point shows (mirror points on the map repeat one).
    let mut point_src: Vec<usize> = (0..np).collect();

    let conn = read_all(&f, "cellsOnVertex")?;
    if conn.len() != nverts * degree {
        bail!("cellsOnVertex does not have nVertices x vertexDegree values");
    }
    // Deepest valid level + 1 per MPAS cell (land is 0).
    let max_level: Option<Vec<f64>> = f.variable("maxLevelCell").ok().and_then(|_| read_all(&f, "maxLevelCell").ok()).filter(|m| m.len() == np);

    // Base cells: (MPAS vertex, point ids, levels it reaches).
    let mut cells: Vec<(usize, Vec<usize>, usize)> = Vec::with_capacity(nverts);
    'verts: for v in 0..nverts {
        let mut ids = Vec::with_capacity(degree);
        for k in 0..degree {
            let c = conn[v * degree + k];
            if c < 1.0 || c > np as f64 {
                continue 'verts;
            }
            ids.push(c as usize - 1);
        }
        let reach = match &max_level {
            Some(m) => ids.iter().map(|&i| m[i].max(0.0) as usize).min().unwrap_or(0),
            None => usize::MAX,
        };
        cells.push((v, ids, reach));
    }

    // On the map, a cell spanning the edge is moved to one side and copied to the other.
    if o.latlon {
        let tol = 5.5_f64.to_degrees();
        let center = o.center_lon;
        let mirror = |src: usize, divider: f64, px: &mut Vec<[f64; 3]>, point_src: &mut Vec<usize>| -> usize {
            let mut p = px[src];
            p[0] += if p[0] < divider { 360.0 } else { -360.0 };
            px.push(p);
            point_src.push(point_src[src]);
            px.len() - 1
        };
        let mut extra = Vec::new();
        for (v, ids, reach) in cells.iter_mut() {
            let n = ids.len();
            if !(0..n).any(|k| (px[ids[k]][0] - px[ids[(k + n - 1) % n]][0]).abs() > tol) {
                continue;
            }
            let orig = ids.clone();
            let anchor = px[orig[0]][0];
            for k in 1..n {
                if (px[orig[k]][0] - anchor).abs() > tol {
                    ids[k] = mirror(orig[k], anchor, &mut px, &mut point_src);
                }
            }
            let mut copy = vec![mirror(orig[0], center, &mut px, &mut point_src)];
            let anchor = px[copy[0]][0];
            for &id in &orig[1..] {
                copy.push(if (px[id][0] - anchor).abs() > tol { mirror(id, anchor, &mut px, &mut point_src) } else { id });
            }
            extra.push((*v, copy, *reach));
        }
        cells.extend(extra);
    }

    // Read the variables: all levels for the multilayer view, else the chosen one.
    let level = if o.multilayer { None } else { Some(o.level) };
    let mut point_vars = Vec::new();
    let mut cell_vars = Vec::new();
    for v in f.variables().map_err(|e| anyhow!("{e}"))? {
        match location(v) {
            Some("nCells") => {
                if let Ok(x) = values(&f, v, step, level, np) {
                    point_vars.push((v.name().to_string(), x));
                }
            }
            Some("nVertices") => {
                if let Ok(x) = values(&f, v, step, level, nverts) {
                    cell_vars.push((v.name().to_string(), x));
                }
            }
            _ => {}
        }
    }

    let nb = px.len();
    let (pts, out_cells, point_at, cell_at): (Vec<f64>, Cells, Vec<(usize, usize)>, Vec<(usize, usize)>) = if o.multilayer {
        // nlev + 1 points per column; level l + 1 lies one layer below (above for the atmosphere).
        let t = if o.thickness > 0.0 {
            o.thickness
        } else if o.latlon {
            20.0 / nlev as f64
        } else {
            let r = px.iter().map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()).fold(0.0, f64::max);
            if sphere { 0.05 * r / nlev as f64 } else { 0.05 * bounds_size(&px) / nlev as f64 }
        };
        let dir = if o.up { 1.0 } else { -1.0 };
        let mut pts = Vec::with_capacity(3 * nb * (nlev + 1));
        let mut point_at = Vec::with_capacity(nb * (nlev + 1));
        for (b, p) in px.iter().enumerate() {
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            for l in 0..=nlev {
                let d = dir * t * l as f64;
                if sphere && !o.latlon && r > 0.0 {
                    let s = (r + d) / r;
                    pts.extend_from_slice(&[p[0] * s, p[1] * s, p[2] * s]);
                } else {
                    pts.extend_from_slice(&[p[0], p[1], p[2] + d]);
                }
                point_at.push((point_src[b], l));
            }
        }
        let ty = if degree == 3 { CellType::Wedge } else { CellType::Hexahedron };
        let mut cs = Cells::new();
        let mut cell_at = Vec::new();
        let col = |id: usize, l: usize| (id * (nlev + 1) + l) as u32;
        for (v, ids, reach) in &cells {
            for l in 0..nlev.min(*reach) {
                let ring: Vec<u32> = ids.iter().map(|&i| col(i, l)).chain(ids.iter().map(|&i| col(i, l + 1))).collect();
                cs.push(ty, &ring);
                cell_at.push((*v, l));
            }
        }
        (pts, cs, point_at, cell_at)
    } else {
        let ty = if degree == 3 { CellType::Triangle } else { CellType::Quad };
        let mut cs = Cells::new();
        let mut cell_at = Vec::new();
        for (v, ids, reach) in &cells {
            if *reach <= o.level {
                continue;
            }
            cs.push(ty, &ids.iter().map(|&i| i as u32).collect::<Vec<_>>());
            cell_at.push((*v, 0));
        }
        (px.iter().flatten().copied().collect(), cs, point_src.iter().map(|&s| (s, 0)).collect(), cell_at)
    };

    let mut pd = Attributes::new();
    for (name, x) in point_vars {
        pd.insert(Array::scalars_f64(name, point_at.iter().map(|&(p, l)| x.at(p, l)).collect()));
    }
    let mut cd = Attributes::new();
    for (name, x) in cell_vars {
        cd.insert(Array::scalars_f64(name, cell_at.iter().map(|&(v, l)| x.at(v, l)).collect()));
    }
    let mut fd = Attributes::new();
    if let Ok(t) = f.read_variable_as_strings("xtime") {
        if let Some(s) = t.get(step.min(t.len().saturating_sub(1))) {
            fd.insert(Array::new("xtime", 1, ArrayData::String(vec![s.trim_end_matches('\0').trim().to_string()])));
        }
    }
    Ok(DataSet::UnstructuredGrid(UnstructuredGrid { points: Points::new_f64(pts), cells: out_cells, point_data: pd, cell_data: cd, field_data: fd }))
}

fn bounds_size(p: &[[f64; 3]]) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for q in p {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max).max(1.0)
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
