//! Hyper tree grids: ParaView's Hyper Tree Grid source (trees from a text descriptor) and the
//! HTG filters (To Unstructured Grid, Depth Limiter, Generate Fields, Evaluate Coarse).

use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

/// One node of a tree being built (depth-first, as VTK's source builds it).
struct Node {
    global: usize,
    refined: bool,
    children: Vec<Node>,
}

/// vtkHyperTreeGridSource with a string descriptor: `dims` points per axis (1 = flat axis),
/// levels separated by `|`, each listing its vertices as `R` (refined) or `.` (leaf) for all
/// trees in order; `mask` (same layout) marks masked leaves with `0`.
pub fn from_descriptor(dims: [usize; 3], branch_factor: usize, max_depth: usize, descriptor: &str, scale: [f64; 3], mask: Option<&str>) -> Result<HyperTreeGrid> {
    if !(2..=3).contains(&branch_factor) {
        bail!("the branch factor must be 2 or 3");
    }
    let dim = dims.iter().filter(|&&n| n > 1).count();
    if dim == 0 {
        bail!("at least one dimension must be larger than 1");
    }
    let block = branch_factor.pow(dim as u32);
    let n_roots: usize = dims.iter().map(|&n| n.saturating_sub(1).max(1)).product();
    // Levels without spaces (and without the piece digits VTK allows at the root level).
    let levels_of = |s: &str| -> Vec<String> { s.split('|').map(|l| l.chars().filter(|c| !c.is_whitespace() && !c.is_ascii_digit()).collect()).collect() };
    let mut levels = levels_of(descriptor);
    if levels.iter().any(|l| l.chars().any(|c| c != 'R' && c != '.')) {
        bail!("the descriptor may only contain R (refined), . (leaf), spaces and | between levels");
    }
    // The mask keeps its digits (0 masks a vertex, 1 keeps it).
    let masks: Option<Vec<String>> = mask.map(|m| m.split('|').map(|l| l.chars().filter(|c| !c.is_whitespace()).collect()).collect());
    if masks.as_ref().is_some_and(|m| m.iter().any(|l| l.chars().any(|c| c != '0' && c != '1'))) {
        bail!("the mask may only contain 0 (hidden), 1 (kept), spaces and | between levels");
    }
    if let Some(m) = &masks {
        if m.len() != levels.len() || m.iter().zip(&levels).any(|(a, b)| a.len() != b.len()) {
            bail!("the mask must have the same layout as the descriptor");
        }
    }
    let mut expected = n_roots;
    for (l, lev) in levels.iter().enumerate() {
        if lev.len() != expected {
            bail!("level {l} of the descriptor has {} vertices; {expected} expected", lev.len());
        }
        expected = lev.chars().filter(|&c| c == 'R').count() * block;
    }
    // Leaves under the refined vertices of the last level, up to the maximum depth.
    if levels.len() < max_depth && expected > 0 {
        levels.push(".".repeat(expected));
    }
    let mut level_start = vec![0usize];
    for l in &levels {
        level_start.push(level_start.last().unwrap() + l.len());
    }
    let level_bytes: Vec<&[u8]> = levels.iter().map(|l| l.as_bytes()).collect();
    let mut counters = vec![0usize; levels.len()];
    let mut next_id = level_start[..levels.len()].to_vec();
    let total = *level_start.last().unwrap();
    let mut masked = vec![false; total];
    // Depth-first construction (VTK's SubdivideFromStringDescriptor).
    #[allow(clippy::too_many_arguments)]
    fn build(
        level: usize,
        pointer: usize,
        max_depth: usize,
        block: usize,
        levels: &[&[u8]],
        masks: Option<&Vec<String>>,
        counters: &mut Vec<usize>,
        next_id: &mut Vec<usize>,
        masked: &mut Vec<bool>,
    ) -> Node {
        let id = next_id[level];
        next_id[level] += 1;
        let refine = level + 1 < max_depth && levels.len() > level + 1 && levels[level].get(pointer) == Some(&b'R');
        if refine {
            let parent = counters[level];
            let children = (0..block).map(|c| build(level + 1, c + parent * block, max_depth, block, levels, masks, counters, next_id, masked)).collect();
            counters[level] += 1;
            Node { global: id, refined: true, children }
        } else {
            if let Some(m) = masks.and_then(|m| m.get(level)) {
                masked[id] = m.as_bytes().get(pointer) == Some(&b'0');
            }
            Node { global: id, refined: false, children: Vec::new() }
        }
    }
    let mut trees = Vec::with_capacity(n_roots);
    for t in 0..n_roots {
        let root = build(0, t, max_depth.max(1), block, &level_bytes, masks.as_ref(), &mut counters, &mut next_id, &mut masked);
        // Breadth-first order.
        let mut tree = HyperTree::default();
        let mut queue = std::collections::VecDeque::from([&root]);
        while let Some(n) = queue.pop_front() {
            tree.refined.push(n.refined);
            tree.global.push(n.global);
            queue.extend(n.children.iter());
        }
        trees.push(Some(tree));
    }
    let coords: [Vec<f64>; 3] = std::array::from_fn(|k| if dims[k] > 1 { (0..dims[k]).map(|i| scale[k] * i as f64).collect() } else { vec![0.0] });
    let mut htg = HyperTreeGrid { coords, branch_factor, trees, ..Default::default() };
    // Depth of every vertex.
    let used = next_id.iter().zip(&level_start).map(|(n, s)| n - s).sum::<usize>();
    let _ = used;
    let mut depth = vec![0.0; total];
    htg.walk(|v| {
        if let Some(d) = depth.get_mut(v.global) {
            *d = v.depth as f64;
        }
        true
    });
    htg.cell_data.insert(Array::new("Depth", 1, ArrayData::F64(depth)));
    if mask.is_some() {
        htg.mask = Some(masked);
    }
    Ok(htg)
}

fn htg_of(ds: &DataSet) -> Result<&HyperTreeGrid> {
    match ds {
        DataSet::HyperTreeGrid(h) => Ok(h),
        other => Err(anyhow!("this filter needs a hyper tree grid, not {}", other.type_name())),
    }
}

/// HTG To Unstructured Grid: the visible leaves as voxels (pixels, lines) sharing their
/// corners, with the leaves' cell data and `OriginalIds` (the leaves' cell indices).
pub fn to_unstructured(ds: &DataSet) -> Result<UnstructuredGrid> {
    let h = htg_of(ds)?;
    let (mut ug, leaves) = h.leaves_mesh();
    ug.cell_data.insert(Array::new("OriginalIds", 1, ArrayData::I64(leaves.iter().map(|&g| g as i64).collect())));
    Ok(ug)
}

pub fn depth_limiter(ds: &DataSet, depth: usize) -> Result<DataSet> {
    Ok(DataSet::HyperTreeGrid(htg_of(ds)?.limit_depth(depth)))
}

/// HTG Generate Fields: CellSize (volume, area or length of every cell), ValidCell (1 on
/// visible leaves), CellCenter (0 on masked cells) and TotalVisibleVolume (field data).
pub fn generate_fields(ds: &DataSet) -> Result<DataSet> {
    let mut h = htg_of(ds)?.clone();
    let n = h.n_vertices().max(h.cell_data.iter().map(|a| a.len()).max().unwrap_or(0));
    let axes = h.axes();
    let (mut size, mut valid, mut center) = (vec![0.0f64; n], vec![0u8; n], vec![0.0f64; 3 * n]);
    let mut total = 0.0;
    // Masked vertices still get their size (VTK computes it for every cell).
    h.walk(|v| {
        if v.global < n {
            let m: f64 = axes.iter().map(|&k| v.size[k]).product();
            size[v.global] = m;
            if !v.masked {
                for k in 0..3 {
                    center[3 * v.global + k] = v.origin[k] + 0.5 * v.size[k];
                }
                if v.leaf {
                    valid[v.global] = 1;
                    total += m;
                }
            }
        }
        true
    });
    h.cell_data.insert(Array::new("CellCenter", 3, ArrayData::F64(center)));
    h.cell_data.insert(Array::new("ValidCell", 1, ArrayData::U8(valid)));
    h.cell_data.insert(Array::new("CellSize", 1, ArrayData::F64(size)));
    h.field_data.insert(Array::new("TotalVisibleVolume", 1, ArrayData::F64(vec![total])));
    Ok(DataSet::HyperTreeGrid(h))
}

