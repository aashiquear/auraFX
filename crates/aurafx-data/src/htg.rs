//! Hyper tree grids (VTK's vtkHyperTreeGrid): a rectilinear grid of root cells, each the root
//! of a tree whose refined cells split into branch_factor^dimension children. Every vertex of
//! every tree (coarse or leaf) is a cell with a global index into the cell data; a mask can
//! hide vertices (and everything below them).

use crate::{Array, ArrayData, Attributes, Bounds, CellType, Cells, Points, UnstructuredGrid};
use std::collections::HashMap;

/// One tree, its vertices in breadth-first order (children of a refined vertex together,
/// x fastest, in the order of their parents).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HyperTree {
    pub refined: Vec<bool>,
    /// Global (cell data) index of each vertex.
    pub global: Vec<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HyperTreeGrid {
    /// Point coordinates of the root grid along x, y, z (one value along a flat axis).
    pub coords: [Vec<f64>; 3],
    pub branch_factor: usize,
    /// Root cells numbered k fastest instead of i fastest.
    pub transposed: bool,
    /// One per root cell (None: no tree there).
    pub trees: Vec<Option<HyperTree>>,
    /// Masked vertices, by global index.
    pub mask: Option<Vec<bool>>,
    /// Material interfaces (VTK's InterfaceNormalsName, InterfaceInterceptsName): the names of
    /// the cell arrays holding each cell's plane normal and its (d1, d2, type) intercepts.
    #[serde(default)]
    pub interface: Option<(String, String)>,
    pub cell_data: Attributes,
    pub field_data: Attributes,
}

/// A vertex met while walking a tree.
#[derive(Debug, Clone, Copy)]
pub struct Visit {
    pub global: usize,
    pub depth: usize,
    pub leaf: bool,
    pub masked: bool,
    pub origin: [f64; 3],
    pub size: [f64; 3],
    /// Lower corner on the integer lattice of the finest level (`lattice_depth`).
    pub lattice: [i64; 3],
}

