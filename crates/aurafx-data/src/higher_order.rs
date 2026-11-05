//! Arbitrary-order Lagrange and Bezier cells (VTK types 68-81) and the complete quadratic
//! types that are the same as order-2 Lagrange cells.
//!
//! Node numbering follows VTK exactly (corners, then edges, faces and the interior); it is
//! checked against VTK's parametric coordinates for orders 1 to 6 in the tests. Shapes are
//! evaluated with Lagrange polynomials on equispaced nodes or Bernstein polynomials, with
//! optional rational weights for Bezier cells.

use crate::CellType;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Basis {
    Lagrange,
    Bezier,
    /// Quadratic cells with corner and mid-edge nodes only (8-node quad, 20-node hex,
    /// 15-node wedge).
    Serendipity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    Curve,
    Triangle,
    Quad,
    Tetra,
    Hexahedron,
    Wedge,
}

impl Shape {
    pub fn dimension(self) -> usize {
        match self {
            Shape::Curve => 1,
            Shape::Triangle | Shape::Quad => 2,
            _ => 3,
        }
    }

    pub fn is_simplex(self) -> bool {
        matches!(self, Shape::Triangle | Shape::Tetra)
    }

    /// The linear VTK type with the same corners.
    pub fn linear_type(self) -> CellType {
        match self {
            Shape::Curve => CellType::Line,
            Shape::Triangle => CellType::Triangle,
            Shape::Quad => CellType::Quad,
            Shape::Tetra => CellType::Tetra,
            Shape::Hexahedron => CellType::Hexahedron,
            Shape::Wedge => CellType::Wedge,
        }
    }

    /// Parametric coordinates of the corners, in VTK order.
    pub fn corners(self) -> &'static [[f64; 3]] {
        match self {
            Shape::Curve => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            Shape::Triangle => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            Shape::Quad => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            Shape::Tetra => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Shape::Hexahedron => {
                &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]
            }
            Shape::Wedge => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
        }
    }
}

impl CellType {
    /// Basis and shape of a higher-order cell this module can evaluate: the Lagrange and
    /// Bezier types, plus the complete quadratic types (3-node edge, 6-node triangle, 9-node
    /// quad, 10-node tet, 27-node hex, 18-node wedge) whose nodes are those of order-2
    /// Lagrange cells.
    pub fn high_order(self) -> Option<(Basis, Shape)> {
        use CellType::*;
        Some(match self {
            LagrangeCurve | QuadraticEdge | CubicLine => (Basis::Lagrange, Shape::Curve),
            LagrangeTriangle | QuadraticTriangle => (Basis::Lagrange, Shape::Triangle),
            LagrangeQuadrilateral | BiquadraticQuad | QuadraticLinearQuad => (Basis::Lagrange, Shape::Quad),
            LagrangeTetrahedron | QuadraticTetra => (Basis::Lagrange, Shape::Tetra),
            LagrangeHexahedron | TriquadraticHexahedron => (Basis::Lagrange, Shape::Hexahedron),
            LagrangeWedge | BiquadraticQuadraticWedge | QuadraticLinearWedge => (Basis::Lagrange, Shape::Wedge),
            QuadraticQuad => (Basis::Serendipity, Shape::Quad),
            QuadraticHexahedron => (Basis::Serendipity, Shape::Hexahedron),
            QuadraticWedge => (Basis::Serendipity, Shape::Wedge),
            BezierCurve => (Basis::Bezier, Shape::Curve),
            BezierTriangle => (Basis::Bezier, Shape::Triangle),
            BezierQuadrilateral => (Basis::Bezier, Shape::Quad),
            BezierTetrahedron => (Basis::Bezier, Shape::Tetra),
            BezierHexahedron => (Basis::Bezier, Shape::Hexahedron),
            BezierWedge => (Basis::Bezier, Shape::Wedge),
            _ => return None,
        })
    }

    /// Lagrange or Bezier type (arbitrary order).
    pub fn is_arbitrary_order(self) -> bool {
        (68..=81).contains(&(self as u8))
    }
}

/// Polynomial orders along r, s, t (simplices use one order; wedges use r/s and t).
pub type Order = [usize; 3];

