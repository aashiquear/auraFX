//! Decomposition of arbitrary linear cells into simplices (tets, triangles, lines, verts).

use aurafx_data::{CellType, Cells, DataSet, PolyData, UnstructuredGrid};

#[derive(Debug, Clone, Copy)]
pub enum Simplex {
    Vert(u32),
    Line([u32; 2]),
    Tri([u32; 3]),
    Tet([u32; 4]),
}

/// Call `f` with every simplex of cell `pts` of type `ty`, along with the owning cell id.
pub fn decompose(ty: CellType, pts: &[u32], faces: Option<&[Vec<u32>]>, mut f: impl FnMut(Simplex)) {
    use CellType::*;
    match ty {
        Empty => {}
        Vertex | PolyVertex => {
            for &p in pts {
                f(Simplex::Vert(p));
            }
        }
        Line | PolyLine | QuadraticEdge | CubicLine | LagrangeCurve | BezierCurve => {
            let n = if matches!(ty, QuadraticEdge) { 2 } else { pts.len() };
            for w in pts[..n].windows(2) {
                f(Simplex::Line([w[0], w[1]]));
            }
        }
        Triangle | QuadraticTriangle | BiquadraticTriangle | LagrangeTriangle | BezierTriangle => f(Simplex::Tri([pts[0], pts[1], pts[2]])),
        Quad | QuadraticQuad | BiquadraticQuad | QuadraticLinearQuad | LagrangeQuadrilateral | BezierQuadrilateral => {
            f(Simplex::Tri([pts[0], pts[1], pts[2]]));
            f(Simplex::Tri([pts[0], pts[2], pts[3]]));
        }
        Pixel => {
            f(Simplex::Tri([pts[0], pts[1], pts[3]]));
            f(Simplex::Tri([pts[0], pts[3], pts[2]]));
        }
        Polygon => {
            for k in 1..pts.len().saturating_sub(1) {
                f(Simplex::Tri([pts[0], pts[k], pts[k + 1]]));
            }
        }
        TriangleStrip => {
            for k in 0..pts.len().saturating_sub(2) {
                if k % 2 == 0 {
                    f(Simplex::Tri([pts[k], pts[k + 1], pts[k + 2]]));
                } else {
                    f(Simplex::Tri([pts[k + 1], pts[k], pts[k + 2]]));
                }
            }
        }
        Tetra => f(Simplex::Tet([pts[0], pts[1], pts[2], pts[3]])),
        Hexahedron | Wedge | Pyramid => split_ids(ty, &pts[..ty.fixed_point_count().unwrap_or(pts.len()).min(pts.len())], &mut f),
        Voxel => {
            let h = [pts[0], pts[1], pts[3], pts[2], pts[4], pts[5], pts[7], pts[6]];
            split_ids(Hexahedron, &h, &mut f)
        }
        QuadraticTetra | LagrangeTetrahedron | BezierTetrahedron => f(Simplex::Tet([pts[0], pts[1], pts[2], pts[3]])),
        QuadraticHexahedron | TriquadraticHexahedron | BiquadraticQuadraticHexahedron | LagrangeHexahedron | BezierHexahedron => {
            split_ids(Hexahedron, &pts[..8], &mut f)
        }
        QuadraticWedge | QuadraticLinearWedge | BiquadraticQuadraticWedge | LagrangeWedge | BezierWedge => split_ids(Wedge, &pts[..6], &mut f),
        QuadraticPyramid | LagrangePyramid | BezierPyramid => split_ids(Pyramid, &pts[..5], &mut f),
        PentagonalPrism | HexagonalPrism => {
            // Prism with n-gon base: fan from base vertex 0 into wedges.
            let n = pts.len() / 2;
            for k in 1..n - 1 {
                let w = [pts[0], pts[k], pts[k + 1], pts[n], pts[n + k], pts[n + k + 1]];
                split_ids(Wedge, &w, &mut f);
            }
        }
        Polyhedron => {
            // Fan tets from the centroid-ish first vertex to each face triangle.
            if let Some(faces) = faces {
                let apex = pts[0];
                for face in faces {
                    if face.contains(&apex) {
                        continue;
                    }
                    for k in 1..face.len().saturating_sub(1) {
                        f(Simplex::Tet([apex, face[0], face[k], face[k + 1]]));
                    }
                }
            }
        }
    }
}

