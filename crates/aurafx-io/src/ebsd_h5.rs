//! EBSD data in HDF5 files: `.h5ebsd` stacks (TSL or HKL slices), EDAX OIM `.h5` and Bruker
//! Esprit `.h5` scans, and `.dream3d` files (image, vertex, edge, triangle, quadrilateral,
//! tetrahedral and hexahedral geometries with their attribute matrices).
//!
//! EBSD maps become image data as the text readers make them (see `ebsd.rs`): `EulerAngles`
//! (radians), `Phases` and the quality columns as cell data, the phases as field data.

use crate::ebsd::{grid, Phase, HEX_ALIGN, HEX_AS_STORED};
use crate::{option, ReaderOption, ReaderOptionKind};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_crystal::Laue;
use aurafx_data::*;
use hdf5_reader::group::Group;
use hdf5_reader::{Dataset, Hdf5File};
use std::path::Path;

fn open(path: &Path) -> Result<Hdf5File> {
    Hdf5File::open(path).with_context(|| format!("opening {} as HDF5", path.display()))
}

fn is_hdf5(path: &Path) -> bool {
    let mut head = [0u8; 8];
    std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && head == *b"\x89HDF\r\n\x1a\n"
}

/// The text of a string dataset (the first element), trimmed.
fn text(g: &Group, name: &str) -> Option<String> {
    let d = g.dataset(name).ok()?;
    let s = d.read_strings().ok()?;
    s.first().map(|v| v.trim_matches(['\0', ' ']).to_string())
}

fn ds_values(d: &Dataset) -> Option<Vec<f64>> {
    match crate::vtkhdf::read(d).ok()?.0 {
        ArrayData::String(s) => s.iter().map(|v| v.trim().parse().ok()).collect(),
        a => Some((0..a.len()).map(|i| a.get_f64(i)).collect()),
    }
}

/// The numbers of a dataset (strings holding numbers are parsed).
fn values(g: &Group, name: &str) -> Option<Vec<f64>> {
    ds_values(&g.dataset(name).ok()?)
}

fn num(g: &Group, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|n| values(g, n).and_then(|v| v.first().copied()))
}

fn attr_text(g: &Group, name: &str) -> Option<String> {
    g.attribute(name).ok().and_then(|a| a.read_string().ok()).map(|s| s.trim_matches(['\0', ' ']).to_string())
}

/// The vendor named by a root `Manufacturer` dataset (EBSD HDF5 files carry one).
fn manufacturer(f: &Hdf5File) -> Option<String> {
    let root = f.root_group().ok()?;
    text(&root, "Manufacturer").or_else(|| text(&root, " Manufacturer"))
}

/// An HDF5 file holding EBSD data (`.h5ebsd`, EDAX or Bruker `.h5`).
pub fn is_ebsd_h5(path: &Path) -> bool {
    path.is_file() && is_hdf5(path) && open(path).ok().and_then(|f| manufacturer(&f)).is_some()
}

/// The scans of an EDAX or Bruker file: root groups holding an `EBSD` group.
fn scans(f: &Hdf5File) -> Vec<String> {
    let Ok(root) = f.root_group() else { return Vec::new() };
    let Ok(groups) = root.groups() else { return Vec::new() };
    groups.into_iter().filter(|g| g.group("EBSD").is_ok()).map(|g| g.name().trim_start_matches('/').to_string()).collect()
}

