//! CGNS (`.cgns`, HDF5 or ADF storage): bases and zones as blocks; structured zones (curvilinear
//! grids) and unstructured zones (every standard element type, MIXED with or without CGNS 4
//! offsets, NGON/NFACE polyhedra); the volume elements form the zone's grid (boundary sections
//! are left out, as ParaView does by default); vertex and cell-center flow solutions, with
//! `NameX/NameY/NameZ` joined into vectors; time steps from `BaseIterativeData` and the
//! zones' `FlowSolutionPointers`/`GridCoordinatesPointers`.

use crate::adf::{AdfFile, AdfNode};
use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use hdf5_reader::group::Group;
use std::path::Path;
use std::sync::Arc;

/// A CGNS node in either storage: an HDF5 group or an ADF node.
enum Node {
    H(Group),
    A(AdfNode),
}

fn h5_attr(g: &Group, name: &str) -> Option<String> {
    g.attribute(name).ok().and_then(|a| a.read_string().ok()).map(|s| s.trim_end_matches('\0').trim().to_string())
}

fn label(g: &Node) -> String {
    match g {
        Node::H(g) => h5_attr(g, "label").unwrap_or_default(),
        Node::A(a) => a.label.clone(),
    }
}

fn node_name(g: &Node) -> String {
    match g {
        Node::H(g) => h5_attr(g, "name").filter(|s| !s.is_empty()).unwrap_or_else(|| g.name().rsplit('/').next().unwrap_or("").to_string()),
        Node::A(a) => a.name.clone(),
    }
}

fn all_children(g: &Node) -> Vec<Node> {
    match g {
        Node::H(g) => g.groups().unwrap_or_default().into_iter().map(Node::H).collect(),
        Node::A(a) => a.children().into_iter().map(Node::A).collect(),
    }
}

fn children(g: &Node, lab: &str) -> Vec<Node> {
    all_children(g).into_iter().filter(|c| label(c) == lab).collect()
}

fn child(g: &Node, lab: &str) -> Option<Node> {
    children(g, lab).into_iter().next()
}

fn named(g: &Node, name: &str) -> Option<Node> {
    all_children(g).into_iter().find(|c| node_name(c) == name)
}

/// A node's data: values (as f64 or i64) and the shape in C order (as HDF5 stores it).
fn data(g: &Node) -> Result<(ArrayData, Vec<usize>)> {
    match g {
        Node::H(h) => {
            let ds = h.dataset(" data").with_context(|| format!("{} has no data", node_name(g)))?;
            crate::vtkhdf::read_dataset(&ds)
        }
        Node::A(a) => a.data(),
    }
}

fn ints(g: &Node) -> Result<Vec<i64>> {
    let (d, _) = data(g)?;
    Ok((0..d.len()).map(|i| d.get_f64(i) as i64).collect())
}

fn text(g: &Node) -> String {
    match data(g) {
        Ok((d, _)) => {
            let bytes: Vec<u8> = (0..d.len()).map(|i| d.get_f64(i) as i64 as u8).collect();
            String::from_utf8_lossy(&bytes).trim_end_matches('\0').trim().to_string()
        }
        Err(_) => String::new(),
    }
}

/// Rows of a (n, 32) character array.
fn strings(g: &Node) -> Vec<String> {
    let Ok((d, shape)) = data(g) else { return Vec::new() };
    let w = shape.last().copied().unwrap_or(32).max(1);
    let bytes: Vec<u8> = (0..d.len()).map(|i| d.get_f64(i) as i64 as u8).collect();
    bytes.chunks(w).map(|c| String::from_utf8_lossy(c).trim_end_matches('\0').trim().to_string()).collect()
}

/// CGNS element type: (nodes, VTK type, reordering CGNS -> VTK).
fn element(code: i64) -> Option<(usize, CellType, &'static [usize])> {
    Some(match code {
        2 => (1, CellType::Vertex, &[]),
        3 => (2, CellType::Line, &[]),
        4 => (3, CellType::QuadraticEdge, &[]),
        5 => (3, CellType::Triangle, &[]),
        6 => (6, CellType::QuadraticTriangle, &[]),
        7 => (4, CellType::Quad, &[]),
        8 => (8, CellType::QuadraticQuad, &[]),
        9 => (9, CellType::BiquadraticQuad, &[]),
        10 => (4, CellType::Tetra, &[]),
        11 => (10, CellType::QuadraticTetra, &[]),
        12 => (5, CellType::Pyramid, &[]),
        21 => (13, CellType::QuadraticPyramid, &[]),
        14 => (6, CellType::Wedge, &[]),
        // CGNS: bottom edges, vertical edges, top edges; VTK: bottom, top, vertical.
        15 => (15, CellType::QuadraticWedge, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 9, 10, 11]),
        17 => (8, CellType::Hexahedron, &[]),
        18 => (20, CellType::QuadraticHexahedron, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15]),
        19 => (27, CellType::TriquadraticHexahedron, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15, 24, 22, 21, 23, 20, 25, 26]),
        _ => return None,
    })
}

