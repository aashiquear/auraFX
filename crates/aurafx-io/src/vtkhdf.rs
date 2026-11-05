//! VTKHDF (`.vtkhdf`, `.hdf`): VTK's HDF5 format. Image data, unstructured grids and poly data
//! (with several parts), time steps (`Steps` group), and partitioned dataset collections /
//! multiblocks (one group per block, nested by the `Assembly`).

use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use hdf5_reader::group::Group;
use hdf5_reader::{Dataset, Datatype};
use std::path::Path;
use std::sync::Arc;

pub fn is_vtkhdf_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "vtkhdf" | "hdf" | "vtkhdf5"))
}

fn open_file(path: &Path) -> Result<hdf5_reader::Hdf5File> {
    hdf5_reader::Hdf5File::open(path).with_context(|| format!("opening {} as HDF5", path.display()))
}

fn attr_string(g: &Group, name: &str) -> Option<String> {
    g.attribute(name).ok().and_then(|a| a.read_string().ok()).map(|s| s.trim_end_matches('\0').to_string())
}

fn attr_f64s(g: &Group, name: &str) -> Option<Vec<f64>> {
    let a = g.attribute(name).ok()?;
    a.read_1d::<f64>()
        .ok()
        .or_else(|| a.read_1d::<i64>().ok().map(|v| v.into_iter().map(|x| x as f64).collect()))
        .or_else(|| a.read_1d::<i32>().ok().map(|v| v.into_iter().map(|x| x as f64).collect()))
}

/// A dataset's values in their stored type, and its shape.
fn read(ds: &Dataset) -> Result<(ArrayData, Vec<usize>)> {
    let shape: Vec<usize> = ds.shape().iter().map(|&s| s as usize).collect();
    macro_rules! get {
        ($t:ty, $v:ident) => {
            ArrayData::$v(ds.read_array::<$t>()?.iter().cloned().collect())
        };
    }
    let data = match ds.dtype() {
        Datatype::FloatingPoint { size: 8, .. } => get!(f64, F64),
        Datatype::FloatingPoint { size: 4, .. } => get!(f32, F32),
        Datatype::FixedPoint { size: 1, signed: true, .. } => get!(i8, I8),
        Datatype::FixedPoint { size: 1, signed: false, .. } => get!(u8, U8),
        Datatype::FixedPoint { size: 2, signed: true, .. } => get!(i16, I16),
        Datatype::FixedPoint { size: 2, signed: false, .. } => get!(u16, U16),
        Datatype::FixedPoint { size: 4, signed: true, .. } => get!(i32, I32),
        Datatype::FixedPoint { size: 4, signed: false, .. } => get!(u32, U32),
        Datatype::FixedPoint { size: 8, signed: true, .. } => get!(i64, I64),
        Datatype::FixedPoint { size: 8, signed: false, .. } => get!(u64, U64),
        Datatype::String { .. } | Datatype::VarLen { .. } => ArrayData::String(ds.read_strings()?),
        other => bail!("{}: unsupported HDF5 type {other:?}", ds.name()),
    };
    Ok((data, shape))
}

fn ints(g: &Group, name: &str) -> Result<Vec<i64>> {
    let ds = g.dataset(name).with_context(|| format!("missing {name}"))?;
    let (d, _) = read(&ds)?;
    Ok(match d {
        ArrayData::String(_) => bail!("{name} is not numeric"),
        d => (0..d.len()).map(|i| d.get_f64(i) as i64).collect(),
    })
}

/// Rows `start..start + n` of an array with `ncomp` components.
fn rows(d: &ArrayData, ncomp: usize, start: usize, n: usize) -> ArrayData {
    let idx: Vec<usize> = (start..start + n).collect();
    d.gather(ncomp, &idx)
}

/// What one time step (or the only one) reads.
#[derive(Clone, Copy, Default)]
struct StepOffsets {
    points: usize,
    cells: usize,
    conn: usize,
    part: usize,
    nparts: usize,
}

