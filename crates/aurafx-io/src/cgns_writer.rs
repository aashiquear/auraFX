//! CGNS writer (HDF5 storage, CGNS 4 layout), with our own HDF5 encoder. Every leaf block is a
//! zone: image, rectilinear and structured grids as structured zones, everything else as an
//! unstructured zone with one element section (a typed one, or MIXED with offsets), plus
//! NGON/NFACE sections for polygons in 2-D and polyhedra in 3-D. Zones go into one base per
//! cell dimension. Point and cell data become vertex and cell-center flow solutions (vectors
//! as `NameX/NameY/NameZ`). Time series keep the mesh of the first step and write a solution
//! (and, when the points move, coordinates) per step, with `BaseIterativeData` and
//! `ZoneIterativeData` pointers.

use crate::hdf5w::{self, AttrValue, Node};
use anyhow::{bail, Context, Result};
use aurafx_data::*;
use std::path::Path;

// ---- CGNS nodes ----

fn padded(s: &str, n: usize) -> String {
    let mut s: String = s.chars().filter(|c| c.is_ascii() && *c != '\0').take(n - 1).collect();
    while s.len() < n {
        s.push('\0');
    }
    s
}

/// A CGNS node: an HDF5 group with name/label/type/flags attributes and its data in " data".
fn cg(name: &str, label: &str, ty: &str, data: Option<(ArrayData, Vec<u64>)>) -> Node {
    let mut g = Node::group();
    g.attr("name", AttrValue::Str(padded(name, 33)));
    g.attr("flags", AttrValue::Data(ArrayData::I32(vec![1])));
    g.attr("label", AttrValue::Str(padded(label, 33)));
    g.attr("type", AttrValue::Str(padded(ty, 3)));
    if let Some((d, shape)) = data {
        g.add(" data", Node::dataset(d, shape));
    }
    g
}

fn c1(name: &str, label: &str, text: &str) -> Node {
    let b: Vec<i8> = text.bytes().map(|b| b as i8).collect();
    let n = b.len() as u64;
    cg(name, label, "C1", Some((ArrayData::I8(b), vec![n])))
}

/// (n, 32) character array of names, one per time step.
fn pointers(name: &str, names: &[String]) -> Node {
    let mut b = Vec::with_capacity(32 * names.len());
    for s in names {
        let mut row: Vec<i8> = s.bytes().take(32).map(|c| c as i8).collect();
        row.resize(32, 0);
        b.extend(row);
    }
    cg(name, "DataArray_t", "C1", Some((ArrayData::I8(b), vec![names.len() as u64, 32])))
}

/// A data array with CGNS's data type for it (floats keep their precision).
fn data_array(name: &str, d: ArrayData, shape: Vec<u64>) -> Node {
    let (d, ty) = match d {
        ArrayData::F32(v) => (ArrayData::F32(v), "R4"),
        ArrayData::F16(v) => (ArrayData::F32(v.iter().map(|x| x.to_f32()).collect()), "R4"),
        ArrayData::I32(v) => (ArrayData::I32(v), "I4"),
        ArrayData::I64(v) => (ArrayData::I64(v), "I8"),
        d => (ArrayData::F64(d.to_f64_vec()), "R8"),
    };
    cg(name, "DataArray_t", ty, Some((d, shape)))
}

/// Integers as I4 when they fit, else I8.
fn int_node(name: &str, label: &str, v: Vec<i64>, shape: Vec<u64>, wide: bool) -> Node {
    if wide {
        cg(name, label, "I8", Some((ArrayData::I64(v), shape)))
    } else {
        cg(name, label, "I4", Some((ArrayData::I32(v.into_iter().map(|x| x as i32).collect()), shape)))
    }
}

// ---- elements ----

