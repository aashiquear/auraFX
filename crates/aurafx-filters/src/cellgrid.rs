//! Cell grid filters and ParaView's Cell Grid source.

use anyhow::{anyhow, bail, Result};
use aurafx_data::cellgrid::{CellGrid, DgAttrInfo, DgAttribute, DgCells, Shape};
use aurafx_data::*;

fn grid_of(ds: &DataSet) -> Result<&CellGrid> {
    match ds {
        DataSet::CellGrid(g) => Ok(g),
        other => Err(anyhow!("this filter needs a cell grid, not {}", other.type_name())),
    }
}

pub fn to_unstructured(ds: &DataSet) -> Result<UnstructuredGrid> {
    Ok(grid_of(ds)?.to_unstructured())
}

pub fn centers(ds: &DataSet) -> Result<PolyData> {
    Ok(grid_of(ds)?.centers())
}

/// Cell Grid Elevation: a new HGRAD attribute laid out like the shape attribute, with the
/// planar distance along `axis` from `origin` (or the distance to `origin`) at every node.
pub fn elevation(ds: &DataSet, name: &str, origin: [f64; 3], axis: [f64; 3], spherical: bool) -> Result<DataSet> {
    let mut g = grid_of(ds)?.clone();
    let shape = g.shape_attribute().ok_or_else(|| anyhow!("the cell grid has no shape"))?.clone();
    let dist = |x: [f64; 3]| -> f64 {
        let d = [x[0] - origin[0], x[1] - origin[1], x[2] - origin[2]];
        if spherical {
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        } else {
            d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2]
        }
    };
    let mut att = DgAttribute { name: name.to_string(), components: 1, shape: false, info: Default::default() };
    for (tname, info) in &shape.info {
        let vals = g.array(&info.values).ok_or_else(|| anyhow!("missing shape values"))?.clone();
        let (group, vname) = match &info.sharing {
            Some(_) => (info.values.0.clone(), name.to_string()),
            None => (tname.clone(), name.to_string()),
        };
        let e: Vec<f64> = match &info.sharing {
            Some(_) => (0..vals.len()).map(|i| dist([vals.get(i, 0), vals.get(i, 1), vals.get(i, 2)])).collect(),
            None => {
                let nf = vals.ncomp / 3;
                (0..vals.len()).flat_map(|c| (0..nf).map(move |k| (c, k))).map(|(c, k)| dist([vals.get(c, 3 * k), vals.get(c, 3 * k + 1), vals.get(c, 3 * k + 2)])).collect()
            }
        };
        let nc = if info.sharing.is_some() { 1 } else { vals.ncomp / 3 };
        let list = g.groups.entry(group.clone()).or_default();
        list.retain(|a| a.name != vname);
        list.push(Array::new(vname.clone(), nc, ArrayData::F64(e)));
        att.info.insert(tname.clone(), DgAttrInfo { values: (group, vname), ..info.clone() });
    }
    g.attributes.retain(|a| a.name != name);
    g.attributes.push(att);
    g.invalidate();
    Ok(DataSet::CellGrid(g))
}

/// ParaView's Cell Grid Source: one cell of a shape (corners at its parametric coordinates)
/// with test attributes: `hgrad` (1, 0, 0 at the first point), and on shapes that have them
/// `hcurl` and `hdiv` (the first coefficient 1); a vertex gets a `constant` attribute.
pub fn source(cell_type: &str) -> Result<DataSet> {
    let i = CELL_TYPES.iter().position(|t| t.eq_ignore_ascii_case(cell_type)).ok_or_else(|| anyhow!("unknown cell type {cell_type} (one of {})", CELL_TYPES.join(", ")))?;
    let type_name = TYPE_NAMES[i];
    let shape = Shape::from_type(type_name).expect("known type");
    let corners = shape.corners();
    let n = corners.len();
    let mut g = CellGrid::default();
    let coords: Vec<f64> = corners.iter().flatten().copied().collect();
    let mut first = vec![0.0; 3 * n];
    first[0] = 1.0;
    let (shape_space, field) = if shape == Shape::Vertex { ("constant", "constant") } else { ("HGRAD", "hgrad") };
    g.groups.insert("points".into(), vec![Array::new("coords", 3, ArrayData::F64(coords)), Array::new(field, 3, ArrayData::F64(first))]);
    let mut cell_arrays = vec![Array::new("connectivity", n, ArrayData::I64((0..n as i64).collect()))];
    let tn = type_name.to_string();
    let conn = (tn.clone(), "connectivity".to_string());
    let shared = |vals: (&str, &str)| DgAttrInfo {
        function_space: shape_space.into(),
        basis: "C".into(),
        order: if shape == Shape::Vertex { 0 } else { 1 },
        sharing: Some("points".into()),
        values: (vals.0.into(), vals.1.into()),
        connectivity: Some(conn.clone()),
    };
    let mut atts = vec![
        DgAttribute { name: "shape".into(), components: 3, shape: true, info: [(tn.clone(), shared(("points", "coords")))].into_iter().collect() },
        DgAttribute { name: field.into(), components: 3, shape: false, info: [(tn.clone(), shared(("points", field)))].into_iter().collect() },
    ];
    let edges_faces = match shape {
        Shape::Hexahedron => Some((12, 6)),
        Shape::Tetrahedron => Some((6, 4)),
        Shape::Wedge => Some((9, 5)),
        Shape::Quadrilateral => Some((4, 4)),
        Shape::Triangle => Some((3, 1)),
        _ => None,
    };
    if let Some((ne, nf)) = edges_faces {
        for (name, space, count) in [("hcurl", "HCURL", ne), ("hdiv", "HDIV", nf)] {
            let mut v = vec![0.0; count];
            v[0] = 1.0;
            cell_arrays.push(Array::new(name, count, ArrayData::F64(v)));
            let info = DgAttrInfo { function_space: space.into(), basis: "I".into(), order: 1, sharing: None, values: (tn.clone(), name.into()), connectivity: None };
            atts.push(DgAttribute { name: name.into(), components: 3, shape: false, info: [(tn.clone(), info)].into_iter().collect() });
        }
    }
    g.groups.insert(tn.clone(), cell_arrays);
    g.cell_types.insert(tn, DgCells { shape, connectivity: conn });
    g.attributes = atts;
    if g.n_cells() != 1 {
        bail!("internal: source cell");
    }
    Ok(DataSet::CellGrid(g))
}

pub const CELL_TYPES: [&str; 8] = ["Hexahedron", "Tetrahedron", "Pyramid", "Wedge", "Quadrilateral", "Triangle", "Edge", "Vertex"];
const TYPE_NAMES: [&str; 8] = ["vtkDGHex", "vtkDGTet", "vtkDGPyr", "vtkDGWdg", "vtkDGQuad", "vtkDGTri", "vtkDGEdge", "vtkDGVert"];