pub const COARSE_OPERATORS: [&str; 9] = ["Don't change fast", "Don't change", "Min", "Max", "Sum", "Average", "Unmasked Average", "Elder Child", "Splatting Average"];

/// HTG Evaluate Coarse: the values of refined cells computed from their children (bottom up),
/// as VTK does (masked children count as 0).
pub fn evaluate_coarse(ds: &DataSet, operator: &str, default: f64) -> Result<DataSet> {
    let mut h = htg_of(ds)?.clone();
    if matches!(operator, "Don't change fast" | "Don't change") {
        return Ok(DataSet::HyperTreeGrid(h));
    }
    let nc = h.children_per_vertex();
    let splat = (h.branch_factor as f64).powi(h.dimension() as i32 - 1);
    let eval = |v: &[f64]| -> f64 {
        match operator {
            "Min" => v.iter().cloned().fold(f64::INFINITY, f64::min),
            "Max" => v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            "Sum" => v.iter().sum(),
            "Average" => (v.iter().sum::<f64>() + default * (nc - v.len()) as f64) / nc as f64,
            "Unmasked Average" => v.iter().sum::<f64>() / v.len() as f64,
            "Elder Child" => v[0],
            "Splatting Average" => (v.iter().sum::<f64>() + default * (nc - v.len()) as f64) / splat,
            _ => f64::NAN,
        }
    };
    let mask = h.mask.clone();
    let masked = |g: usize| mask.as_ref().is_some_and(|m| m.get(g).copied().unwrap_or(false));
    let names: Vec<String> = h.cell_data.iter().filter(|a| a.scalar_type() != ScalarType::String).map(|a| a.name.clone()).collect();
    for name in names {
        let arr = h.cell_data.get(&name).unwrap();
        let nco = arr.ncomp;
        let mut vals: Vec<f64> = arr.data.to_f64_vec();
        for tree in h.trees.iter().flatten() {
            // Children of each refined vertex (breadth-first numbering); deepest first.
            let mut first = vec![usize::MAX; tree.refined.len()];
            let mut next = 1;
            for (v, &r) in tree.refined.iter().enumerate() {
                if r {
                    first[v] = next;
                    next += nc;
                }
            }
            for v in (0..tree.refined.len()).rev() {
                if !tree.refined[v] || masked(tree.global[v]) || first[v] + nc > tree.refined.len() {
                    continue;
                }
                let g = tree.global[v];
                for c in 0..nco {
                    let child: Vec<f64> = (0..nc)
                        .map(|k| {
                            let cg = tree.global[first[v] + k];
                            if masked(cg) {
                                0.0
                            } else {
                                vals[cg * nco + c]
                            }
                        })
                        .collect();
                    vals[g * nco + c] = eval(&child);
                }
            }
        }
        let ty = arr.data.scalar_type();
        let data = if matches!(ty, ScalarType::F32) { ArrayData::F32(vals.iter().map(|&v| v as f32).collect()) } else { ArrayData::F64(vals) };
        let mut a = Array::new(name.clone(), nco, data);
        a.component_names = h.cell_data.get(&name).unwrap().component_names.clone();
        h.cell_data.insert(a);
    }
    Ok(DataSet::HyperTreeGrid(h))
}

/// VTK's minimal standard random sequence (Park and Miller).
struct MinStd(i32);

impl MinStd {
    fn new(seed: i32) -> Self {
        let mut s = seed;
        if s < 1 {
            s += 2147483646;
        } else if s == 2147483647 {
            s = 1;
        }
        let mut r = MinStd(s);
        r.next();
        r.next();
        r.next();
        r
    }
    fn next(&mut self) {
        let (hi, lo) = (self.0 / 127773, self.0 % 127773);
        self.0 = 16807 * lo - 2836 * hi;
        if self.0 <= 0 {
            self.0 += 2147483647;
        }
    }
    fn value(&self) -> f64 {
        self.0 as f64 / 2147483647.0
    }
}

/// ParaView's Random Hyper Tree Grid source (vtkRandomHyperTreeGridSource, without masking):
/// branch factor 2, each cell refined with probability `split` down to `max_depth`, the same
/// random sequence per tree as VTK (seeded with seed + tree index).
pub fn random(dims: [usize; 3], bounds: [f64; 6], seed: i32, max_depth: usize, split: f64, masked_fraction: f64) -> Result<HyperTreeGrid> {
    let dim = dims.iter().filter(|&&n| n > 1).count();
    if dim == 0 {
        bail!("at least one dimension must be larger than 1");
    }
    let nc = 1usize << dim;
    let coords: [Vec<f64>; 3] = std::array::from_fn(|k| {
        let n = dims[k].max(1);
        if n == 1 {
            vec![0.0]
        } else {
            let (lo, hi) = (bounds[2 * k], bounds[2 * k + 1]);
            (0..n).map(|i| lo + (hi - lo) / (n - 1) as f64 * i as f64).collect()
        }
    });
    let mut htg = HyperTreeGrid { coords, branch_factor: 2, ..Default::default() };
    let c = htg.cell_dims();
    htg.trees = vec![None; c[0] * c[1] * c[2]];
    let mut depth: Vec<f64> = Vec::new();
    let mut offset = 0usize;
    // Per tree (in generation order): its id, global offset and vertices in creation order.
    let mut built: Vec<(usize, usize, Vec<(usize, Option<usize>)>)> = Vec::new();
    // Trees in VTK's order (i outermost), each its own random sequence.
    for i in 0..c[0] {
        for j in 0..c[1] {
            for k in 0..c[2] {
                let tree_id = i + c[0] * (j + c[1] * k);
                let mut rng = MinStd::new(seed.wrapping_add(tree_id as i32));
                // Vertices in creation order: (depth, children start or None).
                let mut verts: Vec<(usize, Option<usize>)> = vec![(0, None)];
                fn subdivide(v: usize, verts: &mut Vec<(usize, Option<usize>)>, rng: &mut MinStd, max_depth: usize, split: f64, nc: usize) {
                    let level = verts[v].0;
                    match verts[v].1 {
                        None => {
                            rng.next();
                            if level < max_depth && rng.value() < split {
                                let first = verts.len();
                                verts.extend((0..nc).map(|_| (level + 1, None)));
                                verts[v].1 = Some(first);
                                subdivide(v, verts, rng, max_depth, split, nc);
                            }
                        }
                        Some(first) => {
                            for ch in 0..nc {
                                subdivide(first + ch, verts, rng, max_depth, split, nc);
                            }
                        }
                    }
                }
                subdivide(0, &mut verts, &mut rng, max_depth, split, nc);
                // Breadth-first layout; the global index is the creation order.
                let mut tree = HyperTree::default();
                let mut queue = std::collections::VecDeque::from([0usize]);
                while let Some(v) = queue.pop_front() {
                    tree.refined.push(verts[v].1.is_some());
                    tree.global.push(offset + v);
                    if let Some(f) = verts[v].1 {
                        queue.extend(f..f + nc);
                    }
                }
                depth.resize(offset + verts.len(), 0.0);
                for (v, (d, _)) in verts.iter().enumerate() {
                    depth[offset + v] = *d as f64;
                }
                built.push((tree_id, offset, verts.clone()));
                offset += verts.len();
                htg.trees[tree_id] = Some(tree);
            }
        }
    }
    htg.cell_data.insert(Array::new("Depth", 1, ArrayData::F64(depth)));
    let masked_fraction = masked_fraction.clamp(0.0, 1.0);
    if masked_fraction > 0.0 {
        htg.mask = Some(random_mask(&built, offset, seed, max_depth, nc, masked_fraction));
    }
    Ok(htg)
}

/// VTK's shuffle with a minimal standard sequence (the value is read before advancing).
fn shuffle(a: &mut [usize], rng: &mut MinStd) {
    for i in (1..a.len()).rev() {
        let index = (rng.value() * a.len() as f64).floor() as usize;
        a.swap(i, index % a.len());
        rng.next();
    }
}

