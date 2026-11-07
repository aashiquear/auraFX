//! Tetrahedral meshes with element blocks, side sets and node sets, the conversions between
//! hexahedra and tetrahedra, and uniform refinement of both.
//!
//! Tetrahedra use VTK's corner order (positive when (p1 - p0) x (p2 - p0) points towards
//! p3) and Exodus' side numbers: 1 = (0, 1, 3), 2 = (1, 2, 3), 3 = (0, 3, 2), 4 = (0, 2, 1).

use crate::hex::{HexMesh, HEX8_SIDES, P3};
use aurafx_data::*;
use std::collections::{BTreeMap, HashMap};

/// Corners of the four Exodus sides of a tetrahedron (outward for a positive one).
pub const TET_SIDES: [[usize; 3]; 4] = [[0, 1, 3], [1, 2, 3], [0, 3, 2], [0, 2, 1]];

#[derive(Clone, Debug, Default)]
pub struct TetMesh {
    pub points: Vec<P3>,
    pub tets: Vec<[u32; 4]>,
    pub block: Vec<i32>,
    pub block_names: BTreeMap<i32, String>,
    /// (name, [(element, Exodus side 1..4)]).
    pub side_sets: Vec<(String, Vec<(u32, u8)>)>,
    pub node_sets: Vec<(String, Vec<u32>)>,
    pub elem_fields: Vec<(String, Vec<f64>)>,
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Six times the signed volume of a tetrahedron.
pub fn orient(p: &[P3], t: [u32; 4]) -> f64 {
    let [a, b, c, d] = t.map(|k| p[k as usize]);
    dot(cross(sub(b, a), sub(c, a)), sub(d, a))
}

fn mean(p: &[P3], ids: &[u32]) -> P3 {
    let mut s = [0.0; 3];
    for k in ids {
        for a in 0..3 {
            s[a] += p[*k as usize][a];
        }
    }
    s.map(|x| x / ids.len() as f64)
}

/// A node shared by the elements that make it: keyed by the sorted original nodes.
struct Shared {
    map: HashMap<Vec<u32>, u32>,
}

impl Shared {
    fn new() -> Shared {
        Shared { map: HashMap::new() }
    }
    fn get(&mut self, points: &mut Vec<P3>, ids: &[u32]) -> u32 {
        if ids.len() == 1 {
            return ids[0];
        }
        let mut k = ids.to_vec();
        k.sort_unstable();
        let pos = mean(points, ids);
        *self.map.entry(k).or_insert_with(|| {
            points.push(pos);
            (points.len() - 1) as u32
        })
    }
    /// New nodes all of whose defining nodes are in `set`.
    fn within(&self, set: &std::collections::HashSet<u32>) -> Vec<u32> {
        self.map.iter().filter(|(k, _)| k.iter().all(|n| set.contains(n))).map(|(_, v)| *v).collect()
    }
}

impl TetMesh {
    pub fn n_elements(&self) -> usize {
        self.tets.len()
    }
    pub fn volumes(&self) -> Vec<f64> {
        self.tets.iter().map(|t| orient(&self.points, *t) / 6.0).collect()
    }
    pub fn side_corners(&self, e: usize, s: u8) -> [u32; 3] {
        TET_SIDES[s as usize - 1].map(|k| self.tets[e][k])
    }
    pub fn side_set(&self, name: &str) -> Option<&Vec<(u32, u8)>> {
        self.side_sets.iter().find(|s| s.0 == name).map(|s| &s.1)
    }
    pub fn node_set(&self, name: &str) -> Option<&Vec<u32>> {
        self.node_sets.iter().find(|s| s.0 == name).map(|s| &s.1)
    }
    /// Faces used by exactly one element.
    pub fn boundary_faces(&self) -> Vec<(u32, u8)> {
        let mut count: HashMap<[u32; 3], u32> = HashMap::new();
        for e in 0..self.tets.len() {
            for s in 1..=4u8 {
                let mut k = self.side_corners(e, s);
                k.sort_unstable();
                *count.entry(k).or_insert(0) += 1;
            }
        }
        let mut out = Vec::new();
        for e in 0..self.tets.len() {
            for s in 1..=4u8 {
                let mut k = self.side_corners(e, s);
                k.sort_unstable();
                if count[&k] == 1 {
                    out.push((e as u32, s));
                }
            }
        }
        out
    }

