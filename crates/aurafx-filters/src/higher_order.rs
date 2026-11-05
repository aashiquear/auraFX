//! Linear approximations of curved (higher-order) cells.
//!
//! Like VTK, cutting and integrating filters treat a Lagrange or Bezier cell as the linear
//! sub-cells through its nodes (level 1). Finer levels halve those sub-cells again, with new
//! points evaluated on the curved cell; Bezier cells always evaluate their points because the
//! control points are not on the geometry.

pub use aurafx_data::higher_order::{Basis, HighOrderCell, Shape};
use aurafx_data::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// True if the dataset (or any block) has cells that `linearize` would split.
pub fn has_curved(ds: &DataSet) -> bool {
    match ds {
        DataSet::UnstructuredGrid(ug) => ug.cells.types.iter().any(|t| t.high_order().is_some()),
        DataSet::MultiBlock(mb) => mb.leaves().iter().any(|(_, d)| has_curved(d)),
        _ => false,
    }
}

/// Divisions per parametric axis at a subdivision level (level 1 = one per node interval).
pub fn divisions(cell: &HighOrderCell, level: u32) -> [usize; 3] {
    let f = 1usize << level.saturating_sub(1).min(8);
    let d = cell.shape.dimension();
    std::array::from_fn(|a| if a < d { (cell.order[a] * f).max(1) } else { 0 })
}

/// Evaluates points (and point data) on curved cells, reusing node points where possible and
/// merging new points shared by neighbouring cells.
pub struct Evaluator<'a> {
    points: &'a Points,
    pd: &'a Attributes,
    degrees: Option<&'a Array>,
    rational: Option<&'a Array>,
    cache: HashMap<(CellType, usize, [i32; 3]), HighOrderCell>,
    new_points: Vec<f64>,
    recipes: Vec<(Vec<u32>, Vec<f64>)>,
    merge: HashMap<[i64; 3], u32>,
    inv_tol: f64,
    w: Vec<f64>,
    cw: Vec<f64>,
}

impl<'a> Evaluator<'a> {
    pub fn new(points: &'a Points, pd: &'a Attributes, cd: &'a Attributes) -> Self {
        let diag = points.bounds().diagonal().max(1e-30);
        Self {
            points,
            pd,
            degrees: cd.get("HigherOrderDegrees").filter(|a| a.ncomp == 3),
            rational: pd.get("RationalWeights").filter(|a| a.ncomp == 1),
            cache: HashMap::new(),
            new_points: Vec::new(),
            recipes: Vec::new(),
            merge: HashMap::new(),
            inv_tol: 1.0 / (diag * 1e-9),
            w: Vec::new(),
            cw: Vec::new(),
        }
    }

    /// The curved cell `cid` of type `ty` with `npts` nodes, or None for linear cells.
    pub fn cell(&mut self, cid: usize, ty: CellType, npts: usize) -> Option<HighOrderCell> {
        ty.high_order()?;
        let deg = self.degrees.filter(|a| cid < a.len()).map(|a| [a.get(cid, 0), a.get(cid, 1), a.get(cid, 2)]);
        let key = (ty, npts, deg.map(|d| d.map(|v| v.round() as i32)).unwrap_or([-1; 3]));
        if let Some(c) = self.cache.get(&key) {
            return Some(c.clone());
        }
        let c = HighOrderCell::new(ty, npts, deg)?;
        self.cache.insert(key, c.clone());
        Some(c)
    }

    /// Id of the point at parametric location `pc` of a cell with nodes `ids`.
    pub fn point(&mut self, cell: &HighOrderCell, ids: &[u32], pc: [f64; 3]) -> u32 {
        if cell.basis == Basis::Lagrange {
            if let Some(k) = cell.node_at(pc) {
                return ids[k];
            }
        }
        let rw = self.rational.map(|a| {
            self.cw.clear();
            self.cw.extend(ids.iter().map(|&i| a.get(i as usize, 0)));
            self.cw.as_slice()
        });
        let mut w = std::mem::take(&mut self.w);
        cell.rational_weights(pc, rw, &mut w);
        let mut x = [0.0; 3];
        for (k, &wk) in w.iter().enumerate() {
            if wk != 0.0 {
                let p = self.points.get(ids[k] as usize);
                for a in 0..3 {
                    x[a] += wk * p[a];
                }
            }
        }
        let key = x.map(|v| (v * self.inv_tol).round() as i64);
        let id = match self.merge.get(&key) {
            Some(&id) => id,
            None => {
                let id = (self.points.len() + self.recipes.len()) as u32;
                self.new_points.extend_from_slice(&x);
                let (ri, rw): (Vec<u32>, Vec<f64>) = ids.iter().zip(&w).filter(|(_, &v)| v != 0.0).map(|(&i, &v)| (i, v)).unzip();
                self.recipes.push((ri, rw));
                self.merge.insert(key, id);
                id
            }
        };
        self.w = w;
        id
    }

