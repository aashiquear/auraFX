//! Overlay-grid hexahedral meshing: a Cartesian grid laid over the geometry, one material per
//! grid cell (sampled inside the cell), small features removed, then the cells of each
//! material wrapped in a layer of hexahedra (pillowing) whose outer nodes are moved onto the
//! material's surface, and the grid smoothed with a guard against inverted elements.
//!
//! Materials come from a [`MaterialSource`]: the bodies of a geometry (with exact signed
//! distances, so interface nodes are projected onto the true surfaces) or a labelled voxel
//! image (no distances: the interface nodes are smoothed along the interface instead).
//! Material 0 is void.

use crate::hex::{HexMesh, P3, HEX8_SIDES, HEX_EDGES};
use crate::shape::Shape;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Where the materials are.
pub trait MaterialSource: Sync {
    /// The material at a point (0: void).
    fn material(&self, p: P3) -> u32;
    /// Signed distance (negative inside) to the region of material `m` (0: void), when known.
    fn distance(&self, m: u32, p: P3) -> Option<f64>;
    /// The geometry face of material `m` nearest `p` (0: none known).
    fn face(&self, _m: u32, _p: P3) -> u32 {
        0
    }
}

/// Bodies as materials: a point belongs to the first shape that contains it.
pub struct ShapeSource<'a> {
    pub shapes: Vec<(u32, &'a Shape)>,
}

impl MaterialSource for ShapeSource<'_> {
    fn material(&self, p: P3) -> u32 {
        self.shapes.iter().find(|(_, s)| s.eval(p).0 <= 0.0).map(|(m, _)| *m).unwrap_or(0)
    }
    fn distance(&self, m: u32, p: P3) -> Option<f64> {
        if m == 0 {
            return Some(-self.shapes.iter().map(|(_, s)| s.eval(p).0).fold(f64::INFINITY, f64::min));
        }
        // The region of a material is its shape less the shapes listed before it.
        let i = self.shapes.iter().position(|(k, _)| *k == m)?;
        let mut d = self.shapes[i].1.eval(p).0;
        for (_, s) in &self.shapes[..i] {
            d = d.max(-s.eval(p).0);
        }
        Some(d)
    }
    fn face(&self, m: u32, p: P3) -> u32 {
        self.shapes.iter().find(|(k, _)| *k == m).map(|(_, s)| s.eval(p).1).unwrap_or(0)
    }
}

/// A labelled voxel image: `labels` per voxel, x fastest.
pub struct LabelSource {
    pub n: [usize; 3],
    pub origin: P3,
    pub spacing: P3,
    pub labels: Vec<u32>,
}

impl MaterialSource for LabelSource {
    fn material(&self, p: P3) -> u32 {
        let ix: [usize; 3] = std::array::from_fn(|a| (((p[a] - self.origin[a]) / self.spacing[a]).floor().max(0.0) as usize).min(self.n[a] - 1));
        self.labels[ix[0] + self.n[0] * (ix[1] + self.n[1] * ix[2])]
    }
    fn distance(&self, _: u32, _: P3) -> Option<f64> {
        None
    }
}

#[derive(Clone, Debug)]
pub struct OverlayOptions {
    pub lo: P3,
    pub hi: P3,
    pub cells: [usize; 3],
    /// Sample points per cell along each axis (1: the centre only).
    pub samples: usize,
    /// Connected pieces of a material with fewer cells are given to their surroundings.
    pub min_cells: usize,
    /// Cells with at most one face neighbour of their own material join their neighbours.
    pub protrusions: bool,
    /// Wrap each material in a layer of hexahedra before moving its surface nodes.
    pub pillow: bool,
    /// Move interface nodes onto the surfaces (off: stair-step mesh).
    pub project: bool,
    /// Laplacian smoothing passes.
    pub smoothing: usize,
    /// Elements whose scaled Jacobian stays below this are optimised node by node.
    pub sj_target: f64,
    /// Mesh the void as a material of its own.
    pub mesh_void: bool,
    /// Pair the nodes of opposite domain faces (the domain faces stay flat and unmoved).
    pub periodic: bool,
    /// Put nodes on the sharp edges between geometry faces (geometry with distances only).
    pub curves: bool,
    /// Block names per material.
    pub names: BTreeMap<u32, String>,
    /// Names of geometry faces (index = face id) for the side sets on the model's surface.
    pub face_names: Vec<String>,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        OverlayOptions {
            lo: [0.0; 3],
            hi: [1.0; 3],
            cells: [10; 3],
            samples: 3,
            min_cells: 4,
            protrusions: true,
            pillow: true,
            project: true,
            smoothing: 8,
            sj_target: 0.6,
            mesh_void: false,
            periodic: false,
            curves: true,
            names: BTreeMap::new(),
            face_names: Vec::new(),
        }
    }
}

/// The outward direction of each Exodus side of a grid cell, and the neighbour's offset.
const SIDE_DIR: [(usize, i64); 6] = [(1, -1), (0, 1), (1, 1), (0, -1), (2, -1), (2, 1)];
/// The three corners joined to each corner, in right-handed order.
const CORNER_NB: [[usize; 3]; 8] = [[1, 3, 4], [2, 0, 5], [3, 1, 6], [0, 2, 7], [7, 5, 0], [4, 6, 1], [5, 7, 2], [6, 4, 3]];

/// The scaled Jacobians of the eight corners.
fn corner_sjs(p: &[P3; 8]) -> [f64; 8] {
    std::array::from_fn(|c| {
        let nb = CORNER_NB[c];
        let e: [P3; 3] = nb.map(|k| std::array::from_fn(|a| p[k][a] - p[c][a]));
        let l = e.map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
        let d = e[0][0] * (e[1][1] * e[2][2] - e[1][2] * e[2][1]) - e[0][1] * (e[1][0] * e[2][2] - e[1][2] * e[2][0]) + e[0][2] * (e[1][0] * e[2][1] - e[1][1] * e[2][0]);
        let n = l[0] * l[1] * l[2];
        if n > 0.0 { d / n } else { -1.0 }
    })
}

/// The smallest scaled Jacobian over the eight corners (negative when inverted).
pub fn corner_sj(p: &[P3; 8]) -> f64 {
    let mut q = f64::INFINITY;
    for (c, nb) in CORNER_NB.iter().enumerate() {
        let e: [P3; 3] = nb.map(|k| std::array::from_fn(|a| p[k][a] - p[c][a]));
        let l = e.map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
        let d = e[0][0] * (e[1][1] * e[2][2] - e[1][2] * e[2][1]) - e[0][1] * (e[1][0] * e[2][2] - e[1][2] * e[2][0]) + e[0][2] * (e[1][0] * e[2][1] - e[1][1] * e[2][0]);
        let n = l[0] * l[1] * l[2];
        q = q.min(if n > 0.0 { d / n } else { -1.0 });
    }
    q
}