/// Split a hexahedron, wedge or pyramid (VTK corner order, global point ids) into tetrahedra so
/// that neighbouring cells agree on shared faces: every quadrilateral face is cut along the
/// diagonal through its smallest point id. (A fixed split cuts a face shared by two cells along
/// different diagonals, which leaves cracks in contours and inner faces after Tetrahedralize.)
fn split_ids(ty: CellType, p: &[u32], f: &mut impl FnMut(Simplex)) {
    for t in split_by(ty, |k| p[k]) {
        f(Simplex::Tet([p[t[0]], p[t[1]], p[t[2]], p[t[3]]]));
    }
}

/// The conforming split of a hexahedron, wedge or pyramid given a total order of its corners
/// (`rank` of local corner k; the split depends only on which corner of each quad is smallest).
/// Tetrahedra are positively oriented for cells in VTK's orientation.
pub fn split_by<K: Ord>(ty: CellType, rank: impl Fn(usize) -> K) -> &'static [[usize; 4]] {
    let quad_bit = |q: &[usize]| {
        let m = (0..4).min_by(|&a, &b| rank(q[a]).cmp(&rank(q[b]))).unwrap();
        m == 1 || m == 3
    };
    match ty {
        CellType::Hexahedron => {
            let mut bits = 0;
            for (k, q) in CellType::Hexahedron.faces().iter().enumerate() {
                bits |= (quad_bit(q) as usize) << k;
            }
            let t = HEX_SPLIT[bits];
            if t.is_empty() {
                CellType::Hexahedron.tets()
            } else {
                t
            }
        }
        CellType::Wedge => {
            let mut bits = 0;
            for (k, q) in CellType::Wedge.faces().iter().enumerate() {
                if q.len() == 4 {
                    bits |= (quad_bit(q) as usize) << k;
                }
            }
            let t = WEDGE_SPLIT[bits];
            if t.is_empty() {
                CellType::Wedge.tets()
            } else {
                t
            }
        }
        CellType::Pyramid => {
            if quad_bit(&[0, 1, 2, 3]) {
                &[[0, 1, 3, 4], [1, 2, 3, 4]]
            } else {
                &[[0, 1, 2, 4], [0, 2, 3, 4]]
            }
        }
        other => other.tets(),
    }
}

