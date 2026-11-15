//! Indexed triangle meshes and marching tetrahedra, ported from EZ-YX-craft (`ezyx-mesh`).

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriMesh {
    pub positions: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
    /// Optional per-vertex colours (sRGB 0..1) — used by the simulation renderer.
    #[serde(default)]
    pub colors: Vec<[f32; 3]>,
}

impl TriMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
    pub fn triangle(&self, i: usize) -> [Vec3; 3] {
        let t = self.indices[i];
        [self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]]
    }
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.positions.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }
    pub fn transform(&mut self, m: &Mat4) {
        for p in &mut self.positions {
            *p = m.transform_point3(*p);
        }
        if m.determinant() < 0.0 {
            for t in &mut self.indices {
                t.swap(1, 2);
            }
        }
    }
    pub fn append(&mut self, o: &TriMesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&o.positions);
        self.indices.extend(o.indices.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        if !o.colors.is_empty() || !self.colors.is_empty() {
            self.colors.resize(base as usize, [0.8; 3]);
            if o.colors.is_empty() {
                self.colors.resize(self.positions.len(), [0.8; 3]);
            } else {
                self.colors.extend_from_slice(&o.colors);
            }
        }
    }
    /// Area-weighted vertex normals.
    pub fn vertex_normals(&self) -> Vec<Vec3> {
        let mut n = vec![Vec3::ZERO; self.positions.len()];
        for t in &self.indices {
            let [a, b, c] = [self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]];
            let f = (b - a).cross(c - a);
            for i in t {
                n[*i as usize] += f;
            }
        }
        n.into_iter().map(|v| v.normalize_or_zero()).collect()
    }
    /// Merge vertices closer than `eps` (STL files store unshared vertices).
    pub fn weld(&mut self, eps: f32) {
        let inv = 1.0 / eps.max(1e-9);
        let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut remap = Vec::with_capacity(self.positions.len());
        let mut pos = Vec::new();
        for p in &self.positions {
            let k = ((p.x * inv).round() as i64, (p.y * inv).round() as i64, (p.z * inv).round() as i64);
            let id = *map.entry(k).or_insert_with(|| {
                pos.push(*p);
                (pos.len() - 1) as u32
            });
            remap.push(id);
        }
        self.positions = pos;
        self.colors.clear();
        self.indices = self
            .indices
            .iter()
            .map(|t| [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]])
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect();
    }
    /// Signed volume (positive for outward-oriented closed meshes).
    pub fn volume(&self) -> f32 {
        self.indices
            .iter()
            .map(|t| {
                let [a, b, c] = [self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]];
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
    /// Count edges used by != 2 triangles (0 for a watertight manifold).
    pub fn boundary_edge_count(&self) -> usize {
        let mut m: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.indices {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *m.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        m.values().filter(|c| **c != 2).count()
    }
    /// Taubin λ|μ smoothing (shrink-free) — cleans marching-tetrahedra stair steps.
    pub fn smooth_taubin(&mut self, iterations: usize, lambda: f32, mu: f32) {
        let n = self.positions.len();
        let mut nb: Vec<Vec<u32>> = vec![Vec::new(); n];
        for t in &self.indices {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                nb[a as usize].push(b);
                nb[b as usize].push(a);
            }
        }
        for v in &mut nb {
            v.sort_unstable();
            v.dedup();
        }
        for it in 0..iterations * 2 {
            let f = if it % 2 == 0 { lambda } else { mu };
            let old = self.positions.clone();
            for (i, p) in self.positions.iter_mut().enumerate() {
                if nb[i].is_empty() {
                    continue;
                }
                let avg = nb[i].iter().map(|j| old[*j as usize]).sum::<Vec3>() / nb[i].len() as f32;
                *p = old[i] + (avg - old[i]) * f;
            }
        }
    }

    /// Bytes this mesh occupies once tessellated for display (3 unshared 20-byte egui vertices and
    /// three u32 indices per triangle) — what a GPU vertex/index buffer has to hold.
    pub fn display_bytes(&self) -> usize {
        self.indices.len() * (3 * 20 + 3 * 4)
    }

    /// Simplify to at most `max_tris` triangles by vertex clustering on a uniform grid: vertices in
    /// the same cell merge into their average (colours averaged too), collapsed and duplicate
    /// triangles are dropped. Fast and robust (no topology requirements) — used to keep viewport
    /// meshes inside the GPU's buffer limits. Returns a clone when already small enough.
    pub fn decimated(&self, max_tris: usize) -> TriMesh {
        if self.indices.len() <= max_tris || self.positions.is_empty() {
            return self.clone();
        }
        let (lo, hi) = self.bounds();
        let size = (hi - lo).max(Vec3::splat(1e-6));
        // A surface of area A clustered with cell c has roughly 2·A/c² triangles; start there and
        // grow the cell until the target is met.
        let area: f32 = (0..self.indices.len())
            .map(|i| {
                let [a, b, c] = self.triangle(i);
                (b - a).cross(c - a).length() * 0.5
            })
            .sum();
        let mut cell = (2.0 * area.max(1e-9) / max_tris.max(1) as f32).sqrt().max(size.max_element() * 1e-4);
        let has_col = self.colors.len() == self.positions.len();
        for _ in 0..40 {
            let dims = (size / cell).ceil().as_uvec3() + 1;
            let key = |p: Vec3| {
                let g = ((p - lo) / cell).floor().as_uvec3().min(dims - 1);
                (g.z as u64 * dims.y as u64 + g.y as u64) * dims.x as u64 + g.x as u64
            };
            let mut cluster: HashMap<u64, u32> = HashMap::new();
            let mut sum: Vec<(Vec3, [f32; 3], u32)> = Vec::new();
            let remap: Vec<u32> = self
                .positions
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let id = *cluster.entry(key(*p)).or_insert_with(|| {
                        sum.push((Vec3::ZERO, [0.0; 3], 0));
                        (sum.len() - 1) as u32
                    });
                    let e = &mut sum[id as usize];
                    e.0 += *p;
                    if has_col {
                        for k in 0..3 {
                            e.1[k] += self.colors[i][k];
                        }
                    }
                    e.2 += 1;
                    id
                })
                .collect();
            let mut seen = std::collections::HashSet::new();
            let indices: Vec<[u32; 3]> = self
                .indices
                .iter()
                .map(|t| [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]])
                .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
                .filter(|t| {
                    // Same triangle in either winding counts once per winding (keeps both sides of thin walls).
                    let r = t.iter().position(|v| *v == *t.iter().min().unwrap()).unwrap();
                    seen.insert([t[r], t[(r + 1) % 3], t[(r + 2) % 3]])
                })
                .collect();
            if indices.len() <= max_tris {
                let positions = sum.iter().map(|(p, _, n)| *p / *n as f32).collect();
                let colors = if has_col { sum.iter().map(|(_, c, n)| c.map(|v| v / *n as f32)).collect() } else { vec![] };
                return TriMesh { positions, indices, colors };
            }
            cell *= 1.3;
        }
        // Pathological input: fall back to keeping every k-th triangle.
        let k = self.indices.len().div_ceil(max_tris.max(1));
        TriMesh { positions: self.positions.clone(), indices: self.indices.iter().step_by(k).copied().collect(), colors: self.colors.clone() }
    }

    pub fn cube(min: Vec3, max: Vec3) -> TriMesh {
        let p = |x: bool, y: bool, z: bool| Vec3::new(if x { max.x } else { min.x }, if y { max.y } else { min.y }, if z { max.z } else { min.z });
        let positions = vec![
            p(false, false, false),
            p(true, false, false),
            p(true, true, false),
            p(false, true, false),
            p(false, false, true),
            p(true, false, true),
            p(true, true, true),
            p(false, true, true),
        ];
        let indices = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        TriMesh { positions, indices, colors: vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cube_volume_and_closed() {
        let c = TriMesh::cube(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        assert!((c.volume() - 24.0).abs() < 1e-4);
        assert_eq!(c.boundary_edge_count(), 0);
    }

    #[test]
    fn decimation_hits_budget_and_keeps_shape() {
        // 200×100 grid surface (40 000 triangles) of a 100 × 50 mm sheet with a ramp.
        let (nx, ny) = (201u32, 101u32);
        let mut m = TriMesh::default();
        for j in 0..ny {
            for i in 0..nx {
                m.positions.push(Vec3::new(i as f32 * 0.5, j as f32 * 0.5, i as f32 * 0.01));
                m.colors.push([0.5, 0.25, 0.1]);
            }
        }
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                let a = j * nx + i;
                m.indices.push([a, a + 1, a + nx + 1]);
                m.indices.push([a, a + nx + 1, a + nx]);
            }
        }
        assert_eq!(m.indices.len(), 40_000);
        let d = m.decimated(2_000);
        assert!(d.indices.len() <= 2_000 && d.indices.len() > 500, "{}", d.indices.len());
        // Bounds shrink by at most one cluster cell and the projected area is kept within 10 %.
        let (lo, hi) = d.bounds();
        assert!(lo.x < 3.0 && hi.x > 97.0 && lo.y < 3.0 && hi.y > 47.0, "{lo:?} {hi:?}");
        let area: f32 = (0..d.indices.len())
            .map(|i| {
                let [a, b, c] = d.triangle(i);
                ((b - a).cross(c - a)).z.abs() * 0.5
            })
            .sum();
        assert!((area - 5000.0).abs() / 5000.0 < 0.1, "{area}");
        assert_eq!(d.colors.len(), d.positions.len());
        assert!((d.colors[0][0] - 0.5).abs() < 1e-5);
        // 72 bytes per displayed triangle.
        assert_eq!(m.display_bytes(), 40_000 * 72);
        // Small meshes pass through unchanged.
        let c = TriMesh::cube(Vec3::ZERO, Vec3::ONE);
        assert_eq!(c.decimated(100).indices.len(), 12);
    }
}