    pub fn has_new_points(&self) -> bool {
        !self.recipes.is_empty()
    }

    /// Points and point data: the original points followed by the evaluated ones. The
    /// rational weights are dropped (the output is linear).
    pub fn finish(self) -> (Points, Attributes) {
        let n0 = self.points.len();
        let points = if self.recipes.is_empty() {
            self.points.clone()
        } else {
            let mut v = self.points.data.to_f64_vec();
            v.extend_from_slice(&self.new_points);
            if matches!(self.points.data, ArrayData::F32(_)) {
                Points::new_f32(v.into_iter().map(|x| x as f32).collect())
            } else {
                Points::new_f64(v)
            }
        };
        let mut attrs = Attributes::new();
        for arr in self.pd.iter() {
            if arr.name == "RationalWeights" {
                continue;
            }
            if self.recipes.is_empty() {
                attrs.insert(arr.clone());
                continue;
            }
            if arr.scalar_type() == ScalarType::String {
                continue;
            }
            let nc = arr.ncomp;
            let mut out = arr.data.to_f64_vec();
            out.reserve(self.recipes.len() * nc);
            for (ids, w) in &self.recipes {
                for c in 0..nc {
                    out.push(ids.iter().zip(w).map(|(&i, &wk)| wk * arr.get(i as usize, c)).sum());
                }
            }
            debug_assert_eq!(out.len(), (n0 + self.recipes.len()) * nc);
            let ty = if arr.scalar_type().is_float() { arr.scalar_type() } else { ScalarType::F64 };
            let mut a = Array::new(arr.name.clone(), nc, ArrayData::from_f64(ty, &out));
            a.component_names = arr.component_names.clone();
            attrs.insert(a);
        }
        attrs.active = self.pd.active.clone();
        attrs.active.retain(|_, n| attrs.arrays.contains_key(n));
        (points, attrs)
    }
}

/// Lattice subdivision of a reference cell into linear cells.
pub struct Subdivision {
    /// Sample positions (integer, divide by the divisions for parametric coordinates).
    pub samples: Vec<[usize; 3]>,
    /// Linear cells over sample indices.
    pub cells: Vec<(CellType, Vec<u32>)>,
}

impl Subdivision {
    pub fn pcoords(&self, k: usize, n: [usize; 3]) -> [f64; 3] {
        let s = self.samples[k];
        std::array::from_fn(|a| if n[a] == 0 { 0.0 } else { s[a] as f64 / n[a] as f64 })
    }
}

/// Subdivision of `shape` with `n` divisions per axis (simplices use `n[0]`; wedges `n[0]` in
/// the triangle and `n[2]` along the axis).
pub fn subdivision(shape: Shape, n: [usize; 3]) -> Arc<Subdivision> {
    static CACHE: OnceLock<Mutex<HashMap<(Shape, [usize; 3]), Arc<Subdivision>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(s) = cache.lock().unwrap().get(&(shape, n)) {
        return s.clone();
    }
    let s = Arc::new(build_subdivision(shape, n));
    cache.lock().unwrap().insert((shape, n), s.clone());
    s
}