fn step_offsets(g: &Group, step: Option<usize>) -> Result<(StepOffsets, Option<Group>)> {
    let Some(s) = step else {
        let nparts = g.dataset("NumberOfPoints").map(|d| d.shape().first().copied().unwrap_or(1) as usize).unwrap_or(1);
        return Ok((StepOffsets { nparts, ..Default::default() }, None));
    };
    let steps = g.group("Steps")?;
    let at = |name: &str| -> usize { ints(&steps, name).ok().and_then(|v| v.get(s).copied()).unwrap_or(0) as usize };
    let nparts = steps.dataset("NumberOfParts").ok().and_then(|_| ints(&steps, "NumberOfParts").ok()).and_then(|v| v.get(s).copied()).unwrap_or(1) as usize;
    Ok((StepOffsets { points: at("PointOffsets"), cells: at("CellOffsets"), conn: at("ConnectivityIdOffsets"), part: at("PartOffsets"), nparts }, Some(steps)))
}

/// Point/cell/field arrays of a group; `offset(name, kind)` gives the first row for a step.
fn attributes(g: &Group, kind: &str, n: usize, first: &dyn Fn(&str) -> usize) -> Result<Attributes> {
    let mut out = Attributes::new();
    let Ok(grp) = g.group(kind) else { return Ok(out) };
    for ds in grp.datasets()? {
        let (d, shape) = match read(&ds) {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!("VTKHDF {kind}/{}: {e:#}", ds.name());
                continue;
            }
        };
        let name = ds.name().rsplit('/').next().unwrap_or(ds.name()).to_string();
        // ImageData arrays are (z, y, x[, comp]); others (n[, comp]).
        let ncomp = if kind == "FieldData" {
            shape.get(1).copied().unwrap_or(1)
        } else if shape.len() == 4 || (shape.len() == 2 && g_is_rowwise(&shape, n)) {
            *shape.last().unwrap()
        } else {
            1
        };
        let ncomp = ncomp.max(1);
        let total = d.len() / ncomp;
        let data = if kind == "FieldData" || total == n { d } else { rows(&d, ncomp, first(&name), n.min(total.saturating_sub(first(&name)))) };
        out.insert(Array::new(name, ncomp, data));
    }
    Ok(out)
}

/// Whether a 2-D array is (rows, components) for `n` rows.
fn g_is_rowwise(shape: &[usize], _n: usize) -> bool {
    shape.len() == 2
}

