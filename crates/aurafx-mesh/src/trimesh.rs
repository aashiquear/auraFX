//! Closed triangle surfaces as signed distance fields: the nearest point through a bounding
//! box tree, the sign from angle-weighted pseudonormals (exact for closed, consistently
//! oriented surfaces).

use anyhow::{bail, Result};
use std::collections::HashMap;

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V) -> V {
    mul(a, 1.0 / norm(a).max(1e-300))
}

/// Which part of a triangle a nearest point lies on.
#[derive(Clone, Copy)]
enum Feature {
    Face,
    Vertex(usize),
    /// Edge from corner k to corner (k + 1) % 3.
    Edge(usize),
}

struct Node {
    lo: V,
    hi: V,
    /// Leaf: range of `order`; inner: children.
    start: usize,
    count: usize,
    left: usize,
    right: usize,
}

pub struct TriMesh {
    pub verts: Vec<V>,
    pub tris: Vec<[u32; 3]>,
    face_n: Vec<V>,
    vert_n: Vec<V>,
    edge_n: HashMap<(u32, u32), V>,
    nodes: Vec<Node>,
    order: Vec<usize>,
    /// Patch of each triangle: the pieces of the surface between sharp edges (where the
    /// faces on either side turn by more than 30 degrees).
    pub patch: Vec<u32>,
    pub n_patches: u32,
    /// Bounds of the surface.
    pub lo: V,
    pub hi: V,
}

