//! Exodus II writer (NetCDF-3, 64-bit offset format), readable by ParaView, SEACAS tools and
//! auraFX itself.
//!
//! Every leaf of the input becomes one element block per cell type. Point data become nodal
//! variables and cell data element variables (vectors are written as `nameX/Y/Z`, which
//! readers combine again). Several time steps can be written at once. When a `DISP*` vector is
//! present, the undisplaced coordinates are stored so a reader that applies displacements gets
//! the same geometry back.

use anyhow::{bail, Result};
use aurafx_data::*;
use std::path::Path;

// ------------------------------------------------------------------ NetCDF-3 encoder

const NC_CHAR: u32 = 2;
const NC_INT: u32 = 4;
const NC_FLOAT: u32 = 5;
const NC_DOUBLE: u32 = 6;

enum Att {
    Text(String),
    Int(Vec<i32>),
    Float(Vec<f32>),
}

struct VarDef {
    name: String,
    dims: Vec<usize>,
    atts: Vec<(String, Att)>,
    ty: u32,
    /// Fixed-size variables: their bytes. Record variables: one slab per record.
    data: VarData,
}

enum VarData {
    Fixed(Vec<u8>),
    Records(Vec<Vec<u8>>),
}

#[derive(Default)]
struct NcWriter {
    /// (name, length); length 0 = the unlimited (record) dimension.
    dims: Vec<(String, usize)>,
    gatts: Vec<(String, Att)>,
    vars: Vec<VarDef>,
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

fn put_name(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(&(s.len() as u32).to_be_bytes());
    b.extend_from_slice(s.as_bytes());
    b.resize(pad4(b.len()), 0);
}

fn put_atts(b: &mut Vec<u8>, atts: &[(String, Att)]) {
    if atts.is_empty() {
        b.extend_from_slice(&[0; 8]);
        return;
    }
    b.extend_from_slice(&0x0Cu32.to_be_bytes());
    b.extend_from_slice(&(atts.len() as u32).to_be_bytes());
    for (name, a) in atts {
        put_name(b, name);
        let (ty, n, bytes): (u32, usize, Vec<u8>) = match a {
            Att::Text(s) => (NC_CHAR, s.len(), s.as_bytes().to_vec()),
            Att::Int(v) => (NC_INT, v.len(), v.iter().flat_map(|x| x.to_be_bytes()).collect()),
            Att::Float(v) => (NC_FLOAT, v.len(), v.iter().flat_map(|x| x.to_be_bytes()).collect()),
        };
        b.extend_from_slice(&ty.to_be_bytes());
        b.extend_from_slice(&(n as u32).to_be_bytes());
        b.extend_from_slice(&bytes);
        b.resize(pad4(b.len()), 0);
    }
}

fn type_size(ty: u32) -> usize {
    match ty {
        NC_CHAR => 1,
        NC_INT | NC_FLOAT => 4,
        _ => 8,
    }
}

impl NcWriter {
    fn dim(&mut self, name: &str, len: usize) -> usize {
        if let Some(i) = self.dims.iter().position(|d| d.0 == name) {
            return i;
        }
        self.dims.push((name.to_string(), len));
        self.dims.len() - 1
    }

    fn is_record(&self, v: &VarDef) -> bool {
        v.dims.first().map(|&d| self.dims[d].1 == 0).unwrap_or(false)
    }

    /// Bytes of one variable (fixed) or one record slab (record variable), padded to 4.
    fn vsize(&self, v: &VarDef) -> usize {
        let skip = if self.is_record(v) { 1 } else { 0 };
        pad4(v.dims[skip..].iter().map(|&d| self.dims[d].1).product::<usize>() * type_size(v.ty))
    }