pub fn options(path: &Path) -> Vec<ReaderOption> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "dream3d" {
        let mut names = vec![ALL.to_string()];
        if let Ok(f) = open(path) {
            names.extend(containers(&f).into_iter().map(|(n, _)| n));
        }
        return vec![
            ReaderOption::new("data_container", "Data container", ReaderOptionKind::Choice(names), ALL, "The data container to read; All reads every one with a geometry, as blocks."),
            ReaderOption::new("arrays", "Arrays", ReaderOptionKind::Text, "", "Names of the cell (and vertex) arrays to read, separated by commas; empty reads them all. Large volumes load faster with only the arrays needed."),
        ];
    }
    let Ok(f) = open(path) else { return Vec::new() };
    let m = manufacturer(&f).unwrap_or_default();
    let mut out = Vec::new();
    let s = scans(&f);
    if s.len() > 1 {
        out.push(ReaderOption::new("scan", "Scan", ReaderOptionKind::Choice(s.clone()), &s[0], "The scan of the file to read."));
    }
    if m == "HKL" {
        out.push(ReaderOption::new(
            "hexagonal_axes",
            "Hexagonal axes",
            ReaderOptionKind::Choice(vec![HEX_ALIGN.into(), HEX_AS_STORED.into()]),
            HEX_ALIGN,
            "Oxford data put the hexagonal [10-10] direction along x; adding 30 degrees to phi2 puts [2-1-10] along x.",
        ));
    }
    out
}

const ALL: &str = "All";

/// One measured map: its grid, orientations (radians), phases and other columns.
struct Map {
    nx: usize,
    ny: usize,
    step: [f64; 2],
    first: [f64; 2],
    euler: Vec<f32>,
    phase: Vec<i32>,
    extra: Vec<(String, ArrayData)>,
}

/// The index of each point on the raster from its column and row (None if they do not cover
/// the grid exactly once, in which case the file order is kept).
fn raster(cols: &[f64], rows: &[f64], nx: usize, ny: usize) -> Option<Vec<usize>> {
    let n = nx * ny;
    if cols.len() != n || rows.len() != n {
        return None;
    }
    let mut dest = vec![usize::MAX; n];
    for (k, (c, r)) in cols.iter().zip(rows).enumerate() {
        let (c, r) = (c.round(), r.round());
        if c < 0.0 || r < 0.0 || c as usize >= nx || r as usize >= ny {
            return None;
        }
        let d = r as usize * nx + c as usize;
        if dest[d] != usize::MAX {
            return None;
        }
        dest[d] = k;
    }
    // A permutation: `dest[cell]` is the point stored for that cell.
    if dest.iter().any(|d| *d == usize::MAX) {
        return None;
    }
    Some(dest)
}

/// Read the 1-D datasets of length `n` of a Data group, except those in `skip`.
fn columns(data: &Group, n: usize, skip: &[&str], rename: &[(&str, &str)]) -> Result<Vec<(String, ArrayData)>> {
    let mut out = Vec::new();
    for d in data.datasets()? {
        let name = d.name().rsplit('/').next().unwrap_or("").to_string();
        if skip.contains(&name.as_str()) || d.shape().len() > 2 || d.shape().first().copied().unwrap_or(0) as usize != n || d.shape().get(1).is_some_and(|c| *c != 1) {
            continue;
        }
        let Ok((a, _)) = crate::vtkhdf::read(&d) else { continue };
        if matches!(a, ArrayData::String(_)) {
            continue;
        }
        let name = rename.iter().find(|(f, _)| *f == name).map(|(_, t)| t.to_string()).unwrap_or(name);
        out.push((name, a));
    }
    Ok(out)
}

fn need(g: &Group, names: &[&str], what: &str) -> Result<Vec<f64>> {
    names.iter().find_map(|n| values(g, n)).ok_or_else(|| anyhow!("no {what} ({})", names.join(" / ")))
}

/// The phases of a header: subgroups named 1, 2, ... of the first group found in `groups`.
fn phases(header: &Group, groups: &[&str], laue: &dyn Fn(&Group) -> Option<Laue>, name: &[&str]) -> Vec<Phase> {
    let Some(pg) = groups.iter().find_map(|g| header.group(g).ok()) else { return Vec::new() };
    let mut list: Vec<(i64, Group)> = pg.groups().unwrap_or_default().into_iter().filter_map(|g| g.name().rsplit('/').next().and_then(|n| n.parse().ok()).map(|k| (k, g))).collect();
    list.sort_by_key(|(k, _)| *k);
    list.iter()
        .map(|(_, g)| {
            let mut lattice = [0.0; 6];
            if let Some(v) = values(g, "LatticeConstants") {
                for (k, x) in v.iter().take(6).enumerate() {
                    lattice[k] = *x;
                }
            } else {
                for (k, n) in ["a", "b", "c", "alpha", "beta", "gamma"].iter().enumerate() {
                    lattice[k] = num(g, &[&format!("Lattice Constant {n}")]).unwrap_or(0.0);
                }
            }
            Phase { name: name.iter().find_map(|n| text(g, n)).unwrap_or_default(), laue: laue(g), lattice }
        })
        .collect()
}