fn build_subdivision(shape: Shape, n: [usize; 3]) -> Subdivision {
    let mut samples: Vec<[usize; 3]> = Vec::new();
    let mut index: HashMap<[usize; 3], u32> = HashMap::new();
    let mut id = |s: [usize; 3]| -> u32 {
        *index.entry(s).or_insert_with(|| {
            samples.push(s);
            (samples.len() - 1) as u32
        })
    };
    let mut cells = Vec::new();
    match shape {
        Shape::Curve => {
            for i in 0..n[0] {
                cells.push((CellType::Line, vec![id([i, 0, 0]), id([i + 1, 0, 0])]));
            }
        }
        Shape::Quad => {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    cells.push((CellType::Quad, vec![id([i, j, 0]), id([i + 1, j, 0]), id([i + 1, j + 1, 0]), id([i, j + 1, 0])]));
                }
            }
        }
        Shape::Hexahedron => {
            for k in 0..n[2] {
                for j in 0..n[1] {
                    for i in 0..n[0] {
                        let c = [
                            [i, j, k],
                            [i + 1, j, k],
                            [i + 1, j + 1, k],
                            [i, j + 1, k],
                            [i, j, k + 1],
                            [i + 1, j, k + 1],
                            [i + 1, j + 1, k + 1],
                            [i, j + 1, k + 1],
                        ];
                        cells.push((CellType::Hexahedron, c.iter().map(|&s| id(s)).collect()));
                    }
                }
            }
        }
        Shape::Triangle => {
            for t in tri_lattice(n[0]) {
                cells.push((CellType::Triangle, t.iter().map(|&(i, j)| id([i, j, 0])).collect()));
            }
        }
        Shape::Wedge => {
            for k in 0..n[2] {
                for t in tri_lattice(n[0]) {
                    let mut c: Vec<u32> = t.iter().map(|&(i, j)| id([i, j, k])).collect();
                    c.extend(t.iter().map(|&(i, j)| id([i, j, k + 1])));
                    cells.push((CellType::Wedge, c));
                }
            }
        }
        Shape::Tetra => {
            let m = n[0];
            let add =
                |a: [usize; 3], b: [usize; 3], c: [usize; 3], d: [usize; 3], cells: &mut Vec<(CellType, Vec<u32>)>, id: &mut dyn FnMut([usize; 3]) -> u32| {
                    // Keep the reference cell's (positive) orientation.
                    let e = |p: [usize; 3], q: [usize; 3]| [q[0] as f64 - p[0] as f64, q[1] as f64 - p[1] as f64, q[2] as f64 - p[2] as f64];
                    let (u, v, w) = (e(a, b), e(a, c), e(a, d));
                    let det = u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0]);
                    let t = if det >= 0.0 { [a, b, c, d] } else { [a, c, b, d] };
                    cells.push((CellType::Tetra, t.iter().map(|&s| id(s)).collect()));
                };
            for k in 0..m {
                for j in 0..m - k {
                    for i in 0..m - k - j {
                        let v = [i, j, k];
                        let p = |dx: usize, dy: usize, dz: usize| [v[0] + dx, v[1] + dy, v[2] + dz];
                        add(v, p(1, 0, 0), p(0, 1, 0), p(0, 0, 1), &mut cells, &mut id);
                        if i + j + k + 2 <= m {
                            // Octahedron split around its (e1)-(e2+e3) diagonal.
                            let (a, f) = (p(1, 0, 0), p(0, 1, 1));
                            let ring = [p(0, 1, 0), p(1, 1, 0), p(1, 0, 1), p(0, 0, 1)];
                            for r in 0..4 {
                                add(a, f, ring[r], ring[(r + 1) % 4], &mut cells, &mut id);
                            }
                        }
                        if i + j + k + 3 <= m {
                            add(p(1, 1, 0), p(1, 0, 1), p(0, 1, 1), p(1, 1, 1), &mut cells, &mut id);
                        }
                    }
                }
            }
        }
    }
    Subdivision { samples, cells }
}

/// Sub-triangles of a triangle lattice with `m` divisions, counter-clockwise like the cell.
fn tri_lattice(m: usize) -> Vec<[(usize, usize); 3]> {
    let mut v = Vec::with_capacity(m * m);
    for j in 0..m {
        for i in 0..m - j {
            v.push([(i, j), (i + 1, j), (i, j + 1)]);
            if i + j + 2 <= m {
                v.push([(i + 1, j), (i + 1, j + 1), (i, j + 1)]);
            }
        }
    }
    v
}