/// Order of a cell from its point count, or from `degrees` (VTK's `HigherOrderDegrees` cell
/// array) when given and consistent with the point count.
pub fn cell_order(shape: Shape, npts: usize, degrees: Option<[f64; 3]>) -> Option<Order> {
    if let Some(d) = degrees {
        let o = [d[0].round().max(0.0) as usize, d[1].round().max(0.0) as usize, d[2].round().max(0.0) as usize];
        let o = match shape {
            Shape::Curve => [o[0], 0, 0],
            Shape::Triangle | Shape::Tetra => [o[0], o[0], o[0]],
            Shape::Quad => [o[0], o[1], 0],
            Shape::Hexahedron => o,
            Shape::Wedge => [o[0], o[0], o[2]],
        };
        if o[0] >= 1 && node_count(shape, o) == npts {
            return Some(o);
        }
    }
    let (p, _) = (1..=20).map(|p| (p, node_count(shape, uniform(shape, p)))).find(|&(_, n)| n == npts)?;
    Some(uniform(shape, p))
}

fn uniform(shape: Shape, p: usize) -> Order {
    match shape {
        Shape::Curve => [p, 0, 0],
        Shape::Triangle | Shape::Quad => [p, p, 0],
        _ => [p, p, p],
    }
}

pub fn node_count(shape: Shape, o: Order) -> usize {
    match shape {
        Shape::Curve => o[0] + 1,
        Shape::Triangle => (o[0] + 1) * (o[0] + 2) / 2,
        Shape::Quad => (o[0] + 1) * (o[1] + 1),
        Shape::Tetra => (o[0] + 1) * (o[0] + 2) * (o[0] + 3) / 6,
        Shape::Hexahedron => (o[0] + 1) * (o[1] + 1) * (o[2] + 1),
        Shape::Wedge => (o[0] + 1) * (o[0] + 2) / 2 * (o[2] + 1),
    }
}

/// Integer lattice position of every node, in VTK node order. Parametric coordinates are
/// `lattice[k][a] / order[a]`.
pub fn lattice(shape: Shape, o: Order) -> Arc<Vec<[usize; 3]>> {
    static CACHE: OnceLock<Mutex<HashMap<(Shape, Order), Arc<Vec<[usize; 3]>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(l) = cache.lock().unwrap().get(&(shape, o)) {
        return l.clone();
    }
    let l: Vec<[usize; 3]> = match shape {
        Shape::Curve => {
            let p = o[0];
            let mut v = vec![[0, 0, 0], [p, 0, 0]];
            v.extend((1..p).map(|k| [k, 0, 0]));
            v
        }
        Shape::Triangle => tri_nodes(o[0] as isize).into_iter().map(|(i, j)| [i, j, 0]).collect(),
        Shape::Quad => quad_nodes(o[0], o[1], 0),
        Shape::Tetra => tet_nodes(o[0] as isize),
        Shape::Hexahedron => hex_nodes(o),
        Shape::Wedge => wedge_nodes(o[0], o[2]),
    };
    let l = Arc::new(l);
    cache.lock().unwrap().insert((shape, o), l.clone());
    l
}

/// Triangle nodes: corners, edges 0-1, 1-2, 2-0, then the interior as a triangle of order p-3.
fn tri_nodes(p: isize) -> Vec<(usize, usize)> {
    if p < 0 {
        return vec![];
    }
    if p == 0 {
        return vec![(0, 0)];
    }
    let u = p as usize;
    let mut v = vec![(0, 0), (u, 0), (0, u)];
    v.extend((1..u).map(|k| (k, 0)));
    v.extend((1..u).map(|k| (u - k, k)));
    v.extend((1..u).map(|k| (0, u - k)));
    v.extend(tri_nodes(p - 3).into_iter().map(|(i, j)| (i + 1, j + 1)));
    v
}

/// Quad nodes in a plane with fixed third coordinate: corners, edges along +r (bottom), +s
/// (right), +r (top), +s (left), then the interior row by row.
fn quad_nodes(p: usize, q: usize, z: usize) -> Vec<[usize; 3]> {
    let mut v = vec![[0, 0, z], [p, 0, z], [p, q, z], [0, q, z]];
    v.extend((1..p).map(|k| [k, 0, z]));
    v.extend((1..q).map(|k| [p, k, z]));
    v.extend((1..p).map(|k| [k, q, z]));
    v.extend((1..q).map(|k| [0, k, z]));
    for j in 1..q {
        v.extend((1..p).map(|i| [i, j, z]));
    }
    v
}