fn dimension(t: CellType) -> usize {
    use CellType::*;
    match t {
        Vertex | PolyVertex => 0,
        Line | PolyLine | QuadraticEdge => 1,
        Triangle | Quad | Polygon | QuadraticTriangle | QuadraticQuad | BiquadraticQuad => 2,
        _ => 3,
    }
}

/// Volume cells of an unstructured zone (elements of the base's cell dimension).
fn unstructured_cells(zone: &Node, cell_dim: usize, np: usize) -> Result<Cells> {
    let mut sections: Vec<(i64, Node)> = children(zone, "Elements_t")
        .into_iter()
        .map(|e| (named(&e, "ElementRange").and_then(|r| ints(&r).ok()).and_then(|v| v.first().copied()).unwrap_or(0), e))
        .collect();
    sections.sort_by_key(|s| s.0);
    let mut cells = Cells::new();
    let mut faces: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut ngon: Vec<Vec<u32>> = Vec::new(); // faces of NGON sections, by element number order
    let mut ngon_first = 0i64;
    let check = |ids: &[i64]| -> Result<Vec<u32>> {
        ids.iter().map(|&i| if i >= 1 && (i as usize) <= np { Ok((i - 1) as u32) } else { Err(anyhow!("node {i} out of range")) }).collect()
    };
    for (first, e) in &sections {
        let head = ints(e)?;
        let ty = *head.first().ok_or_else(|| anyhow!("element section without a type"))?;
        let conn = ints(&named(e, "ElementConnectivity").ok_or_else(|| anyhow!("no ElementConnectivity"))?)?;
        let offsets = named(e, "ElementStartOffset").and_then(|o| ints(&o).ok());
        match ty {
            20 => {
                // MIXED: a type before each element's nodes.
                let mut k = 0;
                let mut n = 0;
                while k < conn.len() {
                    let t = conn[k];
                    let (npe, vtk, order) = element(t).ok_or_else(|| anyhow!("unsupported element type {t} in a MIXED section"))?;
                    let start = k + 1;
                    let end = match &offsets {
                        Some(o) => *o.get(n + 1).ok_or_else(|| anyhow!("ElementStartOffset too short"))? as usize,
                        None => start + npe,
                    };
                    let ids = check(&conn[start..end])?;
                    if dimension(vtk) == cell_dim {
                        let ids: Vec<u32> = if order.is_empty() { ids } else { order.iter().map(|&j| ids[j]).collect() };
                        cells.push(vtk, &ids);
                        faces.push(Vec::new());
                    }
                    k = end;
                    n += 1;
                }
            }
            22 => {
                // NGON_n: polygons (faces of polyhedra in 3-D, cells in 2-D).
                let o = offsets.ok_or_else(|| anyhow!("NGON without ElementStartOffset"))?;
                if ngon.is_empty() {
                    ngon_first = *first;
                }
                for w in o.windows(2) {
                    let ids = check(&conn[w[0] as usize..w[1] as usize])?;
                    if cell_dim == 2 {
                        cells.push(CellType::Polygon, &ids);
                        faces.push(Vec::new());
                    }
                    ngon.push(ids);
                }
            }
            23 => {
                // NFACE_n: polyhedra made of NGON faces (signed: negative = reversed).
                let o = offsets.ok_or_else(|| anyhow!("NFACE without ElementStartOffset"))?;
                for w in o.windows(2) {
                    let mut cell_faces = Vec::new();
                    let mut all = Vec::new();
                    for &fi in &conn[w[0] as usize..w[1] as usize] {
                        let f = ngon.get((fi.abs() - ngon_first) as usize).ok_or_else(|| anyhow!("face {fi} out of range"))?;
                        let mut f = f.clone();
                        if fi < 0 {
                            f.reverse();
                        }
                        all.extend_from_slice(&f);
                        cell_faces.push(f);
                    }
                    all.sort_unstable();
                    all.dedup();
                    cells.push(CellType::Polyhedron, &all);
                    faces.push(cell_faces);
                }
            }
            t => {
                let (npe, vtk, order) = element(t).ok_or_else(|| anyhow!("unsupported CGNS element type {t}"))?;
                if dimension(vtk) != cell_dim {
                    continue;
                }
                for el in conn.chunks_exact(npe) {
                    let ids = check(el)?;
                    let ids: Vec<u32> = if order.is_empty() { ids } else { order.iter().map(|&j| ids[j]).collect() };
                    cells.push(vtk, &ids);
                    faces.push(Vec::new());
                }
            }
        }
    }
    if faces.iter().any(|f| !f.is_empty()) {
        cells.polyhedron_faces = Some(faces);
    }
    Ok(cells)
}

