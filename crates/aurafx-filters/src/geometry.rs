//! Geometry and attribute filters: normals, clean, connectivity, shrink, reflect, elevation,
//! cell size, mesh quality, smoothing, triangulation, tetrahedralization, masking, tubes.

use crate::simplex::{as_explicit, decompose, for_each_cell, parts, Simplex};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::collections::HashMap;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = norm(a);
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Point (and cell) normals of a polygonal surface.
pub fn surface_normals(ds: &DataSet, flip: bool, cell_normals: bool) -> Result<PolyData> {
    let mut pd = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => crate::surface::extract_surface(other)?,
    };
    let n = pd.points.len();
    let mut acc = vec![[0.0f64; 3]; n];
    let mut cn = Vec::with_capacity(pd.polys.len());
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        let mut nn = [0.0; 3];
        for k in 0..c.len() {
            let p = pd.points.get(c[k] as usize);
            let q = pd.points.get(c[(k + 1) % c.len()] as usize);
            nn[0] += (p[1] - q[1]) * (p[2] + q[2]);
            nn[1] += (p[2] - q[2]) * (p[0] + q[0]);
            nn[2] += (p[0] - q[0]) * (p[1] + q[1]);
        }
        for &p in c {
            for k in 0..3 {
                acc[p as usize][k] += nn[k];
            }
        }
        cn.push(unit(nn));
    }
    let s = if flip { -1.0 } else { 1.0 };
    let normals: Vec<f64> = acc.iter().flat_map(|a| unit(*a).map(|x| x * s)).collect();
    pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(normals)));
    if cell_normals && pd.verts.is_empty() && pd.lines.is_empty() && pd.strips.is_empty() {
        pd.cell_data.insert(Array::new("Normals", 3, ArrayData::F64(cn.iter().flat_map(|c| c.map(|x| x * s)).collect())));
    }
    Ok(pd)
}

/// Merge points closer than `tol` (absolute) and drop unused points.
pub fn clean(ds: &DataSet, tol: f64) -> Result<DataSet> {
    let ds = as_explicit(ds).into_owned();
    let (points, pd, _, _) = parts(&ds).ok_or_else(|| anyhow!("clean: unsupported dataset"))?;
    let q = if tol > 0.0 { tol } else { ds.bounds().diagonal().max(1e-30) * 1e-12 };
    let mut map: HashMap<[i64; 3], u32> = HashMap::new();
    let mut remap = vec![0u32; points.len()];
    let mut keep: Vec<usize> = Vec::new();
    for i in 0..points.len() {
        let p = points.get(i);
        let key = [(p[0] / q).round() as i64, (p[1] / q).round() as i64, (p[2] / q).round() as i64];
        let id = *map.entry(key).or_insert_with(|| {
            keep.push(i);
            (keep.len() - 1) as u32
        });
        remap[i] = id;
    }
    let new_points = points.gather(&keep);
    let new_pd = pd.gather(&keep);
    let fix = |c: &Cells| -> Cells {
        let mut out = Cells::with_capacity(c.len(), c.connectivity.len());
        for i in 0..c.len() {
            let mut ids: Vec<u32> = c.cell(i).iter().map(|&p| remap[p as usize]).collect();
            // Remove consecutive duplicates created by merging.
            ids.dedup();
            if ids.len() > 1 && ids.first() == ids.last() && c.types[i].dimension() == 2 {
                ids.pop();
            }
            out.push(c.types[i], &ids);
        }
        out
    };
    let mut out = ds.clone();
    match &mut out {
        DataSet::UnstructuredGrid(u) => {
            u.cells = fix(&u.cells);
            u.points = new_points;
            u.point_data = new_pd;
        }
        DataSet::PolyData(p) => {
            p.verts = fix(&p.verts);
            p.lines = fix(&p.lines);
            p.polys = fix(&p.polys);
            p.strips = fix(&p.strips);
            p.points = new_points;
            p.point_data = new_pd;
        }
        _ => unreachable!(),
    }
    Ok(out)
}