fn tsl_laue(g: &Group) -> Option<Laue> {
    num(g, &["Symmetry"]).and_then(|s| Laue::from_tsl_symmetry(s as i64))
}
fn hkl_laue(g: &Group) -> Option<Laue> {
    num(g, &["LaueGroup", "Laue Group"]).and_then(|s| Laue::from_oxford(s as i64))
}
fn bruker_laue(g: &Group) -> Option<Laue> {
    num(g, &["IT"]).and_then(|s| Laue::from_space_group(s as i64))
}


/// TSL data (an `.h5ebsd` slice or an EDAX scan): angles in radians.
fn tsl_map(header: &Group, data: &Group, edax: bool) -> Result<(Map, Vec<Phase>)> {
    let grid_type = text(header, "GRID").or_else(|| text(header, "Grid Type")).unwrap_or_else(|| "SqrGrid".into());
    if !grid_type.starts_with("SqrGrid") {
        bail!("{grid_type} maps are not read (square grids are)");
    }
    let nx = num(header, &["NCOLS_ODD", "nColumns", "NCOLS_EVEN"]).ok_or_else(|| anyhow!("no column count"))? as usize;
    let ny = num(header, &["NROWS", "nRows"]).ok_or_else(|| anyhow!("no row count"))? as usize;
    let xs = num(header, &["XSTEP", "Step X"]).unwrap_or(1.0);
    let ys = num(header, &["YSTEP", "Step Y"]).unwrap_or(1.0);
    let ph = phases(header, &["Phases", "Phase"], &tsl_laue, &["MaterialName"]);
    let n = nx * ny;
    let e: Vec<Vec<f64>> = ["Phi1", "Phi", "Phi2"].iter().map(|c| need(data, &[c], c)).collect::<Result<_>>()?;
    let mut phase: Vec<i32> = need(data, &["Phase", "PhaseData"], "phases")?.iter().map(|v| *v as i32).collect();
    let x = values(data, "X Position").unwrap_or_default();
    let y = values(data, "Y Position").unwrap_or_default();
    if e[0].len() < n {
        bail!("{} points for a {nx} x {ny} grid", e[0].len());
    }
    let ci = values(data, "Confidence Index").or_else(|| values(data, "CI"));
    if ph.len() <= 1 {
        for (k, p) in phase.iter_mut().enumerate() {
            if *p == 0 && ci.as_ref().is_none_or(|c| c.get(k).is_none_or(|v| *v >= 0.0)) {
                *p = 1;
            }
        }
    }
    let rename: &[(&str, &str)] = if edax { &[("IQ", "Image Quality"), ("CI", "Confidence Index")] } else { &[] };
    let extra = columns(data, e[0].len(), &["Phi1", "Phi", "Phi2", "Phase", "PhaseData", "X Position", "Y Position"], rename)?;
    let mut map = Map {
        nx,
        ny,
        step: [xs, ys],
        first: [x.iter().cloned().fold(f64::INFINITY, f64::min), y.iter().cloned().fold(f64::INFINITY, f64::min)],
        euler: (0..e[0].len()).flat_map(|i| [e[0][i] as f32, e[1][i] as f32, e[2][i] as f32]).collect(),
        phase,
        extra,
    };
    if !map.first[0].is_finite() {
        map.first = [0.0, 0.0];
    }
    let cols: Vec<f64> = x.iter().map(|v| (v - map.first[0]) / xs).collect();
    let rows: Vec<f64> = y.iter().map(|v| (v - map.first[1]) / ys).collect();
    order(&mut map, raster(&cols, &rows, nx, ny));
    Ok((map, ph))
}

