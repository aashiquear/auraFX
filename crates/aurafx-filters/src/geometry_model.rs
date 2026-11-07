//! The Geometry source (bodies from geometry commands, drawn as surfaces through the contour
//! of their distance fields) and the Signed Distance Field filter.

use crate::Association;
use anyhow::{bail, Result};
use aurafx_data::*;
use aurafx_mesh::{Model, Shape};
use rayon::prelude::*;

pub const OUT_SURFACE: &str = "Surface";
pub const OUT_DISTANCE: &str = "Distance field";
pub const OUT_MESH: &str = "Hex mesh";
pub const OUT_TET: &str = "Tet mesh";

/// An image grid of points covering `lo`..`hi` with `res` cells along the longest side and
/// two cells of margin, and its spacing.
fn grid(lo: [f64; 3], hi: [f64; 3], res: usize) -> ImageData {
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max).max(1e-12);
    let h = size / res.max(2) as f64;
    let n: [i64; 3] = std::array::from_fn(|k| ((hi[k] - lo[k]) / h).ceil() as i64 + 4);
    ImageData { extent: [0, n[0], 0, n[1], 0, n[2]], origin: std::array::from_fn(|k| lo[k] - 2.0 * h), spacing: [h; 3], ..Default::default() }
}

fn grid_points(img: &ImageData) -> Vec<[f64; 3]> {
    let e = img.extent;
    let (nx, ny, nz) = ((e[1] - e[0] + 1) as usize, (e[3] - e[2] + 1) as usize, (e[5] - e[4] + 1) as usize);
    (0..nx * ny * nz)
        .map(|i| {
            let (x, y, z) = (i % nx, (i / nx) % ny, i / (nx * ny));
            [img.origin[0] + x as f64 * img.spacing[0], img.origin[1] + y as f64 * img.spacing[1], img.origin[2] + z as f64 * img.spacing[2]]
        })
        .collect()
}

fn gradient(shape: &Shape, p: [f64; 3], e: f64) -> [f64; 3] {
    std::array::from_fn(|c| {
        let (mut q, mut r) = (p, p);
        q[c] += e;
        r[c] -= e;
        (shape.eval(q).0 - shape.eval(r).0) / (2.0 * e)
    })
}