/// Label connected regions (cells sharing points). `extract`: "all", "largest" or a region id.
pub fn connectivity(ds: &DataSet, extract: &str) -> Result<DataSet> {
    let ds = as_explicit(ds).into_owned();
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("connectivity: unsupported dataset"))?;
    let np = points.len();
    let mut parent: Vec<u32> = (0..np as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    let mut cells_pts: Vec<Vec<u32>> = Vec::new();
    let mut types = Vec::new();
    for_each_cell(&ds, |_, ty, pts, _| {
        for w in pts.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            if a != b {
                parent[a as usize] = b;
            }
        }
        cells_pts.push(pts.to_vec());
        types.push(ty);
    });
    let mut label: HashMap<u32, i64> = HashMap::new();
    let mut point_region = vec![-1i64; np];
    let mut cell_region = Vec::with_capacity(cells_pts.len());
    for c in &cells_pts {
        let r = c.first().map(|&p| find(&mut parent, p)).unwrap_or(0);
        let next = label.len() as i64;
        let id = *label.entry(r).or_insert(next);
        cell_region.push(id);
        for &p in c {
            point_region[p as usize] = id;
        }
    }
    let nreg = label.len() as i64;
    let wanted: Option<i64> = match extract {
        "all" | "" => None,
        "largest" => {
            let mut counts = vec![0usize; nreg as usize];
            for &r in &cell_region {
                counts[r as usize] += 1;
            }
            counts.iter().enumerate().max_by_key(|(_, c)| **c).map(|(i, _)| i as i64)
        }
        s => Some(s.parse().map_err(|_| anyhow!("region must be 'all', 'largest' or a number"))?),
    };
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    for (i, c) in cells_pts.iter().enumerate() {
        if wanted.map(|w| cell_region[i] == w).unwrap_or(true) {
            cells.push(types[i], c);
            parents.push(i);
        }
    }
    let mut point_data = pd.clone();
    point_data.insert(Array::new("RegionId", 1, ArrayData::I64(point_region)));
    let (points, point_data) = crate::interp::compact_points(points, &point_data, &mut cells.connectivity);
    let mut cell_data = cd.gather(&parents);
    cell_data.insert_active(AttributeKind::Scalars, Array::new("RegionId", 1, ArrayData::I64(parents.iter().map(|&i| cell_region[i]).collect())));
    let mut fd = fd.clone();
    fd.insert(Array::new("NumberOfRegions", 1, ArrayData::I64(vec![nreg])));
    Ok(DataSet::UnstructuredGrid(UnstructuredGrid { points, cells, point_data, cell_data, field_data: fd }))
}

/// Pick blocks of a multi-block dataset by name or index ("0,2" or "Block 1").
pub fn extract_block(ds: &DataSet, which: &str) -> Result<DataSet> {
    let DataSet::MultiBlock(mb) = ds else { return Ok(ds.clone()) };
    let leaves = mb.leaves();
    let wanted: Vec<&str> = which.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let mut out = MultiBlock::default();
    for (i, (name, d)) in leaves.iter().enumerate() {
        let short = name.rsplit('/').next().unwrap_or(name);
        if wanted.is_empty() || wanted.iter().any(|w| *w == i.to_string() || w.eq_ignore_ascii_case(name) || w.eq_ignore_ascii_case(short)) {
            out.blocks.push((name.clone(), Some((*d).clone())));
        }
    }
    if out.blocks.is_empty() {
        bail!("no block matches {which:?}; available: {}", leaves.iter().enumerate().map(|(i, (n, _))| format!("{i}:{n}")).collect::<Vec<_>>().join(", "));
    }
    Ok(if out.blocks.len() == 1 { out.blocks.pop().unwrap().1.unwrap() } else { DataSet::MultiBlock(out) })
}

