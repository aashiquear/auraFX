//! Extract Surface: external faces of volumetric cells, plus all surface/line/vertex cells.

use crate::simplex::{as_explicit, for_each_cell};
use anyhow::Result;
use aurafx_data::*;
use std::collections::HashMap;

/// Returns a PolyData with polys/lines/verts and cell data copied from the parent cells.
/// Curved (higher-order) cells are drawn through their nodes, like ParaView's default
/// nonlinear subdivision level of 1.
pub fn extract_surface(ds: &DataSet) -> Result<PolyData> {
    extract_surface_level(ds, 1)
}

/// Extract Surface with a nonlinear subdivision level for curved cells: 0 uses only their
/// corners, 1 goes through their nodes, and each further level halves those pieces with points
/// evaluated on the curved cell.
pub fn extract_surface_level(ds: &DataSet, level: u32) -> Result<PolyData> {
    match ds {
        DataSet::ImageData(_) | DataSet::RectilinearGrid(_) | DataSet::StructuredGrid(_) => structured_surface(ds),
        DataSet::MultiBlock(mb) => {
            let mut out: Option<PolyData> = None;
            for (_, leaf) in mb.leaves() {
                let pd = extract_surface_level(leaf, level)?;
                match &mut out {
                    None => out = Some(pd),
                    Some(acc) => append_poly(acc, &pd),
                }
            }
            Ok(out.unwrap_or_default())
        }
        DataSet::Table(_) => Ok(PolyData::default()),
        _ => explicit_surface(ds, level),
    }
}

fn append_poly(a: &mut PolyData, b: &PolyData) {
    let off = a.points.len() as u32;
    a.points.append(&b.points);
    a.verts.append(&b.verts, off);
    a.lines.append(&b.lines, off);
    a.polys.append(&b.polys, off);
    a.strips.append(&b.strips, off);
    let names: Vec<String> = a.point_data.arrays.keys().cloned().collect();
    for n in names {
        match b.point_data.get(&n) {
            Some(src) => {
                let _ = a.point_data.get_mut(&n).unwrap().data.append(&src.data);
            }
            None => {
                a.point_data.remove(&n);
            }
        }
    }
    // Cell data ordering across categories is not preserved on append; drop to stay correct.
    a.cell_data = Attributes::new();
}