/// CGNS element code and the node order (indices into the VTK cell) for fixed-size cells.
fn cgns_type(t: CellType) -> Option<(i64, Vec<usize>)> {
    use CellType::*;
    let first = |n: usize| (0..n).collect::<Vec<_>>();
    Some(match t {
        Vertex => (2, first(1)),
        Line | CubicLine | LagrangeCurve | BezierCurve => (3, first(2)),
        QuadraticEdge => (4, first(3)),
        Triangle | LagrangeTriangle | BezierTriangle => (5, first(3)),
        QuadraticTriangle | BiquadraticTriangle => (6, first(6)),
        Quad | QuadraticLinearQuad | LagrangeQuadrilateral | BezierQuadrilateral => (7, first(4)),
        Pixel => (7, vec![0, 1, 3, 2]),
        QuadraticQuad => (8, first(8)),
        BiquadraticQuad => (9, first(9)),
        Tetra | LagrangeTetrahedron | BezierTetrahedron => (10, first(4)),
        QuadraticTetra => (11, first(10)),
        Pyramid | LagrangePyramid | BezierPyramid => (12, first(5)),
        QuadraticPyramid => (21, first(13)),
        Wedge | QuadraticLinearWedge | BiquadraticQuadraticWedge | LagrangeWedge | BezierWedge => (14, first(6)),
        // VTK: bottom, top, vertical edges; CGNS: bottom, vertical, top.
        QuadraticWedge => (15, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 9, 10, 11]),
        Hexahedron | LagrangeHexahedron | BezierHexahedron => (17, first(8)),
        Voxel => (17, vec![0, 1, 3, 2, 4, 5, 7, 6]),
        QuadraticHexahedron | BiquadraticQuadraticHexahedron => (18, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15]),
        // The inverse of the reader's CGNS -> VTK order.
        TriquadraticHexahedron => (19, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15, 24, 22, 21, 23, 20, 25, 26]),
        _ => return None,
    })
}

fn code_dimension(code: i64) -> usize {
    match code {
        2 => 0,
        3 | 4 => 1,
        5..=9 => 2,
        _ => 3,
    }
}

enum Elem {
    Fixed(i64, Vec<u32>),
    Polygon(Vec<u32>),
    Polyhedron(Vec<Vec<u32>>),
}

impl Elem {
    fn dimension(&self) -> usize {
        match self {
            Elem::Fixed(c, _) => code_dimension(*c),
            Elem::Polygon(_) => 2,
            Elem::Polyhedron(_) => 3,
        }
    }
}

fn convert(ty: CellType, ids: &[u32], faces: Option<&Vec<Vec<u32>>>) -> Vec<Elem> {
    use CellType::*;
    match ty {
        PolyVertex => ids.iter().map(|&p| Elem::Fixed(2, vec![p])).collect(),
        PolyLine => ids.windows(2).map(|w| Elem::Fixed(3, w.to_vec())).collect(),
        TriangleStrip => (0..ids.len().saturating_sub(2))
            .map(|k| Elem::Fixed(5, if k % 2 == 0 { vec![ids[k], ids[k + 1], ids[k + 2]] } else { vec![ids[k + 1], ids[k], ids[k + 2]] }))
            .collect(),
        Polygon if ids.len() == 3 => vec![Elem::Fixed(5, ids.to_vec())],
        Polygon if ids.len() == 4 => vec![Elem::Fixed(7, ids.to_vec())],
        Polygon if ids.len() > 4 => vec![Elem::Polygon(ids.to_vec())],
        PentagonalPrism | HexagonalPrism => {
            let n = ids.len() / 2;
            let mut f: Vec<Vec<u32>> = vec![ids[..n].iter().rev().copied().collect(), ids[n..2 * n].to_vec()];
            for i in 0..n {
                let j = (i + 1) % n;
                f.push(vec![ids[i], ids[j], ids[n + j], ids[n + i]]);
            }
            vec![Elem::Polyhedron(f)]
        }
        Polyhedron => match faces {
            Some(f) if !f.is_empty() => vec![Elem::Polyhedron(f.clone())],
            _ => Vec::new(),
        },
        t => match cgns_type(t) {
            Some((code, order)) if order.iter().all(|&k| k < ids.len()) => vec![Elem::Fixed(code, order.iter().map(|&k| ids[k]).collect())],
            _ => Vec::new(),
        },
    }
}