fn tet_nodes(p: isize) -> Vec<[usize; 3]> {
    if p < 0 {
        return vec![];
    }
    if p == 0 {
        return vec![[0, 0, 0]];
    }
    let u = p as usize;
    let c = [[0, 0, 0], [u, 0, 0], [0, u, 0], [0, 0, u]];
    let mut v: Vec<[usize; 3]> = c.to_vec();
    for (a, b) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)] {
        for k in 1..u {
            v.push(std::array::from_fn(|t| (c[a][t] * (u - k) + c[b][t] * k) / u));
        }
    }
    // Face interiors: triangles of order p-3 whose corners sit next to the face corners in
    // this order.
    for f in [[0, 1, 3], [2, 3, 1], [0, 3, 2], [0, 2, 1]] {
        let q = u.saturating_sub(3);
        for (i, j) in tri_nodes(p - 3) {
            let w = [1 + (q - i - j), 1 + i, 1 + j];
            v.push(std::array::from_fn(|t| (0..3).map(|m| w[m] * c[f[m]][t]).sum::<usize>() / u));
        }
    }
    v.extend(tet_nodes(p - 4).into_iter().map(|[i, j, k]| [i + 1, j + 1, k + 1]));
    v
}

fn hex_nodes(o: Order) -> Vec<[usize; 3]> {
    let [p, q, r] = o;
    let mut v = vec![[0, 0, 0], [p, 0, 0], [p, q, 0], [0, q, 0], [0, 0, r], [p, 0, r], [p, q, r], [0, q, r]];
    for z in [0, r] {
        v.extend((1..p).map(|k| [k, 0, z]));
        v.extend((1..q).map(|k| [p, k, z]));
        v.extend((1..p).map(|k| [k, q, z]));
        v.extend((1..q).map(|k| [0, k, z]));
    }
    for (i, j) in [(0, 0), (p, 0), (p, q), (0, q)] {
        v.extend((1..r).map(|k| [i, j, k]));
    }
    for x in [0, p] {
        for k in 1..r {
            v.extend((1..q).map(|j| [x, j, k]));
        }
    }
    for y in [0, q] {
        for k in 1..r {
            v.extend((1..p).map(|i| [i, y, k]));
        }
    }
    for z in [0, r] {
        for j in 1..q {
            v.extend((1..p).map(|i| [i, j, z]));
        }
    }
    for k in 1..r {
        for j in 1..q {
            v.extend((1..p).map(|i| [i, j, k]));
        }
    }
    v
}

fn wedge_nodes(p: usize, q: usize) -> Vec<[usize; 3]> {
    // Wedge triangle faces and the interior use a row-by-row order (unlike triangle cells).
    let tri_int: Vec<(usize, usize)> = (1..p).flat_map(|j| (1..p.saturating_sub(j)).map(move |i| (i, j))).collect();
    let mut v = vec![[0, 0, 0], [p, 0, 0], [0, p, 0], [0, 0, q], [p, 0, q], [0, p, q]];
    for z in [0, q] {
        v.extend((1..p).map(|k| [k, 0, z]));
        v.extend((1..p).map(|k| [p - k, k, z]));
        v.extend((1..p).map(|k| [0, p - k, z]));
    }
    for (i, j) in [(0, 0), (p, 0), (0, p)] {
        v.extend((1..q).map(|k| [i, j, k]));
    }
    for z in [0, q] {
        v.extend(tri_int.iter().map(|&(i, j)| [i, j, z]));
    }
    for k in 1..q {
        v.extend((1..p).map(|i| [i, 0, k]));
    }
    for k in 1..q {
        v.extend((1..p).map(|i| [p - i, i, k]));
    }
    for k in 1..q {
        v.extend((1..p).map(|j| [0, p - j, k]));
    }
    for k in 1..q {
        v.extend(tri_int.iter().map(|&(i, j)| [i, j, k]));
    }
    v
}

/// One higher-order cell type at one order, ready to evaluate.
#[derive(Debug, Clone)]
pub struct HighOrderCell {
    pub basis: Basis,
    pub shape: Shape,
    pub order: Order,
    pub lattice: Arc<Vec<[usize; 3]>>,
    /// Node index of each lattice position (dense over (order+1)^3, `u32::MAX` where none).
    index: Arc<Vec<u32>>,
}

fn lattice_index(o: Order, l: &[[usize; 3]]) -> Vec<u32> {
    let mut v = vec![u32::MAX; (o[0] + 1) * (o[1] + 1) * (o[2] + 1)];
    for (k, n) in l.iter().enumerate() {
        v[(n[2] * (o[1] + 1) + n[1]) * (o[0] + 1) + n[0]] = k as u32;
    }
    v
}