fn structured_surface(ds: &DataSet) -> Result<PolyData> {
    let (dims, points, pd, cd) = match ds {
        DataSet::ImageData(d) => (d.dims(), d.points(), &d.point_data, &d.cell_data),
        DataSet::RectilinearGrid(d) => (d.dims(), d.points(), &d.point_data, &d.cell_data),
        DataSet::StructuredGrid(d) => (d.dims(), d.points.clone(), &d.point_data, &d.cell_data),
        _ => unreachable!(),
    };
    let [nx, ny, nz] = dims;
    if nx == 0 || ny == 0 || nz == 0 {
        return Ok(PolyData::default());
    }
    let pid = |i: usize, j: usize, k: usize| (i + nx * (j + ny * k)) as u32;
    let cx = nx.saturating_sub(1).max(1);
    let cy = ny.saturating_sub(1).max(1);
    let cid = |i: usize, j: usize, k: usize| i + cx * (j + cy * k);
    let mut polys = Cells::new();
    let mut parent: Vec<usize> = Vec::new();
    let ndim = [nx, ny, nz].iter().filter(|&&n| n > 1).count();
    if ndim <= 1 {
        // Line or point: handled via generic path.
        return explicit_surface(&as_explicit(ds), 0);
    }
    if ndim == 2 {
        for k in 0..nz.saturating_sub(1).max(1) {
            for j in 0..ny.saturating_sub(1).max(1) {
                for i in 0..nx.saturating_sub(1).max(1) {
                    let q = if nz == 1 {
                        [pid(i, j, 0), pid(i + 1, j, 0), pid(i + 1, j + 1, 0), pid(i, j + 1, 0)]
                    } else if ny == 1 {
                        [pid(i, 0, k), pid(i + 1, 0, k), pid(i + 1, 0, k + 1), pid(i, 0, k + 1)]
                    } else {
                        [pid(0, j, k), pid(0, j + 1, k), pid(0, j + 1, k + 1), pid(0, j, k + 1)]
                    };
                    polys.push(CellType::Quad, &q);
                    parent.push(cid(i, j, k));
                }
            }
        }
    } else {
        // Six boundary faces.
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                polys.push(CellType::Quad, &[pid(i, j, 0), pid(i, j + 1, 0), pid(i + 1, j + 1, 0), pid(i + 1, j, 0)]);
                parent.push(cid(i, j, 0));
                let k = nz - 1;
                polys.push(CellType::Quad, &[pid(i, j, k), pid(i + 1, j, k), pid(i + 1, j + 1, k), pid(i, j + 1, k)]);
                parent.push(cid(i, j, nz - 2));
            }
        }
        for k in 0..nz - 1 {
            for i in 0..nx - 1 {
                polys.push(CellType::Quad, &[pid(i, 0, k), pid(i + 1, 0, k), pid(i + 1, 0, k + 1), pid(i, 0, k + 1)]);
                parent.push(cid(i, 0, k));
                let j = ny - 1;
                polys.push(CellType::Quad, &[pid(i, j, k), pid(i, j, k + 1), pid(i + 1, j, k + 1), pid(i + 1, j, k)]);
                parent.push(cid(i, ny - 2, k));
            }
        }
        for k in 0..nz - 1 {
            for j in 0..ny - 1 {
                polys.push(CellType::Quad, &[pid(0, j, k), pid(0, j, k + 1), pid(0, j + 1, k + 1), pid(0, j + 1, k)]);
                parent.push(cid(0, j, k));
                let i = nx - 1;
                polys.push(CellType::Quad, &[pid(i, j, k), pid(i, j + 1, k), pid(i, j + 1, k + 1), pid(i, j, k + 1)]);
                parent.push(cid(nx - 2, j, k));
            }
        }
    }
    let (points, point_data) = crate::interp::compact_points(&points, pd, &mut polys.connectivity);
    let mut cell_data = cd.gather(&parent);
    cell_data.insert(Array::new("vtkOriginalCellIds", 1, ArrayData::I64(parent.iter().map(|&i| i as i64).collect())));
    Ok(PolyData { points, polys, point_data, cell_data, ..Default::default() })
}

/// Linear base type and (edge -> mid-edge node) table of quadratic cells, in VTK node order.
fn quadratic_edges(ty: CellType) -> Option<(CellType, &'static [((usize, usize), usize)])> {
    const TET10: [((usize, usize), usize); 6] = [((0, 1), 4), ((1, 2), 5), ((2, 0), 6), ((0, 3), 7), ((1, 3), 8), ((2, 3), 9)];
    const HEX20: [((usize, usize), usize); 12] = [
        ((0, 1), 8),
        ((1, 2), 9),
        ((2, 3), 10),
        ((3, 0), 11),
        ((4, 5), 12),
        ((5, 6), 13),
        ((6, 7), 14),
        ((7, 4), 15),
        ((0, 4), 16),
        ((1, 5), 17),
        ((2, 6), 18),
        ((3, 7), 19),
    ];
    const WEDGE15: [((usize, usize), usize); 9] =
        [((0, 1), 6), ((1, 2), 7), ((2, 0), 8), ((3, 4), 9), ((4, 5), 10), ((5, 3), 11), ((0, 3), 12), ((1, 4), 13), ((2, 5), 14)];
    const PYR13: [((usize, usize), usize); 8] = [((0, 1), 5), ((1, 2), 6), ((2, 3), 7), ((3, 0), 8), ((0, 4), 9), ((1, 4), 10), ((2, 4), 11), ((3, 4), 12)];
    const TRI6: [((usize, usize), usize); 3] = [((0, 1), 3), ((1, 2), 4), ((2, 0), 5)];
    const QUAD8: [((usize, usize), usize); 4] = [((0, 1), 4), ((1, 2), 5), ((2, 3), 6), ((3, 0), 7)];
    Some(match ty {
        CellType::QuadraticTetra => (CellType::Tetra, &TET10),
        CellType::QuadraticHexahedron | CellType::TriquadraticHexahedron => (CellType::Hexahedron, &HEX20),
        CellType::QuadraticWedge => (CellType::Wedge, &WEDGE15),
        CellType::QuadraticPyramid => (CellType::Pyramid, &PYR13),
        CellType::QuadraticTriangle | CellType::BiquadraticTriangle => (CellType::Triangle, &TRI6),
        CellType::QuadraticQuad | CellType::BiquadraticQuad => (CellType::Quad, &QUAD8),
        _ => return None,
    })
}