/// An unstructured zone's sections and, for every CGNS cell (in element-number order), the
/// source cell it came from.
struct Sections {
    dim: usize,
    nodes: Vec<(String, Node)>,
    cell_map: Vec<usize>,
}

fn sections(ug: &UnstructuredGrid, wide: bool, skipped: &mut usize) -> Sections {
    let faces = ug.cells.polyhedron_faces.as_ref();
    let mut elems: Vec<(Elem, usize)> = Vec::new();
    for (c, (ty, ids)) in ug.cells.iter().enumerate() {
        let e = convert(ty, ids, faces.and_then(|f| f.get(c)));
        if e.is_empty() && ty != CellType::Empty {
            *skipped += 1;
        }
        elems.extend(e.into_iter().map(|e| (e, c)));
    }
    let dim = elems.iter().map(|(e, _)| e.dimension()).max().unwrap_or(0);
    // Cells of lower dimension (boundary faces of a volume, ...) are not cells of the zone.
    let before = elems.len();
    elems.retain(|(e, _)| e.dimension() == dim);
    *skipped += before - elems.len();

    let mut nodes = Vec::new();
    let mut cell_map = Vec::new();
    let fixed: Vec<&(Elem, usize)> = elems.iter().filter(|(e, _)| matches!(e, Elem::Fixed(..))).collect();
    let mut next = 1i64; // next element number
    if !fixed.is_empty() {
        let codes: Vec<i64> = fixed.iter().map(|(e, _)| if let Elem::Fixed(c, _) = e { *c } else { 0 }).collect();
        let n = fixed.len() as i64;
        let single = codes.iter().all(|&c| c == codes[0]);
        let mut conn = Vec::new();
        let mut offsets = vec![0i64];
        for (e, _) in &fixed {
            if let Elem::Fixed(c, ids) = e {
                if !single {
                    conn.push(*c);
                }
                conn.extend(ids.iter().map(|&i| i as i64 + 1));
                offsets.push(conn.len() as i64);
            }
        }
        let (name, code) = if single { ("Elements", codes[0]) } else { ("Mixed", 20) };
        let mut s = int_node(name, "Elements_t", vec![code, 0], vec![2], false);
        s.add("ElementRange", int_node("ElementRange", "IndexRange_t", vec![next, next + n - 1], vec![2], wide));
        if !single {
            let len = offsets.len() as u64;
            s.add("ElementStartOffset", int_node("ElementStartOffset", "DataArray_t", offsets, vec![len], wide));
        }
        let len = conn.len() as u64;
        s.add("ElementConnectivity", int_node("ElementConnectivity", "DataArray_t", conn, vec![len], wide));
        nodes.push((name.to_string(), s));
        cell_map.extend(fixed.iter().map(|(_, c)| *c));
        next += n;
    }
    // Polygons (cells of a 2-D zone) or polyhedra (NGON faces, then NFACE cells).
    let mut ngon: Vec<&[u32]> = Vec::new();
    let mut nface: Vec<Vec<i64>> = Vec::new();
    let mut poly_src = Vec::new();
    for (e, c) in &elems {
        match e {
            Elem::Polygon(ids) => {
                ngon.push(ids);
                poly_src.push(*c);
            }
            Elem::Polyhedron(fs) => {
                let first = ngon.len() as i64;
                nface.push((0..fs.len() as i64).map(|k| first + k).collect());
                ngon.extend(fs.iter().map(|f| f.as_slice()));
                poly_src.push(*c);
            }
            Elem::Fixed(..) => {}
        }
    }
    if !ngon.is_empty() {
        let ngon_first = next;
        let mut conn = Vec::new();
        let mut offsets = vec![0i64];
        for f in &ngon {
            conn.extend(f.iter().map(|&i| i as i64 + 1));
            offsets.push(conn.len() as i64);
        }
        let n = ngon.len() as i64;
        let mut s = int_node("NGON", "Elements_t", vec![22, 0], vec![2], false);
        s.add("ElementRange", int_node("ElementRange", "IndexRange_t", vec![next, next + n - 1], vec![2], wide));
        let len = offsets.len() as u64;
        s.add("ElementStartOffset", int_node("ElementStartOffset", "DataArray_t", offsets, vec![len], wide));
        let len = conn.len() as u64;
        s.add("ElementConnectivity", int_node("ElementConnectivity", "DataArray_t", conn, vec![len], wide));
        nodes.push(("NGON".to_string(), s));
        next += n;
        if !nface.is_empty() {
            let mut conn = Vec::new();
            let mut offsets = vec![0i64];
            for fs in &nface {
                conn.extend(fs.iter().map(|&f| ngon_first + f));
                offsets.push(conn.len() as i64);
            }
            let n = nface.len() as i64;
            let mut s = int_node("NFACE", "Elements_t", vec![23, 0], vec![2], false);
            s.add("ElementRange", int_node("ElementRange", "IndexRange_t", vec![next, next + n - 1], vec![2], wide));
            let len = offsets.len() as u64;
            s.add("ElementStartOffset", int_node("ElementStartOffset", "DataArray_t", offsets, vec![len], wide));
            let len = conn.len() as u64;
            s.add("ElementConnectivity", int_node("ElementConnectivity", "DataArray_t", conn, vec![len], wide));
            nodes.push(("NFACE".to_string(), s));
        }
        cell_map.extend(poly_src);
    }
    Sections { dim, nodes, cell_map }
}