/// The Random Hyper Tree Grid source's mask, as VTK makes it: trees in a shuffled order, each
/// masked depth first (children shuffled) until about `fraction` of the volume is hidden; a
/// vertex whose children are all masked is masked too.
fn random_mask(built: &[(usize, usize, Vec<(usize, Option<usize>)>)], total: usize, seed: i32, max_depth: usize, nc: usize, fraction: f64) -> Vec<bool> {
    let mut mask = vec![false; total];
    // VTK counts children for three dimensions whatever the grid's dimension.
    let cost: Vec<f64> = (0..=max_depth).map(|l| 1.0 / 8f64.powi(l as i32)).collect();
    let should_mask = |siblings: f64, level: usize, margin: f64| -> bool {
        let weight = ((level * max_depth) as i64).max(1) as f64;
        siblings * weight <= fraction - margin
    };
    let n = built.len();
    let mut order: Vec<usize> = (0..n).collect();
    let mut trees_rng = MinStd::new(seed);
    shuffle(&mut order, &mut trees_rng);
    let mut tree_siblings = 0.0;
    let margin = 1.0 / n as f64;
    #[allow(clippy::too_many_arguments)]
    fn generate(
        v: usize,
        verts: &[(usize, Option<usize>)],
        off: usize,
        unmasked: f64,
        parent_masked: bool,
        siblings: f64,
        margin: f64,
        nc: usize,
        rng: &mut MinStd,
        cost: &[f64],
        should_mask: &dyn Fn(f64, usize, f64) -> bool,
        mask: &mut [bool],
    ) -> f64 {
        let level = verts[v].0;
        let mut is_masked = false;
        let mut result = unmasked;
        if !parent_masked && should_mask(siblings, level, margin) {
            result -= cost.get(level).copied().unwrap_or(0.0);
            is_masked = true;
        }
        is_masked = is_masked || parent_masked;
        if let Some(first) = verts[v].1 {
            let mut n_masked = 0;
            let mut child_unmasked = unmasked;
            let mut children: Vec<usize> = (0..nc).collect();
            shuffle(&mut children, rng);
            for c in children {
                let previous = child_unmasked;
                let frac = n_masked as f64 / nc as f64;
                child_unmasked = generate(first + c, verts, off, previous, is_masked, frac, 1.0 / nc as f64, nc, rng, cost, should_mask, mask);
                if child_unmasked < previous {
                    n_masked += 1;
                }
            }
            if !is_masked {
                result = child_unmasked;
            }
            if n_masked == nc {
                is_masked = true;
            }
        }
        mask[off + v] = is_masked;
        result
    }
    for t in order {
        let (tree_id, off, verts) = &built[t];
        let mut rng = MinStd::new(seed.wrapping_add(*tree_id as i32));
        let unmasked = if fraction >= 1.0 {
            mask[*off] = true;
            0.0
        } else {
            generate(0, verts, *off, 1.0, false, tree_siblings, margin, nc, &mut rng, &cost, &should_mask, &mut mask)
        };
        tree_siblings += (1.0 - unmasked) / n as f64;
    }
    mask
}

/// A cell next to a leaf, as VTK's Moore super cursor sees it: the cell at the leaf's level
/// at that place, or the coarser leaf covering it.
#[derive(Clone, Copy)]
struct Neighbor {
    global: usize,
    level: usize,
    leaf: bool,
    /// The cell's size along x, y, z.
    size: [f64; 3],
    masked: bool,
}

/// Leaves around each corner of a 3-D leaf, as indices into the 3x3x3 neighborhood (VTK's
/// CornerNeighborCursorsTable3D).
const CORNER_NEIGHBORS_3D: [[usize; 8]; 8] = [
    [0, 1, 3, 4, 9, 10, 12, 13],
    [1, 2, 4, 5, 10, 11, 13, 14],
    [3, 4, 6, 7, 12, 13, 15, 16],
    [4, 5, 7, 8, 13, 14, 16, 17],
    [9, 10, 12, 13, 18, 19, 21, 22],
    [10, 11, 13, 14, 19, 20, 22, 23],
    [12, 13, 15, 16, 21, 22, 24, 25],
    [13, 14, 16, 17, 22, 23, 25, 26],
];

/// A cell of a tree: tree, breadth-first vertex, level, lower corner on the lattice.
#[derive(Clone, Copy)]
struct Entry {
    ti: usize,
    v: usize,
    level: usize,
    lo: [i64; 3],
}

/// The neighbors of cells as VTK's hyper tree grid super cursors (Moore, von Neumann) see them.
struct Cursors<'a> {
    h: &'a HyperTreeGrid,
    axes: Vec<usize>,
    bf: i64,
    nc: usize,
    scale: i64,
    cd: [usize; 3],
    /// First child (breadth-first index) of each vertex, per tree.
    first: Vec<Vec<usize>>,
}

impl<'a> Cursors<'a> {
    fn new(h: &'a HyperTreeGrid) -> Self {
        let bf = h.branch_factor.max(2) as i64;
        let nc = h.children_per_vertex();
        let first = h
            .trees
            .iter()
            .map(|t| {
                let Some(t) = t else { return Vec::new() };
                let mut next = 1;
                t.refined
                    .iter()
                    .map(|&r| {
                        let f = next;
                        if r {
                            next += nc;
                        }
                        f
                    })
                    .collect()
            })
            .collect();
        Cursors { h, axes: h.axes(), bf, nc, scale: bf.pow(h.max_depth() as u32), cd: h.cell_dims(), first }
    }
    fn refined_of(&self, e: &Entry) -> bool {
        let t = self.h.trees[e.ti].as_ref().unwrap();
        t.refined[e.v] && self.first[e.ti][e.v] + self.nc <= t.refined.len()
    }
    fn global_of(&self, e: &Entry) -> usize {
        self.h.trees[e.ti].as_ref().unwrap().global[e.v]
    }
    fn root_entry(&self, x: [i64; 3]) -> Option<Entry> {
        let mut ijk = [0usize; 3];
        for &k in &self.axes {
            if x[k] < 0 || (x[k] / self.scale) as usize >= self.cd[k] {
                return None;
            }
            ijk[k] = (x[k] / self.scale) as usize;
        }
        let cd = self.cd;
        let ti = if self.h.transposed { ijk[2] + cd[2] * (ijk[1] + cd[1] * ijk[0]) } else { ijk[0] + cd[0] * (ijk[1] + cd[1] * ijk[2]) };
        self.h.trees.get(ti)?.as_ref().filter(|t| !t.refined.is_empty())?;
        Some(Entry { ti, v: 0, level: 0, lo: std::array::from_fn(|k| ijk[k] as i64 * self.scale) })
    }
    fn child_containing(&self, e: &Entry, x: [i64; 3]) -> Entry {
        let step = self.scale / self.bf.pow(e.level as u32 + 1);
        let (mut c, mut mult) = (0usize, 1usize);
        let mut lo = e.lo;
        for &k in &self.axes {
            let i = (x[k] - e.lo[k]) / step;
            c += i as usize * mult;
            mult *= self.bf as usize;
            lo[k] += i * step;
        }
        Entry { ti: e.ti, v: self.first[e.ti][e.v] + c, level: e.level + 1, lo }
    }
    fn neighbor_of(&self, e: &Entry) -> Neighbor {
        let ijk: [usize; 3] = std::array::from_fn(|k| (e.lo[k] / self.scale) as usize);
        let f = (self.bf as f64).powi(e.level as i32);
        let h = self.h;
        let size = std::array::from_fn(|k| if self.axes.contains(&k) { (h.coords[k][ijk[k] + 1] - h.coords[k][ijk[k]]) / f } else { 0.0 });
        let global = self.global_of(e);
        Neighbor { global, level: e.level, leaf: !self.refined_of(e), size, masked: h.is_masked(global) }
    }
    /// The ancestors of the cell at `lattice` on `level`, from its root down.
    fn chain(&self, lattice: [i64; 3], level: usize) -> Vec<Entry> {
        let mut chain = vec![self.root_entry(lattice).expect("cell in a tree")];
        while chain.len() <= level {
            let last = *chain.last().unwrap();
            chain.push(self.child_containing(&last, lattice));
        }
        chain
    }
    /// The cell at `x` next to the cell whose ancestors are `chain`, as VTK's super cursors find
    /// it going down with the cell: a neighbor in the same parent comes from the parent (unless
    /// the parent is masked); others come from the neighbor one level up, going down only
    /// through refined, unmasked cells.
    fn locate(&self, chain: &[Entry], x: [i64; 3]) -> Option<Neighbor> {
        let (h, bf, scale) = (self.h, self.bf, self.scale);
        let offset_zero = |k: usize| {
            let s = scale / bf.pow(k as u32);
            self.axes.iter().all(|&a| x[a].div_euclid(s) == chain[k].lo[a] / s)
        };
        let mut e: Option<Entry> = if offset_zero(0) { Some(chain[0]) } else { self.root_entry(x) };
        for k in 1..chain.len() {
            if offset_zero(k) {
                e = Some(chain[k]);
                continue;
            }
            e = if offset_zero(k - 1) {
                let p = chain[k - 1];
                Some(if self.refined_of(&p) && !h.is_masked(self.global_of(&p)) { self.child_containing(&p, x) } else { p })
            } else {
                e.map(|p| if p.level == k - 1 && self.refined_of(&p) && !h.is_masked(self.global_of(&p)) { self.child_containing(&p, x) } else { p })
            };
        }
        e.map(|e| self.neighbor_of(&e))
    }
    /// The 3^d cells around a visited cell (the cell itself in the middle), x fastest over the
    /// grid's axes, as VTK's Moore super cursor numbers them.
    fn neighborhood(&self, v: &aurafx_data::htg::Visit) -> Vec<Option<Neighbor>> {
        let chain = self.chain(v.lattice, v.depth);
        let s = self.scale / self.bf.pow(v.depth as u32);
        let n3 = 3usize.pow(self.axes.len() as u32);
        (0..n3)
            .map(|n| {
                let mut lat = v.lattice;
                for (a, &k) in self.axes.iter().enumerate() {
                    lat[k] += ((n / 3usize.pow(a as u32)) % 3) as i64 * s - s;
                }
                self.locate(&chain, lat)
            })
            .collect()
    }
}

