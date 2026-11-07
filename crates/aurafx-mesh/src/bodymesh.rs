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
        _ => Err("only bricks and cylinders (moved, turned or scaled) are meshed with hexahedra so far".into()),
    }
}

/// Mesh every body named by `mesh body` commands. Blocks: one per body (id and name of the
/// body) unless `block` commands group them. Side sets: one per body face (named as the face)
/// plus those of `sideset` commands; node sets from `nodeset` commands.
pub fn mesh_model(model: &Model) -> Result<HexMesh, String> {
    if model.mesh_requests.is_empty() {
        return Err("no body to mesh: add a command like 'mesh body 1 size 0.1'".into());
    }
    let mut out = HexMesh { npe: 8, ..Default::default() };
    let mut face_of: Vec<(u32, u8, u32)> = Vec::new();
    let mut npe = 8;
    let mut smallest = f64::INFINITY;
    // (transform, radius, side face) of every meshed cylinder.
    let mut cyl_side: Vec<(Affine, f64, u32)> = Vec::new();
    for &(id, size, n) in &model.mesh_requests {
        let body = model.bodies.get(&id).ok_or_else(|| format!("there is no body {id}"))?;
        let (aff, prim) = base(&body.shape);
        let mut m = body_mesh(prim, size).map_err(|e| format!("body {id}: {e}"))?;
        if let Shape::Cylinder { r, faces, .. } = prim {
            cyl_side.push((aff, *r, faces[0]));
        }
        for p in m.points.iter_mut() {
            *p = aff.apply(*p);
        }
        m.fix_orientation();
        let bid = model.user_blocks.iter().find(|b| b.2.contains(&id)).map(|b| b.0).unwrap_or(id as i32);
        let bname = model.user_blocks.iter().find(|b| b.0 == bid).map(|b| b.1.clone()).unwrap_or_else(|| body.name.clone());
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
            face_of.push((e + e0, s, f));
        }
        out.append(&m);
        npe = npe.max(n);
        smallest = smallest.min(size);
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
        // New nodes on cylinder sides go onto the exact surface: in the cylinder's own
        // coordinates their distance from the axis becomes the radius. (Bricks are flat.)
        let curved: Vec<(u32, u8, usize)> = face_of.iter().filter_map(|x| cyl_side.iter().position(|c| c.2 == x.2).map(|i| (x.0, x.1, i))).collect();
        let project = |i: usize, p: P3| -> P3 {
            let (aff, r, _) = &cyl_side[i];
            let inv = aff.inverse();
            let mut q = inv.apply(p);
            let rho = (q[0] * q[0] + q[1] * q[1]).sqrt();
            if rho > 0.0 {
                q[0] *= r / rho;
                q[1] *= r / rho;
            }
            aff.apply(q)
        };
        out.elevate(npe, &curved, &project);
    }
    Ok(out)
}