/// Split a quadratic triangle or quad (corners + one mid node per edge k -> k+1) into linear
/// triangles that pass through the mid nodes, keeping the winding.
fn subdivide_face(c: &[u32], m: &[u32]) -> Vec<Vec<u32>> {
    match c.len() {
        3 => vec![vec![c[0], m[0], m[2]], vec![m[0], c[1], m[1]], vec![m[2], m[1], c[2]], vec![m[0], m[1], m[2]]],
        4 => {
            vec![vec![c[0], m[0], m[3]], vec![m[0], c[1], m[1]], vec![m[1], c[2], m[2]], vec![m[3], m[2], c[3]], vec![m[0], m[1], m[2]], vec![m[0], m[2], m[3]]]
        }
        _ => vec![c.to_vec()],
    }
}

/// Mid nodes of the edges of a face given by local corner indices.
fn face_mids(face: &[usize], edges: &[((usize, usize), usize)], pts: &[u32]) -> Option<Vec<u32>> {
    (0..face.len())
        .map(|k| {
            let (a, b) = (face[k], face[(k + 1) % face.len()]);
            edges.iter().find(|((x, y), _)| (*x == a && *y == b) || (*x == b && *y == a)).and_then(|(_, m)| pts.get(*m).copied())
        })
        .collect()
}