/// Hyper Tree Grid To Dual Grid, as VTK's vtkHyperTreeGridToDualGrid: a point at the center of
/// every visible leaf (moved onto the grid's boundary, and next to masked leaves, as VTK moves
/// them) with the leaf's cell data, and a voxel (pixel, line) joining the leaves around every
/// corner, made by the finest of those leaves. Unlike VTK the output has no points for coarse
/// or masked cells (VTK leaves them at the origin).
pub fn dual_grid(h: &HyperTreeGrid) -> Result<UnstructuredGrid> {
    use std::collections::HashMap;
    let axes = h.axes();
    let dim = axes.len();
    if dim == 0 {
        bail!("the hyper tree grid has no cells");
    }
    let cur = Cursors::new(h);

    let has_mask = h.mask.is_some();
    let n3 = 3usize.pow(dim as u32);
    let center = (n3 - 1) / 2;
    let mut points: HashMap<usize, [f64; 3]> = HashMap::new();
    let mut shifts: [HashMap<usize, f64>; 3] = Default::default();
    let mut shifted_flag: HashMap<usize, bool> = HashMap::new();
    let mut cells: Vec<Vec<usize>> = Vec::new();
    let masked = |g: usize| h.is_masked(g);
    h.walk_structure(|v| {
        if !v.leaf {
            return true;
        }
        let level = v.depth;
        let nb = cur.neighborhood(v);
        let has = |n: usize| nb[n].is_some();
        let leaf = |n: usize| nb[n].is_some_and(|x| x.leaf);
        let lev = |n: usize| nb[n].map(|x| x.level).unwrap_or(usize::MAX);
        let gid = |n: usize| nb[n].map(|x| x.global).unwrap_or(0);
        let id = v.global;
        let mut pt: [f64; 3] = std::array::from_fn(|k| v.origin[k] + if axes.contains(&k) { 0.5 * v.size[k] } else { 0.0 });
        let half: [f64; 3] = v.size.map(|x| 0.5 * x);
        // A masked leaf moves its coarser unmasked neighbors' points towards itself.
        if has_mask && v.masked {
            if dim == 2 {
                let (we, sn) = (axes[0], axes[1]);
                for (n, ax, sign) in [(1usize, sn, 1.0), (3, we, 1.0), (5, we, -1.0), (7, sn, -1.0)] {
                    if let Some(x) = nb[n].filter(|x| x.leaf && x.level < level && !masked(x.global)) {
                        shifted_flag.insert(x.global, true);
                        shifts[ax].insert(x.global, sign * 0.5 * x.size[ax]);
                    }
                }
                // VTK's signs for the diagonal neighbors.
                for (n, swe, ssn) in [(2usize, 1.0, 1.0), (0, -1.0, 1.0), (6, 1.0, -1.0), (8, -1.0, -1.0)] {
                    if let Some(x) = nb[n].filter(|x| x.leaf && x.level < level && !masked(x.global)) {
                        if !shifted_flag.get(&x.global).copied().unwrap_or(false) {
                            shifts[we].insert(x.global, swe * 0.5 * x.size[we]);
                            shifts[sn].insert(x.global, ssn * 0.5 * x.size[sn]);
                        }
                    }
                }
            } else if dim == 3 {
                let mut offset = 1usize;
                for axis in 0..3 {
                    for (n, sign) in [(13 - offset, 1.0), (13 + offset, -1.0)] {
                        if let Some(x) = nb[n].filter(|x| x.leaf && x.level < level && !masked(x.global)) {
                            shifted_flag.insert(x.global, true);
                            shifts[axis].insert(x.global, sign * 0.5 * x.size[axis]);
                        }
                    }
                    offset *= 3;
                }
                let mut i = 1i64;
                for axis1 in 0..2 {
                    let mut j = 3 * i;
                    for axis2 in axis1 + 1..3 {
                        for o2 in [-1i64, 1] {
                            for o1 in [-1i64, 1] {
                                let n = (13 + o1 * (i * o2 + j)) as usize;
                                if let Some(x) = nb[n].filter(|x| x.leaf && x.level < level && !masked(x.global)) {
                                    if !shifted_flag.get(&x.global).copied().unwrap_or(false) {
                                        shifted_flag.insert(x.global, true);
                                        shifts[axis1].insert(x.global, (-o1 * o2) as f64 * 0.5 * x.size[axis1]);
                                        shifts[axis2].insert(x.global, -o1 as f64 * 0.5 * x.size[axis2]);
                                    }
                                }
                            }
                        }
                        j *= 3;
                    }
                    i *= 3;
                }
                for o3 in [-1i64, 1] {
                    for o2 in [-1i64, 1] {
                        let off = o2 * (o3 + 3) + 9;
                        for o1 in [-1i64, 1] {
                            let n = (13 + o1 * off) as usize;
                            if let Some(x) = nb[n].filter(|x| x.leaf && x.level < level && !masked(x.global)) {
                                if !shifted_flag.get(&x.global).copied().unwrap_or(false) {
                                    shifted_flag.insert(x.global, true);
                                    shifts[0].insert(x.global, (-o1 * o2 * o3) as f64 * 0.5 * x.size[0]);
                                    shifts[1].insert(x.global, (-o1 * o2) as f64 * 0.5 * x.size[1]);
                                    shifts[2].insert(x.global, -o1 as f64 * 0.5 * x.size[2]);
                                }
                            }
                        }
                    }
                }
            }
            return true;
        }
        // The point moves onto the boundary (and, with a mask, onto masked neighbors).
        let gone = |n: usize| !has(n) || (has_mask && leaf(n) && masked(gid(n)));
        match dim {
            1 => {
                let k = axes[0];
                if !has(0) {
                    pt[k] -= half[k];
                }
                if !has(2) {
                    pt[k] += half[k];
                }
            }
            2 => {
                let (we, sn) = (axes[0], axes[1]);
                let mut shifted = false;
                for (n, ax, sign) in [(1usize, sn, -1.0), (3, we, -1.0), (5, we, 1.0), (7, sn, 1.0)] {
                    if gone(n) {
                        pt[ax] += sign * half[ax];
                        shifted = true;
                    }
                }
                if has_mask && !shifted {
                    for (n, swe, ssn) in [(0usize, -1.0, -1.0), (2, 1.0, -1.0), (6, -1.0, 1.0), (8, 1.0, 1.0)] {
                        if gone(n) {
                            pt[we] += swe * half[we];
                            pt[sn] += ssn * half[sn];
                        }
                    }
                }
            }
            _ => {
                let mut shifted = false;
                let mut offset = 1usize;
                for axis in 0..3 {
                    if gone(13 - offset) {
                        pt[axis] -= half[axis];
                        shifted = true;
                    }
                    if gone(13 + offset) {
                        pt[axis] += half[axis];
                        shifted = true;
                    }
                    offset *= 3;
                }
                if has_mask && !shifted {
                    let mut i = 1i64;
                    for axis1 in 0..2 {
                        let mut j = 3 * i;
                        for axis2 in axis1 + 1..3 {
                            for o2 in [-1i64, 1] {
                                for o1 in [-1i64, 1] {
                                    if gone((13 + o1 * (i * o2 + j)) as usize) {
                                        pt[axis1] += (o1 * o2) as f64 * half[axis1];
                                        pt[axis2] += o1 as f64 * half[axis2];
                                        shifted = true;
                                    }
                                }
                            }
                            j *= 3;
                        }
                        i *= 3;
                    }
                }
                if has_mask && !shifted {
                    for o3 in [-1i64, 1] {
                        for o2 in [-1i64, 1] {
                            let off = o2 * (o3 + 3) + 9;
                            for o1 in [-1i64, 1] {
                                if gone((13 + o1 * off) as usize) {
                                    pt[0] += (o1 * o2 * o3) as f64 * half[0];
                                    pt[1] += (o1 * o2) as f64 * half[1];
                                    pt[2] += o1 as f64 * half[2];
                                }
                            }
                        }
                    }
                }
            }
        }
        points.insert(id, pt);
        // Dual cells: a corner belongs to this leaf when every leaf around it is visible and
        // none is finer (a tie at the same level goes to the neighbor later in the
        // neighborhood).
        let ok = |n: usize| has(n) && leaf(n) && !(has_mask && masked(gid(n)));
        match dim {
            // VTK's 1-D case does not look at the mask.
            1 => {
                if has(0) && leaf(0) {
                    cells.push(vec![id, gid(0)]);
                }
                if has(2) && leaf(2) && lev(2) != level {
                    cells.push(vec![id, gid(2)]);
                }
            }
            2 => {
                // (corner's x neighbor, y neighbor, diagonal, neighbors that must be coarser)
                for (x, y, dg, coarser) in [(3usize, 1usize, 0usize, &[][..]), (5, 1, 2, &[5usize][..]), (5, 7, 8, &[5, 8, 7][..]), (3, 7, 6, &[6, 7][..])] {
                    if ok(x) && ok(y) && ok(dg) && coarser.iter().all(|&n| lev(n) != level) {
                        cells.push(vec![id, gid(x), gid(y), gid(dg)]);
                    }
                }
            }
            _ => {
                for corner in CORNER_NEIGHBORS_3D {
                    let owner = corner.iter().all(|&n| n == center || (ok(n) && !(lev(n) == level && n > center)));
                    if owner {
                        cells.push(corner.iter().map(|&n| if n == center { id } else { gid(n) }).collect());
                    }
                }
            }
        }
        true
    });
    // Moves towards masked leaves, as VTK applies them (over the first `dim` axes).
    for (d, sh) in shifts.iter().enumerate().take(dim) {
        for (g, s) in sh {
            if let Some(p) = points.get_mut(g) {
                p[d] += s;
            }
        }
    }
    let mut globals: Vec<usize> = points.keys().copied().collect();
    globals.sort_unstable();
    let index: HashMap<usize, u32> = globals.iter().enumerate().map(|(i, &g)| (g, i as u32)).collect();
    let mut pts = Vec::with_capacity(3 * globals.len());
    for g in &globals {
        pts.extend_from_slice(&points[g]);
    }
    let ty = match dim {
        1 => CellType::Line,
        2 => CellType::Pixel,
        _ => CellType::Voxel,
    };
    let mut out = Cells::new();
    for c in &cells {
        let ids: Option<Vec<u32>> = c.iter().map(|g| index.get(g).copied()).collect();
        if let Some(ids) = ids {
            out.push(ty, &ids);
        }
    }
    let n_cells = h.cell_data.iter().map(|a| a.len()).min().unwrap_or(0);
    let point_data = if globals.last().is_none_or(|&g| g < n_cells) { h.cell_data.gather(&globals) } else { Attributes::new() };
    Ok(UnstructuredGrid { points: Points::new_f64(pts), cells: out, point_data, cell_data: Attributes::new(), field_data: h.field_data.clone() })
}

