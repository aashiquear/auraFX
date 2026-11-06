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

/// The attribute named `name` (the shape when empty).
fn attribute_index(g: &CellGrid, name: &str) -> Result<usize> {
    if name.is_empty() {
        return g.attributes.iter().position(|a| a.shape).ok_or_else(|| anyhow!("the cell grid has no shape"));
    }
    g.attributes.iter().position(|a| a.name == name).ok_or_else(|| anyhow!("the cell grid has no attribute {name:?}"))
}

/// Replace array `r` of the grid by `f` applied to it (each array changed once).
fn map_array(g: &mut CellGrid, r: &(String, String), f: &dyn Fn(&Array) -> Result<Array>) -> Result<()> {
    let list = g.groups.get_mut(&r.0).ok_or_else(|| anyhow!("no array group {}", r.0))?;
    let i = list.iter().position(|a| a.name == r.1).ok_or_else(|| anyhow!("no array {} in {}", r.1, r.0))?;
    list[i] = f(&list[i])?;
    Ok(())
}

/// Cell Grid Transform (vtkCellGridTransform): the 3-D values of an HGRAD or constant attribute
/// (the shape by default) moved by ParaView's transform (scale, rotate Y X Z, translate).
pub fn transform(ds: &DataSet, attribute: &str, translate: [f64; 3], rotate: [f64; 3], scale: [f64; 3]) -> Result<DataSet> {
    let mut g = grid_of(ds)?.clone();
    let att = g.attributes[attribute_index(&g, attribute)?].clone();
    let f = crate::transform::affine(translate, rotate, scale);
    let mut done: Vec<(String, String)> = Vec::new();
    for info in att.info.values() {
        if done.contains(&info.values) {
            continue;
        }
        if !matches!(info.function_space.as_str(), "HGRAD" | "constant") {
            bail!("only HGRAD and constant attributes can be transformed (not {})", info.function_space);
        }
        map_array(&mut g, &info.values, &|a| {
            if a.ncomp % 3 != 0 || (info.sharing.is_some() && a.ncomp != 3) {
                bail!("{}: values to transform must be 3-d vectors", a.name);
            }
            let mut v = a.data.to_f64_vec();
            for p in v.chunks_exact_mut(3) {
                p.copy_from_slice(&f([p[0], p[1], p[2]]));
            }
            Ok(Array::new(a.name.clone(), a.ncomp, ArrayData::from_f64(a.scalar_type(), &v)))
        })?;
        done.push(info.values.clone());
    }
    g.invalidate();
    Ok(DataSet::CellGrid(g))
}

/// Cell Grid Warp (vtkCellGridWarp): the shape's shared points moved by `scale` times a shared
/// HGRAD attribute of the same basis, order and number of components.
pub fn warp(ds: &DataSet, attribute: &str, scale: f64) -> Result<DataSet> {
    let mut g = grid_of(ds)?.clone();
    let shape = g.shape_attribute().ok_or_else(|| anyhow!("the cell grid has no shape"))?.clone();
    let def = g.attributes[attribute_index(&g, attribute)?].clone();
    if def.shape {
        bail!("choose an attribute to warp by (not the shape)");
    }
    if def.components != shape.components {
        bail!("{:?} has {} components; the shape has {}", def.name, def.components, shape.components);
    }
    let mut done: Vec<(String, String)> = Vec::new();
    for (t, si) in &shape.info {
        let di = def.info.get(t).ok_or_else(|| anyhow!("{:?} is not defined on {t}", def.name))?;
        if si.sharing.is_none() || di.sharing.is_none() {
            bail!("the shape and {:?} must both be shared at points", def.name);
        }
        if si.function_space != "HGRAD" || di.function_space != "HGRAD" || si.basis != di.basis || si.order != di.order {
            bail!("the shape and {:?} must both be HGRAD with the same basis and order", def.name);
        }
        if done.contains(&si.values) {
            continue;
        }
        let d = g.array(&di.values).ok_or_else(|| anyhow!("missing values of {:?}", def.name))?.clone();
        map_array(&mut g, &si.values, &|a| {
            let mut v = a.data.to_f64_vec();
            for (i, x) in v.iter_mut().enumerate() {
                *x += scale * d.get(i / a.ncomp, (i % a.ncomp).min(d.ncomp.saturating_sub(1)));
            }
            Ok(Array::new(a.name.clone(), a.ncomp, ArrayData::from_f64(a.scalar_type(), &v)))
        })?;
        done.push(si.values.clone());
    }
    g.invalidate();
    Ok(DataSet::CellGrid(g))
}

