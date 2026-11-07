//! Hexahedral meshes of geometry bodies (the `mesh body` command): bricks as structured boxes
//! and cylinders as swept O-grid disks, each moved, turned and scaled like its body. Every
//! boundary face is tagged with the body face it lies on, giving a side set per named face.

use crate::hex::{HexMesh, P3, HEX8_SIDES};
use crate::model::Model;
use crate::shape::Shape;
use crate::structured::{box_mesh, disk_quads, sweep_quads, Division};

/// World = m * local + t.
#[derive(Clone, Copy)]
struct Affine {
    m: [[f64; 3]; 3],
    t: P3,
}

impl Affine {
    fn identity() -> Affine {
        Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] }
    }
    fn apply(&self, p: P3) -> P3 {
        std::array::from_fn(|i| self.m[i][0] * p[0] + self.m[i][1] * p[1] + self.m[i][2] * p[2] + self.t[i])
    }
    fn inverse(&self) -> Affine {
        let a = &self.m;
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        let c = |i: usize, j: usize| -> f64 {
            let (r0, r1) = ((i + 1) % 3, (i + 2) % 3);
            let (c0, c1) = ((j + 1) % 3, (j + 2) % 3);
            a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]
        };
        let m: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| c(j, i) / det));
        let t = std::array::from_fn(|i| -(m[i][0] * self.t[0] + m[i][1] * self.t[1] + m[i][2] * self.t[2]));
        Affine { m, t }
    }
    /// self after `inner` (world = self(inner(p))).
    fn then(&self, inner: &Affine) -> Affine {
        let m = std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| self.m[i][k] * inner.m[k][j]).sum()));
        Affine { m, t: self.apply(inner.t) }
    }
}

/// The primitive under a chain of transforms, with the transform to world coordinates.
fn base(shape: &Shape) -> (Affine, &Shape) {
    match shape {
        Shape::Transform { inner, rot, offset, scale } => {
            let (a, b) = base(inner);
            let outer = Affine { m: std::array::from_fn(|i| std::array::from_fn(|j| rot[i][j] * scale[j])), t: *offset };
            (outer.then(&a), b)
        }
        s => (Affine::identity(), s),
    }
}

fn cells(len: f64, size: f64) -> usize {
    ((len / size).round() as usize).max(1)
}

/// The mesh of one body, in its own coordinates, or why it cannot be meshed.
fn body_mesh(shape: &Shape, size: f64) -> Result<HexMesh, String> {
    match shape {
        Shape::Brick { half, .. } => {
            let d: Vec<Division> = (0..3).map(|a| Division::uniform(cells(2.0 * half[a], size))).collect();
            Ok(box_mesh(half.map(|h| -h), *half, [&d[0], &d[1], &d[2]]))
        }
        Shape::Cylinder { r, h, .. } => {
            let n = cells(0.9 * r, size).max(2);
            let rings = cells(0.55 * r, size).max(1);
            let (mut pts, quads) = disk_quads(*r, n, rings);
            for p in pts.iter_mut() {
                p[2] = -h / 2.0;
            }
            sweep_quads(&pts, &quads, &vec![1; quads.len()], [0.0, 0.0, 1.0], *h, &Division::uniform(cells(*h, size)))
        }
        _ => Err("only bricks and cylinders (moved, turned or scaled) have a structured mesh; use scheme overlay".into()),
    }
}

/// How a body is meshed.
fn scheme_of(model: &Model, id: u32, shape: &Shape) -> &'static str {
    let structured = matches!(base(shape).1, Shape::Brick { .. } | Shape::Cylinder { .. });
    match model.mesh_schemes.get(&id).map(|s| s.as_str()) {
        Some("overlay") => "overlay",
        Some("stairstep") => "stairstep",
        Some("structured") => "structured",
        _ if structured => "structured",
        _ => "overlay",
    }
}

/// Where new HEX20/27 nodes on a curved body face go.
enum Proj {
    Cylinder(Affine, f64),
    Body(u32),
}