struct Grid {
    n: [usize; 3],
    lo: P3,
    h: P3,
}

impl Grid {
    fn cell(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }
    fn ijk(&self, c: usize) -> [usize; 3] {
        [c % self.n[0], (c / self.n[0]) % self.n[1], c / (self.n[0] * self.n[1])]
    }
    fn node(&self, i: usize, j: usize, k: usize) -> usize {
        i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k)
    }
    fn n_nodes(&self) -> usize {
        (self.n[0] + 1) * (self.n[1] + 1) * (self.n[2] + 1)
    }
    fn node_ijk(&self, v: usize) -> [usize; 3] {
        let (a, b) = (self.n[0] + 1, self.n[1] + 1);
        [v % a, (v / a) % b, v / (a * b)]
    }
    /// The neighbour of cell `c` across Exodus side `s` (1..6).
    fn across(&self, c: usize, s: u8) -> Option<usize> {
        let mut p = self.ijk(c).map(|x| x as i64);
        let (a, d) = SIDE_DIR[s as usize - 1];
        p[a] += d;
        (p[a] >= 0 && p[a] < self.n[a] as i64).then(|| self.cell(p[0] as usize, p[1] as usize, p[2] as usize))
    }
    fn nbrs6(&self, c: usize) -> impl Iterator<Item = usize> + '_ {
        (1..=6u8).filter_map(move |s| self.across(c, s))
    }
    /// Grid nodes of cell `c` in Exodus order.
    fn corners(&self, c: usize) -> [usize; 8] {
        let [i, j, k] = self.ijk(c);
        [self.node(i, j, k), self.node(i + 1, j, k), self.node(i + 1, j + 1, k), self.node(i, j + 1, k), self.node(i, j, k + 1), self.node(i + 1, j, k + 1), self.node(i + 1, j + 1, k + 1), self.node(i, j + 1, k + 1)]
    }
    /// The cells around grid node `v` (inside the grid), with their offset bits (x, y, z).
    fn around(&self, v: usize) -> Vec<(usize, usize)> {
        let [i, j, k] = self.node_ijk(v);
        let mut out = Vec::with_capacity(8);
        for b in 0..8 {
            let (di, dj, dk) = (b & 1, (b >> 1) & 1, (b >> 2) & 1);
            if (i + di >= 1 && i + di <= self.n[0]) && (j + dj >= 1 && j + dj <= self.n[1]) && (k + dk >= 1 && k + dk <= self.n[2]) {
                out.push((self.cell(i + di - 1, j + dj - 1, k + dk - 1), b));
            }
        }
        out
    }
}

/// Material per cell: the most frequent among `s`³ sample points (ties: the centre's).
fn classify(src: &dyn MaterialSource, g: &Grid, s: usize) -> Vec<u32> {
    let s = s.max(1);
    (0..g.n[0] * g.n[1] * g.n[2])
        .into_par_iter()
        .map(|c| {
            let p = g.ijk(c);
            let at = |f: [f64; 3]| -> P3 { std::array::from_fn(|a| g.lo[a] + (p[a] as f64 + f[a]) * g.h[a]) };
            if s == 1 {
                return src.material(at([0.5; 3]));
            }
            let mut count: Vec<(u32, usize)> = Vec::new();
            for a in 0..s {
                for b in 0..s {
                    for d in 0..s {
                        let m = src.material(at([(a as f64 + 0.5) / s as f64, (b as f64 + 0.5) / s as f64, (d as f64 + 0.5) / s as f64]));
                        match count.iter_mut().find(|x| x.0 == m) {
                            Some(x) => x.1 += 1,
                            None => count.push((m, 1)),
                        }
                    }
                }
            }
            let best = count.iter().map(|x| x.1).max().unwrap();
            let tied: Vec<u32> = count.iter().filter(|x| x.1 == best).map(|x| x.0).collect();
            if tied.len() == 1 {
                tied[0]
            } else {
                let c = src.material(at([0.5; 3]));
                if tied.contains(&c) {
                    c
                } else {
                    tied[0]
                }
            }
        })
        .collect()
}

fn most_common(v: impl Iterator<Item = u32>) -> Option<u32> {
    let mut c: BTreeMap<u32, usize> = BTreeMap::new();
    for m in v {
        *c.entry(m).or_insert(0) += 1;
    }
    c.into_iter().max_by_key(|(m, n)| (*n, std::cmp::Reverse(*m))).map(|x| x.0)
}

/// Remove small pieces and one-cell protrusions, then make every material face-connected
/// around every grid node (no two cells of a material touching only along an edge or at a
/// corner), which pillowing and projection need.
fn defeature(g: &Grid, mat: &mut [u32], o: &OverlayOptions) {
    let nc = mat.len();
    if o.min_cells > 1 {
        let mut seen = vec![false; nc];
        for start in 0..nc {
            if seen[start] {
                continue;
            }
            let m = mat[start];
            let mut comp = vec![start];
            seen[start] = true;
            let mut k = 0;
            while k < comp.len() {
                let c = comp[k];
                k += 1;
                for d in g.nbrs6(c) {
                    if !seen[d] && mat[d] == m {
                        seen[d] = true;
                        comp.push(d);
                    }
                }
            }
            if comp.len() < o.min_cells {
                let set: BTreeSet<usize> = comp.iter().copied().collect();
                if let Some(r) = most_common(comp.iter().flat_map(|c| g.nbrs6(*c)).filter(|d| !set.contains(d)).map(|d| mat[d])) {
                    for c in comp {
                        mat[c] = r;
                    }
                }
            }
        }
    }
    if o.protrusions {
        for _ in 0..4 {
            let change: Vec<(usize, u32)> = (0..nc)
                .into_par_iter()
                .filter_map(|c| {
                    let nb: Vec<usize> = g.nbrs6(c).collect();
                    let same = nb.iter().filter(|d| mat[**d] == mat[c]).count();
                    // One cell thick along an axis: both neighbours along it are of other materials.
                    let thin = (0..3).any(|a| match (g.across(c, [4u8, 1, 5][a]), g.across(c, [2u8, 3, 6][a])) {
                        (Some(x), Some(y)) => mat[x] != mat[c] && mat[y] != mat[c],
                        _ => false,
                    });
                    if (same <= 1 && nb.len() >= 3) || thin {
                        most_common(nb.iter().map(|d| mat[*d]).filter(|m| *m != mat[c])).map(|m| (c, m))
                    } else {
                        None
                    }
                })
                .collect();
            if change.is_empty() {
                break;
            }
            for (c, m) in change {
                mat[c] = m;
            }
        }
    }
    // Around every grid node, each material's cells and the rest must each be face-connected
    // and the material's faces there must not point both ways along an axis (a saddle):
    // otherwise its pillow layer has no inside to move to. A node with such a problem changes
    // the one cell of its block that removes the most problems (preferring the material most
    // of the cell's neighbours have); the nodes around a changed cell are checked again.
    let n_nodes = g.n_nodes();
    let total = |mat: &[u32], c: usize| -> usize { g.corners(c).iter().map(|w| block_problems(mat, &g.around(*w))).sum() };
    let mut queue: std::collections::VecDeque<usize> = (0..n_nodes).collect();
    let mut queued = vec![true; n_nodes];
    while let Some(v) = queue.pop_front() {
        queued[v] = false;
        let around = g.around(v);
        if around.len() < 2 || block_problems(mat, &around) == 0 {
            continue;
        }
        // The change of one cell of the block that lowers the problems of all the blocks it
        // touches the most (so the total only falls and the loop ends).
        let mut best: Option<(i64, i64, usize, u32)> = None;
        for &(c, _) in &around {
            let old = mat[c];
            let before = total(mat, c) as i64;
            let mut options: Vec<u32> = around.iter().map(|(d, _)| mat[*d]).chain(g.nbrs6(c).map(|d| mat[d])).filter(|t| *t != old).collect();
            options.sort_unstable();
            options.dedup();
            for t in options {
                mat[c] = t;
                let delta = total(mat, c) as i64 - before;
                let agree = g.nbrs6(c).filter(|d| mat[*d] == t).count() as i64;
                mat[c] = old;
                if delta < 0 && best.is_none_or(|b| (delta, -agree) < (b.0, -b.1)) {
                    best = Some((delta, agree, c, t));
                }
            }
        }
        if let Some((_, _, c, t)) = best {
            mat[c] = t;
            for w in g.corners(c) {
                if !queued[w] {
                    queued[w] = true;
                    queue.push_back(w);
                }
            }
        }
    }
}