    fn header(&self, numrecs: usize, begins: &[u64]) -> Vec<u8> {
        let mut b = b"CDF\x02".to_vec();
        b.extend_from_slice(&(numrecs as u32).to_be_bytes());
        b.extend_from_slice(&0x0Au32.to_be_bytes());
        b.extend_from_slice(&(self.dims.len() as u32).to_be_bytes());
        for (n, l) in &self.dims {
            put_name(&mut b, n);
            b.extend_from_slice(&(*l as u32).to_be_bytes());
        }
        put_atts(&mut b, &self.gatts);
        b.extend_from_slice(&0x0Bu32.to_be_bytes());
        b.extend_from_slice(&(self.vars.len() as u32).to_be_bytes());
        for (v, begin) in self.vars.iter().zip(begins) {
            put_name(&mut b, &v.name);
            b.extend_from_slice(&(v.dims.len() as u32).to_be_bytes());
            for d in &v.dims {
                b.extend_from_slice(&(*d as u32).to_be_bytes());
            }
            put_atts(&mut b, &v.atts);
            b.extend_from_slice(&v.ty.to_be_bytes());
            b.extend_from_slice(&(self.vsize(v) as u32).to_be_bytes());
            b.extend_from_slice(&begin.to_be_bytes());
        }
        b
    }

