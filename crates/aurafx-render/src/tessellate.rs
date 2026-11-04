//! Convert datasets to GPU-friendly vertex streams according to display properties.

use aurafx_data::*;
use aurafx_filters::Association;
use aurafx_pipeline::{DisplayProps, Representation};
use bytemuck::{Pod, Zeroable};
use rayon::prelude::*;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub scalar: f32,
    pub normal: [f32; 3],
    pub _pad: f32,
}

/// CPU-side renderable geometry.
#[derive(Debug, Default, Clone)]
pub struct MeshData {
    pub tris: Vec<Vertex>,
    pub lines: Vec<Vertex>,
    pub points: Vec<Vertex>,
    /// Range of the scalar used for coloring, if any.
    pub scalar_range: Option<(f64, f64)>,
    pub has_scalars: bool,
    pub bounds: Bounds,
    /// Local origin subtracted from all positions (for precision with large coordinates).
    pub origin: [f64; 3],
}

struct Leaf {
    pd: PolyData,
}

/// Collect renderable leaves: everything is turned into polydata (surfaces) first.
fn leaves(ds: &DataSet, repr: Representation) -> Vec<Leaf> {
    match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().flat_map(|(_, d)| leaves(d, repr)).collect(),
        DataSet::Table(_) => vec![],
        DataSet::PolyData(pd) => vec![Leaf { pd: pd.clone() }],
        other => match aurafx_filters::surface::extract_surface(other) {
            Ok(pd) => vec![Leaf { pd }],
            Err(e) => {
                tracing::warn!("surface extraction failed: {e:#}");
                vec![]
            }
        },
    }
}

/// Per-point scalar values (component or magnitude) for coloring, or None.
fn point_scalars(attrs: &Attributes, name: &str, comp: Option<usize>) -> Option<Vec<f32>> {
    attrs.get(name).filter(|a| a.scalar_type() != ScalarType::String).map(|a| a.component_f32(comp.filter(|&c| c < a.ncomp)))
}

fn compute_point_normals(pd: &PolyData) -> Vec<[f32; 3]> {
    let n = pd.points.len();
    let mut acc = vec![[0.0f64; 3]; n];
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        if c.len() < 3 {
            continue;
        }
        // Newell's method for polygon normal (area weighted).
        let mut nx = 0.0;
        let mut ny = 0.0;
        let mut nz = 0.0;
        for k in 0..c.len() {
            let p = pd.points.get(c[k] as usize);
            let q = pd.points.get(c[(k + 1) % c.len()] as usize);
            nx += (p[1] - q[1]) * (p[2] + q[2]);
            ny += (p[2] - q[2]) * (p[0] + q[0]);
            nz += (p[0] - q[0]) * (p[1] + q[1]);
        }
        for &pi in c {
            let a = &mut acc[pi as usize];
            a[0] += nx;
            a[1] += ny;
            a[2] += nz;
        }
    }
    acc.into_par_iter()
        .map(|a| {
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            if l > 0.0 { [(a[0] / l) as f32, (a[1] / l) as f32, (a[2] / l) as f32] } else { [0.0, 0.0, 1.0] }
        })
        .collect()
}

/// Triangulate polygon cells and strips into unindexed triangles.
fn emit_triangles(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, cscal: Option<&[f32]>, cell_base: usize, smooth: bool, out: &mut Vec<Vertex>) {
    let normals = if smooth { Some(compute_point_normals(pd)) } else { None };
    let pt = |i: u32| {
        let p = pd.points.get(i as usize);
        [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
    };
    let push_tri = |ids: [u32; 3], cell: usize, out: &mut Vec<Vertex>| {
        let p = [pt(ids[0]), pt(ids[1]), pt(ids[2])];
        let flat = {
            let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 0.0, 1.0] }
        };
        for k in 0..3 {
            let s = match (cscal, pscal) {
                (Some(c), _) => c.get(cell).copied().unwrap_or(f32::NAN),
                (None, Some(ps)) => ps.get(ids[k] as usize).copied().unwrap_or(f32::NAN),
                _ => 0.0,
            };
            let normal = match &normals {
                Some(nn) => nn[ids[k] as usize],
                None => flat,
            };
            out.push(Vertex { pos: p[k], scalar: s, normal, _pad: 0.0 });
        }
    };
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        let cell = cell_base + i;
        if c.len() == 3 {
            push_tri([c[0], c[1], c[2]], cell, out);
        } else if c.len() == 4 {
            push_tri([c[0], c[1], c[2]], cell, out);
            push_tri([c[0], c[2], c[3]], cell, out);
        } else {
            for k in 1..c.len().saturating_sub(1) {
                push_tri([c[0], c[k], c[k + 1]], cell, out);
            }
        }
    }
    let strip_base = cell_base + pd.polys.len();
    for i in 0..pd.strips.len() {
        let c = pd.strips.cell(i);
        for k in 0..c.len().saturating_sub(2) {
            let t = if k % 2 == 0 { [c[k], c[k + 1], c[k + 2]] } else { [c[k + 1], c[k], c[k + 2]] };
            push_tri(t, strip_base + i, out);
        }
    }
}