    pub fn to_dataset(&self) -> DataSet {
        let mut cells = Cells::new();
        for t in &self.tets {
            cells.push(CellType::Tetra, t);
        }
        let mut ug = UnstructuredGrid { points: Points::new_f64(self.points.concat()), cells, ..Default::default() };
        ug.cell_data.insert(Array::new("ElementBlock", 1, ArrayData::I32(self.block.clone())));
        for (name, v) in &self.elem_fields {
            ug.cell_data.insert(Array::scalars_f64(name.clone(), v.clone()));
        }
        ug.field_data.insert(Array::new("ElementBlockIds", 1, ArrayData::I32(self.block_names.keys().copied().collect())));
        ug.field_data.insert(Array::new("ElementBlockNames", 1, ArrayData::String(self.block_names.values().cloned().collect())));
        for (name, faces) in &self.side_sets {
            ug.field_data.insert(Array::new(format!("SideSet {name}"), 2, ArrayData::I32(faces.iter().flat_map(|(e, s)| [*e as i32, *s as i32]).collect())));
        }
        for (name, nodes) in &self.node_sets {
            ug.field_data.insert(Array::new(format!("NodeSet {name}"), 1, ArrayData::I32(nodes.iter().map(|n| *n as i32).collect())));
        }
        DataSet::UnstructuredGrid(ug)
    }

    /// A tetrahedral mesh from auraFX data (all cells tetrahedra), with blocks and sets.
    pub fn from_dataset(ds: &DataSet) -> Result<TetMesh, String> {
        let ug = match ds {
            DataSet::UnstructuredGrid(u) => u.clone(),
            d => d.to_unstructured().ok_or("not a mesh")?,
        };
        let mut m = TetMesh { points: (0..ug.points.len()).map(|i| ug.points.get(i)).collect(), ..Default::default() };
        for (ty, ids) in ug.cells.iter() {
            if ty != CellType::Tetra {
                return Err(format!("{ty:?} cells: only tetrahedra are handled"));
            }
            m.tets.push([ids[0], ids[1], ids[2], ids[3]]);
        }
        let ne = m.tets.len();
        m.block = match ug.cell_data.get("ElementBlock") {
            Some(a) => (0..ne).map(|k| a.get(k, 0) as i32).collect(),
            None => vec![1; ne],
        };
        if let (Some(ids), Some(ArrayData::String(names))) = (ug.field_data.get("ElementBlockIds"), ug.field_data.get("ElementBlockNames").map(|a| &a.data)) {
            for i in 0..ids.len() {
                m.block_names.insert(ids.get(i, 0) as i32, names.get(i).cloned().unwrap_or_default());
            }
        }
        for b in &m.block {
            m.block_names.entry(*b).or_insert_with(|| format!("block_{b}"));
        }
        for a in ug.cell_data.iter() {
            if a.name != "ElementBlock" && a.ncomp == 1 && !matches!(a.data, ArrayData::String(_)) {
                m.elem_fields.push((a.name.clone(), (0..ne).map(|k| a.get(k, 0)).collect()));
            }
        }
        for a in ug.field_data.iter() {
            if let Some(n) = a.name.strip_prefix("SideSet ") {
                m.side_sets.push((n.to_string(), (0..a.len()).map(|i| (a.get(i, 0) as u32, a.get(i, 1) as u8)).collect()));
            } else if let Some(n) = a.name.strip_prefix("NodeSet ") {
                m.node_sets.push((n.to_string(), (0..a.len()).map(|i| a.get(i, 0) as u32).collect()));
            }
        }
        Ok(m)
    }