/// Extract Surface for grids of linear cells (tets, hexes, wedges, pyramids, voxels and any
/// 0-2D cells), in parallel and with little memory: faces are grouped by their smallest point,
/// matched within each group, and the unmatched (boundary) faces are emitted in cell order.
/// Gives exactly what the general path gives. None when the grid has other cells.
fn linear_surface(ds: &DataSet) -> Option<Result<PolyData>> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    let DataSet::UnstructuredGrid(ug) = ds else { return None };
    let cells = &ug.cells;
    let ok = |t: &CellType| {
        t.dimension() < 3 && t.is_linear() || matches!(t, CellType::Tetra | CellType::Voxel | CellType::Hexahedron | CellType::Wedge | CellType::Pyramid)
    };
    if cells.polyhedron_faces.is_some() || !cells.types.par_iter().all(ok) {
        return None;
    }
    let n_cells = cells.len();
    let n_pts = ug.points.len();
    if n_cells.saturating_mul(8) >= u32::MAX as usize {
        return None;
    }
    // Sorted corner ids of a face, padded with u32::MAX.
    let face_key = |cid: usize, f: usize| -> [u32; 4] {
        let pts = cells.cell(cid);
        let fl = cells.types[cid].faces()[f];
        let mut k = [u32::MAX; 4];
        for (i, &c) in fl.iter().enumerate() {
            k[i] = pts[c];
        }
        k.sort_unstable();
        k
    };
    let faces_of = |cid: usize| if cells.types[cid].dimension() == 3 { cells.types[cid].faces().len() } else { 0 };
    // 1. Faces per smallest point.
    let counts: Vec<AtomicU32> = (0..n_pts + 1).map(|_| AtomicU32::new(0)).collect();
    (0..n_cells).into_par_iter().with_min_len(4096).for_each(|cid| {
        for f in 0..faces_of(cid) {
            counts[face_key(cid, f)[0] as usize].fetch_add(1, Ordering::Relaxed);
        }
    });
    let mut start = vec![0u32; n_pts + 1];
    let mut acc = 0u32;
    for (i, c) in counts.iter().enumerate() {
        start[i] = acc;
        acc += c.load(Ordering::Relaxed);
    }
    let total = acc as usize;
    // 2. Bucket the faces (cell * 8 + face).
    let fill: Vec<AtomicU32> = start.iter().map(|&s| AtomicU32::new(s)).collect();
    let bucket: Vec<AtomicU32> = (0..total).map(|_| AtomicU32::new(0)).collect();
    (0..n_cells).into_par_iter().with_min_len(4096).for_each(|cid| {
        for f in 0..faces_of(cid) {
            let slot = fill[face_key(cid, f)[0] as usize].fetch_add(1, Ordering::Relaxed);
            bucket[slot as usize].store((cid * 8 + f) as u32, Ordering::Relaxed);
        }
    });
    // 3. Faces that occur more than once are interior.
    let shared: Vec<AtomicBool> = (0..n_cells * 8).map(|_| AtomicBool::new(false)).collect();
    (0..n_pts).into_par_iter().with_min_len(1024).for_each(|p| {
        let (a, b) = (start[p] as usize, start[p + 1] as usize);
        if b - a < 2 {
            return;
        }
        let mut v: Vec<([u32; 4], u32)> = (a..b)
            .map(|i| {
                let id = bucket[i].load(Ordering::Relaxed);
                (face_key(id as usize / 8, id as usize % 8), id)
            })
            .collect();
        v.sort_unstable();
        for w in 0..v.len() {
            let dup = (w > 0 && v[w - 1].0 == v[w].0) || (w + 1 < v.len() && v[w + 1].0 == v[w].0);
            if dup {
                shared[v[w].1 as usize].store(true, Ordering::Relaxed);
            }
        }
    });
    drop(bucket);
    // 4. Emit in cell order: 0-2D cells, then boundary faces.
    let chunks = crate::interp::par_cell_chunks(
        ds,
        || (Cells::new(), Cells::new(), Cells::new(), Cells::new(), Vec::<usize>::new(), Vec::<usize>::new(), Vec::<usize>::new(), Vec::<usize>::new()),
        |(v, l, p, bf, pv, pl, pp, pf), cid, ty, pts, _| match ty.dimension() {
            0 => {
                v.push(ty, pts);
                pv.push(cid);
            }
            1 => {
                l.push(ty, pts);
                pl.push(cid);
            }
            2 => {
                p.push(ty, pts);
                pp.push(cid);
            }
            _ => {
                for (f, fl) in ty.faces().iter().enumerate() {
                    if !shared[cid * 8 + f].load(Ordering::Relaxed) {
                        let poly: Vec<u32> = fl.iter().map(|&k| pts[k]).collect();
                        bf.push(if poly.len() == 3 { CellType::Triangle } else { CellType::Quad }, &poly);
                        pf.push(cid);
                    }
                }
            }
        },
    );
    let (mut verts, mut lines, mut polys, mut faces) = (Cells::new(), Cells::new(), Cells::new(), Cells::new());
    let (mut pv, mut pl, mut pp, mut pf) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (v, l, p, bf, a, b, c, d) in chunks {
        verts.append(&v, 0);
        lines.append(&l, 0);
        polys.append(&p, 0);
        faces.append(&bf, 0);
        pv.extend(a);
        pl.extend(b);
        pp.extend(c);
        pf.extend(d);
    }
    polys.append(&faces, 0);
    pp.extend(pf);
    let mut all_conn: Vec<u32> = Vec::with_capacity(verts.connectivity.len() + lines.connectivity.len() + polys.connectivity.len());
    all_conn.extend(&verts.connectivity);
    all_conn.extend(&lines.connectivity);
    all_conn.extend(&polys.connectivity);
    let (points, point_data) = crate::interp::compact_points(&ug.points, &ug.point_data, &mut all_conn);
    let (nv, nl) = (verts.connectivity.len(), lines.connectivity.len());
    verts.connectivity.copy_from_slice(&all_conn[..nv]);
    lines.connectivity.copy_from_slice(&all_conn[nv..nv + nl]);
    polys.connectivity.copy_from_slice(&all_conn[nv + nl..]);
    let mut parents = pv;
    parents.extend(pl);
    parents.extend(pp);
    let mut cell_data = ug.cell_data.gather(&parents);
    cell_data.insert(Array::new("vtkOriginalCellIds", 1, ArrayData::I64(parents.iter().map(|&i| i as i64).collect())));
    Some(Ok(PolyData { points, verts, lines, polys, strips: Cells::new(), point_data, cell_data, field_data: ug.field_data.clone() }))
}

