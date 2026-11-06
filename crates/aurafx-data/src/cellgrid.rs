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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// VTK's side tables (vtkDG*::Sides, SidesOfSides, SideOffsets, SideShapes): side `s` has
    /// corners `table().sides[s + 1]` and sides `table().sides_of_sides[s + 1]` (index 0 is the
    /// cell itself); side type `k` covers sides `offsets[k + 1] - 1 .. offsets[k + 2] - 1`, all of
    /// shape `shapes[k + 1]`.
    pub fn side_table(self) -> &'static SideTable {
        match self {
            Shape::Vertex => &VERT_SIDES,
            Shape::Edge => &EDGE_SIDES,
            Shape::Triangle => &TRI_SIDES,
            Shape::Quadrilateral => &QUAD_SIDES,
            Shape::Tetrahedron => &TET_SIDES,
            Shape::Hexahedron => &HEX_SIDES,
            Shape::Wedge => &WDG_SIDES,
            Shape::Pyramid => &PYR_SIDES,
        }
    }
    /// Number of side types (faces, edges, vertices; a wedge's quads and triangles are two).
    pub fn side_type_count(self) -> usize {
        self.side_table().offsets.len() - 2
    }
    /// The side ids of side type `k`.
    pub fn side_range(self, k: usize) -> std::ops::Range<usize> {
        let o = self.side_table().offsets;
        o[k + 1] - 1..o[k + 2] - 1
    }
    /// The corners of side `s` (-1: the cell).
    pub fn side_corners(self, s: isize) -> &'static [usize] {
        self.side_table().sides.get((s + 1) as usize).copied().unwrap_or(&[])
    }
    /// The sides of side `s` (-1: the cell's sides).
    pub fn sides_of_side(self, s: isize) -> &'static [usize] {
        self.side_table().sides_of_sides.get((s + 1) as usize).copied().unwrap_or(&[])
    }
    /// The shape of side `s`.
    pub fn side_shape(self, s: usize) -> Shape {
        let t = self.side_table();
        (0..t.shapes.len()).find(|&k| s + 1 < t.offsets[k + 1]).map(|k| t.shapes[k]).unwrap_or(Shape::Vertex)
    }
    /// The side type whose sides have shape `s`.
    pub fn side_type_for(self, s: Shape) -> Option<usize> {
        (0..self.side_type_count()).find(|&k| self.side_table().shapes[k + 1] == s)
    }
    /// VTK's shape name (vertex, edge, triangle...).
    pub fn name(self) -> &'static str {
        match self {
            Shape::Vertex => "vertex",
            Shape::Edge => "edge",
            Shape::Triangle => "triangle",
            Shape::Quadrilateral => "quadrilateral",
            Shape::Tetrahedron => "tetrahedron",
            Shape::Hexahedron => "hexahedron",
            Shape::Wedge => "wedge",
            Shape::Pyramid => "pyramid",
        }
    }
    /// From VTK's shape names (and their aliases).
    pub fn from_name(n: &str) -> Option<Shape> {
        Some(match n {
            "vert" | "vertex" | "sphere" => Shape::Vertex,
            "edge" | "line" | "spring" => Shape::Edge,
            "tri" | "triangle" => Shape::Triangle,
            "quad" | "quadrilateral" => Shape::Quadrilateral,
            "tet" | "tetrahedron" => Shape::Tetrahedron,
            "hex" | "hexahedron" => Shape::Hexahedron,
            "wdg" | "wedge" => Shape::Wedge,
            "pyr" | "pyramid" => Shape::Pyramid,
            _ => return None,
        })
    }
    /// The sides one dimension down that bound the cell, as corner indices in VTK's order
    /// (vtkDG*::Sides, the first side types), used to tell whether a point is inside.
    pub fn bounding_sides(self) -> &'static [&'static [usize]] {
        match self {
            Shape::Vertex => &[],
            Shape::Edge => &[&[0], &[1]],
            Shape::Triangle => &[&[0, 1], &[1, 2], &[2, 0]],
            Shape::Quadrilateral => &[&[0, 1], &[1, 2], &[3, 2], &[0, 3]],
            Shape::Tetrahedron => &[&[0, 1, 3], &[1, 2, 3], &[2, 0, 3], &[0, 2, 1]],
            Shape::Hexahedron => &[&[0, 1, 5, 4], &[1, 2, 6, 5], &[3, 7, 6, 2], &[0, 4, 7, 3], &[0, 3, 2, 1], &[4, 5, 6, 7]],
            Shape::Wedge => &[&[0, 1, 4, 3], &[1, 2, 5, 4], &[0, 3, 5, 2], &[0, 2, 1], &[3, 4, 5]],
            Shape::Pyramid => &[&[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4], &[0, 3, 2, 1]],
        }
    }
    /// Whether parametric point `r` lies in the cell (VTK's vtkDG*::IsInside).
    pub fn is_inside(self, r: [f64; 3], tol: f64) -> bool {
        let (pb, nb) = (1.0 + tol, -1.0 - tol);
        let flat = |v: f64| v.abs() < tol;
        match self {
            Shape::Vertex => flat(r[0]) && flat(r[1]) && flat(r[2]),
            Shape::Edge => r[0] >= nb && r[0] <= pb && flat(r[1]) && flat(r[2]),
            Shape::Triangle => {
                let u = 1.0 - r[0] - r[1];
                [r[0], r[1], u].iter().all(|&v| v >= -tol && v <= pb) && flat(r[2])
            }
            Shape::Quadrilateral => r[0] >= nb && r[0] <= pb && r[1] >= nb && r[1] <= pb && flat(r[2]),
            Shape::Tetrahedron => {
                let u = 1.0 - r[0] - r[1] - r[2];
                [r[0], r[1], r[2], u].iter().all(|&v| v >= -tol && v <= pb)
            }
            Shape::Hexahedron => r.iter().all(|&v| v >= nb && v <= pb),
            Shape::Wedge => {
                let u = 1.0 - r[0] - r[1];
                [r[0], r[1], u].iter().all(|&v| v >= -tol && v <= pb) && r[2] >= nb && r[2] <= pb
            }
            Shape::Pyramid => r[0] >= nb && r[0] <= pb && r[1] >= nb && r[1] <= pb && r[2] >= -tol && r[2] <= pb,
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

/// A shape's sides (see `Shape::side_table`).
#[derive(Debug)]
pub struct SideTable {
    pub offsets: &'static [usize],
    pub shapes: &'static [Shape],
    pub sides: &'static [&'static [usize]],
    pub sides_of_sides: &'static [&'static [usize]],
}