    /// Uniform refinement: every tetrahedron into eight (the four corners and the inner
    /// octahedron cut along its shortest diagonal). Side sets go to the sub-faces, node sets
    /// gain the new nodes on them.
    pub fn refine(&self) -> TetMesh {
        let mut out = TetMesh { points: self.points.clone(), block_names: self.block_names.clone(), ..Default::default() };
        let mut sh = Shared::new();
        let mut parent = Vec::new();
        for (e, t) in self.tets.iter().enumerate() {
            let m = |a: usize, b: usize, pts: &mut Vec<P3>, sh: &mut Shared| sh.get(pts, &[t[a], t[b]]);
            let (m01, m02, m03) = (m(0, 1, &mut out.points, &mut sh), m(0, 2, &mut out.points, &mut sh), m(0, 3, &mut out.points, &mut sh));
            let (m12, m13, m23) = (m(1, 2, &mut out.points, &mut sh), m(1, 3, &mut out.points, &mut sh), m(2, 3, &mut out.points, &mut sh));
            let mut kids = vec![[t[0], m01, m02, m03], [m01, t[1], m12, m13], [m02, m12, t[2], m23], [m03, m13, m23, t[3]]];
            // The octahedron (m01 m02 m03 m12 m13 m23): four tetrahedra around its shortest
            // diagonal.
            let diags = [(m01, m23, [m02, m03, m13, m12]), (m02, m13, [m01, m12, m23, m03]), (m03, m12, [m01, m13, m23, m02])];
            let d = diags
                .iter()
                .min_by(|a, b| {
                    let la = dot(sub(out.points[a.0 as usize], out.points[a.1 as usize]), sub(out.points[a.0 as usize], out.points[a.1 as usize]));
                    let lb = dot(sub(out.points[b.0 as usize], out.points[b.1 as usize]), sub(out.points[b.0 as usize], out.points[b.1 as usize]));
                    la.total_cmp(&lb)
                })
                .unwrap();
            for k in 0..4 {
                kids.push([d.0, d.1, d.2[k], d.2[(k + 1) % 4]]);
            }
            for mut c in kids {
                if orient(&out.points, c) < 0.0 {
                    c.swap(2, 3);
                }
                out.tets.push(c);
                out.block.push(self.block[e]);
                parent.push(e);
            }
        }
        for (name, v) in &self.elem_fields {
            out.elem_fields.push((name.clone(), parent.iter().map(|p| v[*p]).collect()));
        }
        // Side sets: the children's faces whose nodes all lie on the parent's face.
        for (name, faces) in &self.side_sets {
            let mut nf = Vec::new();
            for (e, s) in faces {
                let f = self.side_corners(*e as usize, *s);
                let mut on: Vec<u32> = f.to_vec();
                for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                    let mut k = vec![f[a], f[b]];
                    k.sort_unstable();
                    on.push(sh.map[&k]);
                }
                for c in 8 * *e as usize..8 * *e as usize + 8 {
                    for cs in 1..=4u8 {
                        if out.side_corners(c, cs).iter().all(|n| on.contains(n)) {
                            nf.push((c as u32, cs));
                        }
                    }
                }
            }
            out.side_sets.push((name.clone(), nf));
        }
        for (name, nodes) in &self.node_sets {
            let set: std::collections::HashSet<u32> = nodes.iter().copied().collect();
            let mut v = nodes.clone();
            v.extend(sh.within(&set));
            v.sort_unstable();
            out.node_sets.push((name.clone(), v));
        }
        out
    }