fn emit_lines(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, cscal: Option<&[f32]>, cell_base: usize, out: &mut Vec<Vertex>) {
    let pt = |i: u32| {
        let p = pd.points.get(i as usize);
        [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
    };
    for i in 0..pd.lines.len() {
        let c = pd.lines.cell(i);
        for w in c.windows(2) {
            for &id in w {
                let s = match (cscal, pscal) {
                    (Some(cs), _) => cs.get(cell_base + i).copied().unwrap_or(f32::NAN),
                    (None, Some(ps)) => ps.get(id as usize).copied().unwrap_or(f32::NAN),
                    _ => 0.0,
                };
                out.push(Vertex { pos: pt(id), scalar: s, normal: [0.0, 0.0, 1.0], _pad: 0.0 });
            }
        }
    }
}

/// Edges of polygons (unique), as line vertices.
fn emit_polygon_edges(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, out: &mut Vec<Vertex>) {
    use std::collections::HashSet;
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let pt = |i: u32| {
        let p = pd.points.get(i as usize);
        [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
    };
    for cells in [&pd.polys, &pd.strips] {
        for i in 0..cells.len() {
            let c = cells.cell(i);
            let n = c.len();
            for k in 0..n {
                let (a, b) = (c[k], c[(k + 1) % n]);
                let key = if a < b { (a, b) } else { (b, a) };
                if seen.insert(key) {
                    for id in [a, b] {
                        let s = pscal.and_then(|p| p.get(id as usize).copied()).unwrap_or(0.0);
                        out.push(Vertex { pos: pt(id), scalar: s, normal: [0.0, 0.0, 1.0], _pad: 0.0 });
                    }
                }
            }
        }
    }
}

fn emit_points(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, cscal: Option<&[f32]>, out: &mut Vec<Vertex>) {
    let n = pd.points.len();
    // If there are vertex cells, cell scalars map through them; otherwise every point.
    if cscal.is_some() && pd.verts.len() > 0 {
        for i in 0..pd.verts.len() {
            for &id in pd.verts.cell(i) {
                let p = pd.points.get(id as usize);
                out.push(Vertex { pos: [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32], scalar: cscal.unwrap()[i], normal: [0.0, 0.0, 1.0], _pad: 0.0 });
            }
        }
        return;
    }
    out.reserve(n);
    for i in 0..n {
        let p = pd.points.get(i);
        let s = pscal.and_then(|ps| ps.get(i).copied()).unwrap_or(0.0);
        out.push(Vertex { pos: [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32], scalar: s, normal: [0.0, 0.0, 1.0], _pad: 0.0 });
    }
}

/// Build renderable geometry for a dataset.
pub fn tessellate(ds: &DataSet, display: &DisplayProps) -> MeshData {
    let bounds = ds.bounds();
    let origin = if bounds.is_empty() { [0.0; 3] } else { bounds.center() };
    let mut mesh = MeshData { bounds, origin, ..Default::default() };
    if display.representation == Representation::Outline {
        let pd = aurafx_filters::outline::outline(ds);
        emit_lines(&pd, origin, None, None, 0, &mut mesh.lines);
        return mesh;
    }
    // Global scalar range for consistent coloring across leaves.
    if let Some(cb) = &display.color_by {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut visit = |d: &DataSet| {
            let attrs = match cb.assoc {
                Association::Points => d.point_data(),
                Association::Cells => d.cell_data(),
            };
            if let Some(a) = attrs.and_then(|x| x.get(&cb.name)) {
                let (l, h) = a.range(cb.component.filter(|&c| c < a.ncomp));
                lo = lo.min(l);
                hi = hi.max(h);
            }
        };
        match ds {
            DataSet::MultiBlock(mb) => mb.leaves().into_iter().for_each(|(_, d)| visit(d)),
            d => visit(d),
        }
        if lo <= hi {
            mesh.scalar_range = Some((lo, hi));
            mesh.has_scalars = true;
        }
    }
    if let Some(r) = display.range {
        if mesh.has_scalars {
            mesh.scalar_range = Some(r);
        }
    }

    for leaf in leaves(ds, display.representation) {
        let pd = &leaf.pd;
        let (pscal, cscal) = match &display.color_by {
            Some(cb) if mesh.has_scalars => match cb.assoc {
                Association::Points => (point_scalars(&pd.point_data, &cb.name, cb.component), None),
                Association::Cells => (None, point_scalars(&pd.cell_data, &cb.name, cb.component)),
            },
            _ => (None, None),
        };
        let (ps, cs) = (pscal.as_deref(), cscal.as_deref());
        let poly_cell_base = pd.verts.len() + pd.lines.len();
        let smooth = display.lighting && pd.point_data.active(AttributeKind::Normals).is_some() || pd.polys.len() > 0 && smooth_default(pd);
        match display.representation {
            Representation::Surface | Representation::SurfaceWithEdges => {
                emit_triangles(pd, origin, ps, cs, poly_cell_base, smooth, &mut mesh.tris);
                emit_lines(pd, origin, ps, cs, pd.verts.len(), &mut mesh.lines);
                if display.representation == Representation::SurfaceWithEdges {
                    emit_polygon_edges(pd, origin, None, &mut mesh.lines);
                }
                if pd.polys.is_empty() && pd.strips.is_empty() && pd.lines.is_empty() {
                    emit_points(pd, origin, ps, cs, &mut mesh.points);
                }
            }
            Representation::Wireframe => {
                emit_polygon_edges(pd, origin, ps, &mut mesh.lines);
                emit_lines(pd, origin, ps, cs, pd.verts.len(), &mut mesh.lines);
                if pd.polys.is_empty() && pd.strips.is_empty() && pd.lines.is_empty() {
                    emit_points(pd, origin, ps, cs, &mut mesh.points);
                }
            }
            Representation::Points => emit_points(pd, origin, ps, cs, &mut mesh.points),
            Representation::Outline => unreachable!(),
        }
    }
    mesh
}

/// Heuristic: smooth-shade surfaces that look like curved geometry (many small polygons).
fn smooth_default(pd: &PolyData) -> bool {
    pd.polys.len() > 50
}