/// Polygonal output with cell data gathered from the grid's cells.
fn surface_output(h: &HyperTreeGrid, pts: Vec<f64>, cells: Vec<Vec<u32>>, sources: Vec<usize>, lines: bool) -> PolyData {
    let mut c = Cells::new();
    for cell in &cells {
        let ty = match (lines, cell.len()) {
            (true, 2) => CellType::Line,
            (true, _) => CellType::PolyLine,
            (false, 3) => CellType::Triangle,
            (false, 4) => CellType::Quad,
            _ => CellType::Polygon,
        };
        c.push(ty, cell);
    }
    let mut pd = PolyData { points: Points::new_f64(pts), ..Default::default() };
    if lines {
        pd.lines = c;
    } else {
        pd.polys = c;
    }
    let n = h.cell_data.iter().map(|a| a.len()).min().unwrap_or(0);
    if sources.iter().all(|&g| g < n) {
        pd.cell_data = h.cell_data.gather(&sources);
    }
    pd
}

/// The corners of a visited cell: bit 0 of the index steps along the first axis, bit 1 the
/// second, bit 2 the third.
fn corners(v: &aurafx_data::htg::Visit, axes: &[usize]) -> Vec<[f64; 3]> {
    let n = 1usize << axes.len();
    (0..n)
        .map(|c| {
            let mut p = v.origin;
            for (b, &k) in axes.iter().enumerate() {
                if c >> b & 1 == 1 {
                    p[k] += v.size[k];
                }
            }
            p
        })
        .collect()
}

/// A cell's material interface (VTK's vtkHyperTreeGridGeometryImpl::ProbeForCellInterface):
/// planes n.x + d = 0 with one normal; type -1: the material is on the positive side of plane
/// A, 1: on the negative side of plane B, 0: between A and B.
#[derive(Debug, Clone, Copy)]
struct CellInterface {
    kind: i32,
    d: [f64; 2],
    n: [f64; 3],
}

impl CellInterface {
    fn dist_a(&self, p: [f64; 3]) -> f64 {
        self.d[0] + self.n[0] * p[0] + self.n[1] * p[1] + self.n[2] * p[2]
    }
    fn dist_b(&self, p: [f64; 3]) -> f64 {
        self.d[1] + self.n[0] * p[0] + self.n[1] * p[1] + self.n[2] * p[2]
    }
}

/// The grid's interface arrays (normals, intercepts), when it declares them and they exist with
/// three components.
fn interface_arrays(h: &HyperTreeGrid) -> Option<(&Array, &Array)> {
    let (n, i) = h.interface.as_ref()?;
    let normals = h.cell_data.get(n)?;
    let intercepts = h.cell_data.get(i)?;
    (normals.ncomp == 3 && intercepts.ncomp == 3).then_some((normals, intercepts))
}

/// The interface of cell `g`, or None for a pure cell. With `invert`, a sandwich whose
/// intercepts come in the wrong order is swapped (as VTK's 3-D geometry does).
fn probe_interface(arrays: Option<(&Array, &Array)>, g: usize, invert: bool) -> Option<CellInterface> {
    let (normals, intercepts) = arrays?;
    if g >= intercepts.len() || g >= normals.len() {
        return None;
    }
    let kind = intercepts.get(g, 2) as i32;
    if kind >= 2 {
        return None;
    }
    let n = [normals.get(g, 0), normals.get(g, 1), normals.get(g, 2)];
    if n == [0.0; 3] {
        return None;
    }
    let mut d = [intercepts.get(g, 0), intercepts.get(g, 1)];
    if kind == 0 && invert && d[1] - d[0] < 0.0 {
        d.swap(0, 1);
    }
    Some(CellInterface { kind, d, n })
}