impl HighOrderCell {
    pub fn new(ty: CellType, npts: usize, degrees: Option<[f64; 3]>) -> Option<Self> {
        let (basis, shape) = ty.high_order()?;
        // Fixed-order quadratic types (quadratic along r/s, linear along the last axis).
        let order = match ty {
            CellType::QuadraticLinearQuad if npts == 6 => [2, 1, 0],
            CellType::QuadraticLinearWedge if npts == 12 => [2, 2, 1],
            CellType::QuadraticQuad if npts == 8 => [2, 2, 0],
            CellType::QuadraticHexahedron if npts == 20 => [2, 2, 2],
            CellType::QuadraticWedge if npts == 15 => [2, 2, 2],
            _ if basis == Basis::Serendipity => return None,
            _ => cell_order(shape, npts, degrees)?,
        };
        Some(Self::with_order(basis, shape, order))
    }

    pub fn with_order(basis: Basis, shape: Shape, order: Order) -> Self {
        type Key = (Shape, Order, bool);
        static CACHE: OnceLock<Mutex<HashMap<Key, (Arc<Vec<[usize; 3]>>, Arc<Vec<u32>>)>>> = OnceLock::new();
        let ser = basis == Basis::Serendipity;
        let cache = CACHE.get_or_init(Default::default);
        let (lattice, index) = cache
            .lock()
            .unwrap()
            .entry((shape, order, ser))
            .or_insert_with(|| {
                let l = if ser { Arc::new(serendipity_nodes(shape)) } else { lattice(shape, order) };
                let i = Arc::new(lattice_index(order, &l));
                (l, i)
            })
            .clone();
        Self { basis, shape, order, lattice, index }
    }

    pub fn len(&self) -> usize {
        self.lattice.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lattice.is_empty()
    }

    /// Parametric coordinates of node `k`.
    pub fn node_pcoords(&self, k: usize) -> [f64; 3] {
        let l = self.lattice[k];
        std::array::from_fn(|a| if self.order[a] == 0 { 0.0 } else { l[a] as f64 / self.order[a] as f64 })
    }

    /// Node at a parametric location, if one sits there.
    pub fn node_at(&self, pc: [f64; 3]) -> Option<usize> {
        let o = self.order;
        let mut l = [0usize; 3];
        for a in 0..self.shape.dimension() {
            let x = pc[a] * o[a] as f64;
            let r = x.round();
            if (x - r).abs() > 1e-9 || r < 0.0 || r as usize > o[a] {
                return None;
            }
            l[a] = r as usize;
        }
        self.node_at_lattice(l)
    }

    /// Node at an integer lattice position.
    pub fn node_at_lattice(&self, l: [usize; 3]) -> Option<usize> {
        let o = self.order;
        if l[0] > o[0] || l[1] > o[1] || l[2] > o[2] {
            return None;
        }
        let k = self.index[(l[2] * (o[1] + 1) + l[1]) * (o[0] + 1) + l[0]];
        (k != u32::MAX).then_some(k as usize)
    }

    /// Shape function values at parametric point `pc`, one per node, in node order.
    pub fn weights(&self, pc: [f64; 3], out: &mut Vec<f64>) {
        out.clear();
        let o = self.order;
        if self.basis == Basis::Serendipity {
            serendipity_weights(self.shape, &self.lattice, pc, out);
            return;
        }
        match self.shape {
            Shape::Curve | Shape::Quad | Shape::Hexahedron => {
                let d = self.shape.dimension();
                let f: Vec<Vec<f64>> = (0..d).map(|a| basis_1d(self.basis, o[a], pc[a])).collect();
                for n in self.lattice.iter() {
                    out.push((0..d).map(|a| f[a][n[a]]).product());
                }
            }
            Shape::Triangle | Shape::Tetra => {
                let d = self.shape.dimension();
                let l0 = 1.0 - (0..d).map(|a| pc[a]).sum::<f64>();
                let lam = [l0, pc[0], pc[1], pc[2]];
                for n in self.lattice.iter() {
                    let alpha = [o[0] - (0..d).map(|a| n[a]).sum::<usize>(), n[0], n[1], n[2]];
                    out.push(simplex_basis(self.basis, o[0], &lam[..=d], &alpha[..=d]));
                }
            }
            Shape::Wedge => {
                let lam = [1.0 - pc[0] - pc[1], pc[0], pc[1]];
                let ft = basis_1d(self.basis, o[2], pc[2]);
                for n in self.lattice.iter() {
                    let alpha = [o[0] - n[0] - n[1], n[0], n[1]];
                    out.push(simplex_basis(self.basis, o[0], &lam, &alpha) * ft[n[2]]);
                }
            }
        }
    }