/// Shrink every cell toward its centroid.
pub fn shrink(ds: &DataSet, factor: f64) -> Result<UnstructuredGrid> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("shrink: unsupported dataset"))?;
    let mut pts = Vec::new();
    let mut src = Vec::new();
    let mut cells = Cells::new();
    for_each_cell(&ds, |_, ty, ids, _| {
        let mut c = [0.0; 3];
        for &p in ids {
            let q = points.get(p as usize);
            for k in 0..3 {
                c[k] += q[k];
            }
        }
        let n = ids.len().max(1) as f64;
        let c = c.map(|x| x / n);
        let base = (pts.len() / 3) as u32;
        let mut local = Vec::with_capacity(ids.len());
        for (j, &p) in ids.iter().enumerate() {
            let q = points.get(p as usize);
            pts.extend_from_slice(&[c[0] + factor * (q[0] - c[0]), c[1] + factor * (q[1] - c[1]), c[2] + factor * (q[2] - c[2])]);
            src.push(p as usize);
            local.push(base + j as u32);
        }
        cells.push(ty, &local);
    });
    Ok(UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: pd.gather(&src), cell_data: cd.clone(), field_data: fd.clone() })
}

/// Mirror across the plane `axis = position`; optionally keep the original too.
pub fn reflect(ds: &DataSet, axis: usize, position: f64, copy_input: bool) -> Result<DataSet> {
    let ds = as_explicit(ds).into_owned();
    let mut m = ds.clone();
    let flip_vec = |a: &mut Array| {
        if a.ncomp == 3 {
            if let ArrayData::F64(v) = &mut a.data {
                for t in v.chunks_mut(3) {
                    t[axis] = -t[axis];
                }
            } else if let ArrayData::F32(v) = &mut a.data {
                for t in v.chunks_mut(3) {
                    t[axis] = -t[axis];
                }
            }
        }
    };
    let reverse = |c: &mut Cells| {
        for i in 0..c.len() {
            if c.types[i].dimension() == 2 {
                let (s, e) = (c.offsets[i] as usize, c.offsets[i + 1] as usize);
                c.connectivity[s..e].reverse();
            }
        }
    };
    match &mut m {
        DataSet::UnstructuredGrid(u) => {
            for i in 0..u.points.len() {
                let mut p = u.points.get(i);
                p[axis] = 2.0 * position - p[axis];
                u.points.set(i, p);
            }
            for a in u.point_data.arrays.values_mut() {
                flip_vec(a);
            }
            reverse(&mut u.cells);
        }
        DataSet::PolyData(p) => {
            for i in 0..p.points.len() {
                let mut q = p.points.get(i);
                q[axis] = 2.0 * position - q[axis];
                p.points.set(i, q);
            }
            for a in p.point_data.arrays.values_mut() {
                flip_vec(a);
            }
            reverse(&mut p.polys);
        }
        _ => bail!("reflect: unsupported dataset"),
    }
    if !copy_input {
        return Ok(m);
    }
    Ok(DataSet::MultiBlock(MultiBlock { blocks: vec![("input".into(), Some(ds)), ("reflection".into(), Some(m))], field_data: Attributes::new() }))
}

/// Scalar "Elevation" = projection of each point on the segment low->high, mapped to [lo, hi].
pub fn elevation(ds: &DataSet, low: [f64; 3], high: [f64; 3], range: (f64, f64)) -> Result<DataSet> {
    let mut out = ds.clone();
    let pts = ds.points().ok_or_else(|| anyhow!("elevation: no points"))?;
    let d = sub(high, low);
    let l2 = dot(d, d).max(1e-300);
    let v: Vec<f64> = (0..pts.len())
        .map(|i| {
            let t = (dot(sub(pts.get(i), low), d) / l2).clamp(0.0, 1.0);
            range.0 + t * (range.1 - range.0)
        })
        .collect();
    out.point_data_mut().ok_or_else(|| anyhow!("elevation: no point data"))?.insert_active(AttributeKind::Scalars, Array::scalars_f64("Elevation", v));
    Ok(out)
}

fn tet_vol(p: [[f64; 3]; 4]) -> f64 {
    dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))).abs() / 6.0
}