/// Whether cell `g` has an interface (VTK's GetHasInterface, used for the neighbors in 3-D).
fn has_interface(arrays: Option<(&Array, &Array)>, g: usize) -> bool {
    let Some((normals, intercepts)) = arrays else { return false };
    g < intercepts.len() && g < normals.len() && intercepts.get(g, 2) < 2.0 && (0..3).any(|k| normals.get(g, k) != 0.0)
}

/// Hyper Tree Grid Geometry, as VTK's vtkHyperTreeGridGeometry: a line (1-D) or quad (2-D) per
/// visible leaf; in 3-D the faces of visible leaves on the grid's boundary or next to masked
/// cells, and the faces of masked cells next to a coarser visible leaf (carrying that leaf's
/// values). Points are shared only within a cell, as in VTK. With material interfaces (the
/// grid's interface normal and intercept arrays), each mixed leaf is cut to its material: in
/// 1-D and 2-D the part of the line or quad on the material's side of the planes, in 3-D the
/// cut faces of the mixed cell and of its neighbors plus the interface polygons. Without
/// `fill_material` pure leaves are left out in 1-D and 2-D, and in 2-D the cuts are lines.
pub fn geometry(h: &HyperTreeGrid, fill_material: bool) -> Result<PolyData> {
    Ok(geometry_with_sources(h, fill_material)?.0)
}

/// As `geometry`, also giving the global index of the cell each output cell comes from.
pub fn geometry_with_sources(h: &HyperTreeGrid, fill_material: bool) -> Result<(PolyData, Vec<usize>)> {
    let axes = h.axes();
    let dim = axes.len();
    if dim == 0 {
        bail!("the hyper tree grid has no cells");
    }
    let arrays = interface_arrays(h);
    let (mut pts, mut cells, mut sources) = (Vec::<f64>::new(), Vec::<Vec<u32>>::new(), Vec::new());
    let add_point = |pts: &mut Vec<f64>, p: [f64; 3]| -> u32 {
        pts.extend_from_slice(&p);
        (pts.len() / 3 - 1) as u32
    };
    if dim < 3 {
        let any_interface = h.interface.is_some() && arrays.is_some();
        h.walk(|v| {
            if !(v.leaf && !v.masked) {
                return true;
            }
            let c = corners(v, &axes);
            let order: &[usize] = if dim == 1 { &[0, 1] } else { &[0, 1, 3, 2] };
            let cp: Vec<[f64; 3]> = order.iter().map(|&k| c[k]).collect();
            let iface = if any_interface { probe_interface(arrays, v.global, false) } else { None };
            let Some(ci) = iface else {
                if !any_interface || fill_material {
                    cells.push(cp.iter().map(|&p| add_point(&mut pts, p)).collect());
                    sources.push(v.global);
                }
                return true;
            };
            let a: Vec<f64> = cp.iter().map(|&p| if ci.kind != 1 { ci.dist_a(p) } else { 0.0 }).collect();
            let b: Vec<f64> = cp.iter().map(|&p| if ci.kind != -1 { ci.dist_b(p) } else { 0.0 }).collect();
            // The crossing of an edge from p to q where the distances are u and w.
            let cross = |p: [f64; 3], q: [f64; 3], u: f64, w: f64, only: Option<usize>| -> [f64; 3] {
                let mut x = p;
                for k in 0..3 {
                    if only.is_none_or(|o| o == k) {
                        x[k] = (w * p[k] - u * q[k]) / (w - u);
                    }
                }
                x
            };
            let mut out: Vec<[f64; 3]> = Vec::new();
            if dim == 1 {
                let ax = Some(axes[0]);
                let (p, q) = (cp[0], cp[1]);
                if ci.kind == 0 {
                    if a[0] >= 0.0 && b[0] <= 0.0 {
                        out.push(p);
                    }
                    let xa = (a[0] * a[1] < 0.0).then(|| cross(p, q, a[0], a[1], ax));
                    let xb = (b[0] * b[1] < 0.0).then(|| cross(p, q, b[0], b[1], ax));
                    let k = axes[0];
                    match (xa, xb) {
                        (Some(xa), Some(xb)) => {
                            if xa[k] > xb[k] {
                                out.extend([xb, xa]);
                            } else if xa[k] == xb[k] {
                                out.push(xa);
                            } else if xa[k] < xb[k] {
                                out.extend([xa, xb]);
                            }
                        }
                        (Some(xa), None) => out.push(xa),
                        (None, Some(xb)) => out.push(xb),
                        _ => {}
                    }
                    if a[1] >= 0.0 && b[1] <= 0.0 {
                        out.push(q);
                    }
                } else {
                    let (sign, d) = if ci.kind == 1 { (-1.0, &b) } else { (1.0, &a) };
                    if sign * d[0] >= 0.0 {
                        out.push(p);
                    }
                    if d[0] * d[1] < 0.0 {
                        out.push(cross(p, q, d[0], d[1], ax));
                    }
                    if sign * d[1] >= 0.0 {
                        out.push(q);
                    }
                }
                // (VTK adds the cell even when nothing of it is in the material.)
                cells.push(out.iter().map(|&p| add_point(&mut pts, p)).collect());
                sources.push(v.global);
                return true;
            }
            let (ax1, ax2) = (axes[0], axes[1]);
            for i in 0..4 {
                let ni = (i + 1) % 4;
                let (p, q) = (cp[i], cp[ni]);
                if ci.kind == 0 {
                    if a[i] >= 0.0 && b[i] <= 0.0 {
                        out.push(p);
                    }
                    let xa = (a[i] * a[ni] < 0.0).then(|| cross(p, q, a[i], a[ni], None));
                    let xb = (b[i] * b[ni] < 0.0).then(|| cross(p, q, b[i], b[ni], None));
                    match (xa, xb) {
                        (Some(xa), Some(xb)) => {
                            // Ordered along the edge's direction of travel.
                            let (k, forward) = match i {
                                0 => (ax1, true),
                                1 => (ax2, true),
                                2 => (ax1, false),
                                _ => (ax2, false),
                            };
                            let (lo, hi) = if forward { (xa[k] > xb[k], xa[k] < xb[k]) } else { (xa[k] < xb[k], xa[k] > xb[k]) };
                            if lo {
                                out.extend([xb, xa]);
                            } else if xa[k] == xb[k] {
                                out.push(xa);
                            } else if hi {
                                out.extend([xa, xb]);
                            }
                        }
                        (Some(xa), None) => out.push(xa),
                        (None, Some(xb)) => out.push(xb),
                        _ => {}
                    }
                } else {
                    let (sign, d) = if ci.kind == 1 { (-1.0, &b) } else { (1.0, &a) };
                    if fill_material && sign * d[i] >= 0.0 {
                        out.push(p);
                    }
                    if d[i] * d[ni] < 0.0 {
                        out.push(cross(p, q, d[i], d[ni], None));
                    }
                }
            }
            if !out.is_empty() {
                cells.push(out.iter().map(|&p| add_point(&mut pts, p)).collect());
                sources.push(v.global);
            }
            true
        });
        let src = sources.clone();
        return Ok((surface_output(h, pts, cells, sources, dim == 1 || !fill_material), src));
    }
    let cur = Cursors::new(h);
    h.walk(|v| {
        if !v.leaf && !v.masked {
            return true;
        }
        let nb = cur.neighborhood(v);
        let iface = probe_interface(arrays, v.global, true);
        let mut cell = Cell3::new(iface, corners(v, &axes));
        for (f, &n) in FACE_NEIGHBOR.iter().enumerate() {
            let x = nb[n];
            let x_iface = x.is_some_and(|x| has_interface(arrays, x.global));
            let emit = if !v.masked {
                x.is_none_or(|x| x.masked) || iface.is_some() || x_iface
            } else {
                x.is_some_and(|x| x.leaf && x.level < v.depth && !x.masked)
            };
            if !emit {
                continue;
            }
            if let Some(face) = cell.face(f, &mut pts) {
                cells.push(face);
                sources.push(if v.masked { x.unwrap().global } else { v.global });
            }
        }
        for poly in cell.interface_polygons() {
            cells.push(poly);
            sources.push(v.global);
        }
        true
    });
    let src = sources.clone();
    Ok((surface_output(h, pts, cells, sources, false), src))
}