/// Mesh every body named by `mesh body` commands. Bricks and cylinders are structured (unless
/// another scheme is asked for); every other body, and those asked to, share one overlay grid
/// of the smallest size asked for, so touching bodies meet node to node. Blocks: one per body
/// (id and name of the body) unless `block` commands group them. Side sets: one per body face
/// (named as the face), the interfaces between overlay bodies, and those of `sideset`
/// commands; node sets from `nodeset` commands.
pub fn mesh_model(model: &Model) -> Result<HexMesh, String> {
    if model.mesh_requests.is_empty() {
        return Err("no body to mesh: add a command like 'mesh body 1 size 0.1'".into());
    }
    let mut out = HexMesh { npe: 8, ..Default::default() };
    // (element, side, face id, projector) of every boundary face.
    let mut face_of: Vec<(u32, u8, u32, Option<usize>)> = Vec::new();
    let mut projs: Vec<Proj> = Vec::new();
    let mut npe = 8;
    let mut smallest = f64::INFINITY;
    let block_of = |id: u32| -> (i32, String) {
        let bid = model.user_blocks.iter().find(|b| b.2.contains(&id)).map(|b| b.0).unwrap_or(id as i32);
        let name = model.user_blocks.iter().find(|b| b.0 == bid).map(|b| b.1.clone()).unwrap_or_else(|| model.bodies[&id].name.clone());
        (bid, name)
    };
    let mut overlay_bodies: Vec<(u32, f64, &'static str)> = Vec::new();
    for &(id, size, n) in &model.mesh_requests {
        let body = model.bodies.get(&id).ok_or_else(|| format!("there is no body {id}"))?;
        npe = npe.max(n);
        smallest = smallest.min(size);
        let scheme = scheme_of(model, id, &body.shape);
        if scheme != "structured" {
            overlay_bodies.push((id, size, scheme));
            continue;
        }
        let (aff, prim) = base(&body.shape);
        let mut m = body_mesh(prim, size).map_err(|e| format!("body {id}: {e}"))?;
        let proj = if let Shape::Cylinder { r, faces, .. } = prim {
            projs.push(Proj::Cylinder(aff, *r));
            Some((faces[0], projs.len() - 1))
        } else {
            None
        };
        for p in m.points.iter_mut() {
            *p = aff.apply(*p);
        }
        m.fix_orientation();
        let (bid, bname) = block_of(id);
        m.block = vec![bid; m.n_elements()];
        m.block_names.clear();
        m.block_names.insert(bid, bname);
        m.side_sets.clear();
        m.node_sets.clear();
        // Tag the boundary faces with the body face nearest their centre.
        let e0 = out.n_elements() as u32;
        for (e, s) in m.boundary_faces() {
            let c = m.side_corners(e as usize, s);
            let cen: P3 = std::array::from_fn(|a| c.iter().map(|k| m.points[*k as usize][a]).sum::<f64>() / 4.0);
            let f = body.shape.eval(cen).1;
            face_of.push((e + e0, s, f, proj.filter(|p| p.0 == f).map(|p| p.1)));
        }
        out.append(&m);
    }
    if !overlay_bodies.is_empty() {
        let h = overlay_bodies.iter().map(|b| b.1).fold(f64::INFINITY, f64::min);
        let shapes: Vec<(u32, &Shape)> = overlay_bodies.iter().map(|b| (b.0, &model.bodies[&b.0].shape)).collect();
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for (_, s) in &shapes {
            let (a, b) = s.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k] - 1.5 * h);
                hi[k] = hi[k].max(b[k] + 1.5 * h);
            }
        }
        let cells = std::array::from_fn(|k| (((hi[k] - lo[k]) / h).round() as usize).max(1));
        let stair = overlay_bodies.iter().all(|b| b.2 == "stairstep");
        let names = overlay_bodies.iter().map(|b| (b.0, block_of(b.0).1)).collect();
        let o = crate::overlay::OverlayOptions { lo, hi, cells, pillow: !stair, project: !stair, names, ..Default::default() };
        let src = crate::overlay::ShapeSource { shapes };
        let r = crate::overlay::overlay(&src, &o)?;
        let mut m = r.mesh;
        let body_of: Vec<u32> = m.block.iter().map(|b| *b as u32).collect();
        for b in m.block.iter_mut() {
            *b = block_of(*b as u32).0;
        }
        m.block_names = overlay_bodies.iter().map(|b| block_of(b.0)).collect();
        m.side_sets.retain(|s| s.0.starts_with("interface "));
        m.node_sets.retain(|s| s.0.starts_with("interface "));
        let e0 = out.n_elements() as u32;
        for (e, s) in m.boundary_faces() {
            let c = m.side_corners(e as usize, s);
            let cen: P3 = std::array::from_fn(|a| c.iter().map(|k| m.points[*k as usize][a]).sum::<f64>() / 4.0);
            let id = body_of[e as usize];
            let f = model.bodies[&id].shape.eval(cen).1;
            let k = match projs.iter().position(|p| matches!(p, Proj::Body(b) if *b == id)) {
                Some(k) => k,
                None => {
                    projs.push(Proj::Body(id));
                    projs.len() - 1
                }
            };
            face_of.push((e + e0, s, f, Some(k)));
        }
        out.append(&m);
    }
    out.weld(1e-9 * smallest);
    // Side sets per face, then the user's.
    let mut ids: Vec<u32> = face_of.iter().map(|f| f.2).filter(|f| *f > 0).collect();
    ids.sort_unstable();
    ids.dedup();
    for f in &ids {
        let faces: Vec<(u32, u8)> = face_of.iter().filter(|x| x.2 == *f).map(|x| (x.0, x.1)).collect();
        out.add_side_set(&model.faces[*f as usize], faces);
    }
    for (_, name, faces) in &model.user_sidesets {
        let fs: Vec<(u32, u8)> = face_of.iter().filter(|x| faces.contains(&x.2)).map(|x| (x.0, x.1)).collect();
        out.add_side_set(name, fs);
    }
    for (_, name, faces) in &model.user_nodesets {
        let nodes: Vec<u32> = face_of.iter().filter(|x| faces.contains(&x.2)).flat_map(|x| HEX8_SIDES[x.1 as usize - 1].map(|k| out.cell(x.0 as usize)[k])).collect();
        out.add_node_set(name, nodes);
    }
    if npe > 8 {
        // New nodes on curved faces go onto the exact surface: on a cylinder's side, by its
        // radius in its own coordinates; on an overlay body, by Newton steps on its distance.
        let curved: Vec<(u32, u8, usize)> = face_of.iter().filter_map(|x| x.3.map(|k| (x.0, x.1, k))).collect();
        let project = |i: usize, p: P3| -> P3 {
            match &projs[i] {
                Proj::Cylinder(aff, r) => {
                    let mut q = aff.inverse().apply(p);
                    let rho = (q[0] * q[0] + q[1] * q[1]).sqrt();
                    if rho > 0.0 {
                        q[0] *= r / rho;
                        q[1] *= r / rho;
                    }
                    aff.apply(q)
                }
                Proj::Body(id) => {
                    let s = &model.bodies[id].shape;
                    let mut q = p;
                    for _ in 0..8 {
                        let d = s.eval(q).0;
                        if d.abs() < 1e-13 {
                            break;
                        }
                        let e = 1e-7;
                        let g: P3 = std::array::from_fn(|a| {
                            let (mut x, mut y) = (q, q);
                            x[a] += e;
                            y[a] -= e;
                            (s.eval(x).0 - s.eval(y).0) / (2.0 * e)
                        });
                        let gg = g[0] * g[0] + g[1] * g[1] + g[2] * g[2];
                        if gg < 1e-12 {
                            break;
                        }
                        for a in 0..3 {
                            q[a] -= d * g[a] / gg;
                        }
                    }
                    q
                }
            }
        };
        out.elevate(npe, &curved, &project);
    }
    Ok(out)
}