/// Per-cell measure: Volume (3D), Area (2D) or Length (1D).
pub fn cell_size(ds: &DataSet) -> Result<DataSet> {
    let ex = as_explicit(ds).into_owned();
    let (points, ..) = parts(&ex).ok_or_else(|| anyhow!("cell size: unsupported dataset"))?;
    let mut vol = vec![0.0; ex.n_cells()];
    let mut area = vec![0.0; ex.n_cells()];
    let mut len = vec![0.0; ex.n_cells()];
    for_each_cell(&ex, |cid, ty, ids, faces| {
        decompose(ty, ids, faces, |s| match s {
            Simplex::Tet(t) => vol[cid] += tet_vol(t.map(|i| points.get(i as usize))),
            Simplex::Tri(t) => {
                let p = t.map(|i| points.get(i as usize));
                area[cid] += 0.5 * norm(cross(sub(p[1], p[0]), sub(p[2], p[0])));
            }
            Simplex::Line([a, b]) => len[cid] += norm(sub(points.get(a as usize), points.get(b as usize))),
            Simplex::Vert(_) => {}
        })
    });
    let mut out = ex.clone();
    let cd = out.cell_data_mut().unwrap();
    cd.insert(Array::scalars_f64("Length", len));
    cd.insert(Array::scalars_f64("Area", area));
    cd.insert_active(AttributeKind::Scalars, Array::scalars_f64("Volume", vol));
    Ok(out)
}

/// Mesh quality per cell: edge ratio (longest/shortest edge, 1 = ideal) and, for simplices,
/// the radius ratio-like "aspect" (normalized so 1 = regular).
pub fn mesh_quality(ds: &DataSet) -> Result<DataSet> {
    let ex = as_explicit(ds).into_owned();
    let (points, ..) = parts(&ex).ok_or_else(|| anyhow!("mesh quality: unsupported dataset"))?;
    let mut edge_ratio = Vec::with_capacity(ex.n_cells());
    let mut min_angle = Vec::with_capacity(ex.n_cells());
    for_each_cell(&ex, |_, ty, ids, _| {
        let edges = ty.edges();
        let pairs: Vec<(u32, u32)> = if edges.is_empty() {
            (0..ids.len()).map(|k| (ids[k], ids[(k + 1) % ids.len()])).collect()
        } else {
            edges.iter().map(|e| (ids[e[0]], ids[e[1]])).collect()
        };
        let ls: Vec<f64> = pairs.iter().map(|(a, b)| norm(sub(points.get(*a as usize), points.get(*b as usize)))).collect();
        let (mn, mx) = ls.iter().fold((f64::INFINITY, 0.0f64), |(a, b), &l| (a.min(l), b.max(l)));
        edge_ratio.push(if mn > 0.0 { mx / mn } else { f64::INFINITY });
        // Minimum corner angle over the cell's faces (or the polygon itself), in degrees.
        let faces: Vec<Vec<u32>> =
            if ty.dimension() == 3 { ty.faces().iter().map(|f| f.iter().map(|&k| ids[k]).collect()).collect() } else { vec![ids.to_vec()] };
        let mut ma = 180.0f64;
        for f in faces.iter().filter(|f| f.len() >= 3) {
            for k in 0..f.len() {
                let p = points.get(f[k] as usize);
                let a = unit(sub(points.get(f[(k + f.len() - 1) % f.len()] as usize), p));
                let b = unit(sub(points.get(f[(k + 1) % f.len()] as usize), p));
                ma = ma.min(dot(a, b).clamp(-1.0, 1.0).acos().to_degrees());
            }
        }
        min_angle.push(if ty.dimension() >= 2 { ma } else { f64::NAN });
    });
    let mut out = ex.clone();
    let cd = out.cell_data_mut().unwrap();
    cd.insert(Array::scalars_f64("MinAngle", min_angle));
    cd.insert_active(AttributeKind::Scalars, Array::scalars_f64("EdgeRatio", edge_ratio));
    Ok(out)
}