fn read_image(g: &Group) -> Result<DataSet> {
    let ext = attr_f64s(g, "WholeExtent").ok_or_else(|| anyhow!("ImageData without WholeExtent"))?;
    let mut extent = [0i64; 6];
    for k in 0..6 {
        extent[k] = *ext.get(k).unwrap_or(&0.0) as i64;
    }
    let origin = attr_f64s(g, "Origin").unwrap_or(vec![0.0; 3]);
    let spacing = attr_f64s(g, "Spacing").unwrap_or(vec![1.0; 3]);
    let dir = attr_f64s(g, "Direction").filter(|d| d.len() == 9).unwrap_or(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let dims = [(extent[1] - extent[0] + 1) as usize, (extent[3] - extent[2] + 1) as usize, (extent[5] - extent[4] + 1) as usize];
    let np = dims[0] * dims[1] * dims[2];
    let nc = dims.iter().map(|d| d.saturating_sub(1).max(1)).product::<usize>();
    let zero = |_: &str| 0usize;
    let mut img = ImageData {
        extent,
        origin: [origin[0], origin[1], origin[2]],
        spacing: [spacing[0], spacing[1], spacing[2]],
        direction: dir.try_into().unwrap(),
        point_data: Attributes::new(),
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    };
    img.point_data = image_attributes(g, "PointData", np)?;
    img.cell_data = image_attributes(g, "CellData", nc)?;
    img.field_data = attributes(g, "FieldData", 0, &zero)?;
    Ok(DataSet::ImageData(img))
}

/// Image arrays: (z, y, x) scalars or (z, y, x, c) vectors, x fastest as in VTK.
fn image_attributes(g: &Group, kind: &str, n: usize) -> Result<Attributes> {
    let mut out = Attributes::new();
    let Ok(grp) = g.group(kind) else { return Ok(out) };
    for ds in grp.datasets()? {
        let (d, shape) = read(&ds)?;
        let name = ds.name().rsplit('/').next().unwrap_or(ds.name()).to_string();
        let ncomp = if shape.len() == 4 || (shape.len() == 2 && shape[0] == n) { *shape.last().unwrap() } else { 1 };
        if d.len() != n * ncomp {
            tracing::warn!("VTKHDF {kind}/{name}: {} values for {n} tuples", d.len());
            continue;
        }
        out.insert(Array::new(name, ncomp, d));
    }
    Ok(out)
}

/// Cells of one cell group (poly data Vertices/Lines/... or the grid itself) for a step.
fn cells(g: &Group, o: &StepOffsets, point_starts: &[usize], types: Option<CellType>) -> Result<Cells> {
    let ncells = ints(g, "NumberOfCells")?;
    let nconn = ints(g, "NumberOfConnectivityIds")?;
    let offsets = ints(g, "Offsets")?;
    let conn = ints(g, "Connectivity")?;
    let tys = if types.is_none() {
        Some(g.dataset("Types").ok().map(|d| read(&d)).transpose()?.map(|(d, _)| d).ok_or_else(|| anyhow!("missing Types"))?)
    } else {
        None
    };
    let mut out = Cells::new();
    let (mut cell0, mut conn0) = (o.cells, o.conn);
    for p in 0..o.nparts {
        let gp = o.part + p;
        let nc = *ncells.get(gp).ok_or_else(|| anyhow!("part {gp} out of range"))? as usize;
        let ncn = *nconn.get(gp).unwrap_or(&0) as usize;
        // Each part's offsets have one more entry than it has cells.
        let off0 = cell0 + gp;
        let pbase = point_starts.get(p).copied().unwrap_or(0);
        for c in 0..nc {
            let a = offsets[off0 + c] as usize;
            let b = offsets[off0 + c + 1] as usize;
            let ids: Vec<u32> = conn[conn0 + a..conn0 + b].iter().map(|&i| (i as usize + pbase) as u32).collect();
            let ty = match (&types, &tys) {
                (Some(t), _) => *t,
                (None, Some(t)) => {
                    let code = t.get_f64(cell0 + c) as u8;
                    CellType::from_vtk(code).ok_or_else(|| anyhow!("unsupported VTK cell type {code}"))?
                }
                _ => unreachable!(),
            };
            out.push(ty, &ids);
        }
        cell0 += nc;
        conn0 += ncn;
    }
    Ok(out)
}

/// Unstructured grid or poly data at a step (None: not temporal).
fn read_mesh(g: &Group, ty: &str, step: Option<usize>) -> Result<DataSet> {
    let (o, steps) = step_offsets(g, step)?;
    let npts = ints(g, "NumberOfPoints")?;
    let mut point_starts = Vec::new();
    let mut n = 0usize;
    for p in 0..o.nparts {
        point_starts.push(n);
        n += *npts.get(o.part + p).ok_or_else(|| anyhow!("part out of range"))? as usize;
    }
    let (pts, _) = read(&g.dataset("Points")?)?;
    let pts = rows(&pts, 3, o.points, n);
    let points = match pts {
        ArrayData::F32(v) => Points::new_f32(v),
        d => Points::new_f64(d.to_f64_vec()),
    };
    let data_offset = |kind: &str, name: &str| -> usize {
        steps
            .as_ref()
            .and_then(|s| s.group(&format!("{kind}Offsets")).ok())
            .and_then(|grp| ints(&grp, name).ok())
            .and_then(|v| step.and_then(|s| v.get(s).copied()))
            .unwrap_or(0) as usize
    };
    let ncell_total: usize = {
        let groups: Vec<Group> =
            if ty == "PolyData" { ["Vertices", "Lines", "Polygons", "Strips"].iter().filter_map(|k| g.group(k).ok()).collect() } else { vec![g.clone()] };
        groups
            .iter()
            .map(|gg| ints(gg, "NumberOfCells").map(|v| (0..o.nparts).map(|p| v.get(o.part + p).copied().unwrap_or(0) as usize).sum::<usize>()).unwrap_or(0))
            .sum()
    };
    let point_data = attributes(g, "PointData", n, &|name| data_offset("PointData", name))?;
    let cell_data = attributes(g, "CellData", ncell_total, &|name| data_offset("CellData", name))?;
    let field_data = attributes(g, "FieldData", 0, &|_| 0)?;
    if ty == "PolyData" {
        let kind = |k: &str, t: CellType| -> Result<Cells> {
            let Ok(gg) = g.group(k) else { return Ok(Cells::new()) };
            let (mut ko, _) = step_offsets(g, step)?;
            if let Some(s) = step {
                let steps = g.group("Steps")?;
                let at = |name: &str| ints(&steps, name).ok().and_then(|v| v.get(s).copied()).unwrap_or(0) as usize;
                // Per-kind offsets: Steps/<Kind>CellOffsets, <Kind>ConnectivityIdOffsets (2-D).
                ko.cells = steps.dataset("CellOffsets").ok().and_then(|_| step_kind(&steps, "CellOffsets", s, k)).unwrap_or(at("CellOffsets"));
                ko.conn = step_kind(&steps, "ConnectivityIdOffsets", s, k).unwrap_or(at("ConnectivityIdOffsets"));
            }
            cells(&gg, &ko, &point_starts, Some(t))
        };
        let mut pd = PolyData {
            points,
            verts: kind("Vertices", CellType::Vertex)?,
            lines: kind("Lines", CellType::PolyLine)?,
            polys: kind("Polygons", CellType::Polygon)?,
            strips: kind("Strips", CellType::TriangleStrip)?,
            point_data,
            cell_data,
            field_data,
        };
        fix_poly_types(&mut pd);
        return Ok(DataSet::PolyData(pd));
    }
    let cells = cells(g, &o, &point_starts, None)?;
    Ok(DataSet::UnstructuredGrid(UnstructuredGrid { points, cells, point_data, cell_data, field_data }))
}

/// Poly data cell offsets per kind (Steps/CellOffsets is (steps, 4) for poly data).
fn step_kind(steps: &Group, name: &str, s: usize, kind: &str) -> Option<usize> {
    let ds = steps.dataset(name).ok()?;
    let shape = ds.shape().to_vec();
    if shape.len() != 2 {
        return None;
    }
    let k = ["Vertices", "Lines", "Polygons", "Strips"].iter().position(|x| *x == kind)?;
    let (d, _) = read(&ds).ok()?;
    Some(d.get_f64(s * shape[1] as usize + k) as usize)
}

/// Lines with two points and polygons with three or four are stored as such (VTK readers
/// give them the generic types; our filters prefer the specific ones).
fn fix_poly_types(pd: &mut PolyData) {
    for (cells, by_size) in [(&mut pd.lines, [(2usize, CellType::Line)].as_slice()), (&mut pd.polys, [(3, CellType::Triangle), (4, CellType::Quad)].as_slice())]
    {
        for i in 0..cells.types.len() {
            let n = (cells.offsets[i + 1] - cells.offsets[i]) as usize;
            if let Some((_, t)) = by_size.iter().find(|(k, _)| *k == n) {
                cells.types[i] = *t;
            }
        }
    }
}

fn read_any(g: &Group, step: Option<usize>, root: &Group) -> Result<Option<DataSet>> {
    let Some(ty) = attr_string(g, "Type") else { return Ok(None) };
    Ok(Some(match ty.as_str() {
        "ImageData" => read_image(g)?,
        "UnstructuredGrid" | "PolyData" => {
            let has_steps = g.group("Steps").is_ok() || root.group("Steps").is_ok();
            read_mesh(g, &ty, if has_steps { step.or(Some(0)) } else { None })?
        }
        "PartitionedDataSetCollection" | "MultiBlockDataSet" => DataSet::MultiBlock(read_collection(g, step)?),
        t => bail!("VTKHDF type {t} is not supported yet"),
    }))
}

/// Blocks nested as the Assembly says (or flat, in Index order, without one).
fn read_collection(g: &Group, step: Option<usize>) -> Result<MultiBlock> {
    fn walk(asm: &Group, data: &Group, step: Option<usize>) -> Result<MultiBlock> {
        let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
        for child in asm.groups()? {
            let name = child.name().rsplit('/').next().unwrap_or(child.name()).to_string();
            let block = data.group(&name).ok().and_then(|b| attr_string(&b, "Type").map(|_| b));
            match block {
                Some(b) => mb.blocks.push((name, read_any(&b, step, data)?)),
                None => {
                    let sub = walk(&child, data, step)?;
                    // Empty leaves (node or side sets without geometry) are left out.
                    if !sub.blocks.is_empty() {
                        mb.blocks.push((name, Some(DataSet::MultiBlock(sub))));
                    } else if child.groups()?.is_empty() {
                        continue;
                    }
                }
            }
        }
        Ok(mb)
    }
    if let Ok(asm) = g.group("Assembly") {
        return walk(&asm, g, step);
    }
    let mut blocks: Vec<(i64, String, Group)> = g
        .groups()?
        .into_iter()
        .filter(|b| attr_string(b, "Type").is_some())
        .map(|b| {
            let idx = b.attribute("Index").ok().and_then(|a| a.read_as_f64().ok()).unwrap_or(0.0) as i64;
            (idx, b.name().rsplit('/').next().unwrap_or(b.name()).to_string(), b)
        })
        .collect();
    blocks.sort_by_key(|b| b.0);
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    for (_, name, b) in blocks {
        mb.blocks.push((name, read_any(&b, step, g)?));
    }
    Ok(mb)
}

/// Time values, if the file has steps.
fn times(root: &Group) -> Option<Vec<f64>> {
    let find = |g: &Group| g.group("Steps").ok().and_then(|s| s.dataset("Values").ok()).and_then(|d| read(&d).ok()).map(|(d, _)| d.to_f64_vec());
    if let Some(t) = find(root) {
        return Some(t);
    }
    // Collections: the first block with steps.
    root.groups().ok()?.iter().find_map(find)
}

pub fn open(path: &Path) -> Result<Loaded> {
    let f = open_file(path)?;
    let root = f.group("VTKHDF").context("no VTKHDF group")?;
    match times(&root) {
        Some(t) if t.len() > 1 => {
            let mut s = TimeSeries { times: t, steps: Vec::new() };
            for i in 0..s.times.len() {
                s.steps.push(TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i });
            }
            Ok(Loaded::Series(s))
        }
        _ => Ok(Loaded::Static(Arc::new(read_step(path, 0)?))),
    }
}

pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    let f = open_file(path)?;
    let root = f.group("VTKHDF").context("no VTKHDF group")?;
    read_any(&root, Some(step), &root)?.ok_or_else(|| anyhow!("{}: the VTKHDF group has no Type", path.display()))
}

/// An HDF5 dataset's values in their stored type, and its shape (for other HDF5-based readers).
pub(crate) fn read_dataset(ds: &Dataset) -> Result<(ArrayData, Vec<usize>)> {
    read(ds)
}
