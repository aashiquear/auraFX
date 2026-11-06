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
    let masks = mask.map(levels_of);
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
pub fn random(dims: [usize; 3], bounds: [f64; 6], seed: i32, max_depth: usize, split: f64) -> Result<HyperTreeGrid> {
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
                offset += verts.len();
                htg.trees[tree_id] = Some(tree);
            }
        }
    }
    htg.cell_data.insert(Array::new("Depth", 1, ArrayData::F64(depth)));
    Ok(htg)
}