// ---- zones ----

/// The mesh of one zone, from the first time step.
enum Mesh {
    /// Point counts along the axes that have more than one point (i fastest).
    Structured(Vec<usize>),
    Unstructured {
        cells: Cells,
        sections: Sections,
    },
}

struct Zone {
    name: String,
    dim: usize,
    n_points: usize,
    mesh: Mesh,
}

fn points_of(d: &DataSet) -> Option<Points> {
    Some(match d {
        DataSet::ImageData(im) => im.points(),
        DataSet::RectilinearGrid(g) => g.points(),
        DataSet::StructuredGrid(g) => g.points.clone(),
        DataSet::UnstructuredGrid(g) => g.points.clone(),
        DataSet::PolyData(p) => p.points.clone(),
        _ => return None,
    })
}

fn structured_dims(d: &DataSet) -> Option<[usize; 3]> {
    match d {
        DataSet::ImageData(im) => Some(im.dims()),
        DataSet::RectilinearGrid(g) => Some(g.dims()),
        DataSet::StructuredGrid(g) => Some(g.dims()),
        _ => None,
    }
}

fn zone_of(name: &str, d: &DataSet, wide: bool, skipped: &mut usize) -> Option<Zone> {
    let n_points = d.n_points();
    if n_points == 0 {
        return None;
    }
    if let Some(dims) = structured_dims(d) {
        let mut kept: Vec<usize> = dims.iter().copied().filter(|&n| n > 1).collect();
        if kept.is_empty() {
            kept.push(1);
        }
        return Some(Zone { name: name.to_string(), dim: kept.len(), n_points, mesh: Mesh::Structured(kept) });
    }
    let ug = d.to_unstructured()?;
    let s = sections(&ug, wide, skipped);
    Some(Zone { name: name.to_string(), dim: s.dim.max(1), n_points, mesh: Mesh::Unstructured { cells: ug.cells, sections: s } })
}

impl Zone {
    fn n_cells(&self) -> usize {
        match &self.mesh {
            Mesh::Structured(d) => d.iter().map(|&n| n.saturating_sub(1).max(1)).product(),
            Mesh::Unstructured { sections, .. } => sections.cell_map.len(),
        }
    }