    /// THex: every tetrahedron into four hexahedra (one per corner, through the edge
    /// midpoints, face centres and centre).
    pub fn to_hexes(&self) -> HexMesh {
        let mut out = HexMesh { points: self.points.clone(), npe: 8, block_names: self.block_names.clone(), ..Default::default() };
        let mut sh = Shared::new();
        let mut corner_of: Vec<Vec<(u32, [u32; 3])>> = Vec::new();
        let mut parent = Vec::new();
        for (e, t) in self.tets.iter().enumerate() {
            let centre = sh.get(&mut out.points, t);
            let mut here = Vec::new();
            for i in 0..4 {
                let v = t[i];
                let mut o: Vec<u32> = (0..4).filter(|k| *k != i).map(|k| t[k]).collect();
                if orient(&self.points, [v, o[0], o[1], o[2]]) < 0.0 {
                    o.swap(1, 2);
                }
                let (a, b, c) = (o[0], o[1], o[2]);
                let mva = sh.get(&mut out.points, &[v, a]);
                let mvb = sh.get(&mut out.points, &[v, b]);
                let mvc = sh.get(&mut out.points, &[v, c]);
                let fvab = sh.get(&mut out.points, &[v, a, b]);
                let fvac = sh.get(&mut out.points, &[v, a, c]);
                let fvbc = sh.get(&mut out.points, &[v, b, c]);
                out.cells.extend([v, mva, fvab, mvb, mvc, fvac, centre, fvbc]);
                out.block.push(self.block[e]);
                parent.push(e);
                here.push((v, [a, b, c]));
            }
            corner_of.push(here);
        }
        for (name, v) in &self.elem_fields {
            out.elem_fields.push((name.clone(), parent.iter().map(|p| v[*p]).collect()));
        }
        // Side sets: on a tetrahedron face (v, x, y) the hexahedron of corner v has bottom
        // (side 5) for (v, a, b), side 1 for (v, a, c), side 4 for (v, b, c).
        for (name, faces) in &self.side_sets {
            let mut nf = Vec::new();
            for (e, s) in faces {
                let f = self.side_corners(*e as usize, *s);
                for (k, (v, abc)) in corner_of[*e as usize].iter().enumerate() {
                    if !f.contains(v) {
                        continue;
                    }
                    let (a, b) = (abc[0], abc[1]);
                    let side = if f.contains(&a) && f.contains(&b) {
                        5
                    } else if f.contains(&a) {
                        1
                    } else {
                        4
                    };
                    nf.push(((4 * *e as usize + k) as u32, side));
                }
            }
            out.side_sets.push((name.clone(), nf));
        }
        for (name, nodes) in &self.node_sets {
            let set: std::collections::HashSet<u32> = nodes.iter().copied().collect();
            let mut v = nodes.clone();
            v.extend(sh.within(&set));
            v.sort_unstable();
            out.node_sets.push((name.clone(), v));
        }
        out
    }
}

/// HTet: every hexahedron (HEX8) into twelve tetrahedra: each face cut into two triangles
/// along the diagonal through its smallest node number (so neighbours agree), each triangle
/// joined to the centre.
pub fn hexes_to_tets(m: &HexMesh) -> Result<TetMesh, String> {
    if m.npe != 8 {
        return Err("only HEX8 meshes can be split into tetrahedra".into());
    }
    let mut out = TetMesh { points: m.points.clone(), block_names: m.block_names.clone(), ..Default::default() };
    let mut parent = Vec::new();
    // For each hexahedron and side: the two tetrahedra (indices) on it.
    let mut on_side: Vec<[[u32; 2]; 6]> = Vec::with_capacity(m.n_elements());
    for e in 0..m.n_elements() {
        let c = m.cell(e);
        let centre = out.points.len() as u32;
        out.points.push(mean(&m.points, c));
        let mut sides = [[0u32; 2]; 6];
        for (s, side) in HEX8_SIDES.iter().enumerate() {
            let f = side.map(|k| c[k]);
            // Outward quad (a b c d); split through its smallest node.
            let k0 = (0..4).min_by_key(|k| f[*k]).unwrap();
            let q = [f[k0], f[(k0 + 1) % 4], f[(k0 + 2) % 4], f[(k0 + 3) % 4]];
            for (n, tri) in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]].iter().enumerate() {
                // Inward-facing base towards the centre: positive tetrahedron (t0, t2, t1, c).
                let mut t = [tri[0], tri[2], tri[1], centre];
                if orient(&out.points, t) < 0.0 {
                    t.swap(1, 2);
                }
                sides[s][n] = out.tets.len() as u32;
                out.tets.push(t);
                out.block.push(m.block[e]);
                parent.push(e);
            }
        }
        on_side.push(sides);
    }
    for (name, v) in &m.elem_fields {
        out.elem_fields.push((name.clone(), parent.iter().map(|p| v[*p]).collect()));
    }
    for (name, faces) in &m.side_sets {
        let mut nf = Vec::new();
        for (e, s) in faces {
            for t in on_side[*e as usize][*s as usize - 1] {
                // The face without the centre (node 3) is side 4 (0, 2, 1).
                let tet = out.tets[t as usize];
                let side = (1..=4u8).find(|cs| !TET_SIDES[*cs as usize - 1].iter().any(|k| tet[*k] as usize >= m.points.len())).unwrap_or(4);
                nf.push((t, side));
            }
        }
        out.side_sets.push((name.clone(), nf));
    }
    out.node_sets = m.node_sets.clone();
    Ok(out)
}