/// HKL data (an `.h5ebsd` slice): angles in degrees; hexagonal phases moved to a1 along x.
fn hkl_map(header: &Group, data: &Group) -> Result<(Map, Vec<Phase>)> {
    let nx = num(header, &["XCells"]).ok_or_else(|| anyhow!("no XCells"))? as usize;
    let ny = num(header, &["YCells"]).ok_or_else(|| anyhow!("no YCells"))? as usize;
    let xs = num(header, &["XStep"]).unwrap_or(1.0);
    let ys = num(header, &["YStep"]).unwrap_or(1.0);
    let ph = phases(header, &["Phases"], &hkl_laue, &["PhaseName", "MaterialName"]);
    let e: Vec<Vec<f64>> = ["Euler1", "Euler2", "Euler3"].iter().map(|c| need(data, &[c], c)).collect::<Result<_>>()?;
    let phase: Vec<i32> = need(data, &["Phase"], "phases")?.iter().map(|v| *v as i32).collect();
    let align = option("hexagonal_axes").is_none_or(|v| v != HEX_AS_STORED);
    let euler = (0..e[0].len())
        .flat_map(|i| {
            let hex = phase[i] > 0 && ph.get(phase[i] as usize - 1).and_then(|p| p.laue).is_some_and(|l| l.is_hexagonal());
            let mut p2 = e[2][i].to_radians();
            if align && hex {
                p2 = (p2 + 30f64.to_radians()).rem_euclid(std::f64::consts::TAU);
            }
            [e[0][i].to_radians() as f32, e[1][i].to_radians() as f32, p2 as f32]
        })
        .collect();
    let x = values(data, "X").unwrap_or_default();
    let y = values(data, "Y").unwrap_or_default();
    let extra = columns(data, e[0].len(), &["Euler1", "Euler2", "Euler3", "Phase", "X", "Y", "Z"], &[])?;
    let first = [x.first().copied().unwrap_or(0.0), y.first().copied().unwrap_or(0.0)];
    Ok((Map { nx, ny, step: [xs, ys], first, euler, phase, extra }, ph))
}

/// Bruker Esprit data: angles in degrees, points placed by their beam position.
fn bruker_map(header: &Group, data: &Group) -> Result<(Map, Vec<Phase>)> {
    let nx = num(header, &["NCOLS"]).ok_or_else(|| anyhow!("no NCOLS"))? as usize;
    let ny = num(header, &["NROWS"]).ok_or_else(|| anyhow!("no NROWS"))? as usize;
    let xs = num(header, &["XSTEP"]).unwrap_or(1.0);
    let ys = num(header, &["YSTEP"]).unwrap_or(1.0);
    let ph = phases(header, &["Phases"], &bruker_laue, &["Name", "MaterialName"]);
    let e: Vec<Vec<f64>> = ["phi1", "PHI", "phi2"].iter().map(|c| need(data, &[c], c)).collect::<Result<_>>()?;
    let phase: Vec<i32> = need(data, &["Phase"], "phases")?.iter().map(|v| *v as i32).collect();
    let bx = values(data, "X BEAM").unwrap_or_default();
    let by = values(data, "Y BEAM").unwrap_or_default();
    let extra = columns(data, e[0].len(), &["phi1", "PHI", "phi2", "Phase", "X BEAM", "Y BEAM"], &[])?;
    let mut map = Map {
        nx,
        ny,
        step: [xs, ys],
        first: [0.0, 0.0],
        euler: (0..e[0].len()).flat_map(|i| [e[0][i].to_radians() as f32, e[1][i].to_radians() as f32, e[2][i].to_radians() as f32]).collect(),
        phase,
        extra,
    };
    let (mx, my) = (bx.iter().cloned().fold(f64::INFINITY, f64::min), by.iter().cloned().fold(f64::INFINITY, f64::min));
    let cols: Vec<f64> = bx.iter().map(|v| v - mx).collect();
    let rows: Vec<f64> = by.iter().map(|v| v - my).collect();
    if mx.is_finite() {
        map.first = [mx * xs, my * ys];
    }
    order(&mut map, raster(&cols, &rows, nx, ny));
    Ok((map, ph))
}