/// The surface of one solid: the zero contour of its sampled distance, with the points moved
/// onto the surface, triangles facing outward. Returns points, triangles and face ids.
fn surface(shape: &Shape, res: usize, project: bool) -> Result<(Vec<[f64; 3]>, Vec<[u32; 3]>, Vec<i32>)> {
    let (lo, hi) = shape.bounds();
    if (0..3).any(|k| !(hi[k] > lo[k]) || !lo[k].is_finite() || !hi[k].is_finite()) {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    let mut img = grid(lo, hi, res);
    let h = img.spacing[0];
    // Off round coordinates, and no exact zeros: faces lying on grid points make the contour
    // degenerate (slivers and holes).
    for k in 0..3 {
        img.origin[k] -= h * [0.0137, 0.0219, 0.0071][k];
    }
    let d: Vec<f64> = grid_points(&img).par_iter().map(|p| shape.eval(*p).0).map(|d| if d == 0.0 { 1e-9 * h } else { d }).collect();
    img.point_data.insert(Array::scalars_f64("Distance", d));
    let pd = crate::contour::contour(&DataSet::ImageData(img), Association::Points, "Distance", None, &[0.0])?;
    let mut pts: Vec<[f64; 3]> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let before = pts.clone();
    if project {
        pts.par_iter_mut().for_each(|p| {
            // Newton steps along the gradient, kept only while they bring the point closer to
            // the surface and never more than 1/5 of a cell in all: near edges the gradient
            // jumps between faces and longer moves fold the triangles.
            let start = *p;
            let g0 = gradient(shape, start, h * 1e-3);
            for _ in 0..4 {
                let d = shape.eval(*p).0;
                if d.abs() < 1e-12 * h {
                    break;
                }
                let g = gradient(shape, *p, h * 1e-3);
                let gg = g[0] * g[0] + g[1] * g[1] + g[2] * g[2];
                if gg < 1e-12 {
                    break;
                }
                let q: [f64; 3] = std::array::from_fn(|k| p[k] - d * g[k] / gg);
                let moved = ((q[0] - start[0]).powi(2) + (q[1] - start[1]).powi(2) + (q[2] - start[2]).powi(2)).sqrt();
                if moved > 0.2 * h || shape.eval(q).0.abs() >= d.abs() {
                    break;
                }
                *p = q;
            }
            // Near an edge the surface turns: leave such points where the contour put them.
            if *p != start {
                let g1 = gradient(shape, *p, h * 1e-3);
                let dotg = g0[0] * g1[0] + g0[1] * g1[1] + g0[2] * g1[2];
                let n = ((g0[0] * g0[0] + g0[1] * g0[1] + g0[2] * g0[2]) * (g1[0] * g1[0] + g1[1] * g1[1] + g1[2] * g1[2])).sqrt();
                if n == 0.0 || dotg < 0.9 * n {
                    *p = start;
                }
            }
        });
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for (_, c) in pd.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            tris.push([c[0], c[k], c[k + 1]]);
        }
    }
    // Snapping must not turn a triangle over (near edges it can fold the surface, and folded
    // triangles show as holes): put the corners of such triangles back, until none turns.
    if project {
        for _ in 0..8 {
            let mut undo = Vec::new();
            for t in &tris {
                let (n0, n1) = (normal(&before, t), normal(&pts, t));
                if n0[0] * n1[0] + n0[1] * n1[1] + n0[2] * n1[2] <= 0.0 {
                    undo.extend(t.iter().copied());
                }
            }
            if undo.is_empty() {
                break;
            }
            for i in undo {
                pts[i as usize] = before[i as usize];
            }
        }
    }
    // Triangles that snapping collapsed have no normal (they would draw as specks).
    tris.retain(|t| tri_area(&pts, t) > 1e-10 * h * h);
    orient(&pts, &mut tris);
    let faces: Vec<i32> = tris
        .par_iter()
        .map(|t| {
            let c: [f64; 3] = std::array::from_fn(|k| (pts[t[0] as usize][k] + pts[t[1] as usize][k] + pts[t[2] as usize][k]) / 3.0);
            shape.eval(c).1 as i32
        })
        .collect();
    Ok((pts, tris, faces))
}

/// Orient the triangles of each connected piece alike (neighbours traverse their shared edge
/// in opposite directions), then turn each piece outward (positive enclosed volume).
fn orient(pts: &[[f64; 3]], tris: &mut [[u32; 3]]) {
    use std::collections::HashMap;
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let has_dir = |t: &[u32; 3], a: u32, b: u32| (0..3).any(|k| t[k] == a && t[(k + 1) % 3] == b);
    let mut seen = vec![false; tris.len()];
    for seed in 0..tris.len() {
        if seen[seed] {
            continue;
        }
        seen[seed] = true;
        let mut piece = vec![seed];
        let mut k = 0;
        while k < piece.len() {
            let i = piece[k];
            k += 1;
            let t = tris[i];
            for e in 0..3 {
                let (a, b) = (t[e], t[(e + 1) % 3]);
                for &j in &by_edge[&(a.min(b), a.max(b))] {
                    if seen[j] {
                        continue;
                    }
                    // Consistent when the neighbour runs the edge b -> a.
                    if has_dir(&tris[j], a, b) {
                        tris[j].swap(1, 2);
                    }
                    seen[j] = true;
                    piece.push(j);
                }
            }
        }
        let vol: f64 = piece.iter().map(|i| signed_volume(pts, &tris[*i])).sum();
        if vol < 0.0 {
            for i in piece {
                tris[i].swap(1, 2);
            }
        }
    }
}

fn signed_volume(p: &[[f64; 3]], t: &[u32; 3]) -> f64 {
    let [a, b, c] = t.map(|i| p[i as usize]);
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
}