impl HexMesh {
    /// Uniform refinement of a HEX8 mesh: every hexahedron into eight, new nodes at edge
    /// midpoints, face centres and centres (shared between neighbours). Side sets go to the
    /// sub-faces, node sets gain the new nodes on them.
    pub fn refine(&self) -> Result<HexMesh, String> {
        if self.npe != 8 {
            return Err("refine a HEX8 mesh (elevate afterwards)".into());
        }
        // VTK corner index of each (x, y, z) bit pattern, and back.
        const BITS: [[usize; 3]; 8] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]];
        let corner_of = |b: [usize; 3]| BITS.iter().position(|x| *x == b).unwrap();
        let mut out = HexMesh { points: self.points.clone(), npe: 8, block_names: self.block_names.clone(), ..Default::default() };
        let mut sh = Shared::new();
        let mut parent = Vec::new();
        for e in 0..self.n_elements() {
            let c: Vec<u32> = self.cell(e).to_vec();
            // Nodes of the 3x3x3 lattice of the hexahedron: digit 0/2 at a face, 1 halfway.
            let mut lat = [[[0u32; 3]; 3]; 3];
            for (i, row) in lat.iter_mut().enumerate() {
                for (j, col) in row.iter_mut().enumerate() {
                    for (k, slot) in col.iter_mut().enumerate() {
                        let d = [i, j, k];
                        let mut ids = Vec::new();
                        for b in BITS {
                            if (0..3).all(|a| d[a] == 1 || d[a] == 2 * b[a]) {
                                ids.push(c[corner_of(b)]);
                            }
                        }
                        *slot = sh.get(&mut out.points, &ids);
                    }
                }
            }
            for child in BITS {
                let ids: Vec<u32> = BITS.iter().map(|b| lat[child[0] + b[0]][child[1] + b[1]][child[2] + b[2]]).collect();
                out.cells.extend(ids);
                out.block.push(self.block[e]);
                parent.push(e);
            }
        }
        for (name, v) in &self.elem_fields {
            out.elem_fields.push((name.clone(), parent.iter().map(|p| v[*p]).collect()));
        }
        // Side s of a parent: the children touching it, same side number.
        let side_axis = |s: u8| -> (usize, usize) {
            match s {
                1 => (1, 0),
                2 => (0, 1),
                3 => (1, 1),
                4 => (0, 0),
                5 => (2, 0),
                _ => (2, 1),
            }
        };
        for (name, faces) in &self.side_sets {
            let mut nf = Vec::new();
            for (e, s) in faces {
                let (a, v) = side_axis(*s);
                for (k, child) in BITS.iter().enumerate() {
                    if child[a] == v {
                        nf.push(((8 * *e as usize + k) as u32, *s));
                    }
                }
            }
            out.side_sets.push((name.clone(), nf));
        }
        for (name, nodes) in &self.node_sets {
            let set: std::collections::HashSet<u32> = nodes.iter().copied().collect();
            let mut v = nodes.clone();
            v.extend(sh.within(&set));
            v.sort_unstable();
            out.node_sets.push((name.clone(), v));
        }
        Ok(out)
    }
}