    fn write(&self, path: &Path) -> Result<()> {
        let numrecs = self.vars.iter().filter_map(|v| if let VarData::Records(r) = &v.data { Some(r.len()) } else { None }).max().unwrap_or(0);
        let hlen = self.header(numrecs, &vec![0; self.vars.len()]).len();
        let mut begins = vec![0u64; self.vars.len()];
        let mut off = hlen as u64;
        for (i, v) in self.vars.iter().enumerate().filter(|(_, v)| !self.is_record(v)) {
            begins[i] = off;
            off += self.vsize(v) as u64;
        }
        let rec_vars: Vec<usize> = (0..self.vars.len()).filter(|&i| self.is_record(&self.vars[i])).collect();
        // A file with a single record variable stores its records unpadded.
        let single = rec_vars.len() == 1;
        let slab = |i: usize| {
            if single {
                self.vars[i].dims[1..].iter().map(|&d| self.dims[d].1).product::<usize>() * type_size(self.vars[i].ty)
            } else {
                self.vsize(&self.vars[i])
            }
        };
        for &i in &rec_vars {
            begins[i] = off;
            off += slab(i) as u64;
        }
        let mut out = self.header(numrecs, &begins);
        for v in &self.vars {
            if let VarData::Fixed(bytes) = &v.data {
                let n = self.vsize(v);
                if bytes.len() > n {
                    bail!("variable {} has {} bytes, expected {n}", v.name, bytes.len());
                }
                out.extend_from_slice(bytes);
                out.resize(out.len() + (n - bytes.len()), 0);
            }
        }
        for r in 0..numrecs {
            for &i in &rec_vars {
                let n = slab(i);
                let VarData::Records(recs) = &self.vars[i].data else { unreachable!() };
                let bytes = recs.get(r).map(|b| b.as_slice()).unwrap_or(&[]);
                out.extend_from_slice(&bytes[..bytes.len().min(n)]);
                out.resize(out.len() + n.saturating_sub(bytes.len()), 0);
            }
        }
        std::fs::write(path, out)?;
        Ok(())
    }
}

fn be_f64(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn be_i32(v: &[i64]) -> Vec<u8> {
    v.iter().flat_map(|x| (*x as i32).to_be_bytes()).collect()
}

fn names_bytes(names: &[String], len: usize) -> Vec<u8> {
    let mut b = Vec::with_capacity(names.len() * len);
    for n in names {
        let s = &n.as_bytes()[..n.len().min(len - 1)];
        b.extend_from_slice(s);
        b.resize(b.len() + (len - s.len()), 0);
    }
    b
}

// ------------------------------------------------------------------ Exodus layout

const LEN_NAME: usize = 33;

/// Arrays that are bookkeeping, not results.
pub(crate) fn is_internal(name: &str) -> bool {
    matches!(
        name,
        "GlobalNodeId"
            | "GlobalElementId"
            | "ObjectId"
            | "PedigreeElementId"
            | "PedigreeNodeId"
            | "vtkOriginalCellIds"
            | "vtkOriginalPointIds"
            | "vtkGhostType"
    )
}

/// Exodus element type and node order (indices into the VTK cell) for a VTK cell type.
fn exodus_type(t: CellType, n: usize) -> Option<(&'static str, Vec<usize>)> {
    let first = |k: usize| (0..k).collect::<Vec<_>>();
    Some(match t {
        CellType::Vertex => ("SPHERE", first(1)),
        CellType::Line | CellType::QuadraticEdge | CellType::CubicLine => ("BAR2", first(2)),
        CellType::Triangle | CellType::QuadraticTriangle | CellType::BiquadraticTriangle => ("TRI3", first(3)),
        CellType::Quad | CellType::QuadraticQuad | CellType::BiquadraticQuad | CellType::QuadraticLinearQuad => ("QUAD4", first(4)),
        CellType::Pixel => ("QUAD4", vec![0, 1, 3, 2]),
        CellType::Tetra => ("TETRA4", first(4)),
        CellType::QuadraticTetra if n == 10 => ("TETRA10", first(10)),
        CellType::QuadraticTetra => ("TETRA4", first(4)),
        CellType::Hexahedron | CellType::QuadraticHexahedron | CellType::TriquadraticHexahedron | CellType::BiquadraticQuadraticHexahedron => {
            ("HEX8", first(8))
        }
        CellType::Voxel => ("HEX8", vec![0, 1, 3, 2, 4, 5, 7, 6]),
        CellType::Wedge | CellType::QuadraticWedge | CellType::QuadraticLinearWedge | CellType::BiquadraticQuadraticWedge => ("WEDGE6", first(6)),
        CellType::Pyramid | CellType::QuadraticPyramid => ("PYRAMID5", first(5)),
        _ => return None,
    })
}

struct Block {
    name: String,
    elem_type: &'static str,
    /// 1-based global node ids, row-major (n_elem × nodes_per).
    connect: Vec<i64>,
    nodes_per: usize,
    /// Index of the source leaf and, per element, the source cell index.
    leaf: usize,
    cells: Vec<usize>,
}

/// Split polygons, strips and polylines into simple cells: (type, node ids, source cell).
fn simple_cells(ug: &UnstructuredGrid) -> Vec<(CellType, Vec<u32>, usize)> {
    let mut out = Vec::with_capacity(ug.cells.len());
    for (i, (t, ids)) in ug.cells.iter().enumerate() {
        match t {
            CellType::Polygon if ids.len() == 3 => out.push((CellType::Triangle, ids.to_vec(), i)),
            CellType::Polygon if ids.len() == 4 => out.push((CellType::Quad, ids.to_vec(), i)),
            CellType::Polygon => {
                for k in 1..ids.len().saturating_sub(1) {
                    out.push((CellType::Triangle, vec![ids[0], ids[k], ids[k + 1]], i));
                }
            }
            CellType::TriangleStrip => {
                for k in 0..ids.len().saturating_sub(2) {
                    let tri = if k % 2 == 0 { vec![ids[k], ids[k + 1], ids[k + 2]] } else { vec![ids[k + 1], ids[k], ids[k + 2]] };
                    out.push((CellType::Triangle, tri, i));
                }
            }
            CellType::PolyLine => {
                for w in ids.windows(2) {
                    out.push((CellType::Line, w.to_vec(), i));
                }
            }
            CellType::PolyVertex => {
                for &p in ids {
                    out.push((CellType::Vertex, vec![p], i));
                }
            }
            _ => out.push((t, ids.to_vec(), i)),
        }
    }
    out
}

/// Component suffixes used for multi-component variables.
fn comp_names(name: &str, ncomp: usize) -> Vec<String> {
    match ncomp {
        1 => vec![name.to_string()],
        // "VX VY VZ" / "velocityx ...": the convention readers (ParaView included) join back into "V" / "velocity".
        2 | 3 => {
            let upper = name.chars().last().map(|c| !c.is_lowercase()).unwrap_or(true);
            let sfx = if upper { ["X", "Y", "Z"] } else { ["x", "y", "z"] };
            sfx[..ncomp].iter().map(|s| format!("{name}{s}")).collect()
        }
        6 => ["_xx", "_yy", "_zz", "_xy", "_yz", "_xz"].iter().map(|s| format!("{name}{s}")).collect(),
        9 => ["_xx", "_xy", "_xz", "_yx", "_yy", "_yz", "_zx", "_zy", "_zz"].iter().map(|s| format!("{name}{s}")).collect(),
        n => (1..=n).map(|k| format!("{name}_{k}")).collect(),
    }
}

/// Flatten a dataset into named unstructured leaves.
fn leaves(ds: &DataSet) -> Result<Vec<(String, UnstructuredGrid)>> {
    let mut out = Vec::new();
    match ds {
        DataSet::MultiBlock(mb) => {
            for (name, d) in mb.leaves() {
                if let Some(ug) = d.to_unstructured() {
                    let short = name.rsplit('/').next().unwrap_or(&name).to_string();
                    out.push((short, ug));
                }
            }
        }
        DataSet::Table(_) => bail!("tables cannot be written as Exodus"),
        d => out.push(("Block 1".to_string(), d.to_unstructured().ok_or_else(|| anyhow::anyhow!("cannot convert to a mesh"))?)),
    }
    if out.is_empty() {
        bail!("nothing to write");
    }
    Ok(out)
}

/// Write one or more time steps (all with the same mesh) to an Exodus II file.
pub fn write_exodus_steps(steps: &[(f64, &DataSet)], path: &Path) -> Result<()> {
    let (_, first) = steps.first().ok_or_else(|| anyhow::anyhow!("no data"))?;
    let base = leaves(first)?;

    // Nodes: leaves concatenated.
    let mut node_off = Vec::with_capacity(base.len());
    let mut num_nodes = 0;
    for (_, ug) in &base {
        node_off.push(num_nodes);
        num_nodes += ug.points.len();
    }

    // Element blocks: one per (leaf, Exodus type), in first-appearance order.
    let mut blocks: Vec<Block> = Vec::new();
    let mut skipped = 0usize;
    for (li, (name, ug)) in base.iter().enumerate() {
        let mut mine: Vec<usize> = Vec::new();
        for (t, ids, src) in simple_cells(ug) {
            let Some((et, order)) = exodus_type(t, ids.len()) else {
                skipped += 1;
                continue;
            };
            if ids.len() < order.len() {
                skipped += 1;
                continue;
            }
            let bi = match mine.iter().copied().find(|&b| blocks[b].elem_type == et) {
                Some(b) => b,
                None => {
                    blocks.push(Block { name: name.clone(), elem_type: et, connect: vec![], nodes_per: order.len(), leaf: li, cells: vec![] });
                    mine.push(blocks.len() - 1);
                    blocks.len() - 1
                }
            };
            let b = &mut blocks[bi];
            b.connect.extend(order.iter().map(|&k| (node_off[li] + ids[k] as usize + 1) as i64));
            b.cells.push(src);
        }
        if mine.len() > 1 {
            for &b in &mine {
                blocks[b].name = format!("{name} {}", blocks[b].elem_type);
            }
        }
    }
    if skipped > 0 {
        tracing::warn!("Exodus writer: {skipped} polyhedral or unsupported cells were skipped");
    }
    if blocks.is_empty() {
        bail!("no cells that Exodus can store (polyhedra are not supported)");
    }
    let num_elem: usize = blocks.iter().map(|b| b.cells.len()).sum();

    // Variables: union over leaves (missing values are written as 0).
    let collect = |get: &dyn Fn(&UnstructuredGrid) -> &Attributes| -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = Vec::new();
        for (_, ug) in &base {
            for a in get(ug).iter() {
                if is_internal(&a.name) || a.scalar_type() == ScalarType::String || v.iter().any(|(n, _)| *n == a.name) {
                    continue;
                }
                v.push((a.name.clone(), a.ncomp));
            }
        }
        v
    };
    let nod_vars = collect(&|u| &u.point_data);
    let elem_vars = collect(&|u| &u.cell_data);
    let nod_names: Vec<String> = nod_vars.iter().flat_map(|(n, c)| comp_names(n, *c)).collect();
    let elem_names: Vec<String> = elem_vars.iter().flat_map(|(n, c)| comp_names(n, *c)).collect();
    let disp = nod_vars.iter().find(|(n, c)| *c == 3 && n.to_ascii_uppercase().starts_with("DISP")).map(|(n, _)| n.clone());

    let mut w = NcWriter::default();
    w.gatts.push(("api_version".into(), Att::Float(vec![8.03])));
    w.gatts.push(("version".into(), Att::Float(vec![8.03])));
    w.gatts.push(("floating_point_word_size".into(), Att::Int(vec![8])));
    w.gatts.push(("file_size".into(), Att::Int(vec![1])));
    w.gatts.push(("maximum_name_length".into(), Att::Int(vec![(LEN_NAME - 1) as i32])));
    w.gatts.push(("int64_status".into(), Att::Int(vec![0])));
    w.gatts.push(("title".into(), Att::Text(format!("Written by auraFX {}", env!("CARGO_PKG_VERSION")))));
    // Standard Exodus dimensions (len_string, len_line and four are expected even when unused).
    w.dim("len_string", 33);
    let d_len_name = w.dim("len_name", LEN_NAME);
    w.dim("len_line", 81);
    w.dim("four", 4);
    let d_time = w.dim("time_step", 0);
    let d_dim = w.dim("num_dim", 3);
    let d_nodes = w.dim("num_nodes", num_nodes);
    let _d_elem = w.dim("num_elem", num_elem);
    let d_blk = w.dim("num_el_blk", blocks.len());
    let d_nod_var = if nod_names.is_empty() { None } else { Some(w.dim("num_nod_var", nod_names.len())) };
    let d_elem_var = if elem_names.is_empty() { None } else { Some(w.dim("num_elem_var", elem_names.len())) };
    let mut blk_dims = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let de = w.dim(&format!("num_el_in_blk{}", i + 1), b.cells.len());
        let dn = w.dim(&format!("num_nod_per_el{}", i + 1), b.nodes_per);
        blk_dims.push((de, dn));
    }

