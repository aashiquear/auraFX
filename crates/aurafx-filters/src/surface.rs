//! Extract Surface: external faces of volumetric cells, plus all surface/line/vertex cells.

use crate::simplex::{as_explicit, for_each_cell};
use anyhow::Result;
use aurafx_data::*;
use std::collections::HashMap;

/// Returns a PolyData with polys/lines/verts and cell data copied from the parent cells.
pub fn extract_surface(ds: &DataSet) -> Result<PolyData> {
    match ds {
        DataSet::ImageData(_) | DataSet::RectilinearGrid(_) | DataSet::StructuredGrid(_) => structured_surface(ds),
        DataSet::MultiBlock(mb) => {
            let mut out: Option<PolyData> = None;
            for (_, leaf) in mb.leaves() {
                let pd = extract_surface(leaf)?;
                match &mut out {
                    None => out = Some(pd),
                    Some(acc) => append_poly(acc, &pd),
                }
            }
            Ok(out.unwrap_or_default())
        }
        DataSet::Table(_) => Ok(PolyData::default()),
        _ => explicit_surface(ds),
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
        return explicit_surface(&as_explicit(ds));
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
    Ok(PolyData { points, polys, point_data, cell_data: cd.gather(&parent), ..Default::default() })
}

fn explicit_surface(ds: &DataSet) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = crate::simplex::parts(&ds).ok_or_else(|| anyhow::anyhow!("unsupported dataset for surface extraction"))?;
    // Face hashing for 3D cells.
    struct FaceRec {
        count: u32,
        cell: usize,
        pts: Vec<u32>,
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
        match ty.dimension() {
            0 => {
                verts.push(ty, pts);
                parent_v.push(cid);
            }
            1 => {
                lines.push(ty, pts);
                parent_l.push(cid);
            }
            2 => {
                polys.push(ty, pts);
                parent_p.push(cid);
            }
            _ => {
                let mut add_face = |f: Vec<u32>| {
                    let mut key = f.clone();
                    key.sort_unstable();
                    match faces.get_mut(&key) {
                        Some(r) => r.count += 1,
                        None => {
                            face_order.push(key.clone());
                            faces.insert(key, FaceRec { count: 1, cell: cid, pts: f });
                        }
                    }
                };
                if ty == CellType::Polyhedron {
                    if let Some(fs) = ph_faces {
                        for f in fs {
                            add_face(f.clone());
                        }
                    }
                } else if ty.is_linear() {
                    for f in ty.faces() {
                        add_face(f.iter().map(|&k| pts[k]).collect());
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
                        add_face(f.iter().map(|&k| pts[k]).collect());
                    }
                }
            }
        }
    });
    for key in face_order {
        let r = &faces[&key];
        if r.count == 1 {
            let ty = match r.pts.len() {
                3 => CellType::Triangle,
                4 => CellType::Quad,
                _ => CellType::Polygon,
            };
            polys.push(ty, &r.pts);
            parent_p.push(r.cell);
        }
    }
    // Compact points.
    let mut all_conn: Vec<u32> = Vec::with_capacity(verts.connectivity.len() + lines.connectivity.len() + polys.connectivity.len());
    all_conn.extend(&verts.connectivity);
    all_conn.extend(&lines.connectivity);
    all_conn.extend(&polys.connectivity);
    let (points, point_data) = crate::interp::compact_points(points, pd, &mut all_conn);
    let (nv, nl) = (verts.connectivity.len(), lines.connectivity.len());
    verts.connectivity.copy_from_slice(&all_conn[..nv]);
    lines.connectivity.copy_from_slice(&all_conn[nv..nv + nl]);
    polys.connectivity.copy_from_slice(&all_conn[nv + nl..]);
    let mut parents = parent_v;
    parents.extend(parent_l);
    parents.extend(parent_p);
    Ok(PolyData { points, verts, lines, polys, strips: Cells::new(), point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}