/// Put the points of `map` in raster order.
fn order(map: &mut Map, dest: Option<Vec<usize>>) {
    let Some(dest) = dest else { return };
    if dest.iter().enumerate().all(|(k, d)| k == *d) {
        return;
    }
    map.euler = ArrayData::F32(std::mem::take(&mut map.euler)).gather(3, &dest).to_f64_vec().iter().map(|v| *v as f32).collect();
    map.phase = dest.iter().map(|d| map.phase[*d]).collect();
    for (_, a) in map.extra.iter_mut() {
        *a = a.gather(1, &dest);
    }
}

/// Image data of maps stacked along z (all of the same grid).
fn stack(maps: Vec<Map>, zstep: f64, z0: f64, phases: &[Phase]) -> Result<DataSet> {
    let m0 = maps.first().ok_or_else(|| anyhow!("no slices"))?;
    let (nx, ny, step, first) = (m0.nx, m0.ny, m0.step, m0.first);
    let n = nx * ny;
    if let Some(m) = maps.iter().find(|m| m.nx != nx || m.ny != ny) {
        bail!("slices of different sizes ({nx} x {ny} and {} x {})", m.nx, m.ny);
    }
    let mut cells = Attributes::new();
    let euler: Vec<f32> = maps.iter().flat_map(|m| m.euler[..3 * n].iter().copied()).collect();
    cells.insert(Array::new("EulerAngles", 3, ArrayData::F32(euler)));
    cells.insert(Array::new("Phases", 1, ArrayData::I32(maps.iter().flat_map(|m| m.phase[..n].iter().copied()).collect())));
    for (k, (name, _)) in m0.extra.iter().enumerate() {
        let parts: Vec<ArrayData> = maps.iter().filter_map(|m| m.extra.get(k).filter(|(nm, _)| nm == name).map(|(_, a)| a.gather(1, &(0..n).collect::<Vec<_>>()))).collect();
        if parts.len() != maps.len() {
            continue;
        }
        let mut all = parts[0].clone();
        for p in &parts[1..] {
            all.append(p)?;
        }
        cells.insert(Array::new(name.clone(), 1, all));
    }
    Ok(grid([nx, ny, maps.len()], [step[0], step[1], zstep], [first[0], first[1], z0], cells, phases))
}

/// An `.h5ebsd` file, or an EDAX or Bruker `.h5` file.
pub fn read(path: &Path) -> Result<DataSet> {
    let f = open(path)?;
    let m = manufacturer(&f).ok_or_else(|| anyhow!("{}: no Manufacturer dataset (not an EBSD HDF5 file)", path.display()))?;
    let root = f.root_group()?;
    let ctx = || format!("reading {}", path.display());
    if (m == "TSL" || m == "HKL") && root.dataset("ZStartIndex").is_ok() {
        let z0 = num(&root, &["ZStartIndex"]).unwrap_or(0.0) as i64;
        let z1 = num(&root, &["ZEndIndex"]).unwrap_or(z0 as f64) as i64;
        let zres = num(&root, &["Z Resolution", "ZResolution"]).unwrap_or(1.0);
        let high_to_low = num(&root, &["Stacking Order", "StackingOrder"]).unwrap_or(0.0) == 1.0;
        let mut maps = Vec::new();
        let mut phases_all = Vec::new();
        for z in z0..=z1 {
            let g = root.group(&z.to_string()).with_context(|| format!("{}: slice {z} missing", path.display()))?;
            let (header, data) = (g.group("Header")?, g.group("Data")?);
            let (map, ph) = if m == "TSL" { tsl_map(&header, &data, false) } else { hkl_map(&header, &data) }.with_context(|| format!("{}: slice {z}", path.display()))?;
            if phases_all.is_empty() {
                phases_all = ph;
            }
            maps.push(map);
        }
        if high_to_low {
            maps.reverse();
        }
        let mut ds = stack(maps, zres, z0 as f64 * zres, &phases_all).with_context(ctx)?;
        if let DataSet::ImageData(img) = &mut ds {
            for name in ["SampleTransformationAngle", "SampleTransformationAxis", "EulerTransformationAngle", "EulerTransformationAxis"] {
                if let Some(v) = values(&root, name) {
                    img.field_data.insert(Array::scalars_f64(name, v));
                }
            }
        }
        return Ok(ds);
    }
    let all = scans(&f);
    let scan = option("scan").filter(|s| all.contains(s)).or_else(|| all.first().cloned()).ok_or_else(|| anyhow!("{}: no scan (a group holding EBSD)", path.display()))?;
    let ebsd = root.group(&scan)?.group("EBSD")?;
    let (header, data) = (ebsd.group("Header")?, ebsd.group("Data")?);
    let (map, ph) = if m.contains("Bruker") {
        bruker_map(&header, &data)
    } else if m.contains("EDAX") || m.contains("TSL") {
        tsl_map(&header, &data, true)
    } else {
        bail!("{}: {m} files are not read (EDAX, Bruker and .h5ebsd files are)", path.display())
    }
    .with_context(ctx)?;
    stack(vec![map], 1.0, 0.0, &ph).with_context(ctx)
}