/// Problems of the cells around a grid node (offset bits in `around`): for each material, its
/// cells in more than one face-connected piece, the other cells in more than one piece, and
/// a saddle (faces of the material pointing both ways along an axis).
fn block_problems(mat: &[u32], around: &[(usize, usize)]) -> usize {
    let adj = |a: usize, b: usize| (around[a].1 ^ around[b].1).count_ones() == 1;
    let pieces = |set: &[usize]| -> usize {
        let mut seen = [false; 8];
        let mut n = 0;
        for &s0 in set {
            if seen[s0] {
                continue;
            }
            n += 1;
            let mut stack = vec![s0];
            seen[s0] = true;
            while let Some(a) = stack.pop() {
                for &b in set {
                    if !seen[b] && adj(a, b) {
                        seen[b] = true;
                        stack.push(b);
                    }
                }
            }
        }
        n
    };
    let mut mats: Vec<u32> = around.iter().map(|(c, _)| mat[*c]).collect();
    mats.sort_unstable();
    mats.dedup();
    if mats.len() < 2 {
        return 0;
    }
    let mut n = 0;
    for m in mats {
        let mine: Vec<usize> = (0..around.len()).filter(|k| mat[around[*k].0] == m).collect();
        let rest: Vec<usize> = (0..around.len()).filter(|k| mat[around[*k].0] != m).collect();
        if pieces(&mine) > 1 {
            n += 1;
        }
        if pieces(&rest) > 1 {
            n += 1;
        }
        for ax in 0..3 {
            let bit = 1usize << ax;
            let (mut plus, mut minus) = (false, false);
            for (i, (_, bi)) in around.iter().enumerate() {
                if bi & bit != 0 {
                    continue;
                }
                if let Some(j) = around.iter().position(|(_, bj)| *bj == bi | bit) {
                    let (mi, mj) = (mat[around[i].0] == m, mat[around[j].0] == m);
                    plus |= mi && !mj;
                    minus |= mj && !mi;
                }
            }
            if plus && minus {
                n += 1;
                break;
            }
        }
    }
    n
}

/// The overlay mesh and a summary of its quality.
pub struct Overlay {
    pub mesh: HexMesh,
    pub min_sj: f64,
    pub inverted: usize,
}

