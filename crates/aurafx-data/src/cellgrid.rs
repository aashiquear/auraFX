//! Cell grids (VTK's vtkCellGrid with discontinuous-Galerkin cells): cells of a few shapes
//! whose fields ("cell attributes") are polynomials given by coefficients of a basis on each
//! cell: HGRAD (nodal values, linear or quadratic), HCURL (edge), HDIV (face) and constant
//! fields. The values of a field anywhere in a cell come from its basis functions at the
//! cell's parametric coordinates; HCURL and HDIV fields are mapped by the cell's Jacobian.

use crate::dg_basis::{BasisFn, TABLE};
use crate::{Array, ArrayData, Attributes, Bounds, CellType, Cells, Points, UnstructuredGrid};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Vertex,
    Edge,
    Triangle,
    Quadrilateral,
    Tetrahedron,
    Hexahedron,
    Wedge,
    Pyramid,
}

impl Shape {
    /// From a VTK DG cell type name (vtkDGHex, vtkDGTet...).
    pub fn from_type(name: &str) -> Option<Shape> {
        Some(match name {
            "vtkDGVert" => Shape::Vertex,
            "vtkDGEdge" => Shape::Edge,
            "vtkDGTri" => Shape::Triangle,
            "vtkDGQuad" => Shape::Quadrilateral,
            "vtkDGTet" => Shape::Tetrahedron,
            "vtkDGHex" => Shape::Hexahedron,
            "vtkDGWdg" => Shape::Wedge,
            "vtkDGPyr" => Shape::Pyramid,
            _ => return None,
        })
    }
    pub fn type_name(self) -> &'static str {
        match self {
            Shape::Vertex => "vtkDGVert",
            Shape::Edge => "vtkDGEdge",
            Shape::Triangle => "vtkDGTri",
            Shape::Quadrilateral => "vtkDGQuad",
            Shape::Tetrahedron => "vtkDGTet",
            Shape::Hexahedron => "vtkDGHex",
            Shape::Wedge => "vtkDGWdg",
            Shape::Pyramid => "vtkDGPyr",
        }
    }
    /// Name used by the basis functions (Hex, Tet, ...).
    fn basis_name(self) -> &'static str {
        match self {
            Shape::Vertex => "Vert",
            Shape::Edge => "Edge",
            Shape::Triangle => "Tri",
            Shape::Quadrilateral => "Quad",
            Shape::Tetrahedron => "Tet",
            Shape::Hexahedron => "Hex",
            Shape::Wedge => "Wdg",
            Shape::Pyramid => "Pyr",
        }
    }
    pub fn dimension(self) -> usize {
        match self {
            Shape::Vertex => 0,
            Shape::Edge => 1,
            Shape::Triangle | Shape::Quadrilateral => 2,
            _ => 3,
        }
    }
    /// The linear VTK cell of this shape.
    pub fn vtk_type(self) -> CellType {
        match self {
            Shape::Vertex => CellType::Vertex,
            Shape::Edge => CellType::Line,
            Shape::Triangle => CellType::Triangle,
            Shape::Quadrilateral => CellType::Quad,
            Shape::Tetrahedron => CellType::Tetra,
            Shape::Hexahedron => CellType::Hexahedron,
            Shape::Wedge => CellType::Wedge,
            Shape::Pyramid => CellType::Pyramid,
        }
    }
    /// Parametric coordinates of the corners (VTK's vtkDG* Parameters).
    pub fn corners(self) -> &'static [[f64; 3]] {
        match self {
            Shape::Vertex => &[[0.0, 0.0, 0.0]],
            Shape::Edge => &[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            Shape::Triangle => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            Shape::Quadrilateral => &[[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
            Shape::Tetrahedron => &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Shape::Hexahedron => &[
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
            Shape::Wedge => &[[0.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 1.0, -1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
            Shape::Pyramid => &[[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }
    /// Parametric center: the mean of the corners (as VTK).
    pub fn center(self) -> [f64; 3] {
        let c = self.corners();
        let mut m = [0.0; 3];
        for p in c {
            (0..3).for_each(|k| m[k] += p[k]);
        }
        m.map(|v| v / c.len() as f64)
    }
}

/// A group name and an array name within it.
pub type ArrayRef = (String, String);

/// One cell type of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DgCells {
    pub shape: Shape,
    /// Point ids of each cell (corners, then higher-order nodes).
    pub connectivity: ArrayRef,
}

/// How a cell attribute is stored on one cell type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DgAttrInfo {
    /// HGRAD, HCURL, HDIV or constant.
    pub function_space: String,
    /// C (complete), I (incomplete/serendipity), F (full).
    pub basis: String,
    pub order: u32,
    /// The group holding values per point (shared through the connectivity), or None for
    /// coefficients per cell.
    pub sharing: Option<String>,
    pub values: ArrayRef,
    pub connectivity: Option<ArrayRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DgAttribute {
    pub name: String,
    pub components: usize,
    /// True for the attribute giving the cells' coordinates.
    pub shape: bool,
    /// By cell type name.
    pub info: IndexMap<String, DgAttrInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CellGrid {
    /// Arrays by group ("points", a cell type's name...).
    pub groups: IndexMap<String, Vec<Array>>,
    /// Cell types by name (vtkDGHex...).
    pub cell_types: IndexMap<String, DgCells>,
    pub attributes: Vec<DgAttribute>,
    pub field_data: Attributes,
    /// The cells with the fields at their corners (what is drawn, listed and picked), made on
    /// first use. Call `invalidate` after changing the grid.
    #[serde(skip)]
    mesh: std::sync::OnceLock<UnstructuredGrid>,
}

/// A basis function and its number of functions.
fn basis(space: &str, kind: &str, gradient: bool) -> Option<(BasisFn, usize)> {
    TABLE.iter().find(|e| e.0 == space && e.1 == kind && e.2 == gradient).map(|e| (e.3, if gradient { basis_size(space, kind) } else { e.4 }))
}

fn basis_size(space: &str, kind: &str) -> usize {
    TABLE.iter().find(|e| e.0 == space && e.1 == kind && !e.2).map(|e| e.4).unwrap_or(0)
}

fn basis_kind(shape: Shape, info: &DgAttrInfo) -> (String, String) {
    match info.function_space.as_str() {
        "constant" => ("constant".into(), "CellC0".into()),
        s => (s.to_string(), format!("{}{}{}", shape.basis_name(), info.basis, info.order)),
    }
}

fn det3(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inverse3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let d = det3(m);
    if d == 0.0 || !d.is_finite() {
        return None;
    }
    let c = |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    Some([
        [c(1, 1, 2, 2) / d, -c(0, 1, 2, 2) / d, c(0, 1, 1, 2) / d],
        [-c(1, 0, 2, 2) / d, c(0, 0, 2, 2) / d, -c(0, 0, 1, 2) / d],
        [c(1, 0, 2, 1) / d, -c(0, 0, 2, 1) / d, c(0, 0, 1, 1) / d],
    ])
}

impl CellGrid {
    pub fn array(&self, r: &ArrayRef) -> Option<&Array> {
        self.groups.get(&r.0)?.iter().find(|a| a.name == r.1)
    }
    pub fn shape_attribute(&self) -> Option<&DgAttribute> {
        self.attributes.iter().find(|a| a.shape)
    }
    pub fn n_cells(&self) -> usize {
        self.cell_types.values().map(|c| self.array(&c.connectivity).map(|a| a.len()).unwrap_or(0)).sum()
    }
    /// Cells of one type: (number, nodes per cell).
    fn cell_count(&self, t: &DgCells) -> (usize, usize) {
        self.array(&t.connectivity).map(|a| (a.len(), a.ncomp)).unwrap_or((0, 0))
    }

    /// Value of attribute `att` on cell `c` of type `tname` at parametric coordinates `p`.
    pub fn evaluate(&self, att: &DgAttribute, tname: &str, c: usize, p: [f64; 3]) -> Option<Vec<f64>> {
        let t = self.cell_types.get(tname)?;
        let info = att.info.get(tname)?;
        let (space, kind) = basis_kind(t.shape, info);
        let (f, nfun) = basis(&space, &kind, false)?;
        let vector_basis = matches!(space.as_str(), "HCURL" | "HDIV");
        let mut b = vec![0.0; if vector_basis { 3 * nfun } else { nfun }];
        f(p, &mut b);
        let vals = self.array(&info.values)?;
        let nc = att.components;
        let mut out = vec![0.0; nc];
        match &info.sharing {
            Some(_) => {
                let conn = self.array(info.connectivity.as_ref().unwrap_or(&t.connectivity))?;
                for k in 0..nfun.min(conn.ncomp) {
                    let pid = conn.get(c, k) as usize;
                    for (j, o) in out.iter_mut().enumerate() {
                        *o += b[k] * vals.get(pid, j.min(vals.ncomp.saturating_sub(1)));
                    }
                }
            }
            None if vector_basis => {
                for k in 0..nfun.min(vals.ncomp) {
                    let coef = vals.get(c, k);
                    for (j, o) in out.iter_mut().enumerate().take(3) {
                        *o += coef * b[3 * k + j];
                    }
                }
            }
            None => {
                for k in 0..nfun {
                    for (j, o) in out.iter_mut().enumerate() {
                        let idx = k * nc + j;
                        if idx < vals.ncomp {
                            *o += b[k] * vals.get(c, idx);
                        }
                    }
                }
            }
        }
        if vector_basis {
            let jac = self.jacobian(tname, c, p)?;
            let v = [out[0], out.get(1).copied().unwrap_or(0.0), out.get(2).copied().unwrap_or(0.0)];
            let r = if space == "HCURL" {
                // Covariant: (J^T)^-1 v.
                let jt = [[jac[0][0], jac[1][0], jac[2][0]], [jac[0][1], jac[1][1], jac[2][1]], [jac[0][2], jac[1][2], jac[2][2]]];
                let inv = inverse3(&jt)?;
                std::array::from_fn::<f64, 3, _>(|i| (0..3).map(|k| inv[i][k] * v[k]).sum())
            } else {
                // Contravariant: J v / det J.
                let d = det3(&jac);
                std::array::from_fn::<f64, 3, _>(|i| (0..3).map(|k| jac[i][k] * v[k]).sum::<f64>() / d)
            };
            out[..3.min(nc)].copy_from_slice(&r[..3.min(nc)]);
        }
        Some(out)
    }

    /// The Jacobian dx_i/dxi_j of cell `c` at `p` (2-D cells completed with their unit normal,
    /// edges with two unit vectors across them).
    pub fn jacobian(&self, tname: &str, c: usize, p: [f64; 3]) -> Option<[[f64; 3]; 3]> {
        let t = self.cell_types.get(tname)?;
        let shape = self.shape_attribute()?;
        let info = shape.info.get(tname)?;
        let (space, kind) = basis_kind(t.shape, info);
        let (g, nfun) = basis(&space, &kind, true)?;
        let mut grad = vec![0.0; 3 * nfun];
        g(p, &mut grad);
        let vals = self.array(&info.values)?;
        let mut j = [[0.0; 3]; 3];
        for k in 0..nfun {
            let x: [f64; 3] = match &info.sharing {
                Some(_) => {
                    let conn = self.array(info.connectivity.as_ref().unwrap_or(&t.connectivity))?;
                    if k >= conn.ncomp {
                        continue;
                    }
                    let pid = conn.get(c, k) as usize;
                    std::array::from_fn(|i| vals.get(pid, i))
                }
                None => std::array::from_fn(|i| vals.get(c, 3 * k + i)),
            };
            for i in 0..3 {
                for jj in 0..3 {
                    j[i][jj] += grad[3 * k + jj] * x[i];
                }
            }
        }
        let col = |m: &[[f64; 3]; 3], k: usize| [m[0][k], m[1][k], m[2][k]];
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let unit = |a: [f64; 3]| {
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            if l > 0.0 {
                a.map(|v| v / l)
            } else {
                a
            }
        };
        if t.shape.dimension() == 2 {
            let n = unit(cross(col(&j, 0), col(&j, 1)));
            (0..3).for_each(|i| j[i][2] = n[i]);
        }
        Some(j)
    }

    /// The position of corner `k` of cell `c`: the shape's node coordinates (as ParaView takes
    /// them), not an evaluation (the pyramid's basis is singular at its apex).
    pub fn corner_position(&self, tname: &str, c: usize, k: usize) -> Option<[f64; 3]> {
        let t = self.cell_types.get(tname)?;
        let shape = self.shape_attribute()?;
        let info = shape.info.get(tname)?;
        let vals = self.array(&info.values)?;
        match &info.sharing {
            Some(_) => {
                let conn = self.array(info.connectivity.as_ref().unwrap_or(&t.connectivity))?;
                let pid = conn.get(c, k) as usize;
                Some(std::array::from_fn(|i| vals.get(pid, i)))
            }
            None if info.function_space == "HGRAD" => Some(std::array::from_fn(|i| vals.get(c, 3 * k + i))),
            None => self.position(tname, c, t.shape.corners()[k]),
        }
    }

    /// The position of parametric point `p` of cell `c` (from the shape attribute).
    pub fn position(&self, tname: &str, c: usize, p: [f64; 3]) -> Option<[f64; 3]> {
        let shape = self.shape_attribute()?;
        let v = self.evaluate(shape, tname, c, p)?;
        Some([v[0], v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)])
    }

    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        if let Some(s) = self.shape_attribute() {
            for (tname, t) in &self.cell_types {
                let (n, _) = self.cell_count(t);
                if !s.info.contains_key(tname) {
                    continue;
                }
                for c in 0..n {
                    for k in 0..t.shape.corners().len() {
                        if let Some(x) = self.corner_position(tname, c, k) {
                            b.add_point(x);
                        }
                    }
                }
            }
        }
        b
    }

    /// As ParaView's Cell Grid To Unstructured Grid: linear cells through the corners (points
    /// at the same place merged), each attribute evaluated at every cell's corners and averaged
    /// where cells meet.
    pub fn to_unstructured(&self) -> UnstructuredGrid {
        self.mesh().clone()
    }

    /// The cached corner mesh (see `to_unstructured`).
    pub fn mesh(&self) -> &UnstructuredGrid {
        self.mesh.get_or_init(|| self.build_mesh())
    }

    /// Forget the cached corner mesh after the grid changed.
    pub fn invalidate(&mut self) {
        self.mesh = Default::default();
    }

    fn build_mesh(&self) -> UnstructuredGrid {
        let mut ids: HashMap<[u64; 3], u32> = HashMap::new();
        let mut pts: Vec<f64> = Vec::new();
        let mut cells = Cells::new();
        // (cell type, cell, corner, output point)
        let mut corners_of: Vec<(String, usize, usize, u32)> = Vec::new();
        for (tname, t) in &self.cell_types {
            let (n, _) = self.cell_count(t);
            for c in 0..n {
                let mut conn = Vec::new();
                for k in 0..t.shape.corners().len() {
                    let Some(x) = self.corner_position(tname, c, k) else { continue };
                    let key = x.map(|v| (v + 0.0).to_bits());
                    let id = *ids.entry(key).or_insert_with(|| {
                        pts.extend_from_slice(&x);
                        (pts.len() / 3 - 1) as u32
                    });
                    conn.push(id);
                    corners_of.push((tname.clone(), c, k, id));
                }
                if conn.len() == t.shape.corners().len() {
                    cells.push(t.shape.vtk_type(), &conn);
                }
            }
        }
        let np = pts.len() / 3;
        let mut count = vec![0u32; np];
        for (_, _, _, id) in &corners_of {
            count[*id as usize] += 1;
        }
        let mut point_data = Attributes::new();
        for att in self.attributes.iter().filter(|a| !a.shape) {
            let nc = att.components;
            let mut sum = vec![0.0; np * nc];
            let mut seen = vec![0u32; np];
            for (tname, c, k, id) in &corners_of {
                let shape = self.cell_types[tname].shape;
                if let Some(v) = self.evaluate(att, tname, *c, shape.corners()[*k]) {
                    for j in 0..nc {
                        sum[*id as usize * nc + j] += v[j];
                    }
                    seen[*id as usize] += 1;
                }
            }
            for i in 0..np {
                let w = if seen[i] > 0 { seen[i] as f64 } else { f64::NAN };
                for j in 0..nc {
                    sum[i * nc + j] /= w;
                }
            }
            point_data.insert(Array::new(att.name.clone(), nc, ArrayData::F64(sum)));
        }
        UnstructuredGrid { points: Points::new_f64(pts), cells, point_data, cell_data: Attributes::new(), field_data: self.field_data.clone() }
    }

    /// Points at the cells' parametric centers with every attribute's value there.
    pub fn centers(&self) -> crate::PolyData {
        let mut pts = Vec::new();
        let mut rows: Vec<(String, usize)> = Vec::new();
        for (tname, t) in &self.cell_types {
            let (n, _) = self.cell_count(t);
            for c in 0..n {
                if let Some(x) = self.position(tname, c, t.shape.center()) {
                    pts.extend_from_slice(&x);
                    rows.push((tname.clone(), c));
                }
            }
        }
        let mut pd = crate::PolyData::from_points(Points::new_f64(pts));
        for att in self.attributes.iter().filter(|a| !a.shape) {
            let nc = att.components;
            let v: Vec<f64> = rows
                .iter()
                .flat_map(|(tname, c)| self.evaluate(att, tname, *c, self.cell_types[tname].shape.center()).unwrap_or_else(|| vec![f64::NAN; nc]))
                .collect();
            pd.point_data.insert(Array::new(att.name.clone(), nc, ArrayData::F64(v)));
        }
        pd
    }
}