// Generated by brute-force search: every split of the cube / prism into tetrahedra on its
// corners whose boundary triangles follow the face diagonals given by the index bits (bit f
// set: face f of `CellType::faces()` is cut along its 2nd-4th corner diagonal). Empty entries
// cannot occur with the smallest-corner rule.
#[rustfmt::skip]
pub(crate) const HEX_SPLIT: [&[[usize; 4]]; 64] = [
    &[[0, 4, 6, 7], [0, 6, 3, 7], [0, 1, 2, 6], [0, 5, 1, 6], [0, 4, 5, 6], [0, 2, 3, 6]],
    &[[0, 3, 4, 6], [3, 4, 6, 7], [0, 1, 2, 6], [0, 5, 1, 6], [0, 4, 5, 6], [0, 2, 3, 6]],
    &[[0, 4, 6, 7], [0, 6, 3, 7], [0, 1, 2, 5], [0, 5, 2, 6], [0, 4, 5, 6], [0, 2, 3, 6]],
    &[[0, 2, 3, 4], [3, 4, 6, 7], [0, 1, 2, 5], [2, 4, 5, 6], [0, 2, 4, 5], [2, 3, 4, 6]],
    &[[0, 4, 6, 7], [0, 6, 3, 7], [0, 1, 2, 6], [1, 4, 5, 6], [0, 4, 1, 6], [0, 2, 3, 6]],
    &[[0, 2, 3, 4], [3, 4, 6, 7], [1, 2, 4, 6], [1, 4, 5, 6], [0, 1, 2, 4], [2, 3, 4, 6]],
    &[],
    &[[0, 2, 3, 4], [3, 4, 6, 7], [1, 2, 4, 5], [2, 4, 5, 6], [0, 1, 2, 4], [2, 3, 4, 6]],
    &[[0, 4, 6, 7], [0, 2, 3, 7], [0, 1, 2, 6], [0, 5, 1, 6], [0, 4, 5, 6], [0, 6, 2, 7]],
    &[],
    &[[0, 4, 2, 7], [0, 2, 3, 7], [0, 1, 2, 5], [2, 4, 5, 6], [0, 2, 4, 5], [2, 4, 6, 7]],
    &[[0, 2, 3, 4], [2, 3, 4, 7], [0, 1, 2, 5], [2, 4, 5, 6], [0, 2, 4, 5], [2, 4, 6, 7]],
    &[[0, 4, 1, 7], [0, 2, 3, 7], [1, 6, 2, 7], [1, 4, 5, 6], [0, 1, 2, 7], [1, 4, 6, 7]],
    &[[0, 2, 3, 4], [2, 3, 4, 7], [1, 2, 4, 6], [1, 4, 5, 6], [0, 1, 2, 4], [2, 4, 6, 7]],
    &[[0, 4, 2, 7], [0, 2, 3, 7], [1, 2, 4, 5], [2, 4, 5, 6], [0, 1, 2, 4], [2, 4, 6, 7]],
    &[[0, 2, 3, 4], [2, 3, 4, 7], [1, 2, 4, 5], [2, 4, 5, 6], [0, 1, 2, 4], [2, 4, 6, 7]],
    &[[0, 4, 6, 7], [0, 6, 3, 7], [1, 2, 3, 6], [0, 5, 1, 6], [0, 4, 5, 6], [0, 1, 3, 6]],
    &[[0, 3, 4, 5], [3, 4, 6, 7], [1, 2, 3, 6], [1, 3, 5, 6], [0, 1, 3, 5], [3, 4, 5, 6]],
    &[],
    &[[0, 3, 4, 5], [3, 4, 6, 7], [1, 2, 3, 5], [2, 3, 5, 6], [0, 1, 3, 5], [3, 4, 5, 6]],
    &[[0, 4, 1, 7], [0, 1, 3, 7], [1, 2, 3, 6], [1, 4, 5, 6], [1, 6, 3, 7], [1, 4, 6, 7]],
    &[[0, 1, 3, 4], [1, 3, 4, 7], [1, 2, 3, 6], [1, 4, 5, 6], [1, 6, 3, 7], [1, 4, 6, 7]],
    &[],
    &[[0, 1, 3, 4], [3, 4, 6, 7], [1, 2, 3, 5], [2, 3, 5, 6], [1, 3, 4, 5], [3, 4, 5, 6]],
    &[],
    &[],
    &[],
    &[],
    &[[0, 4, 1, 7], [0, 1, 3, 7], [1, 6, 2, 7], [1, 4, 5, 6], [1, 2, 3, 7], [1, 4, 6, 7]],
    &[[0, 1, 3, 4], [1, 3, 4, 7], [1, 6, 2, 7], [1, 4, 5, 6], [1, 2, 3, 7], [1, 4, 6, 7]],
    &[],
    &[[0, 1, 3, 4], [2, 3, 4, 7], [1, 2, 4, 5], [2, 4, 5, 6], [2, 4, 6, 7], [1, 2, 3, 4]],
    &[[0, 4, 5, 7], [0, 6, 3, 7], [0, 1, 2, 6], [0, 5, 1, 6], [0, 2, 3, 6], [0, 5, 6, 7]],
    &[],
    &[[0, 4, 5, 7], [0, 5, 3, 7], [0, 1, 2, 5], [2, 3, 5, 6], [3, 5, 6, 7], [0, 2, 3, 5]],
    &[[0, 3, 4, 5], [3, 4, 5, 7], [0, 1, 2, 5], [2, 3, 5, 6], [3, 5, 6, 7], [0, 2, 3, 5]],
    &[],
    &[],
    &[],
    &[],
    &[[0, 4, 5, 7], [0, 2, 3, 7], [0, 1, 2, 6], [0, 5, 1, 6], [0, 6, 2, 7], [0, 5, 6, 7]],
    &[],
    &[[0, 4, 2, 7], [0, 2, 3, 7], [0, 1, 2, 5], [2, 5, 6, 7], [0, 2, 4, 5], [2, 4, 5, 7]],
    &[[0, 2, 3, 4], [2, 3, 4, 7], [0, 1, 2, 5], [2, 5, 6, 7], [0, 2, 4, 5], [2, 4, 5, 7]],
    &[[0, 4, 1, 7], [0, 2, 3, 7], [1, 6, 2, 7], [1, 5, 6, 7], [1, 4, 5, 7], [0, 1, 2, 7]],
    &[],
    &[[0, 4, 1, 7], [0, 2, 3, 7], [1, 5, 2, 7], [2, 5, 6, 7], [1, 4, 5, 7], [0, 1, 2, 7]],
    &[[0, 2, 3, 4], [2, 3, 4, 7], [1, 2, 4, 5], [2, 5, 6, 7], [0, 1, 2, 4], [2, 4, 5, 7]],
    &[[0, 4, 5, 7], [0, 1, 3, 7], [1, 2, 3, 6], [1, 5, 6, 7], [0, 5, 1, 7], [1, 6, 3, 7]],
    &[[0, 3, 4, 5], [3, 4, 5, 7], [1, 2, 3, 6], [1, 3, 5, 6], [0, 1, 3, 5], [3, 5, 6, 7]],
    &[[0, 4, 5, 7], [0, 5, 3, 7], [1, 2, 3, 5], [2, 3, 5, 6], [0, 1, 3, 5], [3, 5, 6, 7]],
    &[[0, 3, 4, 5], [3, 4, 5, 7], [1, 2, 3, 5], [2, 3, 5, 6], [0, 1, 3, 5], [3, 5, 6, 7]],
    &[[0, 4, 1, 7], [0, 1, 3, 7], [1, 2, 3, 6], [1, 5, 6, 7], [1, 4, 5, 7], [1, 6, 3, 7]],
    &[[0, 1, 3, 4], [1, 3, 4, 7], [1, 2, 3, 6], [1, 5, 6, 7], [1, 4, 5, 7], [1, 6, 3, 7]],
    &[],
    &[[0, 1, 3, 4], [3, 4, 5, 7], [1, 2, 3, 5], [2, 3, 5, 6], [1, 3, 4, 5], [3, 5, 6, 7]],
    &[[0, 4, 5, 7], [0, 1, 3, 7], [1, 6, 2, 7], [1, 5, 6, 7], [0, 5, 1, 7], [1, 2, 3, 7]],
    &[],
    &[[0, 4, 5, 7], [0, 1, 3, 7], [1, 5, 2, 7], [2, 5, 6, 7], [0, 5, 1, 7], [1, 2, 3, 7]],
    &[[0, 3, 4, 5], [3, 4, 5, 7], [1, 2, 3, 5], [2, 5, 6, 7], [0, 1, 3, 5], [2, 3, 5, 7]],
    &[[0, 4, 1, 7], [0, 1, 3, 7], [1, 6, 2, 7], [1, 5, 6, 7], [1, 4, 5, 7], [1, 2, 3, 7]],
    &[[0, 1, 3, 4], [1, 3, 4, 7], [1, 6, 2, 7], [1, 5, 6, 7], [1, 4, 5, 7], [1, 2, 3, 7]],
    &[[0, 4, 1, 7], [0, 1, 3, 7], [1, 5, 2, 7], [2, 5, 6, 7], [1, 4, 5, 7], [1, 2, 3, 7]],
    &[[0, 1, 3, 4], [1, 3, 4, 7], [1, 5, 2, 7], [2, 5, 6, 7], [1, 4, 5, 7], [1, 2, 3, 7]],
];
pub(crate) const WEDGE_SPLIT: [&[[usize; 4]]; 32] = [
    &[],
    &[],
    &[],
    &[],
    &[[0, 2, 1, 3], [1, 4, 3, 5], [1, 3, 2, 5]],
    &[],
    &[],
    &[],
    &[[0, 2, 1, 4], [2, 4, 3, 5], [0, 3, 2, 4]],
    &[],
    &[],
    &[],
    &[[0, 2, 1, 3], [2, 4, 3, 5], [1, 3, 2, 4]],
    &[],
    &[],
    &[],
    &[[0, 2, 1, 5], [0, 4, 3, 5], [0, 1, 4, 5]],
    &[],
    &[],
    &[],
    &[[0, 2, 1, 5], [1, 4, 3, 5], [0, 1, 3, 5]],
    &[],
    &[],
    &[],
    &[[0, 2, 1, 4], [0, 4, 3, 5], [0, 2, 4, 5]],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
];