    let fixed = |name: &str, dims: Vec<usize>, ty: u32, data: Vec<u8>, atts: Vec<(String, Att)>| VarDef {
        name: name.into(),
        dims,
        atts,
        ty,
        data: VarData::Fixed(data),
    };
    w.vars.push(VarDef {
        name: "time_whole".into(),
        dims: vec![d_time],
        atts: vec![],
        ty: NC_DOUBLE,
        data: VarData::Records(steps.iter().map(|(t, _)| be_f64(&[*t])).collect()),
    });
    w.vars.push(fixed("eb_status", vec![d_blk], NC_INT, be_i32(&vec![1; blocks.len()]), vec![]));
    w.vars.push(fixed("eb_prop1", vec![d_blk], NC_INT, be_i32(&(1..=blocks.len() as i64).collect::<Vec<_>>()), vec![("name".into(), Att::Text("ID".into()))]));
    w.vars.push(fixed("eb_names", vec![d_blk, d_len_name], NC_CHAR, names_bytes(&blocks.iter().map(|b| b.name.clone()).collect::<Vec<_>>(), LEN_NAME), vec![]));
    w.vars.push(fixed("coor_names", vec![d_dim, d_len_name], NC_CHAR, names_bytes(&["X".into(), "Y".into(), "Z".into()], LEN_NAME), vec![]));