/// Convert To Cell Grid (vtkUnstructuredGridToCellGrid): linear cells of the eight DG shapes as
/// a cell grid; the points in a shared "coordinates" group, point data as shared HGRAD C1
/// attributes, cell data as constant attributes (a name already used by point data gets
/// " (cell)"). Other cell types are skipped.
pub fn convert(ds: &DataSet) -> Result<DataSet> {
    let u = crate::simplex::as_explicit(ds).into_owned();
    let (points, cells, pd, cd) = match &u {
        DataSet::UnstructuredGrid(u) => (&u.points, vec![&u.cells], &u.point_data, &u.cell_data),
        DataSet::PolyData(p) => (&p.points, vec![&p.verts, &p.lines, &p.polys], &p.point_data, &p.cell_data),
        other => bail!("Convert To Cell Grid needs a mesh, not {}", other.type_name()),
    };
    let shape_of = |t: CellType| -> Option<Shape> {
        Some(match t {
            CellType::Vertex => Shape::Vertex,
            CellType::Line => Shape::Edge,
            CellType::Triangle => Shape::Triangle,
            CellType::Quad => Shape::Quadrilateral,
            CellType::Tetra => Shape::Tetrahedron,
            CellType::Hexahedron => Shape::Hexahedron,
            CellType::Wedge => Shape::Wedge,
            CellType::Pyramid => Shape::Pyramid,
            _ => return None,
        })
    };
    // Cells by DG type: (connectivity, original cell index).
    let mut by_type: indexmap::IndexMap<String, (Shape, Vec<i64>, Vec<usize>)> = indexmap::IndexMap::new();
    let mut idx = 0usize;
    for c in &cells {
        for i in 0..c.len() {
            if let Some(s) = shape_of(c.cell_type(i)) {
                let e = by_type.entry(s.type_name().to_string()).or_insert((s, Vec::new(), Vec::new()));
                e.1.extend(c.cell(i).iter().map(|&p| p as i64));
                e.2.push(idx);
            }
            idx += 1;
        }
    }
    if by_type.is_empty() {
        bail!("no cells of the cell grid shapes (vertices, lines, triangles, quads, tetrahedra, hexahedra, wedges, pyramids)");
    }
    let mut g = CellGrid::default();
    let n = points.len();
    let coords: Vec<f64> = (0..n).flat_map(|i| points.get(i)).collect();
    let mut shared = vec![Array::new("Points", 3, ArrayData::F64(coords))];
    let pnames: Vec<String> = pd.iter().filter(|a| a.len() == n && !matches!(a.data, ArrayData::String(_))).map(|a| a.name.clone()).collect();
    for name in &pnames {
        shared.push(pd.get(name).unwrap().clone());
    }
    g.groups.insert("coordinates".into(), shared);
    let shared_info = |_t: &str, values: (&str, &str), conn: &(String, String)| DgAttrInfo {
        function_space: "HGRAD".into(),
        basis: "C".into(),
        order: 1,
        sharing: Some("CG".into()),
        values: (values.0.into(), values.1.into()),
        connectivity: Some(conn.clone()),
    };
    let mut shape = DgAttribute { name: "shape".into(), components: 3, shape: true, info: Default::default() };
    let mut point_atts: Vec<DgAttribute> = pnames.iter().map(|nm| DgAttribute { name: nm.clone(), components: pd.get(nm).unwrap().ncomp, shape: false, info: Default::default() }).collect();
    let cnames: Vec<String> = cd.iter().filter(|a| !matches!(a.data, ArrayData::String(_))).map(|a| a.name.clone()).collect();
    let mut cell_atts: Vec<DgAttribute> = cnames
        .iter()
        .map(|nm| {
            let out = if pnames.contains(nm) { format!("{nm} (cell)") } else { nm.clone() };
            DgAttribute { name: out, components: cd.get(nm).unwrap().ncomp, shape: false, info: Default::default() }
        })
        .collect();
    for (t, (s, conn, orig)) in &by_type {
        let nn = s.corners().len();
        let conn_ref = (t.clone(), "conn".to_string());
        let mut arrays = vec![Array::new("conn", nn, ArrayData::I64(conn.clone()))];
        shape.info.insert(t.clone(), shared_info(t, ("coordinates", "Points"), &conn_ref));
        for (k, nm) in pnames.iter().enumerate() {
            point_atts[k].info.insert(t.clone(), shared_info(t, ("coordinates", nm), &conn_ref));
        }
        for (k, nm) in cnames.iter().enumerate() {
            let a = cd.get(nm).unwrap();
            let vals = a.gather(orig);
            let out = cell_atts[k].name.clone();
            arrays.push(Array::new(out.clone(), vals.ncomp, vals.data));
            cell_atts[k].info.insert(t.clone(), DgAttrInfo { function_space: "constant".into(), basis: "C".into(), order: 0, sharing: None, values: (t.clone(), out), connectivity: None });
        }
        g.groups.insert(t.clone(), arrays);
        g.cell_types.insert(t.clone(), DgCells { shape: *s, connectivity: conn_ref });
    }
    g.attributes.push(shape);
    g.attributes.extend(point_atts);
    g.attributes.extend(cell_atts);
    g.field_data = ds.field_data().clone();
    Ok(DataSet::CellGrid(g))
}