/// Iterate every cell of a dataset as (cell_id, type, point ids, polyhedron faces).
pub fn for_each_cell(ds: &DataSet, mut f: impl FnMut(usize, CellType, &[u32], Option<&[Vec<u32>]>)) -> Option<UnstructuredGridRef> {
    match ds {
        DataSet::UnstructuredGrid(ug) => {
            walk(&ug.cells, 0, &mut f);
            Some(UnstructuredGridRef::Unstructured)
        }
        DataSet::PolyData(pd) => {
            let mut base = 0;
            for c in [&pd.verts, &pd.lines, &pd.polys, &pd.strips] {
                walk(c, base, &mut f);
                base += c.len();
            }
            Some(UnstructuredGridRef::Poly)
        }
        _ => None,
    }
}

/// Like [`for_each_cell`], for the cells with ids in `range` (polydata cells are numbered
/// verts, lines, polys, strips).
pub fn for_each_cell_in(ds: &DataSet, range: std::ops::Range<usize>, mut f: impl FnMut(usize, CellType, &[u32], Option<&[Vec<u32>]>)) {
    let mut walk_range = |cells: &Cells, base: usize| {
        let (lo, hi) = (range.start.max(base), range.end.min(base + cells.len()));
        for cid in lo..hi {
            let i = cid - base;
            let faces = cells.polyhedron_faces.as_ref().and_then(|pf| pf.get(i)).map(|v| v.as_slice());
            f(cid, cells.types[i], cells.cell(i), faces);
        }
    };
    match ds {
        DataSet::UnstructuredGrid(ug) => walk_range(&ug.cells, 0),
        DataSet::PolyData(pd) => {
            let mut base = 0;
            for c in [&pd.verts, &pd.lines, &pd.polys, &pd.strips] {
                walk_range(c, base);
                base += c.len();
            }
        }
        _ => {}
    }
}

