//! Cell types and connectivity storage (VTK 9 "offsets + connectivity" layout).

use serde::{Deserialize, Serialize};

/// VTK cell type ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum CellType {
    Empty = 0,
    Vertex = 1,
    PolyVertex = 2,
    Line = 3,
    PolyLine = 4,
    Triangle = 5,
    TriangleStrip = 6,
    Polygon = 7,
    Pixel = 8,
    Quad = 9,
    Tetra = 10,
    Voxel = 11,
    Hexahedron = 12,
    Wedge = 13,
    Pyramid = 14,
    PentagonalPrism = 15,
    HexagonalPrism = 16,
    QuadraticEdge = 21,
    QuadraticTriangle = 22,
    QuadraticQuad = 23,
    QuadraticTetra = 24,
    QuadraticHexahedron = 25,
    QuadraticWedge = 26,
    QuadraticPyramid = 27,
    BiquadraticQuad = 28,
    TriquadraticHexahedron = 29,
    QuadraticLinearQuad = 30,
    QuadraticLinearWedge = 31,
    BiquadraticQuadraticWedge = 32,
    BiquadraticQuadraticHexahedron = 33,
    BiquadraticTriangle = 34,
    CubicLine = 35,
    Polyhedron = 42,
    LagrangeCurve = 68,
    LagrangeTriangle = 69,
    LagrangeQuadrilateral = 70,
    LagrangeTetrahedron = 71,
    LagrangeHexahedron = 72,
    LagrangeWedge = 73,
    LagrangePyramid = 74,
    BezierCurve = 75,
    BezierTriangle = 76,
    BezierQuadrilateral = 77,
    BezierTetrahedron = 78,
    BezierHexahedron = 79,
    BezierWedge = 80,
    BezierPyramid = 81,
}

impl CellType {
    pub fn from_vtk(id: u8) -> Option<CellType> {
        use CellType::*;
        Some(match id {
            0 => Empty,
            1 => Vertex,
            2 => PolyVertex,
            3 => Line,
            4 => PolyLine,
            5 => Triangle,
            6 => TriangleStrip,
            7 => Polygon,
            8 => Pixel,
            9 => Quad,
            10 => Tetra,
            11 => Voxel,
            12 => Hexahedron,
            13 => Wedge,
            14 => Pyramid,
            15 => PentagonalPrism,
            16 => HexagonalPrism,
            21 => QuadraticEdge,
            22 => QuadraticTriangle,
            23 => QuadraticQuad,
            24 => QuadraticTetra,
            25 => QuadraticHexahedron,
            26 => QuadraticWedge,
            27 => QuadraticPyramid,
            28 => BiquadraticQuad,
            29 => TriquadraticHexahedron,
            30 => QuadraticLinearQuad,
            31 => QuadraticLinearWedge,
            32 => BiquadraticQuadraticWedge,
            33 => BiquadraticQuadraticHexahedron,
            34 => BiquadraticTriangle,
            35 => CubicLine,
            42 => Polyhedron,
            68 => LagrangeCurve,
            69 => LagrangeTriangle,
            70 => LagrangeQuadrilateral,
            71 => LagrangeTetrahedron,
            72 => LagrangeHexahedron,
            73 => LagrangeWedge,
            74 => LagrangePyramid,
            75 => BezierCurve,
            76 => BezierTriangle,
            77 => BezierQuadrilateral,
            78 => BezierTetrahedron,
            79 => BezierHexahedron,
            80 => BezierWedge,
            81 => BezierPyramid,
            _ => return None,
        })
    }

    pub fn vtk_id(self) -> u8 {
        self as u8
    }

    /// Topological dimension (0 = point, 1 = curve, 2 = surface, 3 = volume).
    pub fn dimension(self) -> u8 {
        use CellType::*;
        match self {
            Empty | Vertex | PolyVertex => 0,
            Line | PolyLine | QuadraticEdge | CubicLine | LagrangeCurve | BezierCurve => 1,
            Triangle
            | TriangleStrip
            | Polygon
            | Pixel
            | Quad
            | QuadraticTriangle
            | QuadraticQuad
            | BiquadraticQuad
            | QuadraticLinearQuad
            | BiquadraticTriangle
            | LagrangeTriangle
            | LagrangeQuadrilateral
            | BezierTriangle
            | BezierQuadrilateral => 2,
            _ => 3,
        }
    }

