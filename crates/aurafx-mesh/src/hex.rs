//! All-hexahedral meshes with element blocks, side sets, node sets and element variables.
//!
//! Connectivity follows the Exodus II conventions: HEX8 corners 0-3 on the zeta = -1 face
//! (counter-clockwise seen from zeta = +1), 4-7 above them; HEX20 adds the twelve edge
//! midpoints; HEX27 then the volume centre and the six face centres in the order of sides
//! 5, 6, 4, 2, 1, 3. Side numbers are Exodus' (1-6).

use aurafx_data::*;
use std::collections::{BTreeMap, HashMap};

pub type P3 = [f64; 3];

/// Corner pairs of the twelve edges (Exodus nodes 9-20).
pub const HEX_EDGES: [(usize, usize); 12] = [(0, 1), (1, 2), (2, 3), (3, 0), (0, 4), (1, 5), (2, 6), (3, 7), (4, 5), (5, 6), (6, 7), (7, 4)];
/// Corners of the six Exodus sides.
pub const HEX8_SIDES: [[usize; 4]; 6] = [[0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7], [0, 3, 2, 1], [4, 5, 6, 7]];
/// Mid-edge nodes of each side (HEX20/27), after its four corners.
pub const SIDE_EDGE_NODES: [[usize; 4]; 6] = [[8, 13, 16, 12], [9, 14, 17, 13], [10, 15, 18, 14], [11, 12, 19, 15], [11, 10, 9, 8], [16, 17, 18, 19]];
/// HEX27 face-centre slot (21..26) of each side.
pub const SIDE_CENTER_SLOT: [usize; 6] = [25, 24, 26, 23, 21, 22];
/// Sides whose centres fill slots 21..26.
pub const HEX27_FACE_CENTER_SIDES: [usize; 6] = [5, 6, 4, 2, 1, 3];

#[derive(Clone, Debug, Default)]
pub struct HexMesh {
    pub points: Vec<P3>,
    /// `npe` node ids per element.
    pub cells: Vec<u32>,
    /// Nodes per element: 8, 20 or 27.
    pub npe: usize,
    pub block: Vec<i32>,
    pub block_names: BTreeMap<i32, String>,
    /// (name, [(element index, Exodus side 1..6)]).
    pub side_sets: Vec<(String, Vec<(u32, u8)>)>,
    /// (name, node indices).
    pub node_sets: Vec<(String, Vec<u32>)>,
    /// (name, one value per element).
    pub elem_fields: Vec<(String, Vec<f64>)>,
    /// Pairs of nodes matched across opposite faces (periodic meshes): (axis name, [(minus, plus)]).
    pub periodic_pairs: Vec<(String, Vec<(u32, u32)>)>,
}