// ---------------------------------------------------------------- .dream3d

/// The data containers with a geometry: (name, geometry type name).
fn containers(f: &Hdf5File) -> Vec<(String, String)> {
    let Ok(dcs) = f.group("DataContainers") else { return Vec::new() };
    dcs.groups()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|g| {
            let geo = g.group("_SIMPL_GEOMETRY").ok()?;
            let t = attr_text(&geo, "GeometryTypeName")?;
            Some((g.name().rsplit('/').next().unwrap_or("").to_string(), t))
        })
        .collect()
}

/// The arrays of an attribute matrix (NeighborList arrays are left out).
fn matrix_arrays(g: &Group, keep: &dyn Fn(&str) -> bool) -> Result<Vec<Array>> {
    let mut out = Vec::new();
    for d in g.datasets()? {
        let name = d.name().rsplit('/').next().unwrap_or("").to_string();
        if !keep(&name) {
            continue;
        }
        let ot = d.attribute("ObjectType").ok().and_then(|a| a.read_string().ok()).unwrap_or_default();
        if ot.starts_with("NeighborList") {
            continue;
        }
        let (data, shape) = crate::vtkhdf::read(&d).with_context(|| format!("array {name}"))?;
        let ncomp = d
            .attribute("ComponentDimensions")
            .ok()
            .map(|a| crate::chombo::attr_nums(&a).iter().product::<f64>() as usize)
            .filter(|c| *c > 0)
            .unwrap_or(if shape.len() > 1 { *shape.last().unwrap() } else { 1 });
        let ncomp = if matches!(data, ArrayData::String(_)) { 1 } else { ncomp };
        out.push(Array::new(name, ncomp, data));
    }
    Ok(out)
}

