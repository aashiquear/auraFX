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
                let s = [(1.0 - u) * (1.0 - v) * (1.0 - w), u * (1.0 - v) * (1.0 - w), u * v * (1.0 - w), (1.0 - u) * v * (1.0 - w), (1.0 - u) * (1.0 - v) * w, u * (1.0 - v) * w, u * v * w, (1.0 - u) * v * w];
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
                m.cells.extend_from_slice(&[id(i, j, k), id(i + 1, j, k), id(i + 1, j + 1, k), id(i, j + 1, k), id(i, j, k + 1), id(i + 1, j, k + 1), id(i + 1, j + 1, k + 1), id(i, j + 1, k + 1)]);
                // Exodus sides: 4 = -u, 2 = +u, 1 = -v, 3 = +v, 5 = -w, 6 = +w.
                if i == 0 { faces[0].push((e, 4)); }
                if i + 2 == nu { faces[1].push((e, 2)); }
                if j == 0 { faces[2].push((e, 1)); }
                if j + 2 == nv { faces[3].push((e, 3)); }
                if k == 0 { faces[4].push((e, 5)); }
                if k + 2 == nw { faces[5].push((e, 6)); }
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
    let c = |x: usize, y: usize, z: usize| -> P3 { [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }] };
    mapped_block([c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(0, 1, 0), c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)], div)
}