    /// Does `d` have this zone's mesh (same kind, sizes and cells)?
    fn same_mesh(&self, d: &DataSet) -> bool {
        if d.n_points() != self.n_points {
            return false;
        }
        match (&self.mesh, structured_dims(d)) {
            (Mesh::Structured(k), Some(dims)) => dims.iter().copied().filter(|&n| n > 1).collect::<Vec<_>>() == *k || (k == &[1] && dims == [1, 1, 1]),
            (Mesh::Unstructured { cells, .. }, None) => d.to_unstructured().is_some_and(|u| u.cells == *cells),
            _ => false,
        }
    }

    /// HDF5 shape of an array over the points (structured: slowest axis first).
    fn point_shape(&self) -> Vec<u64> {
        match &self.mesh {
            Mesh::Structured(k) => k.iter().rev().map(|&n| n as u64).collect(),
            Mesh::Unstructured { .. } => vec![self.n_points as u64],
        }
    }

    fn cell_shape(&self) -> Vec<u64> {
        match &self.mesh {
            Mesh::Structured(k) => k.iter().rev().map(|&n| n.saturating_sub(1).max(1) as u64).collect(),
            Mesh::Unstructured { .. } => vec![self.n_cells() as u64],
        }
    }

    fn coordinates(&self, name: &str, d: &DataSet) -> Node {
        let p = points_of(d).unwrap_or_else(Points::empty);
        let mut g = cg(name, "GridCoordinates_t", "MT", None);
        for (a, axis) in ["CoordinateX", "CoordinateY", "CoordinateZ"].iter().enumerate() {
            let v: Vec<f64> = (0..p.len()).map(|i| p.get(i)[a]).collect();
            g.add(axis, data_array(axis, ArrayData::F64(v), self.point_shape()));
        }
        g
    }

    /// A flow solution of the point or cell arrays of `d` (None when there are none).
    fn solution(&self, name: &str, d: &DataSet, cells: bool) -> Option<Node> {
        let attrs = if cells { d.cell_data()? } else { d.point_data()? };
        let (n, shape) = if cells { (self.n_cells(), self.cell_shape()) } else { (self.n_points, self.point_shape()) };
        let map = match (&self.mesh, cells) {
            (Mesh::Unstructured { sections, .. }, true) => Some(&sections.cell_map),
            _ => None,
        };
        let mut g = cg(name, "FlowSolution_t", "MT", None);
        if cells {
            g.add("GridLocation", c1("GridLocation", "GridLocation_t", "CellCenter"));
        }
        let mut any = false;
        let mut used: Vec<String> = Vec::new();
        for a in attrs.iter() {
            if a.scalar_type() == ScalarType::String || crate::exodus_writer::is_internal(&a.name) {
                continue;
            }
            let a = match map {
                Some(m) => {
                    if m.iter().any(|&c| c >= a.len()) {
                        continue;
                    }
                    a.gather(m)
                }
                None => a.clone(),
            };
            if a.len() != n {
                continue;
            }
            let comps: Vec<String> = match a.ncomp {
                1 => vec![a.name.clone()],
                3 => ["X", "Y", "Z"].iter().map(|s| format!("{}{s}", truncate(&a.name, 31))).collect(),
                k => (0..k).map(|c| format!("{}_{c}", truncate(&a.name, 28))).collect(),
            };
            for (c, cname) in comps.iter().enumerate() {
                let cname = truncate(cname, 32);
                if used.contains(&cname) {
                    continue;
                }
                let v = if a.ncomp == 1 { a.data.clone() } else { ArrayData::F64((0..n).map(|i| a.get(i, c)).collect()) };
                let v = match v {
                    ArrayData::F32(_) | ArrayData::F64(_) | ArrayData::I32(_) | ArrayData::I64(_) => v,
                    other => ArrayData::F64(other.to_f64_vec()),
                };
                g.add(&cname, data_array(&cname, v, shape.clone()));
                used.push(cname);
                any = true;
            }
        }
        any.then_some(g)
    }