fn explicit_surface(ds: &DataSet, level: u32) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = crate::simplex::parts(&ds).ok_or_else(|| anyhow::anyhow!("unsupported dataset for surface extraction"))?;
    if let Some(r) = linear_surface(&ds) {
        return r;
    }
    // Curved cells evaluate extra points (appended after the original ones).
    let curved = crate::higher_order::has_curved(&ds);
    let mut ev = crate::higher_order::Evaluator::new(points, pd, cd);
    let cells_of = match ds.as_ref() {
        DataSet::UnstructuredGrid(ug) => Some(&ug.cells),
        _ => None,
    };
    // Face hashing for 3D cells.
    struct FaceRec {
        count: u32,
        cell: usize,
        /// Linear polygons drawn for this face (one, or several for a curved quadratic face).
        polys: Vec<Vec<u32>>,
        /// Face index (and orientation) of a curved cell, tessellated only if on the boundary.
        curved: Option<(usize, bool)>,
    }
    let mut faces: HashMap<Vec<u32>, FaceRec> = HashMap::new();
    let mut verts = Cells::new();
    let mut lines = Cells::new();
    let mut polys = Cells::new();
    let mut parent_v: Vec<usize> = Vec::new();
    let mut parent_l: Vec<usize> = Vec::new();
    let mut parent_p: Vec<usize> = Vec::new();
    let mut face_order: Vec<Vec<u32>> = Vec::new();

    for_each_cell(&ds, |cid, ty, pts, ph_faces| {
        let ho = if curved { ev.cell(cid, ty, pts.len()) } else { None };
        if let Some(cell) = ho.as_ref().filter(|_| level == 0 && ty.dimension() < 3) {
            // Level 0: straight cells through the corners.
            let corners = &pts[..cell.shape.corners().len()];
            if ty.dimension() == 1 {
                lines.push(CellType::Line, corners);
                parent_l.push(cid);
            } else {
                polys.push(cell.shape.linear_type(), corners);
                parent_p.push(cid);
            }
            return;
        }
        if let Some(cell) = ho.filter(|_| level > 0) {
            let corners = cell.shape.corners().len();
            let key_of = |f: &[usize]| f.iter().map(|&k| pts[k]).collect::<Vec<u32>>();
            match cell.shape.dimension() {
                1 => {
                    let n = crate::higher_order::divisions(&cell, level)[0];
                    let line: Vec<u32> = (0..=n).map(|i| ev.point(&cell, pts, [i as f64 / n as f64, 0.0, 0.0])).collect();
                    lines.push(CellType::PolyLine, &line);
                    parent_l.push(cid);
                }
                2 => {
                    let all: Vec<usize> = (0..corners).collect();
                    for p in crate::higher_order::face_polys(&mut ev, &cell, pts, &all, level) {
                        polys.push(if p.len() == 3 { CellType::Triangle } else { CellType::Quad }, &p);
                        parent_p.push(cid);
                    }
                }
                _ => {
                    let flip = crate::higher_order::flipped(points, pts, cell.shape);
                    for (fi, f) in crate::higher_order::faces_of(cell.shape, flip).iter().enumerate() {
                        let mut key = key_of(f);
                        key.sort_unstable();
                        match faces.get_mut(&key) {
                            Some(r) => r.count += 1,
                            None => {
                                face_order.push(key.clone());
                                faces.insert(key, FaceRec { count: 1, cell: cid, polys: Vec::new(), curved: Some((fi, flip)) });
                            }
                        }
                    }
                }
            }
            return;
        }
        match ty.dimension() {
            0 => {
                verts.push(ty, pts);
                parent_v.push(cid);
            }
            1 => {
                if ty == CellType::QuadraticEdge && pts.len() >= 3 {
                    lines.push(CellType::PolyLine, &[pts[0], pts[2], pts[1]]);
                } else {
                    lines.push(ty, pts);
                }
                parent_l.push(cid);
            }
            2 => match quadratic_edges(ty).filter(|_| level > 0) {
                Some((base, edges)) if pts.len() >= base.fixed_point_count().unwrap_or(0) + edges.len() => {
                    let corners: Vec<usize> = (0..base.fixed_point_count().unwrap_or(0)).collect();
                    let c: Vec<u32> = corners.iter().map(|&k| pts[k]).collect();
                    let m = face_mids(&corners, edges, pts).unwrap_or_default();
                    for t in subdivide_face(&c, &m) {
                        polys.push(CellType::Triangle, &t);
                        parent_p.push(cid);
                    }
                }
                _ => {
                    polys.push(ty, pts);
                    parent_p.push(cid);
                }
            },
            _ => {
                // Faces are matched by their sorted corner ids.
                let mut add_face = |f: Vec<u32>, polys: Vec<Vec<u32>>| {
                    let mut key = f;
                    key.sort_unstable();
                    match faces.get_mut(&key) {
                        Some(r) => r.count += 1,
                        None => {
                            face_order.push(key.clone());
                            faces.insert(key, FaceRec { count: 1, cell: cid, polys, curved: None });
                        }
                    }
                };
                if ty == CellType::Polyhedron {
                    if let Some(fs) = ph_faces {
                        for f in fs {
                            add_face(f.clone(), vec![f.clone()]);
                        }
                    }
                } else if ty.is_linear() {
                    for f in ty.faces() {
                        let c: Vec<u32> = f.iter().map(|&k| pts[k]).collect();
                        add_face(c.clone(), vec![c]);
                    }
                } else if let Some((base, edges)) = quadratic_edges(ty).filter(|(b, e)| level > 0 && pts.len() >= b.fixed_point_count().unwrap_or(0) + e.len())
                {
                    // Curved faces through the mid-edge nodes.
                    for f in base.faces() {
                        let c: Vec<u32> = f.iter().map(|&k| pts[k]).collect();
                        let polys = match face_mids(f, edges, pts) {
                            Some(m) => subdivide_face(&c, &m),
                            None => vec![c.clone()],
                        };
                        add_face(c, polys);
                    }
                } else {
                    // Higher-order: use the linear corner faces of the base type.
                    let base = match ty {
                        CellType::QuadraticTetra | CellType::LagrangeTetrahedron | CellType::BezierTetrahedron => CellType::Tetra,
                        CellType::QuadraticWedge
                        | CellType::QuadraticLinearWedge
                        | CellType::BiquadraticQuadraticWedge
                        | CellType::LagrangeWedge
                        | CellType::BezierWedge => CellType::Wedge,
                        CellType::QuadraticPyramid | CellType::LagrangePyramid | CellType::BezierPyramid => CellType::Pyramid,
                        _ => CellType::Hexahedron,
                    };
                    for f in base.faces() {
                        let c: Vec<u32> = f.iter().map(|&k| pts[k]).collect();
                        add_face(c.clone(), vec![c]);
                    }
                }
            }
        }
    });
    for key in face_order {
        let r = &faces[&key];
        if r.count == 1 {
            let tess;
            let face_polys = match (r.curved, cells_of) {
                (Some((fi, flip)), Some(cells)) => {
                    let ids = cells.cell(r.cell);
                    match ev.cell(r.cell, cells.types[r.cell], ids.len()) {
                        Some(cell) => {
                            tess = crate::higher_order::face_polys(&mut ev, &cell, ids, crate::higher_order::faces_of(cell.shape, flip)[fi], level);
                            &tess
                        }
                        None => &r.polys,
                    }
                }
                _ => &r.polys,
            };
            for p in face_polys {
                let ty = match p.len() {
                    3 => CellType::Triangle,
                    4 => CellType::Quad,
                    _ => CellType::Polygon,
                };
                polys.push(ty, p);
                parent_p.push(r.cell);
            }
        }
    }
    // Compact points.
    let mut all_conn: Vec<u32> = Vec::with_capacity(verts.connectivity.len() + lines.connectivity.len() + polys.connectivity.len());
    all_conn.extend(&verts.connectivity);
    all_conn.extend(&lines.connectivity);
    all_conn.extend(&polys.connectivity);
    let finished;
    let (all_points, all_pd) = if ev.has_new_points() {
        finished = ev.finish();
        (&finished.0, &finished.1)
    } else {
        (points, pd)
    };
    let (points, point_data) = crate::interp::compact_points(all_points, all_pd, &mut all_conn);
    let (nv, nl) = (verts.connectivity.len(), lines.connectivity.len());
    verts.connectivity.copy_from_slice(&all_conn[..nv]);
    lines.connectivity.copy_from_slice(&all_conn[nv..nv + nl]);
    polys.connectivity.copy_from_slice(&all_conn[nv + nl..]);
    let mut parents = parent_v;
    parents.extend(parent_l);
    parents.extend(parent_p);
    let mut cell_data = cd.gather(&parents);
    cell_data.insert(Array::new("vtkOriginalCellIds", 1, ArrayData::I64(parents.iter().map(|&i| i as i64).collect())));
    Ok(PolyData { points, verts, lines, polys, strips: Cells::new(), point_data, cell_data, field_data: fd.clone() })
}