    // Coordinates (undisplaced when a displacement vector is present).
    let mut xyz = [Vec::with_capacity(num_nodes), Vec::with_capacity(num_nodes), Vec::with_capacity(num_nodes)];
    for (_, ug) in &base {
        let d = disp.as_ref().and_then(|n| ug.point_data.get(n));
        for i in 0..ug.points.len() {
            let p = ug.points.get(i);
            for k in 0..3 {
                xyz[k].push(p[k] - d.map(|a| a.get(i, k)).unwrap_or(0.0));
            }
        }
    }
    for (k, n) in ["coordx", "coordy", "coordz"].iter().enumerate() {
        w.vars.push(fixed(n, vec![d_nodes], NC_DOUBLE, be_f64(&xyz[k]), vec![]));
    }
    // Global ids, when the data carries them.
    let all_have = |name: &str, pts: bool| base.iter().all(|(_, u)| if pts { u.point_data.get(name).is_some() } else { u.cell_data.get(name).is_some() });
    if all_have("GlobalNodeId", true) {
        let ids: Vec<i64> = base
            .iter()
            .flat_map(|(_, u)| {
                let a = u.point_data.get("GlobalNodeId").unwrap();
                (0..a.len()).map(|i| a.get(i, 0) as i64).collect::<Vec<_>>()
            })
            .collect();
        w.vars.push(fixed("node_num_map", vec![d_nodes], NC_INT, be_i32(&ids), vec![]));
    }
    for (i, b) in blocks.iter().enumerate() {
        let (de, dn) = blk_dims[i];
        w.vars.push(fixed(&format!("connect{}", i + 1), vec![de, dn], NC_INT, be_i32(&b.connect), vec![("elem_type".into(), Att::Text(b.elem_type.into()))]));
    }