impl TriMesh {
    /// A surface from points and triangles. Points closer than a millionth of the size are
    /// merged first (STL files repeat every corner).
    pub fn new(points: &[V], tris: &[[u32; 3]]) -> Result<TriMesh> {
        if tris.is_empty() {
            bail!("the surface has no triangles");
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in points {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let tol = norm(sub(hi, lo)) * 1e-6;
        let key = |p: &V| -> [i64; 3] { std::array::from_fn(|k| ((p[k] - lo[k]) / tol.max(1e-300)).round() as i64) };
        let mut map: HashMap<[i64; 3], u32> = HashMap::new();
        let mut verts = Vec::new();
        let remap: Vec<u32> = points
            .iter()
            .map(|p| {
                *map.entry(key(p)).or_insert_with(|| {
                    verts.push(*p);
                    verts.len() as u32 - 1
                })
            })
            .collect();
        let tris: Vec<[u32; 3]> = tris.iter().map(|t| t.map(|i| remap[i as usize])).filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).collect();
        let mut face_n = Vec::with_capacity(tris.len());
        let mut vert_n = vec![[0.0; 3]; verts.len()];
        let mut edge_n: HashMap<(u32, u32), V> = HashMap::new();
        for t in &tris {
            let p = t.map(|i| verts[i as usize]);
            let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
            face_n.push(n);
            for k in 0..3 {
                let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
                let ang = dot(unit(sub(b, a)), unit(sub(c, a))).clamp(-1.0, 1.0).acos();
                vert_n[t[k] as usize] = add(vert_n[t[k] as usize], mul(n, ang));
                let e = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
                let en = edge_n.entry(e).or_insert([0.0; 3]);
                *en = add(*en, n);
            }
        }
        // Patches: triangles joined across edges where the surface turns by less than 30 degrees.
        let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (i, t) in tris.iter().enumerate() {
            for k in 0..3 {
                by_edge.entry((t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]))).or_default().push(i);
            }
        }
        let mut patch = vec![u32::MAX; tris.len()];
        let mut n_patches = 0;
        let cos30 = 30f64.to_radians().cos();
        for s0 in 0..tris.len() {
            if patch[s0] != u32::MAX {
                continue;
            }
            patch[s0] = n_patches;
            let mut stack = vec![s0];
            while let Some(i) = stack.pop() {
                let t = tris[i];
                for k in 0..3 {
                    for &j in &by_edge[&(t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]))] {
                        if patch[j] == u32::MAX && dot(face_n[i], face_n[j]) >= cos30 {
                            patch[j] = n_patches;
                            stack.push(j);
                        }
                    }
                }
            }
            n_patches += 1;
        }
        let mut m = TriMesh { verts, tris, face_n, vert_n, edge_n, nodes: Vec::new(), order: Vec::new(), patch, n_patches, lo, hi };
        m.build();
        Ok(m)
    }

    fn tri_bounds(&self, t: usize) -> (V, V) {
        let p = self.tris[t].map(|i| self.verts[i as usize]);
        (std::array::from_fn(|k| p[0][k].min(p[1][k]).min(p[2][k])), std::array::from_fn(|k| p[0][k].max(p[1][k]).max(p[2][k])))
    }

    fn build(&mut self) {
        self.order = (0..self.tris.len()).collect();
        let cent: Vec<V> = (0..self.tris.len())
            .map(|t| {
                let p = self.tris[t].map(|i| self.verts[i as usize]);
                mul(add(add(p[0], p[1]), p[2]), 1.0 / 3.0)
            })
            .collect();
        let mut stack = vec![(0usize, self.tris.len(), usize::MAX, false)];
        while let Some((start, count, parent, is_right)) = stack.pop() {
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for &t in &self.order[start..start + count] {
                let (a, b) = self.tri_bounds(t);
                for k in 0..3 {
                    lo[k] = lo[k].min(a[k]);
                    hi[k] = hi[k].max(b[k]);
                }
            }
            let id = self.nodes.len();
            self.nodes.push(Node { lo, hi, start, count, left: 0, right: 0 });
            if parent != usize::MAX {
                if is_right {
                    self.nodes[parent].right = id;
                } else {
                    self.nodes[parent].left = id;
                }
            }
            if count > 4 {
                let ext = sub(hi, lo);
                let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
                    0
                } else if ext[1] >= ext[2] {
                    1
                } else {
                    2
                };
                let mid = count / 2;
                self.order[start..start + count].select_nth_unstable_by(mid, |a, b| cent[*a][axis].total_cmp(&cent[*b][axis]));
                self.nodes[id].count = 0;
                stack.push((start + mid, count - mid, id, true));
                stack.push((start, mid, id, false));
            }
        }
    }

    /// Nearest point on triangle `t` to `p` and the feature it lies on.
    fn closest(&self, t: usize, p: V) -> (V, Feature) {
        let [a, b, c] = self.tris[t].map(|i| self.verts[i as usize]);
        let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
        let (d1, d2) = (dot(ab, ap), dot(ac, ap));
        if d1 <= 0.0 && d2 <= 0.0 {
            return (a, Feature::Vertex(0));
        }
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        if d3 >= 0.0 && d4 <= d3 {
            return (b, Feature::Vertex(1));
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            let v = d1 / (d1 - d3);
            return (add(a, mul(ab, v)), Feature::Edge(0));
        }
        let cp = sub(p, c);
        let (d5, d6) = (dot(ab, cp), dot(ac, cp));
        if d6 >= 0.0 && d5 <= d6 {
            return (c, Feature::Vertex(2));
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            let w = d2 / (d2 - d6);
            return (add(a, mul(ac, w)), Feature::Edge(2));
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
            return (add(b, mul(sub(c, b), w)), Feature::Edge(1));
        }
        let den = 1.0 / (va + vb + vc);
        let (v, w) = (vb * den, vc * den);
        (add(a, add(mul(ab, v), mul(ac, w))), Feature::Face)
    }

    fn box_dist2(lo: V, hi: V, p: V) -> f64 {
        (0..3)
            .map(|k| {
                let d = (lo[k] - p[k]).max(0.0).max(p[k] - hi[k]);
                d * d
            })
            .sum()
    }

    /// Signed distance to the surface (negative inside).
    pub fn distance(&self, p: V) -> f64 {
        self.distance_patch(p).0
    }

    /// Signed distance to the surface and the patch of the nearest triangle.
    pub fn distance_patch(&self, p: V) -> (f64, u32) {
        let mut best = (f64::INFINITY, 0usize, [0.0; 3], Feature::Face);
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n];
            if Self::box_dist2(node.lo, node.hi, p) >= best.0 {
                continue;
            }
            if node.count > 0 {
                for &t in &self.order[node.start..node.start + node.count] {
                    let (q, f) = self.closest(t, p);
                    let d = dot(sub(p, q), sub(p, q));
                    if d < best.0 {
                        best = (d, t, q, f);
                    }
                }
            } else {
                let (l, r) = (node.left, node.right);
                let (dl, dr) = (Self::box_dist2(self.nodes[l].lo, self.nodes[l].hi, p), Self::box_dist2(self.nodes[r].lo, self.nodes[r].hi, p));
                if dl < dr {
                    stack.push(r);
                    stack.push(l);
                } else {
                    stack.push(l);
                    stack.push(r);
                }
            }
        }
        let (d2, t, q, f) = best;
        let tri = self.tris[t];
        let n = match f {
            Feature::Face => self.face_n[t],
            Feature::Vertex(k) => self.vert_n[tri[k] as usize],
            Feature::Edge(k) => {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                self.edge_n.get(&(a.min(b), a.max(b))).copied().unwrap_or(self.face_n[t])
            }
        };
        let d = d2.sqrt();
        (if dot(sub(p, q), n) < 0.0 { -d } else { d }, self.patch[t])
    }

    /// Enclosed volume (divergence theorem; positive for outward-facing triangles).
    pub fn volume(&self) -> f64 {
        self.tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.verts[i as usize]);
                dot(a, cross(b, c)) / 6.0
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_distances() {
        let v: Vec<V> = (0..8).map(|i| [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64]).collect();
        // Outward triangles of the unit cube.
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let tris: Vec<[u32; 3]> = quads.iter().flat_map(|q: &[u32; 4]| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        let m = TriMesh::new(&v, &tris).unwrap();
        assert!((m.volume() - 1.0).abs() < 1e-12);
        assert!((m.distance([0.5, 0.5, 0.5]) + 0.5).abs() < 1e-12);
        assert!((m.distance([2.0, 0.5, 0.5]) - 1.0).abs() < 1e-12);
        // Near a corner and an edge, outside.
        assert!((m.distance([2.0, 2.0, 2.0]) - 3f64.sqrt()).abs() < 1e-12);
        assert!((m.distance([-1.0, -1.0, 0.5]) - 2f64.sqrt()).abs() < 1e-12);
        assert!((m.distance([0.9, 0.95, 0.2]) + 0.05).abs() < 1e-12);
    }
}