impl HyperTreeGrid {
    /// Points per axis of the root grid.
    pub fn dims(&self) -> [usize; 3] {
        [self.coords[0].len(), self.coords[1].len(), self.coords[2].len()]
    }
    /// Root cells per axis (1 along a flat axis).
    pub fn cell_dims(&self) -> [usize; 3] {
        self.dims().map(|n| n.saturating_sub(1).max(1))
    }
    /// The axes with more than one point (the grid's dimension is their number).
    pub fn axes(&self) -> Vec<usize> {
        (0..3).filter(|&k| self.coords[k].len() > 1).collect()
    }
    pub fn dimension(&self) -> usize {
        self.axes().len()
    }
    pub fn children_per_vertex(&self) -> usize {
        self.branch_factor.pow(self.dimension() as u32)
    }
    /// Number of cells (vertices of all trees, coarse and leaves).
    pub fn n_vertices(&self) -> usize {
        self.trees.iter().flatten().map(|t| t.refined.len()).sum()
    }
    pub fn is_masked(&self, global: usize) -> bool {
        self.mask.as_ref().is_some_and(|m| m.get(global).copied().unwrap_or(false))
    }
    /// Root cell (i, j, k) of tree `index`.
    pub fn root_ijk(&self, index: usize) -> [usize; 3] {
        let c = self.cell_dims();
        if self.transposed {
            [index / (c[1] * c[2]), (index / c[2]) % c[1], index % c[2]]
        } else {
            [index % c[0], (index / c[0]) % c[1], index / (c[0] * c[1])]
        }
    }
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::default();
        let lo: [f64; 3] = std::array::from_fn(|k| self.coords[k].iter().cloned().fold(f64::INFINITY, f64::min));
        let hi: [f64; 3] = std::array::from_fn(|k| self.coords[k].iter().cloned().fold(f64::NEG_INFINITY, f64::max));
        if lo.iter().chain(&hi).all(|v| v.is_finite()) {
            b.add_point(lo);
            b.add_point(hi);
        }
        b
    }
    /// Deepest level of any tree (0 for root cells only).
    pub fn max_depth(&self) -> usize {
        self.trees.iter().flatten().map(|t| tree_depth(t, self.children_per_vertex())).max().unwrap_or(0)
    }

    /// Walk every tree depth first (trees in index order, children in order), calling `f` for
    /// each vertex; a vertex for which `f` returns false is not descended into. Masked vertices
    /// are visited (with `masked`) but not descended into.
    pub fn walk(&self, f: impl FnMut(&Visit) -> bool) {
        self.walk_impl(false, f)
    }

    /// As `walk`, but also descending into masked vertices (VTK's cursors follow the trees'
    /// structure whatever the mask).
    pub fn walk_structure(&self, f: impl FnMut(&Visit) -> bool) {
        self.walk_impl(true, f)
    }

    fn walk_impl(&self, into_masked: bool, mut f: impl FnMut(&Visit) -> bool) {
        let nc = self.children_per_vertex();
        let axes = self.axes();
        let bf = self.branch_factor.max(2) as i64;
        let depth_max = self.max_depth();
        let scale = bf.pow(depth_max as u32);
        for (ti, tree) in self.trees.iter().enumerate() {
            let Some(tree) = tree else { continue };
            if tree.refined.is_empty() {
                continue;
            }
            let ijk = self.root_ijk(ti);
            let mut origin = [0.0; 3];
            let mut size = [0.0; 3];
            for k in 0..3 {
                let c = &self.coords[k];
                if c.len() > 1 {
                    origin[k] = c[ijk[k]];
                    size[k] = c[ijk[k] + 1] - c[ijk[k]];
                } else {
                    origin[k] = c.first().copied().unwrap_or(0.0);
                }
            }
            // First child of each refined vertex (breadth-first numbering).
            let mut first_child = vec![usize::MAX; tree.refined.len()];
            let mut next = 1;
            for (v, &r) in tree.refined.iter().enumerate() {
                if r {
                    first_child[v] = next;
                    next += nc;
                }
            }
            let lattice0: [i64; 3] = std::array::from_fn(|k| if self.coords[k].len() > 1 { ijk[k] as i64 * scale } else { 0 });
            let mut stack = vec![(0usize, 0usize, origin, size, lattice0)];
            while let Some((v, depth, o, s, lat)) = stack.pop() {
                let Some(&global) = tree.global.get(v) else { continue };
                let masked = self.is_masked(global);
                let refined = tree.refined.get(v).copied().unwrap_or(false) && first_child[v] + nc <= tree.refined.len();
                let visit = Visit { global, depth, leaf: !refined, masked, origin: o, size: s, lattice: lat };
                if !f(&visit) || (masked && !into_masked) || !refined {
                    continue;
                }
                let cs: [f64; 3] = std::array::from_fn(|k| if axes.contains(&k) { s[k] / bf as f64 } else { s[k] });
                let step = scale / bf.pow(depth as u32 + 1);
                // Push in reverse so children come off the stack in order.
                for c in (0..nc).rev() {
                    let mut rem = c;
                    let mut co = o;
                    let mut cl = lat;
                    for &k in &axes {
                        let i = rem % bf as usize;
                        rem /= bf as usize;
                        co[k] = o[k] + i as f64 * cs[k];
                        cl[k] = lat[k] + i as i64 * step;
                    }
                    stack.push((first_child[v] + c, depth + 1, co, cs, cl));
                }
            }
        }
    }

    /// The visible leaves as cells (voxels, pixels or lines) with shared corners, in VTK's
    /// order (trees by index, leaves depth first), the cell data of each leaf, and the global
    /// index of each leaf.
    pub fn leaves_mesh(&self) -> (UnstructuredGrid, Vec<usize>) {
        let axes = self.axes();
        let (ty, corners): (CellType, Vec<[i64; 3]>) = match axes.len() {
            3 => (CellType::Voxel, (0..8).map(|c| [c & 1, (c >> 1) & 1, (c >> 2) & 1]).collect()),
            2 => (CellType::Pixel, (0..4).map(|c| [c & 1, (c >> 1) & 1, 0]).collect()),
            1 => (CellType::Line, vec![[0, 0, 0], [1, 0, 0]]),
            _ => (CellType::Vertex, vec![[0, 0, 0]]),
        };
        let bf = self.branch_factor.max(2) as i64;
        let scale = bf.pow(self.max_depth() as u32);
        let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
        let mut pts: Vec<f64> = Vec::new();
        let mut cells = Cells::new();
        let mut leaves = Vec::new();
        self.walk(|v| {
            if v.leaf && !v.masked {
                let step = scale / bf.pow(v.depth as u32);
                let mut conn = Vec::with_capacity(corners.len());
                for c in &corners {
                    let mut key = v.lattice;
                    let mut p = v.origin;
                    for (a, &k) in axes.iter().enumerate() {
                        key[k] += c[a] * step;
                        p[k] += c[a] as f64 * v.size[k];
                    }
                    let id = *ids.entry(key).or_insert_with(|| {
                        pts.extend_from_slice(&p);
                        (pts.len() / 3 - 1) as u32
                    });
                    conn.push(id);
                }
                cells.push(ty, &conn);
                leaves.push(v.global);
            }
            true
        });
        let cell_data = self.cell_data.gather(&leaves);
        (UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: Attributes::new(), cell_data, field_data: self.field_data.clone() }, leaves)
    }

    /// Number of visible leaves.
    pub fn n_leaves(&self) -> usize {
        let mut n = 0;
        self.walk(|v| {
            n += (v.leaf && !v.masked) as usize;
            true
        });
        n
    }

    /// A copy whose trees stop at `depth` (deeper vertices dropped, cell data renumbered tree
    /// by tree in breadth-first order), as VTK's depth limiter.
    pub fn limit_depth(&self, depth: usize) -> HyperTreeGrid {
        let nc = self.children_per_vertex();
        let mut out = HyperTreeGrid {
            coords: self.coords.clone(),
            branch_factor: self.branch_factor,
            transposed: self.transposed,
            interface: self.interface.clone(),
            field_data: self.field_data.clone(),
            ..Default::default()
        };
        let mut order: Vec<usize> = Vec::new();
        let mut mask = Vec::new();
        for tree in &self.trees {
            let Some(t) = tree else {
                out.trees.push(None);
                continue;
            };
            // Breadth-first with depths.
            let mut refined = Vec::new();
            let mut global = Vec::new();
            let mut queue = std::collections::VecDeque::from([(0usize, 0usize)]);
            let mut next = 1;
            let mut first_child = vec![usize::MAX; t.refined.len()];
            for (v, &r) in t.refined.iter().enumerate() {
                if r {
                    first_child[v] = next;
                    next += nc;
                }
            }
            while let Some((v, d)) = queue.pop_front() {
                if v >= t.refined.len() {
                    continue;
                }
                let g = t.global[v];
                let masked = self.is_masked(g);
                let r = t.refined[v] && d < depth && !masked;
                global.push(order.len());
                order.push(g);
                mask.push(masked);
                refined.push(r);
                if r {
                    for c in 0..nc {
                        queue.push_back((first_child[v] + c, d + 1));
                    }
                }
            }
            out.trees.push(Some(HyperTree { refined, global }));
        }
        out.cell_data = self.cell_data.gather(&order);
        if self.mask.is_some() {
            out.mask = Some(mask);
        }
        out
    }
}

fn tree_depth(t: &HyperTree, nc: usize) -> usize {
    // Depth of each vertex in breadth-first order.
    let mut depth = vec![0usize; t.refined.len()];
    let mut next = 1;
    let mut max = 0;
    for v in 0..t.refined.len() {
        let dv = depth[v];
        max = max.max(dv);
        if t.refined[v] {
            for c in 0..nc {
                if let Some(d) = depth.get_mut(next + c) {
                    *d = dv + 1;
                }
            }
            next += nc;
        }
    }
    max
}

/// The depth of every vertex, by global index.
pub fn depth_array(htg: &HyperTreeGrid) -> Array {
    let mut d = vec![0.0; htg.n_vertices()];
    htg.walk(|v| {
        if let Some(x) = d.get_mut(v.global) {
            *x = v.depth as f64;
        }
        true
    });
    Array::new("Depth", 1, ArrayData::F64(d))
}
