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
        Tetra | Hexahedron | Voxel | Wedge | Pyramid => {
            for t in ty.tets() {
                f(Simplex::Tet([pts[t[0]], pts[t[1]], pts[t[2]], pts[t[3]]]));
            }
        }
        QuadraticTetra | LagrangeTetrahedron | BezierTetrahedron => f(Simplex::Tet([pts[0], pts[1], pts[2], pts[3]])),
        QuadraticHexahedron | TriquadraticHexahedron | BiquadraticQuadraticHexahedron | LagrangeHexahedron | BezierHexahedron => {
            for t in Hexahedron.tets() {
                f(Simplex::Tet([pts[t[0]], pts[t[1]], pts[t[2]], pts[t[3]]]));
            }
        }
        QuadraticWedge | QuadraticLinearWedge | BiquadraticQuadraticWedge | LagrangeWedge | BezierWedge => {
            for t in Wedge.tets() {
                f(Simplex::Tet([pts[t[0]], pts[t[1]], pts[t[2]], pts[t[3]]]));
            }
        }
        QuadraticPyramid | LagrangePyramid | BezierPyramid => {
            for t in Pyramid.tets() {
                f(Simplex::Tet([pts[t[0]], pts[t[1]], pts[t[2]], pts[t[3]]]));
            }
        }
        PentagonalPrism | HexagonalPrism => {
            // Prism with n-gon base: fan from base vertex 0 into wedges.
            let n = pts.len() / 2;
            for k in 1..n - 1 {
                let w = [pts[0], pts[k], pts[k + 1], pts[n], pts[n + k], pts[n + k + 1]];
                for t in Wedge.tets() {
                    f(Simplex::Tet([w[t[0]], w[t[1]], w[t[2]], w[t[3]]]));
                }
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