/// Arrays of a flow solution, with `NameX/NameY/NameZ` joined into vectors.
fn solution_arrays(sol: &Node, n: usize) -> Vec<Array> {
    let mut arrays: Vec<(String, Vec<f64>)> = Vec::new();
    for a in children(sol, "DataArray_t") {
        if let Ok((d, _)) = data(&a) {
            if d.len() == n {
                arrays.push((node_name(&a), d.to_f64_vec()));
            }
        }
    }
    let mut out = Vec::new();
    let mut used = vec![false; arrays.len()];
    for i in 0..arrays.len() {
        if used[i] {
            continue;
        }
        let name = &arrays[i].0;
        if let Some(base) = name.strip_suffix('X').filter(|b| !b.is_empty()) {
            let find = |suffix: char| arrays.iter().position(|a| a.0 == format!("{base}{suffix}"));
            if let (Some(y), Some(z)) = (find('Y'), find('Z')) {
                let v: Vec<f64> = (0..n).flat_map(|k| [arrays[i].1[k], arrays[y].1[k], arrays[z].1[k]]).collect();
                out.push(Array::new(base.to_string(), 3, ArrayData::F64(v)));
                used[i] = true;
                used[y] = true;
                used[z] = true;
                continue;
            }
        }
        used[i] = true;
        out.push(Array::scalars_f64(name.clone(), arrays[i].1.clone()));
    }
    out
}

fn read_zone(zone: &Node, cell_dim: usize, step: usize) -> Result<DataSet> {
    let ztype = child(zone, "ZoneType_t").map(|z| text(&z)).unwrap_or_default();
    let structured = ztype.eq_ignore_ascii_case("Structured") || (ztype.is_empty() && children(zone, "Elements_t").is_empty());
    let size = ints(zone)?;
    let iter = child(zone, "ZoneIterativeData_t");
    let pointer = |name: &str| -> Option<String> {
        iter.as_ref().and_then(|z| named(z, name)).map(|p| strings(&p)).and_then(|v| v.get(step).cloned()).filter(|s| !s.is_empty() && s != "Null")
    };
    // Coordinates (moving grids: GridCoordinatesPointers).
    let coords = pointer("GridCoordinatesPointers")
        .and_then(|n| named(zone, &n))
        .or_else(|| children(zone, "GridCoordinates_t").into_iter().find(|g| node_name(g) == "GridCoordinates"))
        .or_else(|| child(zone, "GridCoordinates_t"))
        .ok_or_else(|| anyhow!("zone without coordinates"))?;
    let coord = |n: &str| named(&coords, n).map(|g| data(&g).map(|d| d.0.to_f64_vec())).transpose();
    let x = coord("CoordinateX")?.ok_or_else(|| anyhow!("no CoordinateX"))?;
    let np = x.len();
    let y = coord("CoordinateY")?.unwrap_or(vec![0.0; np]);
    let z = coord("CoordinateZ")?.unwrap_or(vec![0.0; np]);
    let points = Points::new_f64((0..np).flat_map(|i| [x[i], y[i], z[i]]).collect());
    let (mut pd, mut cd) = (Attributes::new(), Attributes::new());
    let mut ds = if structured {
        // Vertex sizes come first (the HDF5 array is (3, index dimension)).
        let idim = size.len() / 3;
        let mut n = [1usize; 3];
        for k in 0..idim.min(3) {
            n[k] = size[k] as usize;
        }
        DataSet::StructuredGrid(StructuredGrid {
            extent: [0, n[0] as i64 - 1, 0, n[1] as i64 - 1, 0, n[2] as i64 - 1],
            points,
            point_data: Attributes::new(),
            cell_data: Attributes::new(),
            field_data: Attributes::new(),
        })
    } else {
        let cells = unstructured_cells(zone, cell_dim, np)?;
        DataSet::UnstructuredGrid(UnstructuredGrid {
            points,
            cells,
            point_data: Attributes::new(),
            cell_data: Attributes::new(),
            field_data: Attributes::new(),
        })
    };
    let nc = ds.n_cells();
    // Solutions: the step's pointed ones, else every flow solution.
    let sols = children(zone, "FlowSolution_t");
    // Solutions some step points at are used at that step only; the others always.
    const POINTERS: [&str; 3] = ["FlowSolutionPointers", "FlowSolutionVertexPointers", "FlowSolutionCellPointers"];
    let wanted: Vec<String> = POINTERS.iter().filter_map(|p| pointer(p)).collect();
    let pointed: Vec<String> = POINTERS.iter().filter_map(|p| iter.as_ref().and_then(|z| named(z, p))).flat_map(|g| strings(&g)).collect();
    for s in sols {
        let name = node_name(&s);
        if pointed.contains(&name) && !wanted.contains(&name) {
            continue;
        }
        let loc = child(&s, "GridLocation_t").map(|g| text(&g)).unwrap_or_else(|| "Vertex".into());
        let cell = loc.eq_ignore_ascii_case("CellCenter");
        for a in solution_arrays(&s, if cell { nc } else { np }) {
            if cell {
                cd.insert(a);
            } else {
                pd.insert(a);
            }
        }
    }
    if let Some(p) = ds.point_data_mut() {
        *p = pd;
    }
    if let Some(c) = ds.cell_data_mut() {
        *c = cd;
    }
    Ok(ds)
}