/// Faces of a curved 3D cell as corner indices, outward for a cell whose corners have the
/// linear VTK orientation; `flip` reverses them (see [`flipped`]).
pub fn faces_of(shape: Shape, flip: bool) -> &'static [&'static [usize]] {
    const TET: [&[usize]; 4] = [&[0, 1, 2], &[0, 3, 1], &[1, 3, 2], &[0, 2, 3]];
    const HEX: [&[usize]; 6] = [&[0, 3, 7, 4], &[1, 5, 6, 2], &[0, 4, 5, 1], &[3, 2, 6, 7], &[0, 1, 2, 3], &[4, 7, 6, 5]];
    const WEDGE: [&[usize]; 5] = [&[0, 2, 1], &[3, 4, 5], &[0, 1, 4, 3], &[1, 2, 5, 4], &[2, 0, 3, 5]];
    match (shape, flip) {
        (Shape::Tetra, true) => &TET,
        (Shape::Hexahedron, true) => &HEX,
        (Shape::Wedge, true) => &WEDGE,
        _ => shape.linear_type().faces(),
    }
}

/// Whether a curved 3D cell is oriented opposite to the linear VTK cell with the same corners.
/// VTK's Lagrange and Bezier wedges have the base triangle counter-clockwise seen from the
/// top, the opposite of linear wedges; inverted cells are flipped too, so faces stay outward.
pub fn flipped(points: &Points, ids: &[u32], shape: Shape) -> bool {
    let (a, b, c, d) = match shape {
        Shape::Tetra | Shape::Wedge => (0, 1, 2, 3),
        Shape::Hexahedron => (0, 1, 3, 4),
        _ => return false,
    };
    let p: [[f64; 3]; 4] = [a, b, c, d].map(|k| points.get(ids[k] as usize));
    let e = |q: [f64; 3]| [q[0] - p[0][0], q[1] - p[0][1], q[2] - p[0][2]];
    let (u, v, w) = (e(p[1]), e(p[2]), e(p[3]));
    let det = u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0]);
    // Linear VTK wedges have det < 0 for these corners; tets and hexes det > 0.
    if shape == Shape::Wedge {
        det > 0.0
    } else {
        det < 0.0
    }
}

/// Divisions along the parametric direction from corner `a` to corner `b` of a cell.
fn edge_divisions(shape: Shape, n: [usize; 3], a: [f64; 3], b: [f64; 3]) -> usize {
    match shape {
        Shape::Tetra | Shape::Triangle => n[0],
        Shape::Wedge => {
            if (a[2] - b[2]).abs() > 0.5 {
                n[2]
            } else {
                n[0]
            }
        }
        _ => (0..3).find(|&k| (a[k] - b[k]).abs() > 0.5).map(|k| n[k]).unwrap_or(1),
    }
}

/// Polygons covering face `f` (corner indices into the cell's corners) of a curved cell at
/// `level`, as point ids. Quads for quadrilateral faces, triangles for triangular ones.
pub fn face_polys(ev: &mut Evaluator, cell: &HighOrderCell, ids: &[u32], f: &[usize], level: u32) -> Vec<Vec<u32>> {
    if cell.basis == Basis::Serendipity {
        return serendipity_face_polys(ev, cell, ids, f, level);
    }
    let n = divisions(cell, level);
    let r = cell.shape.corners();
    let c: Vec<[f64; 3]> = f.iter().map(|&k| r[k]).collect();
    let mut out = Vec::new();
    if c.len() == 3 {
        let m = edge_divisions(cell.shape, n, c[0], c[1]);
        let mut grid: HashMap<(usize, usize), u32> = HashMap::new();
        for t in tri_lattice(m) {
            let poly = t
                .iter()
                .map(|&(i, j)| {
                    *grid.entry((i, j)).or_insert_with(|| {
                        let (u, v) = (i as f64 / m as f64, j as f64 / m as f64);
                        let pc = std::array::from_fn(|a| c[0][a] + u * (c[1][a] - c[0][a]) + v * (c[2][a] - c[0][a]));
                        ev.point(cell, ids, pc)
                    })
                })
                .collect();
            out.push(poly);
        }
    } else if c.len() == 4 {
        let (nu, nv) = (edge_divisions(cell.shape, n, c[0], c[1]), edge_divisions(cell.shape, n, c[0], c[3]));
        let mut grid = vec![u32::MAX; (nu + 1) * (nv + 1)];
        let mut at = |i: usize, j: usize, ev: &mut Evaluator| {
            let g = &mut grid[j * (nu + 1) + i];
            if *g == u32::MAX {
                let (u, v) = (i as f64 / nu as f64, j as f64 / nv as f64);
                let pc = std::array::from_fn(|a| (1.0 - u) * (1.0 - v) * c[0][a] + u * (1.0 - v) * c[1][a] + u * v * c[2][a] + (1.0 - u) * v * c[3][a]);
                *g = ev.point(cell, ids, pc);
            }
            *g
        };
        for j in 0..nv {
            for i in 0..nu {
                out.push(vec![at(i, j, ev), at(i + 1, j, ev), at(i + 1, j + 1, ev), at(i, j + 1, ev)]);
            }
        }
    }
    out
}