    /// The zone node with its mesh (no coordinates or solutions yet).
    fn node(&self, wide: bool) -> Node {
        match &self.mesh {
            Mesh::Structured(k) => {
                let mut size: Vec<i64> = k.iter().map(|&n| n as i64).collect();
                size.extend(k.iter().map(|&n| n.saturating_sub(1).max(1) as i64));
                size.extend(k.iter().map(|_| 0));
                let mut z = int_node(&self.name, "Zone_t", size, vec![3, k.len() as u64], wide);
                z.add("ZoneType", c1("ZoneType", "ZoneType_t", "Structured"));
                z
            }
            Mesh::Unstructured { sections, .. } => {
                let mut z = int_node(&self.name, "Zone_t", vec![self.n_points as i64, self.n_cells() as i64, 0], vec![3, 1], wide);
                z.add("ZoneType", c1("ZoneType", "ZoneType_t", "Unstructured"));
                for (name, s) in &sections.nodes {
                    z.add(name, s.clone());
                }
                z
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().filter(|c| *c != '/' && c.is_ascii()).take(n).collect()
}

fn leaves(ds: &DataSet) -> Result<Vec<(String, &DataSet)>> {
    Ok(match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().filter(|(_, d)| !matches!(d, DataSet::Table(_))).collect(),
        DataSet::Table(_) => bail!("tables cannot be written as CGNS"),
        d => vec![("Zone".to_string(), d)],
    })
}

/// Unique zone names of at most 32 characters.
fn zone_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let base = truncate(&n.replace('/', "_"), 28);
        let base = if base.is_empty() { "Zone".to_string() } else { base };
        let mut name = base.clone();
        let mut k = 1;
        while out.contains(&name) {
            name = format!("{base}_{k}");
            k += 1;
        }
        out.push(name);
    }
    out
}

/// Write `ds` as a CGNS file.
pub fn write_cgns(ds: &DataSet, path: &Path) -> Result<()> {
    write_cgns_steps(&[(0.0, ds)], path)
}