    // Nodal variables, one record variable per component.
    if let Some(dv) = d_nod_var {
        w.vars.push(fixed("name_nod_var", vec![dv, d_len_name], NC_CHAR, names_bytes(&nod_names, LEN_NAME), vec![]));
        let mut j = 0;
        for (name, ncomp) in &nod_vars {
            for k in 0..*ncomp {
                j += 1;
                let mut recs = Vec::with_capacity(steps.len());
                for (_, ds) in steps {
                    let ls = leaves(ds)?;
                    let mut v = Vec::with_capacity(num_nodes);
                    for (li, (_, ug)) in base.iter().enumerate() {
                        let a = ls.get(li).and_then(|(_, u)| u.point_data.get(name)).filter(|a| a.ncomp == *ncomp && a.len() == ug.points.len());
                        v.extend((0..ug.points.len()).map(|i| a.map(|a| a.get(i, k)).unwrap_or(0.0)));
                    }
                    recs.push(be_f64(&v));
                }
                w.vars.push(VarDef {
                    name: format!("vals_nod_var{j}"),
                    dims: vec![d_time, d_nodes],
                    atts: vec![],
                    ty: NC_DOUBLE,
                    data: VarData::Records(recs),
                });
            }
        }
    }

    // Element variables: one record variable per component and block, with a truth table.
    if let Some(dv) = d_elem_var {
        w.vars.push(fixed("name_elem_var", vec![dv, d_len_name], NC_CHAR, names_bytes(&elem_names, LEN_NAME), vec![]));
        let mut truth = Vec::with_capacity(blocks.len() * elem_names.len());
        for b in &blocks {
            for (name, ncomp) in &elem_vars {
                let has = base[b.leaf].1.cell_data.get(name).is_some();
                truth.extend(std::iter::repeat_n(has as i64, *ncomp));
            }
        }
        w.vars.push(fixed("elem_var_tab", vec![d_blk, dv], NC_INT, be_i32(&truth), vec![]));
        let step_leaves: Vec<Vec<(String, UnstructuredGrid)>> = steps.iter().map(|(_, ds)| leaves(ds)).collect::<Result<_>>()?;
        let mut j = 0;
        for (name, ncomp) in &elem_vars {
            for k in 0..*ncomp {
                j += 1;
                for (bi, b) in blocks.iter().enumerate() {
                    if base[b.leaf].1.cell_data.get(name).is_none() {
                        continue;
                    }
                    let recs = step_leaves
                        .iter()
                        .map(|ls| {
                            let a = ls.get(b.leaf).and_then(|(_, u)| u.cell_data.get(name)).filter(|a| a.ncomp == *ncomp);
                            be_f64(&b.cells.iter().map(|&c| a.filter(|a| c < a.len()).map(|a| a.get(c, k)).unwrap_or(0.0)).collect::<Vec<_>>())
                        })
                        .collect();
                    w.vars.push(VarDef {
                        name: format!("vals_elem_var{j}eb{}", bi + 1),
                        dims: vec![d_time, blk_dims[bi].0],
                        atts: vec![],
                        ty: NC_DOUBLE,
                        data: VarData::Records(recs),
                    });
                }
            }
        }
    }
    w.write(path)
}

/// Write a single dataset (its `TimeValue`, if any, becomes the time of the step).
pub fn write_exodus(ds: &DataSet, path: &Path) -> Result<()> {
    let t = ds.field_data().get("TimeValue").map(|a| a.get(0, 0)).unwrap_or(0.0);
    write_exodus_steps(&[(t, ds)], path)
}
