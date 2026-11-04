//! Exodus II reader (NetCDF-3 based files: .e .exo .ex2 .g .gen).
//!
//! Each element block becomes an unstructured-grid block of a multi-block dataset.
//! Nodal, element and global variables are read per time step; scalar components named
//! `FOO_X/FOO_Y/FOO_Z` (or `FOOX/FOOY/FOOZ`) are combined into 3-vectors. When a
//! displacement vector exists it is applied to the coordinates, like ParaView does.

use crate::netcdf3::NcFile;
use crate::Loaded;
use anyhow::{anyhow, Result};
use aurafx_data::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Arc<NcFile>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn open_nc(path: &Path) -> Result<Arc<NcFile>> {
    if let Some(f) = CACHE.lock().get(path) {
        return Ok(f.clone());
    }
    let f = Arc::new(NcFile::open(path)?);
    CACHE.lock().insert(path.to_path_buf(), f.clone());
    Ok(f)
}

pub fn is_exodus_ext(path: &Path) -> bool {
    let e = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(e.as_str(), "e" | "exo" | "ex2" | "exii" | "g" | "gen" | "ex2v2")
}

pub fn open(path: &Path) -> Result<Loaded> {
    let nc = open_nc(path)?;
    let times = if nc.var("time_whole").is_some() && nc.numrecs > 0 { nc.read_all_records("time_whole")? } else { vec![] };
    if times.len() <= 1 {
        return Ok(Loaded::Static(Arc::new(read_step(path, 0)?)));
    }
    let mut s = TimeSeries::default();
    for (i, t) in times.iter().enumerate() {
        s.times.push(*t);
        s.steps.push(TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i });
    }
    Ok(Loaded::Series(s))
}

/// Exodus element type -> (VTK type, number of nodes to take, optional reordering).
fn map_type(elem_type: &str, nodes_per: usize) -> (CellType, usize, Option<&'static [usize]>) {
    let t = elem_type.to_ascii_uppercase();
    let base: String = t.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    const HEX20: [usize; 20] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15];
    const WEDGE15: [usize; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 9, 10, 11];
    match (base.as_str(), nodes_per) {
        ("HEX" | "HEXAHEDRON", 8) => (CellType::Hexahedron, 8, None),
        ("HEX" | "HEXAHEDRON", 20) => (CellType::QuadraticHexahedron, 20, Some(&HEX20)),
        ("HEX" | "HEXAHEDRON", n) if n >= 8 => (CellType::Hexahedron, 8, None),
        ("TETRA" | "TET", 4) => (CellType::Tetra, 4, None),
        ("TETRA" | "TET", 10) => (CellType::QuadraticTetra, 10, None),
        ("TETRA" | "TET", n) if n >= 4 => (CellType::Tetra, 4, None),
        ("WEDGE", 6) => (CellType::Wedge, 6, None),
        ("WEDGE", 15) => (CellType::QuadraticWedge, 15, Some(&WEDGE15)),
        ("WEDGE", n) if n >= 6 => (CellType::Wedge, 6, None),
        ("PYRAMID", n) if n >= 5 => (CellType::Pyramid, 5, None),
        ("QUAD" | "SHELL" | "QUADRILATERAL", 8) => (CellType::QuadraticQuad, 8, None),
        ("QUAD" | "SHELL" | "QUADRILATERAL", 9) => (CellType::BiquadraticQuad, 9, None),
        ("QUAD" | "SHELL" | "QUADRILATERAL", n) if n >= 4 => (CellType::Quad, 4, None),
        ("TRI" | "TRIANGLE" | "TRISHELL", 6) => (CellType::QuadraticTriangle, 6, None),
        ("TRI" | "TRIANGLE" | "TRISHELL", n) if n >= 3 => (CellType::Triangle, 3, None),
        ("BAR" | "BEAM" | "TRUSS" | "EDGE", 3) => (CellType::QuadraticEdge, 3, None),
        ("BAR" | "BEAM" | "TRUSS" | "EDGE", n) if n >= 2 => (CellType::Line, 2, None),
        ("SPHERE" | "CIRCLE" | "POINT", _) => (CellType::Vertex, 1, None),
        (_, 1) => (CellType::Vertex, 1, None),
        (_, 2) => (CellType::Line, 2, None),
        (_, 3) => (CellType::Triangle, 3, None),
        (_, 4) => (CellType::Quad, 4, None),
        (_, 8) => (CellType::Hexahedron, 8, None),
        _ => (CellType::Polygon, nodes_per, None),
    }
}