/// Tetrahedral meshes of every body named by `mesh body` commands (isosurface stuffing at the
/// size asked for), each body its own block (or the block of a `block` command), a side set per
/// body face and those of `sideset` / `nodeset` commands.
pub fn tet_mesh_model(model: &Model) -> Result<crate::tet::TetMesh, String> {
    use crate::tet::TetMesh;
    if model.mesh_requests.is_empty() {
        return Err("no body to mesh: add a command like 'mesh body 1 size 0.1'".into());
    }
    let mut out = TetMesh::default();
    let mut face_of: Vec<(u32, u8, u32)> = Vec::new();
    for &(id, size, _) in &model.mesh_requests {
        let body = model.bodies.get(&id).ok_or_else(|| format!("there is no body {id}"))?;
        let (lo, hi) = body.shape.bounds();
        let lo: P3 = std::array::from_fn(|a| lo[a] - 1.5 * size);
        let hi: P3 = std::array::from_fn(|a| hi[a] + 1.5 * size);
        let shape = &body.shape;
        let m = crate::stuffing::stuff(&|p| shape.eval(p).0, lo, hi, size).map_err(|e| format!("body {id}: {e}"))?;
        let bid = model.user_blocks.iter().find(|b| b.2.contains(&id)).map(|b| b.0).unwrap_or(id as i32);
        let bname = model.user_blocks.iter().find(|b| b.0 == bid).map(|b| b.1.clone()).unwrap_or_else(|| body.name.clone());
        let (n0, e0) = (out.points.len() as u32, out.tets.len() as u32);
        for (e, s) in m.boundary_faces() {
            let c = m.side_corners(e as usize, s);
            let cen: P3 = std::array::from_fn(|a| c.iter().map(|k| m.points[*k as usize][a]).sum::<f64>() / 3.0);
            face_of.push((e + e0, s, shape.eval(cen).1));
        }
        out.points.extend(m.points);
        out.tets.extend(m.tets.iter().map(|t| t.map(|k| k + n0)));
        out.block.extend(std::iter::repeat_n(bid, m.tets.len()));
        out.block_names.insert(bid, bname);
    }
    let mut ids: Vec<u32> = face_of.iter().map(|f| f.2).filter(|f| *f > 0).collect();
    ids.sort_unstable();
    ids.dedup();
    for f in &ids {
        out.side_sets.push((model.faces[*f as usize].clone(), face_of.iter().filter(|x| x.2 == *f).map(|x| (x.0, x.1)).collect()));
    }
    for (_, name, faces) in &model.user_sidesets {
        out.side_sets.push((name.clone(), face_of.iter().filter(|x| faces.contains(&x.2)).map(|x| (x.0, x.1)).collect()));
    }
    for (_, name, faces) in &model.user_nodesets {
        let mut nodes: Vec<u32> = face_of.iter().filter(|x| faces.contains(&x.2)).flat_map(|x| out.side_corners(x.0 as usize, x.1)).collect();
        nodes.sort_unstable();
        nodes.dedup();
        out.node_sets.push((name.clone(), nodes));
    }
    Ok(out)
}
