//! Structured hexahedral blocks: a box, or any hexahedral region given by its eight corners,
//! with optional grading along each direction.

use crate::hex::{HexMesh, P3};
use crate::inclusions::graded_weights;

/// Cells along one direction and how they are spread.
#[derive(Clone, Debug)]
pub struct Division {
    pub cells: usize,
    /// Largest over smallest interval (1 = uniform).
    pub ratio: f64,
    /// "none", "start", "end" or "both" (where the small intervals go).
    pub cluster: String,
}

impl Division {
    pub fn uniform(cells: usize) -> Division {
        Division { cells, ratio: 1.0, cluster: "none".into() }
    }
    pub fn weights(&self) -> Vec<f64> {
        graded_weights(self.cells.max(1), self.ratio, &self.cluster)
    }
}

/// A hexahedral block from its eight corners (Exodus order) by trilinear mapping, named
/// `block 1` with side sets and node sets on its six faces named `x-`, `x+`, `y-`, `y+`, `z-`,
/// `z+` (the faces at the start and end of each of the block's three directions).
pub fn mapped_block(corners: [P3; 8], div: [&Division; 3]) -> HexMesh {
    let (wu, wv, ww) = (div[0].weights(), div[1].weights(), div[2].weights());
    let (nu, nv, nw) = (wu.len(), wv.len(), ww.len());
    let mut m = HexMesh { npe: 8, ..Default::default() };
    for k in 0..nw {
        for j in 0..nv {
            for i in 0..nu {
                let (u, v, w) = (wu[i], wv[j], ww[k]);
                let s = [
                    (1.0 - u) * (1.0 - v) * (1.0 - w),
                    u * (1.0 - v) * (1.0 - w),
                    u * v * (1.0 - w),
                    (1.0 - u) * v * (1.0 - w),
                    (1.0 - u) * (1.0 - v) * w,
                    u * (1.0 - v) * w,
                    u * v * w,
                    (1.0 - u) * v * w,
                ];
                m.points.push(std::array::from_fn(|a| (0..8).map(|c| s[c] * corners[c][a]).sum()));
            }
        }
    }
    let id = |i: usize, j: usize, k: usize| ((k * nv + j) * nu + i) as u32;
    let mut faces: [Vec<(u32, u8)>; 6] = Default::default();
    let mut e = 0u32;
    for k in 0..nw - 1 {
        for j in 0..nv - 1 {
            for i in 0..nu - 1 {
                m.cells.extend_from_slice(&[
                    id(i, j, k),
                    id(i + 1, j, k),
                    id(i + 1, j + 1, k),
                    id(i, j + 1, k),
                    id(i, j, k + 1),
                    id(i + 1, j, k + 1),
                    id(i + 1, j + 1, k + 1),
                    id(i, j + 1, k + 1),
                ]);
                // Exodus sides: 4 = -u, 2 = +u, 1 = -v, 3 = +v, 5 = -w, 6 = +w.
                if i == 0 {
                    faces[0].push((e, 4));
                }
                if i + 2 == nu {
                    faces[1].push((e, 2));
                }
                if j == 0 {
                    faces[2].push((e, 1));
                }
                if j + 2 == nv {
                    faces[3].push((e, 3));
                }
                if k == 0 {
                    faces[4].push((e, 5));
                }
                if k + 2 == nw {
                    faces[5].push((e, 6));
                }
                e += 1;
            }
        }
    }
    m.block = vec![1; e as usize];
    m.block_names.insert(1, "block_1".into());
    let names = ["x-", "x+", "y-", "y+", "z-", "z+"];
    for (f, name) in names.iter().enumerate() {
        let mut nodes = Vec::new();
        for (el, s) in &faces[f] {
            nodes.extend(m.side_corners(*el as usize, *s));
        }
        m.add_side_set(name, std::mem::take(&mut faces[f]));
        m.add_node_set(name, nodes);
    }
    let vol: f64 = m.signed_volumes().iter().sum();
    if vol < 0.0 {
        m.fix_orientation();
    }
    m
}

/// A box from `lo` to `hi`.
pub fn box_mesh(lo: P3, hi: P3, div: [&Division; 3]) -> HexMesh {
    let c =
        |x: usize, y: usize, z: usize| -> P3 { [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }] };
    mapped_block([c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(0, 1, 0), c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)], div)
}