/// Mesh the materials of `src` on the grid of `o`.
pub fn overlay(src: &dyn MaterialSource, o: &OverlayOptions) -> Result<Overlay, String> {
    let n = o.cells.map(|c| c.max(1));
    let h: P3 = std::array::from_fn(|a| (o.hi[a] - o.lo[a]) / n[a] as f64);
    if h.iter().any(|x| !(*x > 0.0)) {
        return Err("the overlay grid has no volume".into());
    }
    if n[0] * n[1] * n[2] > 50_000_000 {
        return Err(format!("{} x {} x {} cells is more than 50 million", n[0], n[1], n[2]));
    }
    let g = Grid { n, lo: o.lo, h };
    let timing = std::env::var("AURAFX_OVERLAY_TIMING").is_ok();
    let t0 = std::time::Instant::now();
    let tick = |what: &str| {
        if timing {
            eprintln!("overlay: {what} at {:.2} s", t0.elapsed().as_secs_f64());
        }
    };
    let hmin = h.iter().cloned().fold(f64::INFINITY, f64::min);
    let hmax = h.iter().cloned().fold(0.0, f64::max);
    let mut mat = classify(src, &g, o.samples);
    tick("classified");
    defeature(&g, &mut mat, o);
    tick("defeatured");
    let active = |m: u32| m != 0 || o.mesh_void;
    if !mat.iter().any(|m| active(*m)) {
        return Err("no material inside the overlay grid".into());
    }
    let stair = !o.pillow && !o.project;

    // Nodes and elements of the active cells.
    let mut node_of = vec![u32::MAX; g.n_nodes()];
    let mut points: Vec<P3> = Vec::new();
    let mut flags: Vec<u8> = Vec::new();
    let mut grid_node: Vec<usize> = Vec::new();
    let mut conn: Vec<[u32; 8]> = Vec::new();
    let mut emat: Vec<u32> = Vec::new();
    let mut egrid: Vec<usize> = Vec::new();
    for c in 0..mat.len() {
        if !active(mat[c]) {
            continue;
        }
        let corners = g.corners(c).map(|v| {
            if node_of[v] == u32::MAX {
                node_of[v] = points.len() as u32;
                let p = g.node_ijk(v);
                points.push(std::array::from_fn(|a| if p[a] == n[a] { o.hi[a] } else { o.lo[a] + p[a] as f64 * h[a] }));
                let mut f = 0u8;
                for a in 0..3 {
                    if p[a] == 0 {
                        f |= 1 << (2 * a);
                    }
                    if p[a] == n[a] {
                        f |= 1 << (2 * a + 1);
                    }
                }
                flags.push(f);
                grid_node.push(v);
            }
            node_of[v]
        });
        conn.push(corners);
        emat.push(mat[c]);
        egrid.push(c);
    }
    let n_grid_nodes = points.len();
    // Materials around each grid node (void included).
    let sig: Vec<Vec<u32>> = grid_node
        .iter()
        .map(|v| {
            let s: BTreeSet<u32> = g.around(*v).iter().map(|(c, _)| mat[*c]).collect();
            s.into_iter().collect()
        })
        .collect();
    let on_surface = |k: usize| k < n_grid_nodes && sig[k].len() >= 2;
    // Stair-step normal of each grid node: the sum over the faces between two materials
    // around it of their direction, from the lower material to the higher.
    let mut snormal: Vec<P3> = vec![[0.0; 3]; n_grid_nodes];
    for c in 0..mat.len() {
        let [i, j, k] = g.ijk(c);
        for a in 0..3 {
            let mut q = [i, j, k];
            q[a] += 1;
            if q[a] >= n[a] {
                continue;
            }
            let d = g.cell(q[0], q[1], q[2]);
            if mat[c] == mat[d] {
                continue;
            }
            let sgn = if mat[c] < mat[d] { 1.0 } else { -1.0 };
            // The four grid nodes of the shared face.
            let (b1, b2) = ((a + 1) % 3, (a + 2) % 3);
            for (u, w) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                let mut r = [i, j, k];
                r[a] += 1;
                r[b1] += u;
                r[b2] += w;
                let v = node_of[g.node(r[0], r[1], r[2])];
                if v != u32::MAX {
                    snormal[v as usize][a] += sgn;
                }
            }
        }
    }
    let fixed_box = |f: u8| o.periodic && f != 0;

    // Pillow: a layer of hexahedra inside each material's boundary.
    let mut inward: Vec<(u32, u32, P3, u32)> = Vec::new();
    if o.pillow {
        let mats: BTreeSet<u32> = emat.iter().copied().collect();
        for m in mats {
            let region: Vec<usize> = (0..conn.len()).filter(|e| emat[*e] == m && egrid[*e] != usize::MAX).collect();
            let mut faces: Vec<([u32; 4], u8)> = Vec::new();
            for &e in &region {
                for s in 1..=6u8 {
                    if let Some(d) = g.across(egrid[e], s) {
                        if mat[d] != m {
                            faces.push((HEX8_SIDES[s as usize - 1].map(|k| conn[e][k]), s));
                        }
                    }
                }
            }
            if faces.is_empty() {
                continue;
            }
            let mut new_of: HashMap<u32, u32> = HashMap::new();
            let mut normal: HashMap<u32, P3> = HashMap::new();
            for (f, s) in &faces {
                let (a, d) = SIDE_DIR[*s as usize - 1];
                for v in f {
                    let id = *new_of.entry(*v).or_insert_with(|| {
                        points.push(points[*v as usize]);
                        flags.push(flags[*v as usize]);
                        (points.len() - 1) as u32
                    });
                    normal.entry(id).or_insert([0.0; 3])[a] += d as f64;
                }
            }
            for &e in &region {
                for k in 0..8 {
                    if let Some(nv) = new_of.get(&conn[e][k]) {
                        conn[e][k] = *nv;
                    }
                }
            }
            for (f, _) in &faces {
                conn.push([new_of[&f[0]], new_of[&f[1]], new_of[&f[2]], new_of[&f[3]], f[0], f[1], f[2], f[3]]);
                emat.push(m);
                egrid.push(usize::MAX);
            }
            for (old, new) in &new_of {
                inward.push((*new, *old, normal[new], m));
            }
        }
    }

    tick("pillowed");
    // Edges and elements of each node.
    let nn = points.len();
    let mut elems_of: Vec<Vec<u32>> = vec![Vec::new(); nn];
    let mut nbr: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); nn];
    for (e, c) in conn.iter().enumerate() {
        for v in c {
            elems_of[*v as usize].push(e as u32);
        }
        for (a, b) in HEX_EDGES {
            nbr[c[a] as usize].insert(c[b]);
            nbr[c[b] as usize].insert(c[a]);
        }
    }

    // Projection of a point onto the interface of the materials `mats`, moving only along
    // axes not fixed by the domain box.
    let has_distance = src.distance(1, o.lo).is_some();
    let fval = |a: u32, b: u32, p: P3| -> f64 { 0.5 * (src.distance(a, p).unwrap_or(0.0) - src.distance(b, p).unwrap_or(0.0)) };
    // Newton steps on the distance functions (nodes of three or more materials, and a
    // fallback), moving only along axes not fixed by the domain box.
    let newton = |mats: &[u32], f: u8, p0: P3| -> P3 {
        let free: [bool; 3] = std::array::from_fn(|a| f & (3 << (2 * a)) == 0);
        let eps = 1e-4 * hmin;
        let mut p = p0;
        for _ in 0..12 {
            let mut moved = 0.0;
            for &b in &mats[1..] {
                let a = mats[0];
                let v = fval(a, b, p);
                let gr: P3 = std::array::from_fn(|k| {
                    if !free[k] {
                        return 0.0;
                    }
                    let (mut x, mut y) = (p, p);
                    x[k] += eps;
                    y[k] -= eps;
                    (fval(a, b, x) - fval(a, b, y)) / (2.0 * eps)
                });
                let gg = gr[0] * gr[0] + gr[1] * gr[1] + gr[2] * gr[2];
                if gg < 1e-12 {
                    continue;
                }
                let mut step: P3 = gr.map(|x| -v * x / gg);
                let l = (step[0] * step[0] + step[1] * step[1] + step[2] * step[2]).sqrt();
                if l > 0.5 * hmax {
                    step = step.map(|x| x * 0.5 * hmax / l);
                }
                for k in 0..3 {
                    p[k] += step[k];
                }
                moved += l;
            }
            if moved < 1e-12 * hmax {
                break;
            }
        }
        p
    };
    // Projection of grid node `k` (at `p0`) onto the interface of its materials: along its
    // stair-step normal to the nearest crossing within a cell, else by Newton steps; never
    // further than a cell.
    let project = |k: usize, p0: P3| -> P3 {
        let mats = &sig[k];
        let f = flags[k];
        if !has_distance || mats.len() < 2 {
            return p0;
        }
        let mut nv: P3 = std::array::from_fn(|a| if f & (3 << (2 * a)) != 0 { 0.0 } else { snormal[k][a] });
        let l = (nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]).sqrt();
        let mut p = None;
        if mats.len() == 2 && l > 0.0 {
            nv = nv.map(|x| x / l);
            let at = |t: f64| -> P3 { std::array::from_fn(|a| p0[a] + t * nv[a]) };
            let fv = |t: f64| fval(mats[0], mats[1], at(t));
            let f0 = fv(0.0);
            if f0 == 0.0 {
                return p0;
            }
            let steps = 16;
            'search: for k in 1..=steps {
                for sgn in [1.0, -1.0] {
                    let (t0, t1) = (sgn * hmax * (k - 1) as f64 / steps as f64, sgn * hmax * k as f64 / steps as f64);
                    let (mut a, mut b) = (t0, t1);
                    let (fa, fb) = (fv(a), fv(b));
                    if fa == 0.0 {
                        p = Some(at(a));
                        break 'search;
                    }
                    if fa * fb < 0.0 {
                        let mut fa = fa;
                        for _ in 0..40 {
                            let m = 0.5 * (a + b);
                            let fm = fv(m);
                            if fm * fa <= 0.0 {
                                b = m;
                            } else {
                                a = m;
                                fa = fm;
                            }
                        }
                        p = Some(at(0.5 * (a + b)));
                        break 'search;
                    }
                }
            }
        }
        let p = p.unwrap_or_else(|| newton(mats, f, p0));
        let d: P3 = std::array::from_fn(|a| p[a] - p0[a]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l > hmax {
            return std::array::from_fn(|a| p0[a] + d[a] * hmax / l);
        }
        p
    };

    if o.project && !stair {
        if has_distance {
            let moved: Vec<(usize, P3)> = (0..n_grid_nodes)
                .into_par_iter()
                .filter(|k| on_surface(*k) && !fixed_box(flags[*k]))
                .map(|k| (k, project(k, points[k])))
                .collect();
            for (k, p) in moved {
                points[k] = p;
            }
        } else {
            // No distances (voxels): the interface is smoothed below, with its pillow layer.
        }
    }
    tick("projected");
    // Sharp edges: where two surface nodes joined by an edge lie on different geometry faces,
    // the point between them where the face changes (found by bisection on the surface) is on
    // the edge between the faces; the nearer node moves there and stays (nearest first, and
    // not within half a cell of a neighbour already moved).
    let mut pinned = vec![false; points.len()];
    if has_distance && o.project && !stair && o.curves {
        let body = |k: usize| -> Option<u32> { (sig[k].len() == 2).then(|| sig[k].iter().copied().find(|m| *m != 0)).flatten() };
        let fid: Vec<u32> = (0..n_grid_nodes).into_par_iter().map(|k| if on_surface(k) && flags[k] == 0 { body(k).map(|m| src.face(m, points[k])).unwrap_or(0) } else { 0 }).collect();
        let mut cands: Vec<(f64, usize, P3)> = (0..n_grid_nodes)
            .into_par_iter()
            .filter(|k| fid[*k] > 0)
            .flat_map_iter(|k| {
                let m = body(k).unwrap();
                let (fid, sig, nbr, points) = (&fid, &sig, &nbr, &points);
                nbr[k].iter().copied().filter(move |w| (*w as usize) > k && (*w as usize) < n_grid_nodes && fid[*w as usize] > 0 && fid[*w as usize] != fid[k] && sig[*w as usize] == sig[k]).map(move |w| {
                    let (a, b) = (points[k], points[w as usize]);
                    let at = |t: f64| -> P3 { newton(&sig[k], 0, std::array::from_fn(|i| a[i] + t * (b[i] - a[i]))) };
                    let (mut lo, mut hi) = (0.0, 1.0);
                    for _ in 0..30 {
                        let t = 0.5 * (lo + hi);
                        if src.face(m, at(t)) == fid[k] {
                            lo = t;
                        } else {
                            hi = t;
                        }
                    }
                    let e = at(0.5 * (lo + hi));
                    let dk = (0..3).map(|i| (e[i] - a[i]).powi(2)).sum::<f64>().sqrt();
                    let dw = (0..3).map(|i| (e[i] - b[i]).powi(2)).sum::<f64>().sqrt();
                    // Only convex edges: stepping inward across one face and outward across the
                    // other leaves the material.
                    let grad = |p: P3| -> P3 {
                        let h = 1e-4 * hmin;
                        let gr: P3 = std::array::from_fn(|i| {
                            let (mut x, mut y) = (p, p);
                            x[i] += h;
                            y[i] -= h;
                            (src.distance(m, x).unwrap_or(0.0) - src.distance(m, y).unwrap_or(0.0)) / (2.0 * h)
                        });
                        let l = (gr[0] * gr[0] + gr[1] * gr[1] + gr[2] * gr[2]).sqrt().max(1e-12);
                        gr.map(|x| x / l)
                    };
                    let (na, nb) = (grad(a), grad(b));
                    let eps = 0.2 * hmin;
                    let q1: P3 = std::array::from_fn(|i| e[i] - eps * na[i] + eps * nb[i]);
                    let q2: P3 = std::array::from_fn(|i| e[i] + eps * na[i] - eps * nb[i]);
                    let convex = src.material(q1) != m || src.material(q2) != m;
                    let d = if convex { if dk <= dw { dk } else { dw } } else { f64::INFINITY };
                    if dk <= dw { (d, k, e) } else { (d, w as usize, e) }
                })
            })
            .collect();
        cands.sort_by(|x, y| x.0.total_cmp(&y.0));
        for (d, v, e) in cands {
            let (dmax, gap) = (0.5, 0.7);
            if pinned[v] || d > dmax * hmax {
                continue;
            }
            let close = nbr[v].iter().any(|u| pinned[*u as usize] && (0..3).map(|i| (points[*u as usize][i] - e[i]).powi(2)).sum::<f64>() < (gap * hmin).powi(2));
            if close {
                continue;
            }
            points[v] = e;
            pinned[v] = true;
        }
        // Corners: where a node and its surface neighbours lie on three faces, the meeting
        // point of the three tangent planes (each through a neighbour on that face, normal to
        // the distance gradient there); the nearest node goes there.
        let mut corners: Vec<(f64, usize, P3)> = (0..n_grid_nodes)
            .into_par_iter()
            .filter(|k| fid[*k] > 0)
            .filter_map(|k| {
                let m = body(k)?;
                // Faces met within two edges along the surface (nearest first).
                let mut by_face: Vec<(u32, usize)> = vec![(fid[k], k)];
                let ring1: Vec<usize> = nbr[k].iter().map(|w| *w as usize).filter(|w| *w < n_grid_nodes && fid[*w] > 0 && sig[*w] == sig[k]).collect();
                let ring2: Vec<usize> = ring1.iter().flat_map(|w| nbr[*w].iter().map(|u| *u as usize)).filter(|w| *w < n_grid_nodes && fid[*w] > 0 && sig[*w] == sig[k]).collect();
                for w in ring1.into_iter().chain(ring2) {
                    if !by_face.iter().any(|x| x.0 == fid[w]) {
                        by_face.push((fid[w], w));
                    }
                }
                if by_face.len() < 3 {
                    return None;
                }
                let planes: Vec<(P3, f64)> = by_face[..3]
                    .iter()
                    .map(|(_, w)| {
                        let p = grid_free_point(&points, *w);
                        let h = 1e-4 * hmin;
                        let gr: P3 = std::array::from_fn(|i| {
                            let (mut x, mut y) = (p, p);
                            x[i] += h;
                            y[i] -= h;
                            (src.distance(m, x).unwrap_or(0.0) - src.distance(m, y).unwrap_or(0.0)) / (2.0 * h)
                        });
                        let l = (gr[0] * gr[0] + gr[1] * gr[1] + gr[2] * gr[2]).sqrt().max(1e-12);
                        let n = gr.map(|x| x / l);
                        (n, n[0] * p[0] + n[1] * p[1] + n[2] * p[2])
                    })
                    .collect();
                let x = solve3(&planes)?;
                let d = (0..3).map(|i| (x[i] - points[k][i]).powi(2)).sum::<f64>().sqrt();
                (d < 1.2 * hmax && src.distance(m, x).is_some_and(|dd| dd.abs() < 0.05 * hmin)).then_some((d, k, x))
            })
            .collect();
        corners.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut taken: Vec<P3> = Vec::new();
        for (_, v, x) in corners {
            if taken.iter().any(|t| (0..3).map(|i| (t[i] - x[i]).powi(2)).sum::<f64>() < (0.7 * hmin).powi(2)) {
                continue;
            }
            taken.push(x);
            points[v] = x;
            pinned[v] = true;
        }
    }
    // The inner pillow nodes: half way between the surface point of their outer node and the
    // interior nodes they are joined to (a fraction of a cell inside along the surface normal
    // when they have none).
    let interior = |j: u32| (j as usize) < n_grid_nodes && !on_surface(j as usize);
    // Grid positions (before any move) of every node, for the cell centres.
    let grid_at: Vec<P3> = (0..points.len()).map(|k| if k < n_grid_nodes { let q = g.node_ijk(grid_node[k]); std::array::from_fn(|a| o.lo[a] + q[a] as f64 * h[a]) } else { [f64::NAN; 3] }).collect();
    let pillow_twin: HashMap<u32, u32> = inward.iter().map(|x| (x.0, x.1)).collect();
    let grid_ref = |k: u32, a: usize| -> f64 { let k = pillow_twin.get(&k).copied().unwrap_or(k); grid_at[k as usize][a] };
    let inner: Vec<(u32, P3)> = inward
        .iter()
        .map(|(v, outer, nrm, m)| {
            let f = flags[*v as usize];
            let base = points[*outer as usize];
            let ins: Vec<u32> = nbr[*v as usize].iter().copied().filter(|j| interior(*j)).collect();
            if !ins.is_empty() && has_distance {
                let c: P3 = std::array::from_fn(|a| ins.iter().map(|j| points[*j as usize][a]).sum::<f64>() / ins.len() as f64);
                return (*v, std::array::from_fn(|a| if f & (3 << (2 * a)) != 0 { base[a] } else { 0.5 * (base[a] + c[a]) }));
            }
            let mut dir = *nrm;
            if has_distance {
                let e = 1e-4 * hmin;
                let gr: P3 = std::array::from_fn(|k| {
                    let (mut x, mut y) = (base, base);
                    x[k] += e;
                    y[k] -= e;
                    (src.distance(*m, x).unwrap_or(0.0) - src.distance(*m, y).unwrap_or(0.0)) / (2.0 * e)
                });
                if gr.iter().map(|x| x * x).sum::<f64>() > 1e-12 && gr[0] * nrm[0] + gr[1] * nrm[1] + gr[2] * nrm[2] > 0.0 {
                    dir = gr;
                }
            }
            let nv: P3 = std::array::from_fn(|a| if f & (3 << (2 * a)) != 0 { 0.0 } else { dir[a] });
            let l = (nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]).sqrt();
            if l > 0.0 && has_distance {
                return (*v, std::array::from_fn(|a| base[a] - nv[a] / l * 0.25 * hmin));
            }
            // Towards the centres of the material's grid cells around the node.
            let cells: Vec<&[u32; 8]> = elems_of[*v as usize].iter().filter(|e| egrid[**e as usize] != usize::MAX && emat[**e as usize] == *m).map(|e| &conn[*e as usize]).collect();
            if cells.is_empty() {
                return (*v, base);
            }
            let c: P3 = std::array::from_fn(|a| cells.iter().map(|c| c.iter().map(|k| grid_ref(*k, a)).sum::<f64>() / 8.0).sum::<f64>() / cells.len() as f64);
            let t = 0.3;
            (*v, std::array::from_fn(|a| if f & (3 << (2 * a)) != 0 { base[a] } else { base[a] + t * (c[a] - base[a]) }))
        })
        .collect();
    for (v, p) in inner {
        points[v as usize] = p;
    }

    // Smoothing with a guard: a move is kept only if the elements around stay valid. Moves
    // are evaluated with trial positions substituted, and run in parallel over colour classes
    // of nodes (or node groups) that share no element, so a class moves at once.
    let subst = |pts: &[P3], subs: &[(usize, P3)], k: u32| -> P3 { subs.iter().find(|s| s.0 == k as usize).map(|s| s.1).unwrap_or(pts[k as usize]) };
    let around = |subs: &[(usize, P3)]| -> Vec<u32> {
        let mut es: Vec<u32> = subs.iter().flat_map(|s| elems_of[s.0].iter().copied()).collect();
        es.sort_unstable();
        es.dedup();
        es
    };
    let worst_with = |pts: &[P3], subs: &[(usize, P3)]| -> f64 { around(subs).iter().map(|e| corner_sj(&conn[*e as usize].map(|k| subst(pts, subs, k)))).fold(f64::INFINITY, f64::min) };
    let energy_with = |pts: &[P3], subs: &[(usize, P3)]| -> f64 {
        around(subs).iter().map(|e| corner_sjs(&conn[*e as usize].map(|k| subst(pts, subs, k))).iter().map(|q| (0.2 - q).max(0.0).powi(2)).sum::<f64>()).sum()
    };
    let accept = |before: f64, after: f64, floor: f64| after > before + 1e-12 || (after > 0.0 && after >= before.min(floor));
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    let pinned: Vec<AtomicBool> = pinned.into_iter().map(AtomicBool::new).collect();
    let movable = |v: usize| -> bool { !stair && !fixed_box(flags[v]) && !pinned[v].load(Relaxed) };
    let untangle_movable = |v: usize| -> bool { !stair && !fixed_box(flags[v]) };
    let surface_node = |v: usize| on_surface(v) && o.project;
    let constrain = |v: usize, p: P3, old: P3| -> P3 { std::array::from_fn(|a| if flags[v] & (3 << (2 * a)) != 0 { old[a] } else { p[a] }) };
    // Colour classes of groups of nodes: no two groups of a class touch the same element.
    let colour = |groups: &[Vec<u32>]| -> Vec<Vec<usize>> {
        let mut owner = vec![u32::MAX; nn];
        for (g, nodes) in groups.iter().enumerate() {
            for v in nodes {
                owner[*v as usize] = g as u32;
            }
        }
        let mut col = vec![usize::MAX; groups.len()];
        let mut classes: Vec<Vec<usize>> = Vec::new();
        let mut used: Vec<usize> = Vec::new();
        for (g, nodes) in groups.iter().enumerate() {
            used.clear();
            for v in nodes {
                for e in &elems_of[*v as usize] {
                    for w in conn[*e as usize] {
                        let o = owner[w as usize];
                        if o != u32::MAX && o as usize != g && col[o as usize] != usize::MAX {
                            used.push(col[o as usize]);
                        }
                    }
                }
            }
            let c = (0..).find(|c| !used.contains(c)).unwrap();
            col[g] = c;
            if classes.len() <= c {
                classes.resize(c + 1, Vec::new());
            }
            classes[c].push(g);
        }
        classes
    };
    let singles: Vec<Vec<u32>> = (0..nn as u32).map(|v| vec![v]).collect();
    let classes = if stair { Vec::new() } else { colour(&singles) };
    // One sweep: each node of `pick` proposes a position; the proposals of a class apply at once.
    let sweep = |points: &mut Vec<P3>, pick: &(dyn Fn(usize) -> bool + Sync), propose: &(dyn Fn(&[P3], usize) -> Option<P3> + Sync)| {
        for class in &classes {
            let moves: Vec<(usize, P3)> = class.par_iter().filter(|v| pick(**v)).filter_map(|v| propose(points, *v).map(|p| (*v, p))).collect();
            for (v, p) in moves {
                points[v] = p;
            }
        }
    };
    if !stair && o.project && !has_distance {
        // Voxels: the interface nodes move along the interface (Taubin smoothing over their
        // neighbours with the same materials), carrying their pillow twins with them; a move
        // is kept only while the elements around stay valid.
        let mut twins: HashMap<u32, Vec<u32>> = HashMap::new();
        for (inner, outer, _, _) in &inward {
            twins.entry(*outer).or_default().push(*inner);
        }
        let snb: Vec<Vec<u32>> = (0..n_grid_nodes)
            .map(|k| if on_surface(k) { nbr[k].iter().copied().filter(|j| (*j as usize) < n_grid_nodes && sig[*j as usize] == sig[k]).collect() } else { Vec::new() })
            .collect();
        let heads: Vec<usize> = (0..n_grid_nodes).filter(|k| on_surface(*k) && snb[*k].len() >= 2 && !fixed_box(flags[*k])).collect();
        let groups: Vec<Vec<u32>> = heads.iter().map(|k| std::iter::once(*k as u32).chain(twins.get(&(*k as u32)).into_iter().flatten().copied()).collect()).collect();
        let gclasses = colour(&groups);
        for it in 0..30 {
            let lam = if it % 2 == 0 { 0.5 } else { -0.53 };
            for class in &gclasses {
                let moves: Vec<Vec<(usize, P3)>> = class
                    .par_iter()
                    .filter_map(|g| {
                        let k = heads[*g];
                        let c: P3 = std::array::from_fn(|a| snb[k].iter().map(|j| points[*j as usize][a]).sum::<f64>() / snb[k].len() as f64);
                        let p = points[k];
                        let d: P3 = std::array::from_fn(|a| if flags[k] & (3 << (2 * a)) != 0 { 0.0 } else { lam * (c[a] - p[a]) });
                        let subs: Vec<(usize, P3)> = groups[*g]
                            .iter()
                            .map(|v| {
                                let f = flags[*v as usize];
                                let q = points[*v as usize];
                                (*v as usize, std::array::from_fn(|a| if f & (3 << (2 * a)) == 0 { q[a] + d[a] } else { q[a] }))
                            })
                            .collect();
                        let old: Vec<(usize, P3)> = subs.iter().map(|s| (s.0, points[s.0])).collect();
                        let b = worst_with(&points, &old);
                        let after = worst_with(&points, &subs);
                        (after > 0.1 || after > b + 1e-12).then_some(subs)
                    })
                    .collect();
                for m in moves {
                    for (v, p) in m {
                        points[v] = p;
                    }
                }
            }
        }
    }
    tick("interface smoothed");
    if !stair {
        for _ in 0..o.smoothing {
            sweep(&mut points, &|v| movable(v) && !surface_node(v), &|pts, v| {
                let k = nbr[v].len() as f64;
                let c: P3 = std::array::from_fn(|a| nbr[v].iter().map(|j| pts[*j as usize][a]).sum::<f64>() / k);
                let p = constrain(v, c, pts[v]);
                accept(worst_with(pts, &[(v, pts[v])]), worst_with(pts, &[(v, p)]), 0.5).then_some(p)
            });
            // Surface nodes: towards their surface neighbours, back onto the surface.
            if has_distance && o.project {
                sweep(&mut points, &|v| v < n_grid_nodes && movable(v) && surface_node(v), &|pts, v| {
                    let same: Vec<u32> = nbr[v].iter().copied().filter(|j| (*j as usize) < n_grid_nodes && sig[*j as usize] == sig[v]).collect();
                    if same.len() < 2 {
                        return None;
                    }
                    let c: P3 = std::array::from_fn(|a| same.iter().map(|j| pts[*j as usize][a]).sum::<f64>() / same.len() as f64);
                    let p = project(v, constrain(v, c, pts[v]));
                    accept(worst_with(pts, &[(v, pts[v])]), worst_with(pts, &[(v, p)]), 0.5).then_some(p)
                });
            }
        }
        tick("laplacian");
        // A node put on a sharp edge whose elements stay poor goes back to the plain surface.
        let rel = 0.25;
        (0..nn).into_par_iter().filter(|v| pinned[*v].load(Relaxed) && worst_with(&points, &[(*v, points[*v])]) < rel).for_each(|v| pinned[v].store(false, Relaxed));
        // Optimisation in two tiers: nodes of elements below the target try small moves along
        // the axes; nodes of elements below 0.3 then search 26 directions with shrinking
        // steps. Moves keep the worst element around a node rising; surface nodes stay on
        // the surface.
        let dirs = directions();
        let tiers: [(f64, usize, usize); 2] = [(o.sj_target, 3, 6), (0.3_f64.min(o.sj_target), 9, 26)];
        for (target, rounds, nd) in tiers {
            let dset: Vec<P3> = if nd == 6 { vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] } else { dirs.clone() };
            for round in 0..rounds {
                let step = 0.2 * hmin / (1 << (round / 3)) as f64;
                let mut poor = vec![false; nn];
                let bad: Vec<usize> = (0..conn.len()).into_par_iter().filter(|e| corner_sj(&conn[*e].map(|k| points[k as usize])) < target).collect();
                if bad.is_empty() {
                    break;
                }
                for e in bad {
                    for v in conn[e] {
                        poor[v as usize] = true;
                    }
                }
                sweep(&mut points, &|v| poor[v] && movable(v), &|pts, v| {
                    let mut best = (worst_with(pts, &[(v, pts[v])]), None);
                    for d in &dset {
                        let p = constrain(v, std::array::from_fn(|a| pts[v][a] + d[a] * step), pts[v]);
                        let p = if surface_node(v) { project(v, p) } else { p };
                        let q = worst_with(pts, &[(v, p)]);
                        if q > best.0 + 1e-9 {
                            best = (q, Some(p));
                        }
                    }
                    best.1
                });
            }
        }
    }
    tick("smoothed");
    // Untangling: nodes of elements that are still flat or inverted (and, every other round,
    // the nodes one element further out) search, without the surface constraint, for the
    // position that most lowers an untangling energy: how far the corners around them fall
    // below 0.2.
    if !stair {
        let dirs = directions();
        for round in 0..30 {
            let worst: Vec<usize> = (0..conn.len()).into_par_iter().filter(|e| corner_sj(&conn[*e].map(|k| points[k as usize])) < 0.05).collect();
            if worst.is_empty() {
                break;
            }
            let mut bad = vec![false; nn];
            for e in &worst {
                for v in conn[*e] {
                    bad[v as usize] = true;
                    if round % 2 == 1 {
                        for e2 in &elems_of[v as usize] {
                            for w in conn[*e2 as usize] {
                                bad[w as usize] = true;
                            }
                        }
                    }
                }
            }
            let step = 0.3 * hmin / (1 << ((round % 10) / 3)) as f64;
            sweep(&mut points, &|v| bad[v] && untangle_movable(v), &|pts, v| {
                let mut best = (energy_with(pts, &[(v, pts[v])]), None);
                for d in &dirs {
                    for f in [1.0, 0.5, 0.25] {
                        let p = constrain(v, std::array::from_fn(|a| pts[v][a] + d[a] * step * f), pts[v]);
                        let q = energy_with(pts, &[(v, p)]);
                        if q < best.0 - 1e-12 {
                            best = (q, Some(p));
                        }
                    }
                }
                best.1
            });
        }
    }

    tick("untangled");
    // Assemble, with blocks per material and the sets.
    let void_id = emat.iter().copied().max().unwrap_or(0) + 1;
    let block_of = |m: u32| -> i32 { if m == 0 { void_id as i32 } else { m as i32 } };
    let mut mesh = HexMesh { npe: 8, points, ..Default::default() };
    for (e, c) in conn.iter().enumerate() {
        mesh.cells.extend(c);
        mesh.block.push(block_of(emat[e]));
    }
    for m in emat.iter().copied().collect::<BTreeSet<u32>>() {
        let name = if m == 0 { "void".to_string() } else { o.names.get(&m).cloned().unwrap_or_else(|| format!("material {m}")) };
        mesh.block_names.insert(block_of(m), name);
    }
    let mut faces: HashMap<[u32; 4], Vec<(u32, u8)>> = HashMap::new();
    for e in 0..conn.len() {
        for s in 1..=6u8 {
            let mut k = HEX8_SIDES[s as usize - 1].map(|i| conn[e][i]);
            k.sort_unstable();
            faces.entry(k).or_default().push((e as u32, s));
        }
    }
    let mut sets: BTreeMap<String, Vec<(u32, u8)>> = BTreeMap::new();
    let names = ["x-", "x+", "y-", "y+", "z-", "z+"];
    let mut keys: Vec<&[u32; 4]> = faces.keys().collect();
    keys.sort_unstable();
    for k in keys {
        let f = &faces[k];
        if f.len() == 1 {
            let (e, s) = f[0];
            let common = k.iter().fold(0xffu8, |acc, v| acc & mesh_flag(&flags, *v));
            if common != 0 {
                let bit = common.trailing_zeros() as usize;
                sets.entry(names[bit].to_string()).or_default().push((e, s));
            } else {
                let c: P3 = std::array::from_fn(|a| k.iter().map(|v| mesh.points[*v as usize][a]).sum::<f64>() / 4.0);
                let fid = src.face(emat[e as usize], c) as usize;
                let name = o.face_names.get(fid).filter(|n| fid > 0 && !n.is_empty()).cloned().unwrap_or_else(|| format!("surface {}", mesh.block_names[&block_of(emat[e as usize])]));
                sets.entry(name).or_default().push((e, s));
            }
        } else if f.len() == 2 {
            let (a, b) = (emat[f[0].0 as usize], emat[f[1].0 as usize]);
            if a != b {
                let (lo, hi) = if block_of(a) < block_of(b) { (f[0], b) } else { (f[1], a) };
                let name = format!("interface {} - {}", mesh.block_names[&block_of(emat[lo.0 as usize])], mesh.block_names[&block_of(hi)]);
                sets.entry(name).or_default().push(lo);
            }
        }
    }
    for (name, mut fs) in sets {
        fs.sort_unstable();
        let nodes: Vec<u32> = fs.iter().flat_map(|(e, s)| HEX8_SIDES[*s as usize - 1].map(|k| conn[*e as usize][k])).collect();
        mesh.add_side_set(&name, fs);
        mesh.add_node_set(&name, nodes);
    }
    // Drop nodes no element uses (none expected), then periodic pairs.
    if o.periodic {
        mesh.pair_periodic(o.lo, o.hi, 1e-9 * hmin).map_err(|e| format!("periodic pairs: {e}"))?;
    }
    let sj: Vec<f64> = conn.iter().map(|c| corner_sj(&c.map(|k| mesh.points[k as usize]))).collect();
    let min_sj = sj.iter().cloned().fold(f64::INFINITY, f64::min);
    let inverted = sj.iter().filter(|q| **q <= 0.0).count();
    Ok(Overlay { mesh, min_sj, inverted })
}