fn normal(p: &[[f64; 3]], t: &[u32; 3]) -> [f64; 3] {
    let [a, b, c] = t.map(|i| p[i as usize]);
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

fn tri_area(p: &[[f64; 3]], t: &[u32; 3]) -> f64 {
    let [a, b, c] = t.map(|i| p[i as usize]);
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

/// Geometry: the bodies the commands build, as one surface (cell arrays BodyId and FaceId;
/// field arrays FaceNames, BodyIds, BodyNames, Volume and SurfaceArea per body) or as their
/// distance field on a grid (point arrays Distance, BodyId and FaceId).
pub fn geometry(commands: &str, resolution: usize, output: &str, project: bool) -> Result<DataSet> {
    if output == OUT_MESH {
        return crate::mesh_gen::geometry_mesh(commands);
    }
    if output == OUT_TET {
        let model = aurafx_mesh::Model::from_commands(commands, None)?;
        return Ok(aurafx_mesh::bodymesh::tet_mesh_model(&model).map_err(|e| anyhow::anyhow!(e))?.to_dataset());
    }
    let model = Model::from_commands(commands, None)?;
    if model.bodies.is_empty() {
        bail!("the commands make no body (try: brick x 2 y 1 z 1)");
    }
    let mut field = Attributes::new();
    field.insert(Array::new("FaceNames", 1, ArrayData::String(model.faces.clone())));
    field.insert(Array::new("BodyIds", 1, ArrayData::I32(model.bodies.keys().map(|k| *k as i32).collect())));
    field.insert(Array::new("BodyNames", 1, ArrayData::String(model.bodies.values().map(|b| b.name.clone()).collect())));
    if output == OUT_DISTANCE {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for b in model.bodies.values() {
            let (a, c) = b.shape.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
        let mut img = grid(lo, hi, resolution);
        let vals: Vec<(f64, i32, i32)> = grid_points(&img)
            .par_iter()
            .map(|p| {
                let mut best = (f64::INFINITY, 0, 0);
                for b in model.bodies.values() {
                    let (d, f) = b.shape.eval(*p);
                    if d < best.0 {
                        best = (d, b.id as i32, f as i32);
                    }
                }
                best
            })
            .collect();
        img.point_data.insert(Array::scalars_f64("Distance", vals.iter().map(|v| v.0).collect()));
        img.point_data.insert(Array::new("BodyId", 1, ArrayData::I32(vals.iter().map(|v| v.1).collect())));
        img.point_data.insert(Array::new("FaceId", 1, ArrayData::I32(vals.iter().map(|v| v.2).collect())));
        img.field_data = field;
        return Ok(DataSet::ImageData(img));
    }
    let mut pts: Vec<f64> = Vec::new();
    let mut polys = Cells::new();
    let (mut body_ids, mut face_ids) = (Vec::new(), Vec::new());
    let (mut vols, mut areas) = (Vec::new(), Vec::new());
    for b in model.bodies.values() {
        let (p, t, f) = surface(&b.shape, resolution, project)?;
        let base = (pts.len() / 3) as u32;
        pts.extend(p.iter().flatten());
        for tri in &t {
            polys.push(CellType::Triangle, &tri.map(|i| i + base));
        }
        body_ids.extend(std::iter::repeat_n(b.id as i32, t.len()));
        face_ids.extend(f);
        vols.push(t.iter().map(|x| signed_volume(&p, x)).sum::<f64>());
        areas.push(t.iter().map(|x| tri_area(&p, x)).sum::<f64>());
    }
    field.insert(Array::scalars_f64("Volume", vols));
    field.insert(Array::scalars_f64("SurfaceArea", areas));
    let mut pd = PolyData { points: Points::new_f64(pts), polys, field_data: field, ..Default::default() };
    pd.cell_data.insert(Array::new("BodyId", 1, ArrayData::I32(body_ids)));
    pd.cell_data.insert(Array::new("FaceId", 1, ArrayData::I32(face_ids)));
    Ok(DataSet::PolyData(pd))
}

/// Signed Distance Field: the distance to a closed surface (negative inside) on an image grid
/// around it, `resolution` cells along its longest side plus `margin` (a fraction of that size)
/// on every side.
pub fn signed_distance(ds: &DataSet, resolution: usize, margin: f64) -> Result<DataSet> {
    let mesh = aurafx_mesh::model::surface_mesh(ds)?;
    let size = (0..3).map(|k| mesh.hi[k] - mesh.lo[k]).fold(0.0, f64::max);
    let m = size * margin.max(0.0);
    let lo = mesh.lo.map(|v| v - m);
    let hi = mesh.hi.map(|v| v + m);
    let mut img = grid(lo, hi, resolution);
    let d: Vec<f64> = grid_points(&img).par_iter().map(|p| mesh.distance(*p)).collect();
    img.point_data.insert(Array::scalars_f64("Distance", d));
    Ok(DataSet::ImageData(img))
}
