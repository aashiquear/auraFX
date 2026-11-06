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
            (true, _) => CellType::Line,
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

/// Hyper Tree Grid Geometry, as VTK's vtkHyperTreeGridGeometry (without material interfaces):
/// a line (1-D) or quad (2-D) per visible leaf; in 3-D the faces of visible leaves on the grid's
/// boundary or next to masked cells, and the faces of masked cells next to a coarser visible
/// leaf (carrying that leaf's values). Points are shared only within a cell, as in VTK.
pub fn geometry(h: &HyperTreeGrid) -> Result<PolyData> {
    let axes = h.axes();
    let dim = axes.len();
    if dim == 0 {
        bail!("the hyper tree grid has no cells");
    }
    let (mut pts, mut cells, mut sources) = (Vec::new(), Vec::new(), Vec::new());
    if dim < 3 {
        h.walk(|v| {
            if v.leaf && !v.masked {
                let c = corners(v, &axes);
                let order: &[usize] = if dim == 1 { &[0, 1] } else { &[0, 1, 3, 2] };
                let base = (pts.len() / 3) as u32;
                for &k in order {
                    pts.extend_from_slice(&c[k]);
                }
                cells.push((0..order.len() as u32).map(|k| base + k).collect());
                sources.push(v.global);
            }
            true
        });
        return Ok(surface_output(h, pts, cells, sources, dim == 1));
    }
    // Each face's corners in VTK's output order (it walks the face's edges).
    const FACE_PTS: [[usize; 4]; 6] = [[0, 2, 3, 1], [0, 1, 5, 4], [0, 4, 6, 2], [1, 3, 7, 5], [2, 6, 7, 3], [4, 5, 7, 6]];
    // Face neighbors (-z, -y, -x, +x, +y, +z) in the 3x3x3 neighborhood.
    const FACE_NEIGHBOR: [usize; 6] = [4, 10, 12, 14, 16, 22];
    let cur = Cursors::new(h);
    h.walk(|v| {
        if !v.leaf && !v.masked {
            return true;
        }
        let nb = cur.neighborhood(v);
        let c = corners(v, &axes);
        let mut ids: [Option<u32>; 8] = [None; 8];
        for (f, &n) in FACE_NEIGHBOR.iter().enumerate() {
            let x = nb[n];
            let emit = if !v.masked { x.is_none_or(|x| x.masked) } else { x.is_some_and(|x| x.leaf && x.level < v.depth && !x.masked) };
            if !emit {
                continue;
            }
            let face: Vec<u32> = FACE_PTS[f]
                .iter()
                .map(|&k| {
                    *ids[k].get_or_insert_with(|| {
                        pts.extend_from_slice(&c[k]);
                        (pts.len() / 3 - 1) as u32
                    })
                })
                .collect();
            cells.push(face);
            sources.push(if v.masked { x.unwrap().global } else { v.global });
        }
        true
    });
    Ok(surface_output(h, pts, cells, sources, false))
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