/// Group scalar arrays named X_x/X_y/X_z (any case, optional underscore) into 3-vectors.
fn group_vectors(names: &[String], values: Vec<Vec<f64>>) -> Vec<Array> {
    let mut used = vec![false; names.len()];
    let mut out = Vec::new();
    let strip = |n: &str, c: char| -> Option<String> {
        let last = n.chars().last()?;
        if last.to_ascii_lowercase() != c {
            return None;
        }
        let base = &n[..n.len() - 1];
        Some(base.trim_end_matches('_').to_string())
    };
    for i in 0..names.len() {
        if used[i] {
            continue;
        }
        if let Some(base) = strip(&names[i], 'x') {
            if base.is_empty() {
                continue;
            }
            let j = names.iter().position(|n| strip(n, 'y').as_deref() == Some(&base));
            let k = names.iter().position(|n| strip(n, 'z').as_deref() == Some(&base));
            if let Some(j) = j {
                let n = values[i].len();
                let mut v = Vec::with_capacity(3 * n);
                for t in 0..n {
                    v.push(values[i][t]);
                    v.push(values[j][t]);
                    v.push(k.map(|k| values[k][t]).unwrap_or(0.0));
                }
                used[i] = true;
                used[j] = true;
                if let Some(k) = k {
                    used[k] = true;
                }
                out.push(Array::new(base, 3, ArrayData::F64(v)));
            }
        }
    }
    for (i, n) in names.iter().enumerate() {
        if !used[i] {
            out.push(Array::new(n.clone(), 1, ArrayData::F64(values[i].clone())));
        }
    }
    out
}

fn var_names(nc: &NcFile, name_var: &str, prefix: &str, count: usize) -> Vec<String> {
    let mut names = nc.read_strings(name_var).unwrap_or_default();
    names.truncate(count);
    while names.len() < count {
        names.push(format!("{prefix}{}", names.len() + 1));
    }
    for (i, n) in names.iter_mut().enumerate() {
        if n.is_empty() {
            *n = format!("{prefix}{}", i + 1);
        }
    }
    names
}