    pub fn is_linear(self) -> bool {
        (self as u8) <= 16 || self == CellType::Polyhedron
    }

    /// Fixed number of points for linear cell types, `None` for variable-size cells.
    pub fn fixed_point_count(self) -> Option<usize> {
        use CellType::*;
        Some(match self {
            Vertex => 1,
            Line => 2,
            Triangle => 3,
            Pixel | Quad | Tetra => 4,
            Voxel | Hexahedron => 8,
            Wedge => 6,
            Pyramid => 5,
            PentagonalPrism => 10,
            HexagonalPrism => 12,
            QuadraticEdge => 3,
            QuadraticTriangle => 6,
            QuadraticQuad => 8,
            QuadraticTetra => 10,
            QuadraticHexahedron => 20,
            QuadraticWedge => 15,
            QuadraticPyramid => 13,
            BiquadraticQuad => 9,
            TriquadraticHexahedron => 27,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        use CellType::*;
        match self {
            Empty => "Empty",
            Vertex => "Vertex",
            PolyVertex => "PolyVertex",
            Line => "Line",
            PolyLine => "PolyLine",
            Triangle => "Triangle",
            TriangleStrip => "TriangleStrip",
            Polygon => "Polygon",
            Pixel => "Pixel",
            Quad => "Quad",
            Tetra => "Tetra",
            Voxel => "Voxel",
            Hexahedron => "Hexahedron",
            Wedge => "Wedge",
            Pyramid => "Pyramid",
            PentagonalPrism => "PentagonalPrism",
            HexagonalPrism => "HexagonalPrism",
            Polyhedron => "Polyhedron",
            _ => "HigherOrder",
        }
    }

    /// Faces of a linear 3D cell as index lists into the cell's local point list.
    /// Returns an empty slice for non-3D / unsupported types.
    pub fn faces(self) -> &'static [&'static [usize]] {
        use CellType::*;
        match self {
            Tetra => &[&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[0, 3, 2]],
            Voxel => &[&[0, 4, 6, 2], &[1, 3, 7, 5], &[0, 1, 5, 4], &[2, 6, 7, 3], &[0, 2, 3, 1], &[4, 5, 7, 6]],
            Hexahedron => &[&[0, 4, 7, 3], &[1, 2, 6, 5], &[0, 1, 5, 4], &[3, 7, 6, 2], &[0, 3, 2, 1], &[4, 5, 6, 7]],
            Wedge => &[&[0, 1, 2], &[3, 5, 4], &[0, 3, 4, 1], &[1, 4, 5, 2], &[2, 5, 3, 0]],
            Pyramid => &[&[0, 3, 2, 1], &[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4]],
            PentagonalPrism => &[&[0, 4, 3, 2, 1], &[5, 6, 7, 8, 9], &[0, 1, 6, 5], &[1, 2, 7, 6], &[2, 3, 8, 7], &[3, 4, 9, 8], &[4, 0, 5, 9]],
            HexagonalPrism => {
                &[&[0, 5, 4, 3, 2, 1], &[6, 7, 8, 9, 10, 11], &[0, 1, 7, 6], &[1, 2, 8, 7], &[2, 3, 9, 8], &[3, 4, 10, 9], &[4, 5, 11, 10], &[5, 0, 6, 11]]
            }
            _ => &[],
        }
    }

    /// Decomposition of a linear 3D cell into tetrahedra (local indices).
    pub fn tets(self) -> &'static [[usize; 4]] {
        use CellType::*;
        match self {
            Tetra => &[[0, 1, 2, 3]],
            Hexahedron => &[[0, 1, 3, 4], [1, 4, 5, 6], [1, 2, 3, 6], [3, 4, 6, 7], [1, 3, 4, 6]],
            Voxel => &[[0, 1, 2, 4], [1, 4, 5, 7], [1, 3, 2, 7], [2, 4, 7, 6], [1, 2, 4, 7]],
            Wedge => &[[0, 1, 2, 3], [1, 2, 3, 4], [2, 3, 4, 5]],
            Pyramid => &[[0, 1, 2, 4], [0, 2, 3, 4]],
            _ => &[],
        }
    }