pub enum UnstructuredGridRef {
    Unstructured,
    Poly,
}

fn walk(cells: &Cells, base: usize, f: &mut impl FnMut(usize, CellType, &[u32], Option<&[Vec<u32>]>)) {
    for i in 0..cells.len() {
        let faces = cells.polyhedron_faces.as_ref().and_then(|pf| pf.get(i)).map(|v| v.as_slice());
        f(base + i, cells.types[i], cells.cell(i), faces);
    }
}

/// Make sure we have an explicit-cell dataset to work with (structured grids are converted).
pub fn as_explicit(ds: &DataSet) -> std::borrow::Cow<'_, DataSet> {
    match ds {
        DataSet::UnstructuredGrid(_) | DataSet::PolyData(_) => std::borrow::Cow::Borrowed(ds),
        _ => match ds.to_unstructured() {
            Some(ug) => std::borrow::Cow::Owned(DataSet::UnstructuredGrid(ug)),
            None => std::borrow::Cow::Borrowed(ds),
        },
    }
}

/// Helper to get points + attributes from an explicit dataset.
pub fn parts(ds: &DataSet) -> Option<(&aurafx_data::Points, &aurafx_data::Attributes, &aurafx_data::Attributes, &aurafx_data::Attributes)> {
    match ds {
        DataSet::UnstructuredGrid(UnstructuredGrid { points, point_data, cell_data, field_data, .. }) => Some((points, point_data, cell_data, field_data)),
        DataSet::PolyData(PolyData { points, point_data, cell_data, field_data, .. }) => Some((points, point_data, cell_data, field_data)),
        _ => None,
    }
}