impl HexMesh {
    pub fn n_elements(&self) -> usize {
        if self.npe == 0 {
            0
        } else {
            self.cells.len() / self.npe
        }
    }
    pub fn cell(&self, e: usize) -> &[u32] {
        &self.cells[e * self.npe..(e + 1) * self.npe]
    }
    pub fn add_side_set(&mut self, name: &str, faces: Vec<(u32, u8)>) {
        self.side_sets.retain(|(n, _)| n != name);
        self.side_sets.push((name.to_string(), faces));
    }
    pub fn add_node_set(&mut self, name: &str, mut nodes: Vec<u32>) {
        nodes.sort_unstable();
        nodes.dedup();
        self.node_sets.retain(|(n, _)| n != name);
        self.node_sets.push((name.to_string(), nodes));
    }
    pub fn side_set(&self, name: &str) -> Option<&Vec<(u32, u8)>> {
        self.side_sets.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn node_set(&self, name: &str) -> Option<&Vec<u32>> {
        self.node_sets.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn elem_field(&self, name: &str) -> Option<&Vec<f64>> {
        self.elem_fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Append another mesh (with its own numbering); element fields are kept dense (0 where a
    /// part has none).
    pub fn append(&mut self, other: &HexMesh) {
        let n0 = self.points.len() as u32;
        let e0 = self.n_elements();
        if self.npe == 0 {
            self.npe = other.npe;
        }
        assert_eq!(self.npe, other.npe, "appending meshes of different element types");
        self.points.extend_from_slice(&other.points);
        self.cells.extend(other.cells.iter().map(|c| c + n0));
        self.block.extend_from_slice(&other.block);
        for (k, v) in &other.block_names {
            self.block_names.entry(*k).or_insert_with(|| v.clone());
        }
        let ne_other = other.n_elements();
        for (name, vals) in &other.elem_fields {
            if !self.elem_fields.iter().any(|(n, _)| n == name) {
                self.elem_fields.push((name.clone(), vec![0.0; e0]));
            }
            let f = &mut self.elem_fields.iter_mut().find(|(n, _)| n == name).unwrap().1;
            f.extend_from_slice(vals);
        }
        for (_, f) in self.elem_fields.iter_mut() {
            f.resize(e0 + ne_other, 0.0);
        }
        for (name, faces) in &other.side_sets {
            let shifted: Vec<(u32, u8)> = faces.iter().map(|(e, s)| (e + e0 as u32, *s)).collect();
            match self.side_sets.iter_mut().find(|(n, _)| n == name) {
                Some((_, v)) => v.extend(shifted),
                None => self.side_sets.push((name.clone(), shifted)),
            }
        }
        for (name, nodes) in &other.node_sets {
            let shifted: Vec<u32> = nodes.iter().map(|n| n + n0).collect();
            match self.node_sets.iter_mut().find(|(n, _)| n == name) {
                Some((_, v)) => v.extend(shifted),
                None => self.node_sets.push((name.clone(), shifted)),
            }
        }
    }

    /// Merge nodes within `tol` of each other (two lattice passes, as coincident nodes are
    /// produced by identical arithmetic) and drop unused nodes. Each merged node takes the
    /// position of its first occurrence; nodes keep the order of their first occurrence.
    pub fn weld(&mut self, tol: f64) {
        let n = self.points.len();
        if n == 0 {
            return;
        }
        let mut remap: Vec<usize> = (0..n).collect();
        for offset in [0.0, 0.5] {
            let key = |p: &P3| -> [i64; 3] { std::array::from_fn(|k| (p[k] / tol + offset).round() as i64) };
            let mut first: HashMap<[i64; 3], usize> = HashMap::with_capacity(n);
            let mut new = vec![0usize; n];
            // The representative of a group is the smallest original index in it.
            for i in 0..n {
                let r = remap[i];
                let k = key(&self.points[r]);
                let e = first.entry(k).or_insert(r);
                if r < *e {
                    *e = r;
                }
                new[i] = 0;
            }
            for i in 0..n {
                new[i] = first[&key(&self.points[remap[i]])];
            }
            remap = new;
        }
        let mut used = vec![false; n];
        for c in &self.cells {
            used[remap[*c as usize]] = true;
        }
        let mut compact = vec![u32::MAX; n];
        let mut pts = Vec::new();
        for i in 0..n {
            if used[i] {
                compact[i] = pts.len() as u32;
                pts.push(self.points[i]);
            }
        }
        for c in self.cells.iter_mut() {
            *c = compact[remap[*c as usize]];
        }
        for (_, nodes) in self.node_sets.iter_mut() {
            let mut v: Vec<u32> = nodes.iter().map(|x| compact[remap[*x as usize]]).filter(|x| *x != u32::MAX).collect();
            v.sort_unstable();
            v.dedup();
            *nodes = v;
        }
        self.points = pts;
    }

    /// Signed volume of every element from its corners (24 tetrahedra about the centroid).
    pub fn signed_volumes(&self) -> Vec<f64> {
        (0..self.n_elements())
            .map(|e| {
                let c = self.cell(e);
                let p: Vec<P3> = (0..8).map(|k| self.points[c[k] as usize]).collect();
                let cen: P3 = std::array::from_fn(|a| p.iter().map(|q| q[a]).sum::<f64>() / 8.0);
                let mut v = 0.0;
                for side in HEX8_SIDES {
                    let f: Vec<P3> = side.iter().map(|k| p[*k]).collect();
                    let fc: P3 = std::array::from_fn(|a| f.iter().map(|q| q[a]).sum::<f64>() / 4.0);
                    for k in 0..4 {
                        let a = sub(f[k], cen);
                        let b = sub(f[(k + 1) % 4], cen);
                        let cc = sub(fc, cen);
                        v += dot(a, cross(b, cc)) / 6.0;
                    }
                }
                v
            })
            .collect()
    }

    /// Flip inverted elements (zeta reflection; sides 5 and 6 swap); returns how many.
    pub fn fix_orientation(&mut self) -> usize {
        assert_eq!(self.npe, 8, "orientation is fixed on HEX8 meshes");
        let vol = self.signed_volumes();
        let bad: Vec<bool> = vol.iter().map(|v| *v < 0.0).collect();
        let mut n = 0;
        for (e, b) in bad.iter().enumerate() {
            if *b {
                let c = &mut self.cells[e * 8..e * 8 + 8];
                let old: Vec<u32> = c.to_vec();
                for k in 0..8 {
                    c[k] = old[[4, 5, 6, 7, 0, 1, 2, 3][k]];
                }
                n += 1;
            }
        }
        if n > 0 {
            for (_, faces) in self.side_sets.iter_mut() {
                for (e, s) in faces.iter_mut() {
                    if bad[*e as usize] && (*s == 5 || *s == 6) {
                        *s = 11 - *s;
                    }
                }
            }
        }
        n
    }

    /// Corner nodes of side `s` (1..6) of element `e`.
    pub fn side_corners(&self, e: usize, s: u8) -> [u32; 4] {
        let c = self.cell(e);
        HEX8_SIDES[s as usize - 1].map(|k| c[k])
    }

    /// Faces used by exactly one element, in element order then side order.
    pub fn boundary_faces(&self) -> Vec<(u32, u8)> {
        let mut count: HashMap<[u32; 4], u32> = HashMap::new();
        for e in 0..self.n_elements() {
            for s in 1..=6u8 {
                let mut k = self.side_corners(e, s);
                k.sort_unstable();
                *count.entry(k).or_insert(0) += 1;
            }
        }
        let mut out = Vec::new();
        for e in 0..self.n_elements() {
            for s in 1..=6u8 {
                let mut k = self.side_corners(e, s);
                k.sort_unstable();
                if count[&k] == 1 {
                    out.push((e as u32, s));
                }
            }
        }
        out
    }

    /// Boundary faces whose corners all lie on the plane x[axis] = value.
    pub fn faces_on_plane(&self, axis: usize, value: f64, tol: f64) -> Vec<(u32, u8)> {
        self.boundary_faces()
            .into_iter()
            .filter(|(e, s)| {
                let c = self.cell(*e as usize);
                // All the side's nodes, higher-order ones included.
                side_nodes(self.npe, *s).iter().all(|k| (self.points[c[*k] as usize][axis] - value).abs() <= tol)
            })
            .collect()
    }

    pub fn nodes_on_plane(&self, axis: usize, value: f64, tol: f64) -> Vec<u32> {
        (0..self.points.len() as u32).filter(|i| (self.points[*i as usize][axis] - value).abs() <= tol).collect()
    }

    /// Promote a HEX8 mesh to HEX20 or HEX27 with shared edge, face and centre nodes (in the
    /// order of their first sorted key, as the reference implementation allocates them).
    /// `project(face_owner_set, point)` may move new nodes on the listed (element, side) faces.
    pub fn elevate(&mut self, npe: usize, curved: &[(u32, u8, usize)], project: &dyn Fn(usize, P3) -> P3) {
        assert_eq!(self.npe, 8, "only HEX8 meshes are elevated");
        if npe == 8 {
            return;
        }
        assert!(npe == 20 || npe == 27);
        let ne = self.n_elements();
        let old_n = self.points.len() as u32;
        // Unique edges, numbered in sorted order of their (min, max) corner ids.
        let mut edge_keys: Vec<[u32; 2]> = Vec::with_capacity(ne * 12);
        for e in 0..ne {
            let c = self.cell(e);
            for (a, b) in HEX_EDGES {
                let (x, y) = (c[a], c[b]);
                edge_keys.push([x.min(y), x.max(y)]);
            }
        }
        let mut uniq = edge_keys.clone();
        uniq.sort_unstable();
        uniq.dedup();
        let edge_index: HashMap<[u32; 2], u32> = uniq.iter().enumerate().map(|(i, k)| (*k, i as u32)).collect();
        let mut points = self.points.clone();
        points.extend(uniq.iter().map(|k| mid(&[self.points[k[0] as usize], self.points[k[1] as usize]])));
        let mut face_index: HashMap<[u32; 4], u32> = HashMap::new();
        let mut face_base = 0;
        let mut centre_base = 0;
        if npe == 27 {
            let mut fk: Vec<[u32; 4]> = Vec::with_capacity(ne * 6);
            for e in 0..ne {
                for s in HEX27_FACE_CENTER_SIDES {
                    let mut k = self.side_corners(e, s as u8);
                    k.sort_unstable();
                    fk.push(k);
                }
            }
            fk.sort_unstable();
            fk.dedup();
            face_base = points.len() as u32;
            points.extend(fk.iter().map(|k| mid(&k.map(|i| self.points[i as usize]))));
            face_index = fk.iter().enumerate().map(|(i, k)| (*k, i as u32)).collect();
            centre_base = points.len() as u32;
            for e in 0..ne {
                let c = self.cell(e);
                points.push(mid(&std::array::from_fn::<P3, 8, _>(|k| self.points[c[k] as usize])));
            }
        }
        let mut cells = Vec::with_capacity(ne * npe);
        for e in 0..ne {
            let c: Vec<u32> = self.cell(e).to_vec();
            cells.extend_from_slice(&c);
            for i in 0..12 {
                cells.push(old_n + edge_index[&edge_keys[e * 12 + i]]);
            }
            if npe == 27 {
                cells.push(centre_base + e as u32);
                for s in HEX27_FACE_CENTER_SIDES {
                    let mut k = HEX8_SIDES[s - 1].map(|q| c[q]);
                    k.sort_unstable();
                    cells.push(face_base + face_index[&k]);
                }
            }
        }
        // Project the new nodes of curved faces (each node once).
        let mut owner: BTreeMap<u32, usize> = BTreeMap::new();
        for (e, s, shape) in curved {
            let nodes = side_nodes(npe, *s);
            for k in &nodes[4..] {
                owner.insert(cells[*e as usize * npe + *k], *shape);
            }
        }
        for (node, shape) in owner {
            points[node as usize] = project(shape, points[node as usize]);
        }
        self.points = points;
        self.cells = cells;
        self.npe = npe;
    }

    /// Node pairs across opposite faces of the box `lo..hi` matched by their other two
    /// coordinates (tolerance `tol`), for periodic boundary conditions; adds node sets
    /// `x-_node_set`, `x+_node_set` (and y, z) in pair order.
    pub fn pair_periodic(&mut self, lo: P3, hi: P3, tol: f64) -> Result<(), String> {
        for (axis, name) in ["x", "y", "z"].iter().enumerate() {
            let minus = self.nodes_on_plane(axis, lo[axis], tol);
            let plus = self.nodes_on_plane(axis, hi[axis], tol);
            let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
            let key = |p: &P3| -> (i64, i64) { ((p[u] / tol).round() as i64, (p[w] / tol).round() as i64) };
            let map: HashMap<(i64, i64), u32> = plus.iter().map(|n| (key(&self.points[*n as usize]), *n)).collect();
            let mut pairs = Vec::with_capacity(minus.len());
            for m in &minus {
                match map.get(&key(&self.points[*m as usize])) {
                    Some(p) => pairs.push((*m, *p)),
                    None => return Err(format!("node {m} on {name}- has no partner on {name}+: the mesh is not periodic")),
                }
            }
            if pairs.len() != plus.len() {
                return Err(format!("{name}- and {name}+ have different node counts"));
            }
            self.node_sets.retain(|(n, _)| n != &format!("{name}-_node_set") && n != &format!("{name}+_node_set"));
            self.node_sets.push((format!("{name}-_node_set"), pairs.iter().map(|p| p.0).collect()));
            self.node_sets.push((format!("{name}+_node_set"), pairs.iter().map(|p| p.1).collect()));
            self.periodic_pairs.retain(|(n, _)| n != *name);
            self.periodic_pairs.push((name.to_string(), pairs));
        }
        Ok(())
    }

    /// The mesh as auraFX data: an unstructured grid of hexahedra (VTK node order) with the
    /// cell array `ElementBlock`, the element variables, and field arrays `ElementBlockIds`,
    /// `ElementBlockNames`, `SideSet <name>` (element index and Exodus side per face) and
    /// `NodeSet <name>` (node indices).
    pub fn to_dataset(&self) -> DataSet {
        let (ty, perm): (CellType, Vec<usize>) = match self.npe {
            8 => (CellType::Hexahedron, (0..8).collect()),
            20 => (CellType::QuadraticHexahedron, EXODUS_TO_VTK_20.to_vec()),
            _ => (CellType::TriquadraticHexahedron, EXODUS_TO_VTK_27.to_vec()),
        };
        let mut cells = Cells::new();
        for e in 0..self.n_elements() {
            let c = self.cell(e);
            let ids: Vec<u32> = perm.iter().map(|k| c[*k]).collect();
            cells.push(ty, &ids);
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
        for (name, pairs) in &self.periodic_pairs {
            ug.field_data.insert(Array::new(format!("PeriodicPairs {name}"), 2, ArrayData::I32(pairs.iter().flat_map(|(a, b)| [*a as i32, *b as i32]).collect())));
        }
        DataSet::UnstructuredGrid(ug)
    }

    /// A mesh back from auraFX data made by [`HexMesh::to_dataset`] (or any all-hexahedral grid),
    /// or from the blocks an Exodus file reads as (merged by GlobalNodeId, block ids from
    /// ObjectId, side and node sets from the root's field arrays).
    pub fn from_dataset(ds: &DataSet) -> Result<HexMesh, String> {
        if let DataSet::MultiBlock(mb) = ds {
            return Self::from_blocks(mb);
        }
        let ug = match ds {
            DataSet::UnstructuredGrid(u) => u.clone(),
            d => d.to_unstructured().ok_or("not a mesh")?,
        };
        let mut m = HexMesh::default();
        m.points = (0..ug.points.len()).map(|i| ug.points.get(i)).collect();
        for (ty, ids) in ug.cells.iter() {
            let (npe, perm): (usize, Vec<usize>) = match ty {
                CellType::Hexahedron | CellType::Voxel => (8, if ty == CellType::Voxel { vec![0, 1, 3, 2, 4, 5, 7, 6] } else { (0..8).collect() }),
                CellType::QuadraticHexahedron => (20, inverse(&EXODUS_TO_VTK_20)),
                CellType::TriquadraticHexahedron => (27, inverse(&EXODUS_TO_VTK_27)),
                other => return Err(format!("{other:?} cells: only hexahedra are handled")),
            };
            if m.npe == 0 {
                m.npe = npe;
            } else if m.npe != npe {
                return Err("mixed hexahedron types".into());
            }
            m.cells.extend(perm.iter().map(|k| ids[*k]));
        }
        let ne = m.n_elements();
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
            } else if let Some(n) = a.name.strip_prefix("PeriodicPairs ") {
                m.periodic_pairs.push((n.to_string(), (0..a.len()).map(|i| (a.get(i, 0) as u32, a.get(i, 1) as u32)).collect()));
            }
        }
        Ok(m)
    }
}

impl HexMesh {
    fn from_blocks(mb: &MultiBlock) -> Result<HexMesh, String> {
        let leaves = mb.leaves();
        let mut n_nodes = 0usize;
        for (_, d) in &leaves {
            let gid = d.point_data().and_then(|p| p.get("GlobalNodeId")).ok_or("blocks without GlobalNodeId (not read from an Exodus file)")?;
            for i in 0..gid.len() {
                n_nodes = n_nodes.max(gid.get(i, 0) as usize);
            }
        }
        let mut m = HexMesh::default();
        m.points = vec![[0.0; 3]; n_nodes];
        for (name, d) in &leaves {
            let mut part = HexMesh::from_dataset(d)?;
            let gid = d.point_data().unwrap().get("GlobalNodeId").unwrap();
            for (i, p) in part.points.iter().enumerate() {
                m.points[gid.get(i, 0) as usize - 1] = *p;
            }
            for c in part.cells.iter_mut() {
                *c = gid.get(*c as usize, 0) as u32 - 1;
            }
            let id = d.cell_data().and_then(|c| c.get("ObjectId")).map(|a| a.get(0, 0) as i32).unwrap_or(1);
            if m.npe == 0 {
                m.npe = part.npe;
            } else if m.npe != part.npe {
                return Err("blocks of different hexahedron types".into());
            }
            let ne = part.n_elements();
            let e0 = m.n_elements();
            m.cells.extend_from_slice(&part.cells);
            m.block.extend(std::iter::repeat_n(id, ne));
            m.block_names.insert(id, name.rsplit('/').next().unwrap_or(name).to_string());
            for (fname, vals) in part.elem_fields {
                if fname == "ObjectId" || fname == "GlobalElementId" {
                    continue;
                }
                if !m.elem_fields.iter().any(|(n, _)| *n == fname) {
                    m.elem_fields.push((fname.clone(), vec![0.0; e0]));
                }
                let f = &mut m.elem_fields.iter_mut().find(|(n, _)| *n == fname).unwrap().1;
                f.resize(e0, 0.0);
                f.extend(vals);
            }
        }
        let ne = m.n_elements();
        for (_, f) in m.elem_fields.iter_mut() {
            f.resize(ne, 0.0);
        }
        for a in mb.field_data.iter() {
            if let Some(n) = a.name.strip_prefix("SideSet ") {
                m.side_sets.push((n.to_string(), (0..a.len()).map(|i| (a.get(i, 0) as u32, a.get(i, 1) as u8)).collect()));
            } else if let Some(n) = a.name.strip_prefix("NodeSet ") {
                m.node_sets.push((n.to_string(), (0..a.len()).map(|i| a.get(i, 0) as u32).collect()));
            }
        }
        Ok(m)
    }
}

/// Exodus -> VTK local node order (VTK node k is Exodus node perm[k]).
pub const EXODUS_TO_VTK_20: [usize; 20] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15];
pub const EXODUS_TO_VTK_27: [usize; 27] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15, 23, 24, 25, 26, 21, 22, 20];

fn inverse(p: &[usize]) -> Vec<usize> {
    let mut inv = vec![0; p.len()];
    for (k, v) in p.iter().enumerate() {
        inv[*v] = k;
    }
    inv
}

/// All nodes of Exodus side `s` (1..6) for `npe` nodes per element.
pub fn side_nodes(npe: usize, s: u8) -> Vec<usize> {
    let i = s as usize - 1;
    let mut v = HEX8_SIDES[i].to_vec();
    if npe >= 20 {
        v.extend_from_slice(&SIDE_EDGE_NODES[i]);
    }
    if npe == 27 {
        v.push(SIDE_CENTER_SLOT[i]);
    }
    v
}

pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn mid<const N: usize>(p: &[P3; N]) -> P3 {
    std::array::from_fn(|a| p.iter().map(|q| q[a]).sum::<f64>() / N as f64)
}