/// The file's root node (the HDF5 file is returned too, to keep it open while it is read).
fn root(path: &Path) -> Result<(Option<hdf5_reader::Hdf5File>, Node)> {
    let mut head = [0u8; 32];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).with_context(|| format!("opening {}", path.display()))?;
    if crate::adf::is_adf(&head[..n]) {
        return Ok((None, Node::A(AdfFile::open(path)?.root()?)));
    }
    let f = hdf5_reader::Hdf5File::open(path).with_context(|| format!("opening {} (neither an HDF5 nor an ADF CGNS file)", path.display()))?;
    let r = f.root_group().map_err(|e| anyhow!("{e}"))?;
    Ok((Some(f), Node::H(r)))
}

fn bases(root: &Node) -> Result<Vec<Node>> {
    let b = children(root, "CGNSBase_t");
    if b.is_empty() {
        bail!("no CGNS base in this file");
    }
    Ok(b)
}

fn read_step_file(path: &Path, step: usize) -> Result<DataSet> {
    let (_keep, root) = root(path)?;
    let mut top = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    let bases = bases(&root)?;
    let many = bases.len() > 1;
    for b in bases {
        let cell_dim = ints(&b).ok().and_then(|v| v.first().copied()).unwrap_or(3) as usize;
        let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
        for z in children(&b, "Zone_t") {
            let name = node_name(&z);
            match read_zone(&z, cell_dim, step) {
                Ok(d) => mb.blocks.push((name, Some(d))),
                Err(e) => tracing::warn!("CGNS zone {name}: {e:#}"),
            }
        }
        if many {
            top.blocks.push((node_name(&b), Some(DataSet::MultiBlock(mb))));
        } else {
            top = mb;
        }
    }
    Ok(DataSet::MultiBlock(top))
}

pub fn open(path: &Path) -> Result<Loaded> {
    let (_keep, root) = root(path)?;
    let times: Vec<f64> = bases(&root)?
        .iter()
        .find_map(|b| child(b, "BaseIterativeData_t").and_then(|it| named(&it, "TimeValues")).and_then(|t| data(&t).ok()).map(|d| d.0.to_f64_vec()))
        .unwrap_or_default();
    if times.len() > 1 {
        return Ok(Loaded::Series(TimeSeries {
            times: times.clone(),
            steps: (0..times.len()).map(|i| TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i }).collect(),
        }));
    }
    Ok(Loaded::Static(Arc::new(read_step_file(path, 0)?)))
}

pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    read_step_file(path, step)
}