static HEX_SIDES: SideTable = SideTable {
    offsets: &[0, 1, 7, 19, 27],
    shapes: &[Shape::Hexahedron, Shape::Quadrilateral, Shape::Edge, Shape::Vertex],
    sides: &[
        &[0, 1, 2, 3, 4, 5, 6, 7], &[0, 1, 5, 4], &[1, 2, 6, 5], &[3, 7, 6, 2], &[0, 4, 7, 3], &[0, 3, 2, 1], &[4, 5, 6, 7],
        &[0, 1], &[1, 2], &[3, 2], &[0, 3], &[0, 4], &[1, 5], &[3, 7], &[2, 6], &[4, 5], &[5, 6], &[7, 6], &[4, 7],
        &[0], &[1], &[2], &[3], &[4], &[5], &[6], &[7],
    ],
    sides_of_sides: &[
        &[0, 1, 2, 3, 4, 5], &[6, 11, 14, 10], &[7, 13, 15, 11], &[12, 16, 13, 8], &[10, 17, 12, 9], &[9, 8, 7, 6], &[14, 15, 16, 17],
        &[18, 19], &[19, 20], &[21, 20], &[18, 21], &[18, 22], &[19, 23], &[21, 25], &[20, 24], &[22, 23], &[23, 24], &[25, 24], &[22, 25],
        &[], &[], &[], &[], &[], &[], &[], &[],
    ],
};
static TET_SIDES: SideTable = SideTable {
    offsets: &[0, 1, 5, 11, 15],
    shapes: &[Shape::Tetrahedron, Shape::Triangle, Shape::Edge, Shape::Vertex],
    sides: &[&[0, 1, 2, 3], &[0, 1, 3], &[1, 2, 3], &[2, 0, 3], &[0, 2, 1], &[0, 1], &[1, 2], &[2, 0], &[0, 3], &[1, 3], &[2, 3], &[0], &[1], &[2], &[3]],
    sides_of_sides: &[&[0, 1, 2, 3], &[4, 8, 7], &[5, 9, 8], &[6, 7, 9], &[6, 5, 4], &[10, 11], &[11, 12], &[12, 10], &[10, 13], &[11, 13], &[12, 13], &[], &[], &[], &[]],
};
static WDG_SIDES: SideTable = SideTable {
    offsets: &[0, 1, 4, 6, 15, 21],
    shapes: &[Shape::Wedge, Shape::Quadrilateral, Shape::Triangle, Shape::Edge, Shape::Vertex],
    sides: &[
        &[0, 1, 2, 3, 4, 5], &[0, 1, 4, 3], &[1, 2, 5, 4], &[0, 3, 5, 2], &[0, 2, 1], &[3, 4, 5],
        &[0, 1], &[1, 2], &[0, 2], &[0, 3], &[1, 4], &[2, 5], &[3, 4], &[4, 5], &[5, 3],
        &[0], &[1], &[2], &[3], &[4], &[5],
    ],
    sides_of_sides: &[
        &[0, 1, 2, 3, 4], &[5, 9, 11, 8], &[6, 10, 12, 9], &[8, 13, 10, 7], &[7, 6, 5], &[11, 12, 13],
        &[14, 15], &[15, 16], &[14, 16], &[14, 17], &[15, 18], &[16, 19], &[17, 18], &[18, 19], &[19, 17],
        &[], &[], &[], &[], &[], &[],
    ],
};
static PYR_SIDES: SideTable = SideTable {
    offsets: &[0, 1, 5, 6, 14, 19],
    shapes: &[Shape::Pyramid, Shape::Triangle, Shape::Quadrilateral, Shape::Edge, Shape::Vertex],
    sides: &[
        &[0, 1, 2, 3, 4], &[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4], &[0, 3, 2, 1],
        &[0, 1], &[1, 2], &[3, 2], &[0, 3], &[0, 4], &[1, 4], &[2, 4], &[3, 4],
        &[0], &[1], &[2], &[3], &[4],
    ],
    sides_of_sides: &[
        &[0, 1, 2, 3, 4], &[5, 10, 9], &[6, 11, 10], &[7, 12, 11], &[8, 9, 12], &[8, 7, 6, 5],
        &[13, 14], &[14, 15], &[16, 15], &[13, 16], &[13, 17], &[14, 17], &[15, 17], &[16, 17],
        &[], &[], &[], &[], &[],
    ],
};
static QUAD_SIDES: SideTable = SideTable {
    offsets: &[0, 1, 5, 9],
    shapes: &[Shape::Quadrilateral, Shape::Edge, Shape::Vertex],
    sides: &[&[0, 1, 2, 3], &[0, 1], &[1, 2], &[3, 2], &[0, 3], &[0], &[1], &[2], &[3]],
    sides_of_sides: &[&[0, 1, 2, 3], &[4, 5], &[5, 6], &[7, 6], &[4, 7], &[], &[], &[], &[]],
};
static TRI_SIDES: SideTable = SideTable {
    offsets: &[0, 1, 4, 7],
    shapes: &[Shape::Triangle, Shape::Edge, Shape::Vertex],
    sides: &[&[0, 1, 2], &[0, 1], &[1, 2], &[2, 0], &[0], &[1], &[2]],
    sides_of_sides: &[&[0, 1, 2], &[3, 4], &[4, 5], &[5, 3], &[], &[], &[]],
};
static EDGE_SIDES: SideTable = SideTable { offsets: &[0, 1, 3], shapes: &[Shape::Edge, Shape::Vertex], sides: &[&[0, 1], &[0], &[1]], sides_of_sides: &[&[0, 1], &[], &[]] };
static VERT_SIDES: SideTable = SideTable { offsets: &[0, 1], shapes: &[Shape::Vertex], sides: &[&[0]], sides_of_sides: &[&[]] };