/// Read one time step (index into time_whole) as a multi-block dataset.
pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    let nc = open_nc(path)?;
    let num_nodes = nc.dim_len("num_nodes").unwrap_or(0);
    let num_dim = nc.dim_len("num_dim").unwrap_or(3);
    let num_blk = nc.dim_len("num_el_blk").unwrap_or(0);
    let has_time = nc.numrecs > 0 && nc.var("time_whole").is_some();
    let rec = if has_time { Some(step.min(nc.numrecs - 1)) } else { None };

    // Coordinates.
    let mut coords = vec![vec![0.0; num_nodes]; 3];
    if nc.var("coordx").is_some() {
        for (k, n) in ["coordx", "coordy", "coordz"].iter().enumerate().take(num_dim) {
            coords[k] = nc.read_f64(n, None)?;
        }
    } else if nc.var("coord").is_some() {
        let all = nc.read_f64("coord", None)?;
        for k in 0..num_dim {
            coords[k] = all[k * num_nodes..(k + 1) * num_nodes].to_vec();
        }
    }

    // Nodal variables for this step.
    let n_nod_var = nc.dim_len("num_nod_var").unwrap_or(0);
    let nod_names = var_names(&nc, "name_nod_var", "NodalVar", n_nod_var);
    let mut nod_values: Vec<Vec<f64>> = Vec::with_capacity(n_nod_var);
    if let Some(r) = rec {
        if nc.var("vals_nod_var1").is_some() {
            for j in 0..n_nod_var {
                nod_values.push(nc.read_f64(&format!("vals_nod_var{}", j + 1), Some(r))?);
            }
        } else if nc.var("vals_nod_var").is_some() {
            let all = nc.read_f64("vals_nod_var", Some(r))?;
            for j in 0..n_nod_var {
                nod_values.push(all[j * num_nodes..(j + 1) * num_nodes].to_vec());
            }
        }
    }
    let nodal_arrays = if nod_values.len() == n_nod_var { group_vectors(&nod_names, nod_values) } else { vec![] };

    // Apply displacements (ParaView default) when a DISP* vector exists.
    let disp = nodal_arrays.iter().find(|a| a.ncomp == 3 && a.name.to_ascii_uppercase().starts_with("DISP"));
    let mut pts = Vec::with_capacity(3 * num_nodes);
    for i in 0..num_nodes {
        let mut p = [coords[0][i], coords[1][i], coords[2][i]];
        if let Some(d) = disp {
            for k in 0..3 {
                p[k] += d.get(i, k);
            }
        }
        pts.extend_from_slice(&p);
    }
    let global_points = Points::new_f64(pts);
    let mut global_pd = Attributes::new();
    for a in nodal_arrays {
        global_pd.insert(a);
    }
    // Global node ids.
    let node_map: Vec<f64> = nc.read_f64("node_num_map", None).unwrap_or_else(|_| (1..=num_nodes).map(|i| i as f64).collect());
    global_pd.insert(Array::new("GlobalNodeId", 1, ArrayData::I64(node_map.iter().map(|&v| v as i64).collect())));
    global_pd.active.insert(AttributeKind::GlobalIds, "GlobalNodeId".into());

    // Element variables.
    let n_el_var = nc.dim_len("num_elem_var").unwrap_or(0);
    let el_names = var_names(&nc, "name_elem_var", "ElemVar", n_el_var);
    let truth: Vec<f64> = nc.read_f64("elem_var_tab", None).unwrap_or_else(|_| vec![1.0; num_blk * n_el_var]);
    let block_ids = nc.read_f64("eb_prop1", None).unwrap_or_else(|_| (1..=num_blk).map(|i| i as f64).collect());
    let block_names = nc.read_strings("eb_names").unwrap_or_default();
    let elem_map: Vec<f64> = nc.read_f64("elem_num_map", None).unwrap_or_default();

    let mut mb = MultiBlock::default();
    let mut elem_offset = 0usize;
    for b in 0..num_blk {
        let nel = nc.dim_len(&format!("num_el_in_blk{}", b + 1)).unwrap_or(0);
        let npe = nc.dim_len(&format!("num_nod_per_el{}", b + 1)).unwrap_or(0);
        let cname = format!("connect{}", b + 1);
        let id = block_ids.get(b).copied().unwrap_or((b + 1) as f64) as i64;
        let name = block_names.get(b).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| format!("Block {id}"));
        if nel == 0 || nc.var(&cname).is_none() {
            elem_offset += nel;
            continue;
        }
        let etype = nc.var_attr_text(&cname, "elem_type").unwrap_or_default();
        let (ctype, take, reorder) = map_type(&etype, npe);
        let conn = nc.read_f64(&cname, None)?;
        let mut cells = Cells::with_capacity(nel, nel * take);
        let mut ids = vec![0u32; take];
        for e in 0..nel {
            let row = &conn[e * npe..(e + 1) * npe];
            for k in 0..take {
                let src = reorder.map(|r| r[k]).unwrap_or(k);
                ids[k] = (row[src] as i64 - 1).max(0) as u32;
            }
            cells.push(ctype, &ids);
        }
        let mut cd = Attributes::new();
        if let Some(r) = rec {
            let mut names = Vec::new();
            let mut vals = Vec::new();
            for j in 0..n_el_var {
                if truth.get(b * n_el_var + j).copied().unwrap_or(1.0) == 0.0 {
                    continue;
                }
                let vn = format!("vals_elem_var{}eb{}", j + 1, b + 1);
                if nc.var(&vn).is_some() {
                    names.push(el_names[j].clone());
                    vals.push(nc.read_f64(&vn, Some(r))?);
                }
            }
            for a in group_vectors(&names, vals) {
                cd.insert(a);
            }
        }
        cd.insert(Array::new("ObjectId", 1, ArrayData::I64(vec![id; nel])));
        if elem_map.len() >= elem_offset + nel {
            cd.insert(Array::new("GlobalElementId", 1, ArrayData::I64(elem_map[elem_offset..elem_offset + nel].iter().map(|&v| v as i64).collect())));
        }
        elem_offset += nel;
        // Compact nodes used by this block.
        let (points, point_data) = compact(&global_points, &global_pd, &mut cells.connectivity);
        let ug = UnstructuredGrid { points, cells, point_data, cell_data: cd, field_data: Attributes::new() };
        mb.blocks.push((name, Some(DataSet::UnstructuredGrid(ug))));
    }

    // Global variables and time as field data.
    let n_glo = nc.dim_len("num_glo_var").unwrap_or(0);
    if let (Some(r), true) = (rec, n_glo > 0) {
        let names = var_names(&nc, "name_glo_var", "GlobalVar", n_glo);
        if let Ok(v) = nc.read_f64("vals_glo_var", Some(r)) {
            for (n, x) in names.iter().zip(v) {
                mb.field_data.insert(Array::scalars_f64(n.clone(), vec![x]));
            }
        }
    }
    if let Some(r) = rec {
        if let Ok(t) = nc.read_f64("time_whole", Some(r)) {
            mb.field_data.insert(Array::scalars_f64("TimeValue", t));
        }
    }
    if let Some(title) = nc.attrs.get("title").and_then(|a| if let crate::netcdf3::AttrValue::Text(t) = a { Some(t.clone()) } else { None }) {
        mb.field_data.insert(Array::new("Title", 1, ArrayData::String(vec![title])));
    }
    if mb.blocks.is_empty() {
        return Err(anyhow!("Exodus file has no element blocks"));
    }
    Ok(DataSet::MultiBlock(mb))
}

fn compact(points: &Points, attrs: &Attributes, conn: &mut [u32]) -> (Points, Attributes) {
    let mut map: HashMap<u32, u32> = HashMap::with_capacity(conn.len() / 4);
    let mut order: Vec<usize> = Vec::new();
    for c in conn.iter_mut() {
        let id = *map.entry(*c).or_insert_with(|| {
            order.push(*c as usize);
            (order.len() - 1) as u32
        });
        *c = id;
    }
    (points.gather(&order), attrs.gather(&order))
}