    /// Edges of a linear cell (local indices).
    pub fn edges(self) -> &'static [[usize; 2]] {
        use CellType::*;
        match self {
            Line => &[[0, 1]],
            Triangle => &[[0, 1], [1, 2], [2, 0]],
            Quad => &[[0, 1], [1, 2], [2, 3], [3, 0]],
            Pixel => &[[0, 1], [1, 3], [3, 2], [2, 0]],
            Tetra => &[[0, 1], [1, 2], [2, 0], [0, 3], [1, 3], [2, 3]],
            Hexahedron => &[[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]],
            Voxel => &[[0, 1], [1, 3], [3, 2], [2, 0], [4, 5], [5, 7], [7, 6], [6, 4], [0, 4], [1, 5], [3, 7], [2, 6]],
            Wedge => &[[0, 1], [1, 2], [2, 0], [3, 4], [4, 5], [5, 3], [0, 3], [1, 4], [2, 5]],
            Pyramid => &[[0, 1], [1, 2], [2, 3], [3, 0], [0, 4], [1, 4], [2, 4], [3, 4]],
            _ => &[],
        }
    }
}

/// Cell connectivity in VTK 9 layout: `offsets.len() == n_cells + 1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cells {
    pub connectivity: Vec<u32>,
    pub offsets: Vec<u32>,
    /// One type per cell. Empty means "homogeneous of `uniform_type`".
    pub types: Vec<CellType>,
    /// For polyhedra: faces stream (VTK-style) keyed by cell index. Rarely used.
    pub polyhedron_faces: Option<Vec<Vec<Vec<u32>>>>,
}

impl Default for Cells {
    fn default() -> Self {
        Cells::new()
    }
}

impl Cells {
    pub fn new() -> Self {
        Self { connectivity: Vec::new(), offsets: vec![0], types: Vec::new(), polyhedron_faces: None }
    }

    pub fn with_capacity(n_cells: usize, n_conn: usize) -> Self {
        let mut offsets = Vec::with_capacity(n_cells + 1);
        offsets.push(0);
        Self { connectivity: Vec::with_capacity(n_conn), offsets, types: Vec::with_capacity(n_cells), polyhedron_faces: None }
    }

    pub fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn cell(&self, i: usize) -> &[u32] {
        &self.connectivity[self.offsets[i] as usize..self.offsets[i + 1] as usize]
    }

    #[inline]
    pub fn cell_type(&self, i: usize) -> CellType {
        self.types[i]
    }

    pub fn push(&mut self, ty: CellType, pts: &[u32]) {
        if self.offsets.is_empty() {
            self.offsets.push(0);
        }
        self.connectivity.extend_from_slice(pts);
        self.offsets.push(self.connectivity.len() as u32);
        self.types.push(ty);
    }

    pub fn iter(&self) -> impl Iterator<Item = (CellType, &[u32])> {
        (0..self.len()).map(move |i| (self.types[i], self.cell(i)))
    }

    pub fn max_dimension(&self) -> u8 {
        self.types.iter().map(|t| t.dimension()).max().unwrap_or(0)
    }

    pub fn append(&mut self, other: &Cells, point_offset: u32) {
        if self.offsets.is_empty() {
            self.offsets.push(0);
        }
        let base = self.connectivity.len() as u32;
        self.connectivity.extend(other.connectivity.iter().map(|&c| c + point_offset));
        self.offsets.extend(other.offsets.iter().skip(1).map(|&o| o + base));
        self.types.extend_from_slice(&other.types);
    }

    pub fn n_bytes(&self) -> usize {
        self.connectivity.len() * 4 + self.offsets.len() * 4 + self.types.len()
    }
}