/// Laplacian smoothing of a surface.
pub fn smooth(ds: &DataSet, iterations: usize, relaxation: f64) -> Result<PolyData> {
    let mut pd = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => crate::surface::extract_surface(other)?,
    };
    let n = pd.points.len();
    let mut nbr: Vec<Vec<u32>> = vec![Vec::new(); n];
    for c in [&pd.polys, &pd.lines] {
        for i in 0..c.len() {
            let cell = c.cell(i);
            for k in 0..cell.len() {
                let (a, b) = (cell[k], cell[(k + 1) % cell.len()]);
                if a != b {
                    nbr[a as usize].push(b);
                    nbr[b as usize].push(a);
                }
            }
        }
    }
    for v in &mut nbr {
        v.sort_unstable();
        v.dedup();
    }
    let mut p: Vec<[f64; 3]> = (0..n).map(|i| pd.points.get(i)).collect();
    for _ in 0..iterations {
        let prev = p.clone();
        for i in 0..n {
            if nbr[i].is_empty() {
                continue;
            }
            let mut c = [0.0; 3];
            for &j in &nbr[i] {
                for k in 0..3 {
                    c[k] += prev[j as usize][k];
                }
            }
            let m = nbr[i].len() as f64;
            for k in 0..3 {
                p[i][k] = prev[i][k] + relaxation * (c[k] / m - prev[i][k]);
            }
        }
    }
    pd.points = Points::new_f64(p.into_iter().flatten().collect());
    Ok(pd)
}

/// Split polygons/strips into triangles (surfaces) — 3D cells are left alone.
pub fn triangulate(ds: &DataSet) -> Result<PolyData> {
    let pd = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => crate::surface::extract_surface(other)?,
    };
    let mut polys = Cells::new();
    let mut parents = Vec::new();
    let base = pd.verts.len() + pd.lines.len();
    for (ci, c) in [&pd.polys, &pd.strips].into_iter().enumerate() {
        let off = if ci == 0 { base } else { base + pd.polys.len() };
        for i in 0..c.len() {
            decompose(c.types[i], c.cell(i), None, |s| {
                if let Simplex::Tri(t) = s {
                    polys.push(CellType::Triangle, &t);
                    parents.push(off + i);
                }
            });
        }
    }
    let mut out = PolyData { points: pd.points.clone(), polys, point_data: pd.point_data.clone(), field_data: pd.field_data.clone(), ..Default::default() };
    if !pd.cell_data.is_empty() {
        out.cell_data = pd.cell_data.gather(&parents);
    }
    Ok(out)
}

/// Decompose every 3D cell into tetrahedra (2D cells into triangles).
pub fn tetrahedralize(ds: &DataSet) -> Result<UnstructuredGrid> {
    let ex = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ex).ok_or_else(|| anyhow!("tetrahedralize: unsupported dataset"))?;
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    for_each_cell(&ex, |cid, ty, ids, faces| {
        decompose(ty, ids, faces, |s| {
            match s {
                Simplex::Tet(t) => cells.push(CellType::Tetra, &t),
                Simplex::Tri(t) => cells.push(CellType::Triangle, &t),
                Simplex::Line(l) => cells.push(CellType::Line, &l),
                Simplex::Vert(v) => cells.push(CellType::Vertex, &[v]),
            }
            parents.push(cid);
        })
    });
    Ok(UnstructuredGrid { points: points.clone(), cells, point_data: pd.clone(), cell_data: cd.gather(&parents), field_data: fd.clone() })
}

/// Keep every `stride`-th point (or `max` evenly spread points) as vertices.
pub fn mask_points(ds: &DataSet, stride: usize, max_points: usize) -> Result<PolyData> {
    let pts = ds.points().ok_or_else(|| anyhow!("mask points: no points"))?;
    let n = pts.len();
    let step = stride.max(1).max(n.div_ceil(max_points.max(1)));
    let idx: Vec<usize> = (0..n).step_by(step).collect();
    let mut out = PolyData::from_points(pts.gather(&idx));
    if let Some(pd) = ds.point_data() {
        out.point_data = pd.gather(&idx);
    }
    Ok(out)
}

/// Copy a single component of a vector array into a new scalar array.
pub fn extract_component(ds: &DataSet, name: &str, comp: usize, cells: bool) -> Result<DataSet> {
    let mut out = ds.clone();
    let attrs = if cells { out.cell_data_mut() } else { out.point_data_mut() }.ok_or_else(|| anyhow!("no attributes"))?;
    let a = attrs.get(name).ok_or_else(|| anyhow!("no array {name:?}"))?;
    if comp >= a.ncomp {
        bail!("{name} has only {} components", a.ncomp);
    }
    let label = a
        .component_names
        .as_ref()
        .and_then(|c| c.get(comp).cloned())
        .unwrap_or_else(|| ["X", "Y", "Z"].get(comp).map(|s| s.to_string()).unwrap_or(comp.to_string()));
    let v: Vec<f64> = (0..a.len()).map(|i| a.get(i, comp)).collect();
    attrs.insert_active(AttributeKind::Scalars, Array::scalars_f64(format!("{name}_{label}"), v));
    Ok(out)
}