/// Sweep a quadrilateral surface mesh into hexahedra: `layers` layers along `direction` over
/// `distance` (graded by `ratio` towards `cluster`). Blocks come from `quad_block` (one per
/// quad). Side sets: `bottom` (the swept surface), `top`, `side` (faces swept from the
/// surface's boundary edges); node sets the same.
#[allow(clippy::too_many_arguments)]
pub fn sweep_quads(points: &[P3], quads: &[[u32; 4]], quad_block: &[i32], direction: P3, distance: f64, layers: &Division) -> Result<HexMesh, String> {
    let dl = (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt();
    if dl == 0.0 || distance == 0.0 {
        return Err("the sweep needs a direction and a distance".into());
    }
    let d: P3 = direction.map(|v| v / dl * distance);
    let w = layers.weights();
    let nl = w.len() - 1;
    let np = points.len() as u32;
    let mut m = HexMesh { npe: 8, ..Default::default() };
    for t in &w {
        for p in points {
            m.points.push([p[0] + t * d[0], p[1] + t * d[1], p[2] + t * d[2]]);
        }
    }
    // Boundary edges of the surface: used by one quad.
    let mut edge_count: std::collections::HashMap<(u32, u32), u32> = Default::default();
    for q in quads {
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            *edge_count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let (mut bottom, mut top, mut side) = (Vec::new(), Vec::new(), Vec::new());
    for l in 0..nl {
        let (o0, o1) = (l as u32 * np, (l as u32 + 1) * np);
        for (qi, q) in quads.iter().enumerate() {
            let e = m.n_elements() as u32;
            m.cells.extend_from_slice(&[q[0] + o0, q[1] + o0, q[2] + o0, q[3] + o0, q[0] + o1, q[1] + o1, q[2] + o1, q[3] + o1]);
            m.block.push(quad_block.get(qi).copied().unwrap_or(1));
            if l == 0 {
                bottom.push((e, 5u8));
            }
            if l + 1 == nl {
                top.push((e, 6u8));
            }
            // Quad edge k (q[k] -> q[k+1]) sweeps into Exodus side k + 1.
            for k in 0..4 {
                let (a, b) = (q[k], q[(k + 1) % 4]);
                if edge_count[&(a.min(b), a.max(b))] == 1 {
                    side.push((e, k as u8 + 1));
                }
            }
        }
    }
    for b in m.block.clone() {
        m.block_names.entry(b).or_insert_with(|| format!("block_{b}"));
    }
    m.add_side_set("bottom", bottom);
    m.add_side_set("top", top);
    m.add_side_set("side", side);
    if m.fix_orientation() > 0 {
        // A surface wound the other way round: bottom and top kept their meaning.
        let (b, t) = (m.side_set("bottom").cloned().unwrap_or_default(), m.side_set("top").cloned().unwrap_or_default());
        m.add_side_set("bottom", b);
        m.add_side_set("top", t);
    }
    for name in ["bottom", "top", "side"] {
        let faces = m.side_set(name).cloned().unwrap_or_default();
        let nodes: Vec<u32> = faces.iter().flat_map(|(e, s)| m.side_corners(*e as usize, *s)).collect();
        m.add_node_set(name, nodes);
    }
    Ok(m)
}

/// A disk of quadrilaterals (an O-grid "butterfly"): a square core of `n` x `n` cells and a
/// ring of `rings` layers out to the circle of `radius`; returns points (z = 0) and quads
/// wound counter-clockwise.
pub fn disk_quads(radius: f64, n: usize, rings: usize) -> (Vec<P3>, Vec<[u32; 4]>) {
    let n = n.max(1);
    let rings = rings.max(1);
    let s = radius * 0.45;
    let mut pts: Vec<P3> = Vec::new();
    let mut quads = Vec::new();
    let core = |i: usize, j: usize| ((i * (n + 1)) + j) as u32;
    let u: Vec<f64> = (0..=n).map(|i| -s + 2.0 * s * i as f64 / n as f64).collect();
    for x in &u {
        for y in &u {
            pts.push([*x, *y, 0.0]);
        }
    }
    for i in 0..n {
        for j in 0..n {
            quads.push([core(i, j), core(i + 1, j), core(i + 1, j + 1), core(i, j + 1)]);
        }
    }
    // Square perimeter, counter-clockwise from (s, -s).
    let mut per: Vec<u32> = Vec::new();
    per.extend((0..n).map(|j| core(n, j)));
    per.extend((0..n).map(|i| core(n - i, n)));
    per.extend((0..n).map(|j| core(0, n - j)));
    per.extend((0..n).map(|i| core(i, 0)));
    let m = per.len();
    let mut prev = per.clone();
    for r in 1..=rings {
        let t = r as f64 / rings as f64;
        let layer: Vec<u32> = per
            .iter()
            .map(|id| {
                let p = pts[*id as usize];
                let a = p[1].atan2(p[0]);
                let c = [radius * a.cos(), radius * a.sin()];
                pts.push([(1.0 - t) * p[0] + t * c[0], (1.0 - t) * p[1] + t * c[1], 0.0]);
                pts.len() as u32 - 1
            })
            .collect();
        for k in 0..m {
            let kn = (k + 1) % m;
            quads.push([prev[k], layer[k], layer[kn], prev[kn]]);
        }
        prev = layer;
    }
    // Wind every quad counter-clockwise (seen from +z).
    for q in quads.iter_mut() {
        let [a, b, c, _] = q.map(|i| pts[i as usize]);
        if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) < 0.0 {
            q.reverse();
        }
    }
    (pts, quads)
}