fn mesh_flag(flags: &[u8], v: u32) -> u8 {
    flags[v as usize]
}

/// The 26 unit directions to the neighbours of a cube's centre.
fn directions() -> Vec<P3> {
    let mut d = Vec::new();
    for x in -1i32..=1 {
        for y in -1i32..=1 {
            for z in -1i32..=1 {
                if (x, y, z) != (0, 0, 0) {
                    let l = ((x * x + y * y + z * z) as f64).sqrt();
                    d.push([x as f64 / l, y as f64 / l, z as f64 / l]);
                }
            }
        }
    }
    d
}

/// The current position of a node (a helper keeping closures short).
fn grid_free_point(points: &[P3], k: usize) -> P3 {
    points[k]
}

/// The point where three planes (normal, offset) meet, if they are not nearly parallel.
fn solve3(pl: &[(P3, f64)]) -> Option<P3> {
    let (a, b, c) = (pl[0].0, pl[1].0, pl[2].0);
    let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
    if det.abs() < 0.2 {
        return None;
    }
    let d = [pl[0].1, pl[1].1, pl[2].1];
    let col = |k: usize| -> f64 {
        let m = |r: usize, cidx: usize| -> f64 { if cidx == k { d[r] } else { [a, b, c][r][cidx] } };
        m(0, 0) * (m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1)) - m(0, 1) * (m(1, 0) * m(2, 2) - m(1, 2) * m(2, 0)) + m(0, 2) * (m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0))
    };
    Some([col(0) / det, col(1) / det, col(2) / det])
}