pub fn rename_array(ds: &DataSet, from: &str, to: &str) -> Result<DataSet> {
    let mut out = ds.clone();
    let mut done = false;
    let mut fix = |a: Option<&mut Attributes>| {
        if let Some(a) = a {
            if let Some(mut arr) = a.remove(from) {
                arr.name = to.to_string();
                a.insert(arr);
                done = true;
            }
        }
    };
    fix(out.point_data_mut());
    fix(out.cell_data_mut());
    if !done {
        bail!("no array named {from:?}");
    }
    Ok(out)
}

/// Turn polylines into tubes (`sides` facets, radius optionally scaled by a scalar).
pub fn tube(ds: &DataSet, radius: f64, sides: usize, scale_by: Option<&str>) -> Result<PolyData> {
    let pd = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => match other.to_unstructured() {
            Some(u) => {
                let mut lines = Cells::new();
                for (ty, c) in u.cells.iter() {
                    if ty.dimension() == 1 {
                        lines.push(ty, c);
                    }
                }
                PolyData { points: u.points.clone(), lines, point_data: u.point_data.clone(), ..Default::default() }
            }
            None => bail!("tube: needs lines"),
        },
    };
    let sides = sides.max(3);
    let scale = scale_by.and_then(|n| pd.point_data.get(n)).cloned();
    let (smin, smax) = scale.as_ref().map(|a| a.range(None)).unwrap_or((1.0, 1.0));
    let mut pts: Vec<f64> = Vec::new();
    let mut src: Vec<usize> = Vec::new();
    let mut polys = Cells::new();
    for li in 0..pd.lines.len() {
        let line: Vec<u32> = pd.lines.cell(li).to_vec();
        if line.len() < 2 {
            continue;
        }
        let p: Vec<[f64; 3]> = line.iter().map(|&i| pd.points.get(i as usize)).collect();
        // Parallel-transport frame.
        let t0 = unit(sub(p[1], p[0]));
        let mut nrm = unit(cross(t0, if t0[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        let base = (pts.len() / 3) as u32;
        for k in 0..p.len() {
            let t = if k == 0 {
                unit(sub(p[1], p[0]))
            } else if k == p.len() - 1 {
                unit(sub(p[k], p[k - 1]))
            } else {
                unit(sub(p[k + 1], p[k - 1]))
            };
            nrm = unit(sub(nrm, t.map(|x| x * dot(nrm, t))));
            let bin = cross(t, nrm);
            let r = match &scale {
                Some(a) => radius * (0.25 + 0.75 * ((a.magnitude(line[k] as usize) - smin) / (smax - smin).max(1e-300))),
                None => radius,
            };
            for s in 0..sides {
                let ang = s as f64 / sides as f64 * std::f64::consts::TAU;
                let (c, sn) = (ang.cos(), ang.sin());
                pts.extend_from_slice(&[
                    p[k][0] + r * (c * nrm[0] + sn * bin[0]),
                    p[k][1] + r * (c * nrm[1] + sn * bin[1]),
                    p[k][2] + r * (c * nrm[2] + sn * bin[2]),
                ]);
                src.push(line[k] as usize);
            }
        }
        for k in 0..p.len() - 1 {
            for s in 0..sides as u32 {
                let a = base + k as u32 * sides as u32 + s;
                let b = base + k as u32 * sides as u32 + (s + 1) % sides as u32;
                polys.push(CellType::Quad, &[a, b, b + sides as u32, a + sides as u32]);
            }
        }
    }
    Ok(PolyData { points: Points::new_f64(pts), polys, point_data: pd.point_data.gather(&src), ..Default::default() })
}