// VTK's 3-D tables (vtkHyperTreeGridGeometry3DImpl): corner k has bit 0 along x, 1 along y,
// 2 along z.
const FACE_PTS: [[usize; 4]; 6] = [[0, 1, 3, 2], [0, 4, 5, 1], [0, 2, 6, 4], [1, 3, 7, 5], [2, 6, 7, 3], [4, 5, 7, 6]];
const FACE_EDGES: [[usize; 4]; 6] = [[1, 5, 3, 0], [0, 4, 8, 2], [2, 9, 6, 1], [3, 7, 10, 4], [6, 11, 7, 5], [8, 10, 11, 9]];
const EDGE_PTS: [(usize, usize); 12] = [(0, 1), (0, 2), (0, 4), (1, 3), (1, 5), (2, 3), (2, 6), (3, 7), (4, 5), (4, 6), (5, 7), (6, 7)];
const EDGE_AXIS: [usize; 12] = [0, 1, 2, 1, 2, 0, 2, 2, 0, 1, 1, 0];
/// Face neighbors (-z, -y, -x, +x, +y, +z) in the 3x3x3 neighborhood.
const FACE_NEIGHBOR: [usize; 6] = [4, 10, 12, 14, 16, 22];
const NO_EDGE: usize = 42;

/// A point of a 3-D cell being cut (VTK's HTG3DPoint): copied by value as in VTK, so a point
/// copied before it got an output id gets one of its own later.
#[derive(Debug, Clone, Copy, Default)]
struct P3 {
    x: [f64; 3],
    valid: bool,
    id: Option<u32>,
    has_a: bool,
    has_b: bool,
    da: f64,
    db: f64,
}

/// Where an interface polygon's point lives: (edge slot, second of the pair).
type Slot = (usize, bool);

/// One 3-D cell's faces and interface polygons (VTK's GenerateCellSurface state).
struct Cell3 {
    iface: Option<CellInterface>,
    corner_x: Vec<[f64; 3]>,
    corners: [P3; 8],
    /// 12 edges, then 8 corner slots (for interfaces through corners).
    edges: [(P3, P3); 20],
    /// Interface A and B: edge slot -> (point slot, next edge slot); an absent slot reads as
    /// VTK's default-made entry (no point, next 0).
    faces: [std::collections::BTreeMap<usize, (Option<Slot>, usize)>; 2],
}

impl Cell3 {
    fn new(iface: Option<CellInterface>, corner_x: Vec<[f64; 3]>) -> Self {
        Cell3 { iface, corner_x, corners: [P3::default(); 8], edges: [(P3::default(), P3::default()); 20], faces: Default::default() }
    }
    fn kind(&self) -> i32 {
        self.iface.map(|i| i.kind).unwrap_or(2)
    }
    fn set_xyz(&self, x: [f64; 3]) -> P3 {
        let mut p = P3 { x, valid: true, ..Default::default() };
        if let Some(i) = self.iface {
            if i.kind != 1 {
                p.has_a = true;
                p.da = i.dist_a(x);
            }
            if i.kind != -1 {
                p.has_b = true;
                p.db = i.dist_b(x);
            }
        }
        p
    }
    fn set_intersect(&self, x: [f64; 3], is_a: bool) -> P3 {
        let mut p = P3 { x, valid: true, ..Default::default() };
        let i = self.iface;
        if is_a {
            p.has_a = true;
            p.da = 0.0;
            if let Some(i) = i.filter(|i| i.kind != -1) {
                p.has_b = true;
                p.db = i.dist_b(x);
            }
        } else {
            p.has_b = true;
            p.db = 0.0;
            if let Some(i) = i.filter(|i| i.kind != 1) {
                p.has_a = true;
                p.da = i.dist_a(x);
            }
        }
        p
    }
    fn inside(&self, p: &P3) -> bool {
        if !p.valid {
            return false;
        }
        match self.kind() {
            -1 => !(p.has_a && p.da < 0.0),
            0 => !(p.da > 0.0 || p.db < 0.0),
            1 => !(p.has_b && p.db > 0.0),
            _ => true,
        }
    }
    fn slot(&mut self, s: Slot) -> &mut P3 {
        if s.1 {
            &mut self.edges[s.0].1
        } else {
            &mut self.edges[s.0].0
        }
    }
    fn set_interface_face(&mut self, f: usize, edge: usize, s: Slot) {
        self.faces[f].entry(edge).or_insert((Some(s), NO_EDGE));
    }
    fn next(&mut self, f: usize, e: usize) -> usize {
        self.faces[f].entry(e).or_insert((None, 0)).1
    }
    fn set_next(&mut self, f: usize, e: usize, n: usize) {
        self.faces[f].entry(e).or_insert((None, 0)).1 = n;
    }
    /// VTK's CompleteLinkage: link edge slots e1 -> e2 in interface `f`.
    fn link(&mut self, f: usize, e1: usize, e2: usize) {
        if e1 == NO_EDGE || e2 == NO_EDGE || e1 == e2 {
            return;
        }
        let (n1, n2) = (self.next(f, e1), self.next(f, e2));
        if n1 == NO_EDGE {
            if n2 == NO_EDGE || n2 != e1 {
                self.set_next(f, e1, e2);
            }
        } else if n2 == NO_EDGE {
            self.set_next(f, e2, e1);
        } else if n2 != e1 {
            // Reverse the chain from e1, then link its old end (now e1's place) to e2.
            let mut chain = vec![e1];
            let mut next = self.next(f, e1);
            let mut guard = 0;
            while next != NO_EDGE && guard < 64 {
                chain.push(next);
                next = self.next(f, next);
                guard += 1;
            }
            let mut current = NO_EDGE;
            for &c in chain.iter().rev() {
                if current == NO_EDGE {
                    current = c;
                } else {
                    self.set_next(f, current, c);
                    current = c;
                }
            }
            self.set_next(f, current, e2);
        }
    }
    /// VTK's ComputeEdgeInterface: Some(()) when the interface lies along the edge (handled
    /// here); otherwise the crossing point (invalid if none) and its edge slot.
    fn edge_interface(&mut self, p: P3, q: P3, axis: usize, edge: usize, is_a: bool, at: &mut usize) -> Result<P3, ()> {
        let f = if is_a { 0 } else { 1 };
        let (u, w) = if is_a { (p.da, q.da) } else { (p.db, q.db) };
        if u == 0.0 {
            if w == 0.0 {
                let e1 = EDGE_PTS[edge].0 + 12;
                self.edges[e1].0 = p;
                self.edges[e1].1.valid = false;
                self.set_interface_face(f, e1, (e1, false));
                let e2 = EDGE_PTS[edge].1 + 12;
                self.edges[e2].0 = q;
                self.edges[e2].1.valid = false;
                self.set_interface_face(f, e2, (e2, false));
                self.link(f, e1, e2);
                return Err(());
            }
            *at = EDGE_PTS[edge].0 + 12;
            return Ok(p);
        }
        if w == 0.0 {
            *at = EDGE_PTS[edge].1 + 12;
            return Ok(q);
        }
        if u * w < 0.0 {
            let mut x = p.x;
            x[axis] = (w * p.x[axis] - u * q.x[axis]) / (w - u);
            let mut r = self.set_intersect(x, is_a);
            if r.x[axis] == p.x[axis] || r.x[axis] == q.x[axis] {
                r.valid = false;
            }
            *at = edge;
            return Ok(r);
        }
        Ok(P3::default())
    }
    /// VTK's ComputeEdge.
    fn edge(&mut self, p: P3, q: P3, axis: usize, edge: usize, cur: &mut [usize; 2]) {
        let (mut ea, mut eb) = (edge, edge);
        let mut pa = P3::default();
        let mut pb = P3::default();
        if p.has_a {
            match self.edge_interface(p, q, axis, edge, true, &mut ea) {
                Err(()) => return,
                Ok(x) => pa = x,
            }
        }
        if p.has_b {
            match self.edge_interface(p, q, axis, edge, false, &mut eb) {
                Err(()) => return,
                Ok(x) => pb = x,
            }
        }
        let put = |c: &mut Self, f: usize, slot: Slot, val: P3, cur: &mut [usize; 2]| {
            *c.slot(slot) = val;
            c.set_interface_face(f, slot.0, slot);
            c.link(f, cur[f], slot.0);
            cur[f] = slot.0;
        };
        if pa.valid {
            if pb.valid {
                if pa.x[axis] < pb.x[axis] {
                    if edge == ea && edge == eb {
                        put(self, 0, (edge, false), pa, cur);
                        put(self, 1, (edge, true), pb, cur);
                    } else {
                        put(self, 0, (ea, false), pa, cur);
                        put(self, 1, (eb, true), pb, cur);
                    }
                }
                if pa.x[axis] > pb.x[axis] {
                    if edge == ea && edge == eb {
                        put(self, 1, (edge, false), pb, cur);
                        put(self, 0, (edge, true), pa, cur);
                    } else {
                        self.edges[ea].0 = pa;
                        self.edges[ea].1.valid = false;
                        self.set_interface_face(0, ea, (ea, false));
                        self.link(0, cur[0], ea);
                        cur[0] = ea;
                        self.edges[eb].1 = pb;
                        self.edges[eb].1.valid = false;
                        self.set_interface_face(1, eb, (eb, true));
                        self.link(1, cur[1], eb);
                        cur[1] = eb;
                    }
                }
            } else {
                put(self, 0, (ea, false), pa, cur);
            }
        } else if pb.valid {
            put(self, 1, (eb, false), pb, cur);
        }
    }
    /// VTK's GenerateOneCellFace: the face's corners and interface crossings that are in the
    /// material, or None when fewer than three remain.
    fn face(&mut self, f: usize, pts: &mut Vec<f64>) -> Option<Vec<u32>> {
        for &k in &FACE_PTS[f] {
            if !self.corners[k].valid {
                self.corners[k] = self.set_xyz(self.corner_x[k]);
            }
        }
        let mut out: Vec<u32> = Vec::new();
        let mut last: Option<u32> = None;
        let mut cur = [NO_EDGE; 2];
        for e in 0..4 {
            let fe = FACE_EDGES[f][e];
            let (a, b) = EDGE_PTS[fe];
            let (p, q) = (self.corners[a], self.corners[b]);
            self.edge(p, q, EDGE_AXIS[fe], fe, &mut cur);
            let order: [(bool, usize, bool); 3] = if e < 2 { [(true, a, false), (false, fe, false), (false, fe, true)] } else { [(true, b, false), (false, fe, true), (false, fe, false)] };
            for (is_corner, i, second) in order {
                let pt = if is_corner { self.corners[i] } else if second { self.edges[i].1 } else { self.edges[i].0 };
                if !pt.valid {
                    continue;
                }
                let mut id = None;
                if self.inside(&pt) {
                    let target = if is_corner { &mut self.corners[i] } else if second { &mut self.edges[i].1 } else { &mut self.edges[i].0 };
                    id = Some(*target.id.get_or_insert_with(|| {
                        pts.extend_from_slice(&target.x);
                        (pts.len() / 3 - 1) as u32
                    }));
                }
                if let Some(id) = id {
                    if Some(id) != last {
                        out.push(id);
                        last = Some(id);
                    }
                }
            }
        }
        (out.len() > 2).then_some(out)
    }
    /// The interface polygons (A, then B) from the linked crossing points. Points that never
    /// got an output id are skipped (VTK writes id -1 there).
    fn interface_polygons(&self) -> Vec<Vec<u32>> {
        let mut polys = Vec::new();
        for f in 0..2 {
            let m = &self.faces[f];
            if m.len() < 3 {
                continue;
            }
            let Some((&first, _)) = m.iter().next() else { continue };
            if first == NO_EDGE {
                continue;
            }
            let get = |e: usize| m.get(&e).copied().unwrap_or((None, 0));
            let id_of = |s: Option<Slot>| s.and_then(|s| if s.1 { self.edges[s.0].1.id } else { self.edges[s.0].0.id });
            let mut ids: Vec<Option<u32>> = vec![id_of(get(first).0)];
            let mut next = get(first).1;
            let mut guard = 0;
            while next != first && next != NO_EDGE && guard < 64 {
                ids.push(id_of(get(next).0));
                next = get(next).1;
                guard += 1;
            }
            if next == NO_EDGE || guard >= 64 {
                continue;
            }
            let ids: Vec<u32> = ids.into_iter().flatten().collect();
            if !ids.is_empty() {
                polys.push(ids);
            }
        }
        polys
    }
}