/// Faces of serendipity cells as VTK's quadratic cells list them: the triangle pattern starts
/// at the first corner, which decides the diagonal ParaView draws.
fn serendipity_face_order(shape: Shape) -> &'static [&'static [usize]] {
    match shape {
        Shape::Hexahedron => CellType::Hexahedron.faces(),
        Shape::Wedge => &[&[0, 1, 2], &[3, 5, 4], &[0, 1, 4, 3], &[1, 2, 5, 4], &[2, 0, 3, 5]],
        _ => &[&[0, 1, 2, 3]],
    }
}

/// Triangles of a serendipity face like ParaView: through the corners and mid-edge nodes at
/// level 1 (6 triangles for a quad face, 4 for a triangle), each triangle split in 4 per
/// further level with points evaluated on the cell. `outward` gives the winding.
fn serendipity_face_polys(ev: &mut Evaluator, cell: &HighOrderCell, ids: &[u32], outward: &[usize], level: u32) -> Vec<Vec<u32>> {
    let mut key: Vec<usize> = outward.to_vec();
    key.sort_unstable();
    let canon: &[usize] = serendipity_face_order(cell.shape)
        .iter()
        .copied()
        .find(|f| {
            let mut k = f.to_vec();
            k.sort_unstable();
            k == key
        })
        .unwrap_or(outward);
    // Same cyclic order as the outward face, or reversed?
    let start = outward.iter().position(|&c| c == canon[0]).unwrap_or(0);
    let same = (0..canon.len()).all(|k| outward[(start + k) % outward.len()] == canon[k]);
    let r = cell.shape.corners();
    let c: Vec<[f64; 3]> = canon.iter().map(|&k| r[k]).collect();
    let map = |u: f64, v: f64| -> [f64; 3] {
        if c.len() == 3 {
            std::array::from_fn(|a| c[0][a] + u * (c[1][a] - c[0][a]) + v * (c[2][a] - c[0][a]))
        } else {
            std::array::from_fn(|a| (1.0 - u) * (1.0 - v) * c[0][a] + u * (1.0 - v) * c[1][a] + u * v * c[2][a] + (1.0 - u) * v * c[3][a])
        }
    };
    // Level-1 pattern in face coordinates.
    let mut tris: Vec<[[f64; 2]; 3]> = if c.len() == 3 {
        let (c0, c1, c2, m0, m1, m2) = ([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]);
        vec![[c0, m0, m2], [m0, c1, m1], [m2, m1, c2], [m0, m1, m2]]
    } else {
        let (c0, c1, c2, c3) = ([0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]);
        let (m0, m1, m2, m3) = ([0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]);
        vec![[c0, m0, m3], [m0, c1, m1], [m1, c2, m2], [m3, m2, c3], [m0, m1, m2], [m0, m2, m3]]
    };
    for _ in 1..level.max(1) {
        let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        tris = tris
            .iter()
            .flat_map(|&[a, b, c]| {
                let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
                [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
            })
            .collect();
    }
    let mut cache: HashMap<[u64; 2], u32> = HashMap::new();
    tris.iter()
        .map(|t| {
            let mut v: Vec<u32> =
                t.iter().map(|uv| *cache.entry([uv[0].to_bits(), uv[1].to_bits()]).or_insert_with(|| ev.point(cell, ids, map(uv[0], uv[1])))).collect();
            if !same {
                v.swap(1, 2);
            }
            v
        })
        .collect()
}

/// Linear cells covering a curved cell at `level` (point ids, via the evaluator).
pub fn cell_pieces(ev: &mut Evaluator, cell: &HighOrderCell, ids: &[u32], level: u32) -> Vec<(CellType, Vec<u32>)> {
    let n = divisions(cell, level);
    let sub = subdivision(cell.shape, n);
    let pid: Vec<u32> = (0..sub.samples.len()).map(|k| ev.point(cell, ids, sub.pcoords(k, n))).collect();
    // Sub-wedges follow the reference orientation; give them the linear wedge orientation.
    let swap = cell.shape == Shape::Wedge && flipped(ev.points, ids, cell.shape);
    sub.cells
        .iter()
        .map(|(t, c)| {
            let mut v: Vec<u32> = c.iter().map(|&k| pid[k as usize]).collect();
            if swap {
                v.swap(1, 2);
                v.swap(4, 5);
            }
            (*t, v)
        })
        .collect()
}

/// Replace curved cells of an unstructured grid by linear cells (`level` >= 1). Returns the
/// grid and, for each output cell, the input cell it came from. None if nothing is curved.
pub fn linearize_grid(ug: &UnstructuredGrid, level: u32) -> Option<(UnstructuredGrid, Vec<usize>)> {
    if !ug.cells.types.iter().any(|t| t.high_order().is_some()) {
        return None;
    }
    let level = level.max(1);
    let mut ev = Evaluator::new(&ug.points, &ug.point_data, &ug.cell_data);
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    let mut faces: Vec<Vec<Vec<u32>>> = Vec::new();
    let has_faces = ug.cells.polyhedron_faces.is_some();
    for cid in 0..ug.cells.len() {
        let ty = ug.cells.types[cid];
        let ids = ug.cells.cell(cid);
        match ev.cell(cid, ty, ids.len()) {
            Some(cell) => {
                for (t, c) in cell_pieces(&mut ev, &cell, ids, level) {
                    cells.push(t, &c);
                    parents.push(cid);
                    if has_faces {
                        faces.push(Vec::new());
                    }
                }
            }
            None => {
                cells.push(ty, ids);
                parents.push(cid);
                if has_faces {
                    faces.push(ug.cells.polyhedron_faces.as_ref().and_then(|f| f.get(cid)).cloned().unwrap_or_default());
                }
            }
        }
    }
    if has_faces {
        cells.polyhedron_faces = Some(faces);
    }
    let (points, point_data) = ev.finish();
    let mut cell_data = ug.cell_data.gather(&parents);
    cell_data.remove("HigherOrderDegrees");
    let mut out = UnstructuredGrid { points, cells, point_data, cell_data, field_data: ug.field_data.clone() };
    // Drop control points / interior nodes that no linear cell uses (polyhedron faces index
    // points too, so those grids keep them all).
    if !has_faces {
        let (p, a) = crate::interp::compact_points(&out.points, &out.point_data, &mut out.cells.connectivity);
        out.points = p;
        out.point_data = a;
    }
    Some((out, parents))
}

/// The dataset with curved cells replaced by their level-1 linear sub-cells (VTK's
/// approximation for cutting and integration). Multi-block datasets are handled per block.
pub fn as_linear(ds: &DataSet) -> Cow<'_, DataSet> {
    let ex = crate::simplex::as_explicit(ds);
    match ex.as_ref() {
        DataSet::UnstructuredGrid(ug) => match linearize_grid(ug, 1) {
            Some((g, _)) => Cow::Owned(DataSet::UnstructuredGrid(g)),
            None => ex,
        },
        _ => ex,
    }
}

/// Tessellate filter: curved cells become linear cells at a subdivision level; other cells
/// are kept. Level 1 places one linear cell per node interval; each further level halves them.
pub fn tessellate(ds: &DataSet, level: u32) -> anyhow::Result<DataSet> {
    Ok(match ds {
        DataSet::MultiBlock(mb) => {
            let mut out = mb.clone();
            for (_, b) in out.blocks.iter_mut() {
                if let Some(d) = b.as_mut() {
                    *d = tessellate(d, level)?;
                }
            }
            DataSet::MultiBlock(out)
        }
        DataSet::UnstructuredGrid(ug) => match linearize_grid(ug, level) {
            Some((g, parents)) => {
                let mut g = g;
                g.cell_data.insert(Array::new("vtkOriginalCellIds", 1, ArrayData::I64(parents.iter().map(|&i| i as i64).collect())));
                DataSet::UnstructuredGrid(g)
            }
            None => ds.clone(),
        },
        other => other.clone(),
    })
}