/// Surfaces of the pieces of a dataset (in piece order, with each piece's first cell id) as
/// one surface: faces where two pieces meet are dropped (they lie inside the whole) and
/// `vtkOriginalCellIds` are shifted to ids of the whole.
pub fn merge_piece_surfaces(pieces: Vec<(PolyData, usize)>) -> PolyData {
    use std::collections::HashMap;
    let mut parts = Vec::with_capacity(pieces.len());
    let mut b = Bounds::EMPTY;
    for (mut p, offset) in pieces {
        if let Some(a) = p.cell_data.get_mut("vtkOriginalCellIds") {
            let v: Vec<i64> = (0..a.len()).map(|i| a.get(i, 0) as i64 + offset as i64).collect();
            a.data = ArrayData::I64(v);
        }
        b = b.union(&DataSet::PolyData(p.clone()).bounds());
        parts.push(DataSet::PolyData(p));
    }
    let DataSet::PolyData(mut all) = aurafx_data::pieces::merge(parts) else { return PolyData::default() };
    // Faces present twice (same corner positions) are piece interfaces.
    let q = 1.0 / (b.diagonal().max(1e-30) * 1e-9);
    let key = |c: &[u32]| -> Vec<[i64; 3]> {
        let mut k: Vec<[i64; 3]> = c.iter().map(|&i| all.points.get(i as usize).map(|v| (v * q).round() as i64)).collect();
        k.sort_unstable();
        k
    };
    let mut count: HashMap<Vec<[i64; 3]>, u32> = HashMap::new();
    for i in 0..all.polys.len() {
        *count.entry(key(all.polys.cell(i))).or_default() += 1;
    }
    let keep: Vec<usize> = (0..all.polys.len()).filter(|&i| count[&key(all.polys.cell(i))] == 1).collect();
    if keep.len() == all.polys.len() {
        return all;
    }
    let mut polys = Cells::new();
    for &i in &keep {
        polys.push(all.polys.types[i], all.polys.cell(i));
    }
    // Cell data: verts, lines, then the kept polys (strips after).
    let (nv, nl) = (all.verts.len(), all.lines.len());
    let np = all.polys.len();
    let mut cells: Vec<usize> = (0..nv + nl).collect();
    cells.extend(keep.iter().map(|&i| nv + nl + i));
    cells.extend((0..all.strips.len()).map(|i| nv + nl + np + i));
    all.cell_data = all.cell_data.gather(&cells);
    all.polys = polys;
    // Drop points no cell uses any more.
    let mut conn: Vec<u32> = Vec::new();
    for c in [&all.verts, &all.lines, &all.polys, &all.strips] {
        conn.extend_from_slice(&c.connectivity);
    }
    let (points, point_data) = crate::interp::compact_points(&all.points, &all.point_data, &mut conn);
    let mut off = 0;
    for c in [&mut all.verts, &mut all.lines, &mut all.polys, &mut all.strips] {
        let m = c.connectivity.len();
        c.connectivity.copy_from_slice(&conn[off..off + m]);
        off += m;
    }
    all.points = points;
    all.point_data = point_data;
    all
}