/// Hyper Tree Grid Feature Edges, as VTK's vtkHyperTreeGridFeatureEdges: the edges of the
/// visible region (in 2-D the leaf edges where it ends; in 3-D the leaf edges on its outer
/// corners and folds), each a line with its leaf's values, points not merged.
pub fn feature_edges(h: &HyperTreeGrid) -> Result<PolyData> {
    let axes = h.axes();
    let dim = axes.len();
    if dim == 0 {
        bail!("the hyper tree grid has no cells");
    }
    let (mut pts, mut cells, mut sources) = (Vec::new(), Vec::new(), Vec::new());
    let mut add = |a: [f64; 3], b: [f64; 3], src: usize, pts: &mut Vec<f64>| {
        let base = (pts.len() / 3) as u32;
        pts.extend_from_slice(&a);
        pts.extend_from_slice(&b);
        cells.push(vec![base, base + 1]);
        sources.push(src);
    };
    if dim == 1 {
        h.walk(|v| {
            if v.leaf && !v.masked {
                let c = corners(v, &axes);
                add(c[0], c[1], v.global, &mut pts);
            }
            true
        });
        return Ok(surface_output(h, pts, cells, sources, true));
    }
    let cur = Cursors::new(h);
    // 2-D: (-y, -x, +x, +y) neighbors and the edges facing them; corners 0 1 2 3 counter-clockwise.
    const N2: [usize; 4] = [1, 3, 5, 7];
    const E2: [[usize; 2]; 4] = [[0, 1], [0, 3], [1, 2], [2, 3]];
    const E3: [[usize; 2]; 12] = [[0, 1], [2, 3], [0, 2], [1, 3], [4, 5], [6, 7], [4, 6], [5, 7], [0, 4], [1, 5], [2, 6], [3, 7]];
    const M3: [[usize; 3]; 12] = [[4, 1, 10], [4, 7, 16], [4, 3, 12], [4, 5, 14], [22, 19, 10], [22, 25, 16], [22, 21, 12], [22, 23, 14], [12, 9, 10], [14, 11, 10], [12, 15, 16], [14, 17, 16]];
    h.walk(|v| {
        if !v.leaf {
            return true;
        }
        let nb = cur.neighborhood(v);
        let visible = |n: usize| nb[n].is_some_and(|x| !x.masked);
        let inf = |n: usize| visible(n) && nb[n].is_some_and(|x| x.level < v.depth);
        let vleaf = |n: usize| visible(n) && nb[n].is_some_and(|x| x.leaf);
        let c = corners(v, &axes);
        if dim == 2 {
            let quad = [c[0], c[1], c[3], c[2]];
            for (e, &n) in N2.iter().enumerate() {
                if (!v.masked && !visible(n)) || (v.masked && inf(n)) {
                    add(quad[E2[e][0]], quad[E2[e][1]], v.global, &mut pts);
                }
            }
        } else {
            for (e, m) in M3.iter().enumerate() {
                let [a, d, b] = *m;
                let none = !visible(a) && !visible(d) && !visible(b);
                let only_diag = !visible(a) && vleaf(d) && !visible(b);
                let only_two = (!visible(a) && vleaf(d) && vleaf(b)) || (vleaf(a) && !visible(d) && vleaf(b)) || (vleaf(a) && vleaf(d) && !visible(b));
                let only_one = (inf(a) && !visible(d) && !visible(b)) || (!visible(a) && inf(d) && !visible(b)) || (!visible(a) && !visible(d) && inf(b));
                let two_except_diag = inf(a) && !visible(d) && inf(b);
                let all = inf(a) && inf(d) && inf(b);
                if (!v.masked && (none || only_diag || only_two)) || (v.masked && (only_one || two_except_diag || all)) {
                    add(c[E3[e][0]], c[E3[e][1]], v.global, &mut pts);
                }
            }
        }
        true
    });
    Ok(surface_output(h, pts, cells, sources, true))
}