/// Write every step (time, data); the mesh must be the same in every step (the points may move).
pub fn write_cgns_steps(steps: &[(f64, &DataSet)], path: &Path) -> Result<()> {
    if steps.is_empty() {
        bail!("nothing to write");
    }
    let first = leaves(steps[0].1)?;
    let wide = steps[0].1.n_points() > i32::MAX as usize / 2 || steps[0].1.n_cells() > i32::MAX as usize / 32;
    let names = zone_names(&first.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
    let mut skipped = 0usize;
    let zones: Vec<Option<Zone>> = first.iter().zip(&names).map(|((_, d), n)| zone_of(n, d, wide, &mut skipped)).collect();
    if zones.iter().all(|z| z.is_none()) {
        bail!("nothing to write: no blocks with points");
    }
    let all: Vec<Vec<(String, &DataSet)>> = steps.iter().map(|(_, d)| leaves(d)).collect::<Result<_>>()?;
    for (k, l) in all.iter().enumerate() {
        if l.len() != first.len() || l.iter().zip(&zones).any(|((_, d), z)| z.as_ref().is_some_and(|z| !z.same_mesh(d))) {
            bail!("the mesh changes at time step {k}: CGNS keeps one mesh for all steps (save one step, or use VTKHDF or Exodus)");
        }
    }
    let timed = steps.len() > 1;
    let mut dims: Vec<usize> = zones.iter().flatten().map(|z| z.dim).collect();
    dims.sort_unstable();
    dims.dedup();
    dims.reverse();

    let mut root = Node::group();
    root.attr("label", AttrValue::Str(padded("Root Node of HDF5 File", 33)));
    root.attr("name", AttrValue::Str(padded("HDF5 MotherNode", 33)));
    root.attr("type", AttrValue::Str(padded("MT", 3)));
    let fmt: Vec<i8> = b"IEEE_LITTLE_32\0".iter().map(|&b| b as i8).collect();
    root.add(" format", Node::dataset(ArrayData::I8(fmt), vec![15]));
    let mut ver: Vec<i8> = b"HDF5 Version 1.14.2".iter().map(|&b| b as i8).collect();
    ver.resize(33, 0);
    root.add(" hdf5version", Node::dataset(ArrayData::I8(ver), vec![33]));
    root.add("CGNSLibraryVersion", cg("CGNSLibraryVersion", "CGNSLibraryVersion_t", "R4", Some((ArrayData::F32(vec![4.4]), vec![1]))));

    for (bi, &dim) in dims.iter().enumerate() {
        let bname = if bi == 0 { "Base".to_string() } else { format!("Base_{dim}D") };
        let mut base = cg(&bname, "CGNSBase_t", "I4", Some((ArrayData::I32(vec![dim as i32, 3]), vec![2])));
        if timed {
            base.add("SimulationType", c1("SimulationType", "SimulationType_t", "TimeAccurate"));
            let mut it = cg("BaseIterativeData", "BaseIterativeData_t", "I4", Some((ArrayData::I32(vec![steps.len() as i32]), vec![1])));
            let n = steps.len() as u64;
            it.add("TimeValues", data_array("TimeValues", ArrayData::F64(steps.iter().map(|s| s.0).collect()), vec![n]));
            it.add("IterationValues", data_array("IterationValues", ArrayData::I32((0..steps.len() as i32).collect()), vec![n]));
            base.add("BaseIterativeData", it);
        }
        for (zi, z) in zones.iter().enumerate() {
            let Some(z) = z.as_ref().filter(|z| z.dim == dim) else { continue };
            let mut node = z.node(wide);
            let d0 = first[zi].1;
            node.add("GridCoordinates", z.coordinates("GridCoordinates", d0));
            if !timed {
                if let Some(s) = z.solution("FlowSolution", d0, false) {
                    node.add("FlowSolution", s);
                }
                if let Some(s) = z.solution("FlowSolutionCC", d0, true) {
                    node.add("FlowSolutionCC", s);
                }
            } else {
                let p0 = points_of(d0).map(|p| p.data.to_f64_vec());
                let (mut coord_ptrs, mut vert_ptrs, mut cell_ptrs) = (Vec::new(), Vec::new(), Vec::new());
                let mut moved = false;
                for (k, l) in all.iter().enumerate() {
                    let d = l[zi].1;
                    let cname = if k == 0 || points_of(d).map(|p| p.data.to_f64_vec()) == p0 {
                        "GridCoordinates".to_string()
                    } else {
                        moved = true;
                        let n = format!("GridCoordinates{k:04}");
                        node.add(&n, z.coordinates(&n, d));
                        n
                    };
                    coord_ptrs.push(cname);
                    let vname = format!("FlowSolution{k:04}");
                    vert_ptrs.push(match z.solution(&vname, d, false) {
                        Some(s) => {
                            node.add(&vname, s);
                            vname
                        }
                        None => "Null".into(),
                    });
                    let cname = format!("FlowSolutionCC{k:04}");
                    cell_ptrs.push(match z.solution(&cname, d, true) {
                        Some(s) => {
                            node.add(&cname, s);
                            cname
                        }
                        None => "Null".into(),
                    });
                }
                let mut zit = cg("ZoneIterativeData", "ZoneIterativeData_t", "MT", None);
                if moved {
                    zit.add("GridCoordinatesPointers", pointers("GridCoordinatesPointers", &coord_ptrs));
                }
                if vert_ptrs.iter().any(|p| p != "Null") {
                    zit.add("FlowSolutionPointers", pointers("FlowSolutionPointers", &vert_ptrs));
                }
                if cell_ptrs.iter().any(|p| p != "Null") {
                    zit.add("FlowSolutionCellPointers", pointers("FlowSolutionCellPointers", &cell_ptrs));
                }
                node.add("ZoneIterativeData", zit);
            }
            base.add(&z.name, node);
        }
        root.add(&bname, base);
    }
    if skipped > 0 {
        tracing::warn!("CGNS writer: {skipped} cells left out (unsupported types, or of a lower dimension than the zone)");
    }
    hdf5w::write(path, &root).with_context(|| format!("writing {}", path.display()))
}