/// Marching tetrahedra over a regular scalar grid. Negative = inside.
pub mod mt {

    use super::TriMesh;
    use glam::Vec3;
    use rayon::prelude::*;
    use std::collections::HashMap;

    /// Scalar field sampled on (nx × ny × nz) nodes.
    pub struct Grid {
        pub nx: usize,
        pub ny: usize,
        pub nz: usize,
        pub origin: Vec3,
        pub h: f32,
        pub values: Vec<f32>,
    }

    impl Grid {
        /// Sample `f` on a grid covering [min, max] with spacing `h` (parallel).
        pub fn sample(min: Vec3, max: Vec3, h: f32, f: impl Fn(Vec3) -> f32 + Sync) -> Self {
            let pad = h;
            let min = min - Vec3::splat(pad);
            let max = max + Vec3::splat(pad);
            let n = ((max - min) / h).ceil().as_uvec3() + glam::UVec3::ONE;
            let (nx, ny, nz) = (n.x as usize, n.y as usize, n.z as usize);
            let values = (0..nx * ny * nz)
                .into_par_iter()
                .map(|i| {
                    let x = i % nx;
                    let y = (i / nx) % ny;
                    let z = i / (nx * ny);
                    f(min + Vec3::new(x as f32, y as f32, z as f32) * h)
                })
                .collect();
            Self { nx, ny, nz, origin: min, h, values }
        }
        #[inline]
        pub fn idx(&self, x: usize, y: usize, z: usize) -> usize {
            (z * self.ny + y) * self.nx + x
        }
        pub fn pos(&self, i: usize) -> Vec3 {
            let x = i % self.nx;
            let y = (i / self.nx) % self.ny;
            let z = i / (self.nx * self.ny);
            self.origin + Vec3::new(x as f32, y as f32, z as f32) * self.h
        }
    }