    /// Weights including rational weights `w` of the cell's nodes (Bezier only).
    pub fn rational_weights(&self, pc: [f64; 3], w: Option<&[f64]>, out: &mut Vec<f64>) {
        self.weights(pc, out);
        if let (Basis::Bezier, Some(w)) = (self.basis, w) {
            let mut s = 0.0;
            for (b, wi) in out.iter_mut().zip(w) {
                *b *= wi;
                s += *b;
            }
            if s.abs() > 0.0 {
                out.iter_mut().for_each(|b| *b /= s);
            }
        }
    }
}

/// Node positions (x2) of the serendipity cells, in VTK order.
fn serendipity_nodes(shape: Shape) -> Vec<[usize; 3]> {
    match shape {
        Shape::Quad => vec![[0, 0, 0], [2, 0, 0], [2, 2, 0], [0, 2, 0], [1, 0, 0], [2, 1, 0], [1, 2, 0], [0, 1, 0]],
        Shape::Hexahedron => {
            let mut v = vec![[0, 0, 0], [2, 0, 0], [2, 2, 0], [0, 2, 0], [0, 0, 2], [2, 0, 2], [2, 2, 2], [0, 2, 2]];
            for z in [0, 2] {
                v.extend([[1, 0, z], [2, 1, z], [1, 2, z], [0, 1, z]]);
            }
            v.extend([[0, 0, 1], [2, 0, 1], [2, 2, 1], [0, 2, 1]]);
            v
        }
        Shape::Wedge => {
            let mut v = vec![[0, 0, 0], [2, 0, 0], [0, 2, 0], [0, 0, 2], [2, 0, 2], [0, 2, 2]];
            for z in [0, 2] {
                v.extend([[1, 0, z], [1, 1, z], [0, 1, z]]);
            }
            v.extend([[0, 0, 1], [2, 0, 1], [0, 2, 1]]);
            v
        }
        _ => vec![],
    }
}

/// Quadratic serendipity shape functions (corner and mid-edge nodes only).
fn serendipity_weights(shape: Shape, nodes: &[[usize; 3]], pc: [f64; 3], out: &mut Vec<f64>) {
    match shape {
        Shape::Quad | Shape::Hexahedron => {
            let d = shape.dimension();
            let x: Vec<f64> = (0..d).map(|a| 2.0 * pc[a] - 1.0).collect();
            for n in nodes {
                let xi: Vec<f64> = (0..d).map(|a| n[a] as f64 - 1.0).collect();
                let w = match (0..d).find(|&a| xi[a] == 0.0) {
                    // Mid-edge node: quadratic bubble along its edge, linear across.
                    Some(a) => (1.0 - x[a] * x[a]) * (0..d).filter(|&b| b != a).map(|b| (1.0 + x[b] * xi[b]) / 2.0).product::<f64>(),
                    None => {
                        let lin: f64 = (0..d).map(|a| (1.0 + x[a] * xi[a]) / 2.0).product();
                        lin * ((0..d).map(|a| x[a] * xi[a]).sum::<f64>() - (d as f64 - 1.0))
                    }
                };
                out.push(w);
            }
        }
        Shape::Wedge => {
            let lam = [1.0 - pc[0] - pc[1], pc[0], pc[1]];
            let z = 2.0 * pc[2] - 1.0;
            for n in nodes {
                let alpha = [2 - n[0] - n[1], n[0], n[1]];
                let zi = n[2] as f64 - 1.0;
                let w = match alpha.iter().position(|&a| a == 2) {
                    Some(m) if zi != 0.0 => 0.5 * lam[m] * (2.0 * lam[m] - 1.0) * (1.0 + z * zi) - 0.5 * lam[m] * (1.0 - z * z),
                    Some(m) => lam[m] * (1.0 - z * z),
                    None => {
                        let (a, b) = {
                            let mut it = (0..3).filter(|&k| alpha[k] == 1);
                            (it.next().unwrap(), it.next().unwrap())
                        };
                        2.0 * lam[a] * lam[b] * (1.0 + z * zi)
                    }
                };
                out.push(w);
            }
        }
        _ => {}
    }
}