/// A group name and an array name within it.
pub type ArrayRef = (String, String);

/// A list of sides of one cell type to draw (VTK's side spec, as Compute Sides makes them):
/// (cell, side) pairs, all of one side shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DgSideSpec {
    /// Two components per side: the cell and the side id within it.
    pub connectivity: ArrayRef,
    pub shape: Shape,
    /// The side type of the parent cell shape (see `Shape::side_type_for`).
    pub side_type: i64,
    /// Position of the first side in VTK's numbering for picking (kept for the file).
    pub offset: i64,
    /// Not drawn.
    pub blanked: bool,
    /// What picking selects (-1: the input cells), kept for the file.
    pub selection_type: i64,
}

/// One cell type of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DgCells {
    pub shape: Shape,
    /// Point ids of each cell (corners, then higher-order nodes).
    pub connectivity: ArrayRef,
    /// The cells themselves are not drawn (Compute Sides hides 3-D cells behind their sides).
    #[serde(default)]
    pub blanked: bool,
    #[serde(default)]
    pub offset: i64,
    /// Lists of sides drawn instead of or besides the cells.
    #[serde(default)]
    pub sides: Vec<DgSideSpec>,
}

impl DgCells {
    pub fn new(shape: Shape, connectivity: ArrayRef) -> DgCells {
        DgCells { shape, connectivity, blanked: false, offset: 0, sides: Vec::new() }
    }
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

/// A probe point found in a cell (see `CellGrid::locate`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeHit {
    /// Index of the probe point.
    pub point: usize,
    pub cell_type: String,
    /// Cell index within its type.
    pub cell: usize,
    /// Parametric coordinates, or None where the point is near the cell but not in it.
    pub params: Option<[f64; 3]>,
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
    /// Sides drawn from side lists (as Compute Sides makes them).
    pub fn n_sides_shown(&self) -> usize {
        self.cell_types.values().flat_map(|t| &t.sides).filter(|s| !s.blanked).map(|s| self.array(&s.connectivity).map(|a| a.len()).unwrap_or(0)).sum()
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
        } else if t.shape.dimension() == 1 {
            let a = col(&j, 0);
            // The axis least along the edge, made perpendicular to it.
            let k = (0..3).min_by(|&x, &y| a[x].abs().total_cmp(&a[y].abs())).unwrap_or(0);
            let mut e = [0.0; 3];
            e[k] = 1.0;
            let b = unit(cross(a, e));
            let c = unit(cross(a, b));
            (0..3).for_each(|i| {
                j[i][1] = b[i];
                j[i][2] = c[i];
            });
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
            // What is drawn, as ParaView's To Unstructured Grid: the cells unless hidden, then
            // each side list's sides (cell, side id) not hidden.
            let mut drawn: Vec<(usize, Shape, &[usize])> = Vec::new();
            if !t.blanked {
                drawn.extend((0..n).map(|c| (c, t.shape, t.shape.side_corners(-1))));
            }
            for spec in t.sides.iter().filter(|s| !s.blanked) {
                let Some(a) = self.array(&spec.connectivity) else { continue };
                for i in 0..a.len() {
                    let (c, side) = (a.get(i, 0), a.get(i, 1));
                    if c >= 0.0 && (c as usize) < n && side >= 0.0 && (side as usize) < t.shape.side_table().sides.len() - 1 {
                        drawn.push((c as usize, t.shape.side_shape(side as usize), t.shape.side_corners(side as isize)));
                    }
                }
            }
            for (c, shape, corners) in drawn {
                let mut conn = Vec::new();
                for &k in corners {
                    let Some(x) = self.corner_position(tname, c, k) else { continue };
                    let key = x.map(|v| (v + 0.0).to_bits());
                    let id = *ids.entry(key).or_insert_with(|| {
                        pts.extend_from_slice(&x);
                        (pts.len() / 3 - 1) as u32
                    });
                    conn.push(id);
                    corners_of.push((tname.clone(), c, k, id));
                }
                if conn.len() == corners.len() {
                    cells.push(shape.vtk_type(), &conn);
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

    /// The cells holding each of `points`, as ParaView's Cell Grid Probe (vtkDGEvaluator)
    /// finds them: for every cell, the points within the sphere around its corners (their mean,
    /// out to the farthest) that are behind every bounding side's plane (VTK's half-spaces,
    /// through the corners; for 2-D cells the edges' directions, for edges their end points)
    /// are taken; then Newton steps from the parametric origin find their parametric
    /// coordinates (none where they do not converge or fall outside the cell). The hits come by
    /// cell type, then by point, then by cell; a point in several cells is listed for each.
    pub fn locate(&self, points: &[[f64; 3]]) -> Vec<ProbeHit> {
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let normalize = |a: [f64; 3]| {
            let n = dot(a, a).sqrt();
            if n == 0.0 {
                a
            } else {
                a.map(|v| v * (1.0 / n))
            }
        };
        let mut hits = Vec::new();
        for (tname, t) in &self.cell_types {
            let (n, _) = self.cell_count(t);
            let nc = t.shape.corners().len();
            let sides = t.shape.bounding_sides();
            // point -> cells holding it, in point order.
            let mut found: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
            for c in 0..n {
                let Some(cc) = (0..nc).map(|k| self.corner_position(tname, c, k)).collect::<Option<Vec<_>>>() else { continue };
                let mut center = [0.0; 3];
                for x in &cc {
                    (0..3).for_each(|k| center[k] += x[k]);
                }
                center = center.map(|v| (1.0 / nc as f64) * v);
                let radius = cc.iter().map(|x| dot(sub(*x, center), sub(*x, center)).sqrt()).fold(-1.0, f64::max);
                let planes: Vec<([f64; 3], [f64; 3])> = sides
                    .iter()
                    .map(|sd| match sd.len() {
                        // An edge's end point: the plane across the edge there.
                        1 => (cc[sd[0]], normalize(sub(cc[sd[0]], cc[1 - sd[0]]))),
                        // VTK's "half-space" of a 2-D cell's edge is along the edge.
                        2 => (cc[sd[1]], normalize(sub(cc[sd[1]], cc[sd[0]]))),
                        3 => (cc[sd[1]], normalize(cross(sub(cc[sd[1]], cc[sd[0]]), sub(cc[sd[2]], cc[sd[1]])))),
                        _ => {
                            let (mut o, mut nrm) = ([0.0; 3], [0.0; 3]);
                            for ii in 0..4 {
                                o = cc[sd[(ii + 1) % 4]];
                                nrm = cross(sub(o, cc[sd[ii]]), sub(cc[sd[(ii + 2) % 4]], o));
                                // VTK tests the cell's corner (ii + 3) % 4 here, not the side's.
                                if dot(sub(cc[(ii + 3) % 4], o), nrm) < 0.0 {
                                    break;
                                }
                            }
                            (o, normalize(nrm))
                        }
                    })
                    .collect();
                for (i, p) in points.iter().enumerate() {
                    let d = sub(*p, center);
                    if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > radius * radius {
                        continue;
                    }
                    if planes.iter().all(|(o, nrm)| dot(sub(*p, *o), *nrm) <= 0.0) {
                        found.entry(i).or_default().push(c);
                    }
                }
            }
            for (i, cells) in found {
                for c in cells {
                    hits.push(ProbeHit { point: i, cell_type: tname.clone(), cell: c, params: self.parametric(tname, c, points[i]) });
                }
            }
        }
        hits
    }

    /// The parametric coordinates of `p` in cell `c` by Newton's method from the parametric
    /// origin (at most 20 steps, to within 1e-7), or None when that fails or lands outside.
    pub fn parametric(&self, tname: &str, c: usize, p: [f64; 3]) -> Option<[f64; 3]> {
        let shape = self.cell_types.get(tname)?.shape;
        let mut r = [0.0; 3];
        for _ in 0..20 {
            let x = self.position(tname, c, r)?;
            let d = [x[0] - p[0], x[1] - p[1], x[2] - p[2]];
            if (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 1e-7 {
                return shape.is_inside(r, 1e-6).then_some(r);
            }
            let j = self.jacobian(tname, c, r)?;
            let inv = inverse3(&j)?;
            for (i, v) in r.iter_mut().enumerate() {
                *v -= (0..3).map(|k| inv[i][k] * d[k]).sum::<f64>();
            }
        }
        None
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