    /// Kuhn/Freudenthal split of the unit cube into 6 tetrahedra sharing the main diagonal 0–6.
    /// Cube corner numbering: bit0 = x, bit1 = y, bit2 = z.
    pub const KUHN_TETS: [[usize; 4]; 6] = [[0, 1, 3, 7], [0, 1, 5, 7], [0, 2, 3, 7], [0, 2, 6, 7], [0, 4, 5, 7], [0, 4, 6, 7]];

    pub fn extract(g: &Grid, iso: f32) -> TriMesh {
        let mut m = TriMesh::default();
        let mut edge_vert: HashMap<(usize, usize), u32> = HashMap::new();
        let mut vert = |a: usize, b: usize, m: &mut TriMesh| -> u32 {
            let key = if a < b { (a, b) } else { (b, a) };
            *edge_vert.entry(key).or_insert_with(|| {
                let (va, vb) = (g.values[a] - iso, g.values[b] - iso);
                let t = (va / (va - vb)).clamp(0.0, 1.0);
                m.positions.push(g.pos(a).lerp(g.pos(b), t));
                (m.positions.len() - 1) as u32
            })
        };
        for z in 0..g.nz - 1 {
            for y in 0..g.ny - 1 {
                for x in 0..g.nx - 1 {
                    let c: [usize; 8] = std::array::from_fn(|k| g.idx(x + (k & 1), y + ((k >> 1) & 1), z + ((k >> 2) & 1)));
                    // Quick reject: all same sign.
                    let s0 = g.values[c[0]] < iso;
                    if c.iter().all(|i| (g.values[*i] < iso) == s0) {
                        continue;
                    }
                    for t in KUHN_TETS {
                        let v = [c[t[0]], c[t[1]], c[t[2]], c[t[3]]];
                        let inside: Vec<usize> = (0..4).filter(|k| g.values[v[*k]] < iso).collect();
                        let outside: Vec<usize> = (0..4).filter(|k| g.values[v[*k]] >= iso).collect();
                        let emit = |tri: [u32; 3], m: &mut TriMesh, inside_pt: Vec3| {
                            // Orient so the normal points away from the inside.
                            let [a, b, cc] = [m.positions[tri[0] as usize], m.positions[tri[1] as usize], m.positions[tri[2] as usize]];
                            let n = (b - a).cross(cc - a);
                            if n.dot(a - inside_pt) < 0.0 {
                                m.indices.push([tri[0], tri[2], tri[1]]);
                            } else {
                                m.indices.push(tri);
                            }
                        };
                        match inside.len() {
                            1 | 3 => {
                                let (lone, others, lone_inside) = if inside.len() == 1 { (inside[0], outside, true) } else { (outside[0], inside, false) };
                                let tri = [vert(v[lone], v[others[0]], &mut m), vert(v[lone], v[others[1]], &mut m), vert(v[lone], v[others[2]], &mut m)];
                                let ip = if lone_inside { g.pos(v[lone]) } else { g.pos(v[others[0]]) };
                                emit(tri, &mut m, ip);
                            }
                            2 => {
                                let (i0, i1, o0, o1) = (inside[0], inside[1], outside[0], outside[1]);
                                let a = vert(v[i0], v[o0], &mut m);
                                let b = vert(v[i0], v[o1], &mut m);
                                let cq = vert(v[i1], v[o1], &mut m);
                                let d = vert(v[i1], v[o0], &mut m);
                                let ip = (g.pos(v[i0]) + g.pos(v[i1])) * 0.5;
                                emit([a, b, cq], &mut m, ip);
                                emit([a, cq, d], &mut m, ip);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // Nodes lying exactly on the iso-surface create coincident vertices on different edges;
        // weld them so the mesh is a proper manifold for smoothing.
        m.weld(g.h * 1e-4);
        m
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn sphere() {
            let g = Grid::sample(Vec3::splat(-10.0), Vec3::splat(10.0), 0.5, |p| p.length() - 8.0);
            let m = extract(&g, 0.0);
            assert_eq!(m.boundary_edge_count(), 0, "watertight");
            let v = m.volume();
            let exact = 4.0 / 3.0 * std::f32::consts::PI * 512.0;
            assert!((v - exact).abs() / exact < 0.03, "{v} vs {exact}");
        }
    }
}