fn read_container(dc: &Group, wanted: &Option<Vec<String>>) -> Result<DataSet> {
    let geo = dc.group("_SIMPL_GEOMETRY")?;
    let kind = attr_text(&geo, "GeometryTypeName").unwrap_or_default();
    let keep = |n: &str| wanted.as_ref().is_none_or(|w| w.iter().any(|x| x == n));
    let mut point_data = Attributes::new();
    let mut cell_data = Attributes::new();
    let mut field_data = Attributes::new();
    for g in dc.groups()? {
        let Some(t) = g.attribute("AttributeMatrixType").ok().map(|a| crate::chombo::attr_nums(&a).first().copied().unwrap_or(-1.0) as i64) else { continue };
        match t {
            // Vertex
            0 => matrix_arrays(&g, &keep)?.into_iter().for_each(|a| {
                point_data.insert(a);
            }),
            // Edge, Face, Cell
            1..=3 => matrix_arrays(&g, &keep)?.into_iter().for_each(|a| {
                cell_data.insert(a);
            }),
            // Features, ensembles, metadata, generic
            _ => matrix_arrays(&g, &|_| true)?.into_iter().for_each(|a| {
                field_data.insert(a);
            }),
        }
    }
    let ds = match kind.as_str() {
        "ImageGeometry" => {
            let d = values(&geo, "DIMENSIONS").ok_or_else(|| anyhow!("no DIMENSIONS"))?;
            let o = values(&geo, "ORIGIN").unwrap_or(vec![0.0; 3]);
            let s = values(&geo, "SPACING").unwrap_or(vec![1.0; 3]);
            DataSet::ImageData(ImageData {
                extent: [0, d[0] as i64, 0, d[1] as i64, 0, d[2] as i64],
                origin: [o[0], o[1], o[2]],
                spacing: [s[0], s[1], s[2]],
                cell_data,
                field_data,
                ..Default::default()
            })
        }
        "RectGridGeometry" => {
            let b = |n: &str| values(&geo, n).ok_or_else(|| anyhow!("no {n}"));
            let (x, y, z) = (b("xBounds")?, b("yBounds")?, b("zBounds")?);
            let extent = [0, x.len() as i64 - 1, 0, y.len() as i64 - 1, 0, z.len() as i64 - 1];
            DataSet::RectilinearGrid(RectilinearGrid { extent, x, y, z, point_data: Attributes::new(), cell_data, field_data })
        }
        _ => {
            let pts = values(&geo, "SharedVertexList").ok_or_else(|| anyhow!("{kind}: no SharedVertexList"))?;
            let (list, ty, k) = match kind.as_str() {
                "VertexGeometry" => (None, CellType::Vertex, 1),
                "EdgeGeometry" => (Some("SharedEdgeList"), CellType::Line, 2),
                "TriangleGeometry" => (Some("SharedTriList"), CellType::Triangle, 3),
                "QuadrilateralGeometry" | "QuadGeometry" => (Some("SharedQuadList"), CellType::Quad, 4),
                "TetrahedralGeometry" => (Some("SharedTetList"), CellType::Tetra, 4),
                "HexahedralGeometry" => (Some("SharedHexList"), CellType::Hexahedron, 8),
                other => bail!("{other} data containers are not read"),
            };
            let n = pts.len() / 3;
            let conn: Vec<u32> = match list {
                Some(l) => values(&geo, l).ok_or_else(|| anyhow!("{kind}: no {l}"))?.iter().map(|v| *v as u32).collect(),
                None => (0..n as u32).collect(),
            };
            let mut cells = Cells::new();
            for c in conn.chunks_exact(k) {
                cells.push(ty, c);
            }
            DataSet::UnstructuredGrid(UnstructuredGrid { points: Points::new_f64(pts), cells, point_data, cell_data, field_data })
        }
    };
    Ok(ds)
}

/// The pipeline a `.dream3d` file records (the JSON text of `/Pipeline/Pipeline`), if any.
pub fn dream3d_pipeline(path: &Path) -> Result<Option<String>> {
    let f = open(path)?;
    let Ok(g) = f.group("Pipeline") else { return Ok(None) };
    let name = attr_text(&g, "Current Pipeline").unwrap_or_else(|| "Pipeline".into());
    Ok(text(&g, &name).or_else(|| text(&g, "Pipeline")))
}

/// A `.dream3d` file: one data container, or every one as blocks.
pub fn read_dream3d(path: &Path) -> Result<DataSet> {
    let f = open(path)?;
    let list = containers(&f);
    if list.is_empty() {
        bail!("{}: no data container with a geometry", path.display());
    }
    let pick = option("data_container").filter(|s| s != ALL && list.iter().any(|(n, _)| n == s));
    let wanted: Option<Vec<String>> = option("arrays").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()).filter(|v| !v.is_empty());
    let dcs = f.group("DataContainers")?;
    let one = |name: &str| read_container(&dcs.group(name)?, &wanted).with_context(|| format!("{}: data container {name}", path.display()));
    if let Some(p) = pick {
        return one(&p);
    }
    if list.len() == 1 {
        return one(&list[0].0);
    }
    let mut mb = MultiBlock::default();
    for (name, _) in &list {
        mb.blocks.push((name.clone(), Some(one(name)?)));
    }
    Ok(DataSet::MultiBlock(mb))
}