/// 1D Lagrange polynomials on p+1 equispaced nodes of [0, 1] or Bernstein polynomials, indexed
/// by lattice position.
fn basis_1d(basis: Basis, p: usize, x: f64) -> Vec<f64> {
    if p == 0 {
        return vec![1.0];
    }
    match basis {
        Basis::Lagrange | Basis::Serendipity => {
            let pf = p as f64;
            (0..=p)
                .map(|i| {
                    let mut v = 1.0;
                    for m in 0..=p {
                        if m != i {
                            v *= (pf * x - m as f64) / (i as f64 - m as f64);
                        }
                    }
                    v
                })
                .collect()
        }
        Basis::Bezier => (0..=p).map(|i| binomial(p, i) * x.powi(i as i32) * (1.0 - x).powi((p - i) as i32)).collect(),
    }
}

/// Simplex basis for barycentric coordinates `lam` and multi-index `alpha` (sum p).
fn simplex_basis(basis: Basis, p: usize, lam: &[f64], alpha: &[usize]) -> f64 {
    match basis {
        Basis::Lagrange | Basis::Serendipity => {
            let pf = p as f64;
            let mut v = 1.0;
            for (l, &a) in lam.iter().zip(alpha) {
                for m in 0..a {
                    v *= (pf * l - m as f64) / (m as f64 + 1.0);
                }
            }
            v
        }
        Basis::Bezier => {
            let mut v = factorial(p);
            for (l, &a) in lam.iter().zip(alpha) {
                v *= l.powi(a as i32) / factorial(a);
            }
            v
        }
    }
}

fn factorial(n: usize) -> f64 {
    (1..=n).map(|k| k as f64).product()
}

fn binomial(n: usize, k: usize) -> f64 {
    factorial(n) / (factorial(k) * factorial(n - k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lagrange_weights_are_nodal_and_sum_to_one() {
        for (ty, n) in [
            (CellType::LagrangeCurve, 5),
            (CellType::LagrangeTriangle, 15),
            (CellType::LagrangeQuadrilateral, 16),
            (CellType::LagrangeTetrahedron, 35),
            (CellType::LagrangeHexahedron, 64),
            (CellType::LagrangeWedge, 40),
            (CellType::QuadraticQuad, 8),
            (CellType::QuadraticHexahedron, 20),
            (CellType::QuadraticWedge, 15),
            (CellType::QuadraticLinearQuad, 6),
            (CellType::QuadraticLinearWedge, 12),
            (CellType::CubicLine, 4),
        ] {
            let c = HighOrderCell::new(ty, n, None).unwrap();
            let mut w = Vec::new();
            for k in 0..c.len() {
                c.weights(c.node_pcoords(k), &mut w);
                for (m, &v) in w.iter().enumerate() {
                    assert!((v - if m == k { 1.0 } else { 0.0 }).abs() < 1e-9, "{ty:?} node {k} weight {m} = {v}");
                }
                assert_eq!(c.node_at(c.node_pcoords(k)), Some(k));
            }
            c.weights([0.21, 0.13, 0.31], &mut w);
            assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12, "{ty:?} partition of unity");
            for bt in [
                CellType::BezierCurve,
                CellType::BezierTriangle,
                CellType::BezierQuadrilateral,
                CellType::BezierTetrahedron,
                CellType::BezierHexahedron,
                CellType::BezierWedge,
            ] {
                let (_, shape) = bt.high_order().unwrap();
                if shape != c.shape {
                    continue;
                }
                let Some(b) = HighOrderCell::new(bt, n, None) else { continue };
                b.weights([0.21, 0.13, 0.31], &mut w);
                assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn orders_from_point_counts_and_degrees() {
        assert_eq!(cell_order(Shape::Hexahedron, 27, None), Some([2, 2, 2]));
        assert_eq!(cell_order(Shape::Hexahedron, 2 * 3 * 4, Some([1.0, 2.0, 3.0])), Some([1, 2, 3]));
        assert_eq!(cell_order(Shape::Wedge, 6 * 3, None), Some([2, 2, 2]));
        assert_eq!(cell_order(Shape::Wedge, 3 * 4, Some([1.0, 1.0, 3.0])), Some([1, 1, 3]));
        assert_eq!(cell_order(Shape::Tetra, 20, None), Some([3, 3, 3]));
        assert_eq!(cell_order(Shape::Triangle, 7, None), None);
    }
}
