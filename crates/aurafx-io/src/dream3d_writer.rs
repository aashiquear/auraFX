//! `.dream3d` writer: microstructure volumes, surface and volume meshes and point sets as data
//! containers in the file layout version 7.0 (an `_SIMPL_GEOMETRY` group per container, attribute
//! matrices for cell, vertex, feature and ensemble arrays, the array attributes ObjectType,
//! ComponentDimensions, TupleDimensions and DataArrayVersion), with the pipeline that made the
//! data as JSON text in `/Pipeline/Pipeline`. Multiblocks write one data container per block.

use crate::hdf5w::{AttrValue, Node};
use anyhow::{bail, Result};
use aurafx_data::*;
use std::path::Path;

/// Per-ensemble arrays (one row per phase); the rest of the field data is sorted by length.
const ENSEMBLE: &[&str] = &[
    "CrystalStructures",
    "LatticeConstants",
    "MaterialName",
    "PhaseTypes",
    "ShapeTypes",
    "PhaseName",
    "PhaseNames",
    "NumFeatures",
    "PrecipitateBoundaryFraction",
];

fn object_type(d: &ArrayData) -> &'static str {
    match d {
        ArrayData::I8(_) => "DataArray<int8_t>",
        ArrayData::U8(_) => "DataArray<uint8_t>",
        ArrayData::I16(_) => "DataArray<int16_t>",
        ArrayData::U16(_) => "DataArray<uint16_t>",
        ArrayData::I32(_) => "DataArray<int32_t>",
        ArrayData::U32(_) => "DataArray<uint32_t>",
        ArrayData::I64(_) => "DataArray<int64_t>",
        ArrayData::U64(_) => "DataArray<uint64_t>",
        ArrayData::F16(_) | ArrayData::F32(_) => "DataArray<float>",
        ArrayData::F64(_) => "DataArray<double>",
        ArrayData::String(_) => "StringDataArray",
    }
}

fn u64s(v: &[u64]) -> AttrValue {
    AttrValue::Data(ArrayData::U64(v.to_vec()))
}

fn axes(t: &[u64]) -> String {
    t.iter().zip(["x", "y", "z"]).map(|(n, a)| format!("{a}={n}")).collect::<Vec<_>>().join(",")
}

/// One data array: shape (tuple dims reversed, components).
fn array_node(a: &Array, tuple: &[u64]) -> Node {
    let data = match &a.data {
        ArrayData::F16(v) => ArrayData::F32(v.iter().map(|x| x.to_f32()).collect()),
        d => d.clone(),
    };
    let mut shape: Vec<u64> = tuple.iter().rev().copied().collect();
    if !matches!(data, ArrayData::String(_)) {
        shape.push(a.ncomp as u64);
    }
    let mut n = Node::dataset(data, shape);
    n.attr("ComponentDimensions", u64s(&[a.ncomp as u64]));
    n.attr("DataArrayVersion", AttrValue::Data(ArrayData::I32(vec![2])));
    n.attr("ObjectType", AttrValue::Str(object_type(&a.data).into()));
    n.attr("Tuple Axis Dimensions", AttrValue::Str(axes(tuple)));
    n.attr("TupleDimensions", u64s(tuple));
    n
}

fn matrix(kind: u32, tuple: &[u64], arrays: &[&Array]) -> Node {
    let mut g = Node::group();
    g.attr("AttributeMatrixType", AttrValue::Data(ArrayData::U32(vec![kind])));
    g.attr("TupleDimensions", u64s(tuple));
    for a in arrays {
        g.add(&a.name, array_node(a, tuple));
    }
    g
}

/// Field arrays: the ensemble ones, those with one row per feature, and the rest by length.
fn field_matrices(dc: &mut Node, fd: &Attributes, n_features: Option<usize>, feature_matrix: (&str, u32)) {
    let list: Vec<&Array> = fd.iter().filter(|a| !a.name.starts_with("SideSet ") && !a.name.starts_with("NodeSet ")).collect();
    let n_ens = fd.get("CrystalStructures").map(|a| a.len());
    let mut ens = Vec::new();
    let mut feat = Vec::new();
    let mut rest: std::collections::BTreeMap<usize, Vec<&Array>> = Default::default();
    for a in list {
        if ENSEMBLE.contains(&a.name.as_str()) && Some(a.len()) == n_ens {
            ens.push(a);
        } else if Some(a.len()) == n_features {
            feat.push(a);
        } else {
            rest.entry(a.len()).or_default().push(a);
        }
    }
    if !feat.is_empty() {
        dc.add(feature_matrix.0, matrix(feature_matrix.1, &[n_features.unwrap() as u64], &feat));
    }
    if !ens.is_empty() {
        dc.add("CellEnsembleData", matrix(11, &[n_ens.unwrap() as u64], &ens));
    }
    for (n, arrays) in rest {
        dc.add(&format!("FieldData{n}"), matrix(13, &[n as u64], &arrays));
    }
}

fn geometry(kind: &str, code: u32, unit_dim: u32) -> Node {
    let mut g = Node::group();
    g.attr("GeometryName", AttrValue::Str(kind.into()));
    g.attr("GeometryType", AttrValue::Data(ArrayData::U32(vec![code])));
    g.attr("GeometryTypeName", AttrValue::Str(kind.into()));
    g.attr("SpatialDimensionality", AttrValue::Data(ArrayData::U32(vec![3])));
    g.attr("UnitDimensionality", AttrValue::Data(ArrayData::U32(vec![unit_dim])));
    g
}

fn n_features(cd: &Attributes) -> Option<usize> {
    let f = cd.get("FeatureIds").or_else(|| cd.get("grain_id"))?;
    let m = (0..f.len()).map(|i| f.get(i, 0) as i64).max().unwrap_or(0).max(0);
    Some(m as usize + 1)
}

fn container(ds: &DataSet) -> Result<(String, Node)> {
    let mut dc = Node::group();
    let name = match ds {
        DataSet::ImageData(img) => {
            let d: [u64; 3] = std::array::from_fn(|a| (img.extent[2 * a + 1] - img.extent[2 * a]).max(0) as u64);
            // Image data with point arrays only: store the points as cells of a grid one
            // smaller (the file's images hold cell values).
            let mut geo = geometry("ImageGeometry", 0, 3);
            let origin: [f64; 3] = std::array::from_fn(|a| img.origin[a] + img.extent[2 * a] as f64 * img.spacing[a]);
            geo.add("DIMENSIONS", Node::dataset(ArrayData::I64(d.iter().map(|x| *x as i64).collect()), vec![3]));
            geo.add("ORIGIN", Node::dataset(ArrayData::F32(origin.iter().map(|x| *x as f32).collect()), vec![3]));
            geo.add("SPACING", Node::dataset(ArrayData::F32(img.spacing.iter().map(|x| *x as f32).collect()), vec![3]));
            dc.add("_SIMPL_GEOMETRY", geo);
            let cells: Vec<&Array> = img.cell_data.iter().filter(|a| !matches!(a.data, ArrayData::String(_))).collect();
            if !cells.is_empty() {
                dc.add("CellData", matrix(3, &d, &cells));
            }
            field_matrices(&mut dc, &img.field_data, n_features(&img.cell_data), ("CellFeatureData", 7));
            "ImageDataContainer"
        }
        DataSet::PolyData(_) | DataSet::UnstructuredGrid(_) => {
            let ug = ds.to_unstructured().unwrap();
            let first = ug.cells.iter().next().map(|(t, _)| t);
            let same = ug.cells.iter().all(|(t, _)| Some(t) == first);
            let one = |t: CellType| same && first == Some(t);
            let (kind, code, unit, list, k, name) = if ug.cells.is_empty() || one(CellType::Vertex) {
                ("VertexGeometry", 2, 0, None, 1, "VertexDataContainer")
            } else if one(CellType::Line) {
                ("EdgeGeometry", 3, 1, Some("SharedEdgeList"), 2, "EdgeDataContainer")
            } else if one(CellType::Triangle) {
                ("TriangleGeometry", 4, 2, Some("SharedTriList"), 3, "TriangleDataContainer")
            } else if one(CellType::Quad) {
                ("QuadGeometry", 5, 2, Some("SharedQuadList"), 4, "QuadDataContainer")
            } else if one(CellType::Tetra) {
                ("TetrahedralGeometry", 6, 3, Some("SharedTetList"), 4, "TetrahedralDataContainer")
            } else if one(CellType::Hexahedron) {
                ("HexahedralGeometry", 7, 3, Some("SharedHexList"), 8, "HexahedralDataContainer")
            } else {
                bail!("the file holds meshes of one linear element type (vertices, lines, triangles, quads, tetrahedra or hexahedra); convert the mesh first")
            };
            let mut geo = geometry(kind, code, unit);
            let np = ug.points.len();
            let pts: Vec<f32> = (0..np).flat_map(|i| ug.points.get(i).map(|x| x as f32)).collect();
            let vl = Array::new("SharedVertexList", 3, ArrayData::F32(pts));
            geo.add("SharedVertexList", array_node(&vl, &[np as u64]));
            if let Some(l) = list {
                let conn: Vec<i64> = ug.cells.iter().flat_map(|(_, ids)| ids.iter().map(|x| *x as i64).collect::<Vec<_>>()).collect();
                let a = Array::new(l, k, ArrayData::I64(conn));
                geo.add(l, array_node(&a, &[ug.cells.len() as u64]));
            }
            dc.add("_SIMPL_GEOMETRY", geo);
            let pd: Vec<&Array> = ug.point_data.iter().filter(|a| !matches!(a.data, ArrayData::String(_))).collect();
            if !pd.is_empty() {
                dc.add("VertexData", matrix(0, &[np as u64], &pd));
            }
            let cd: Vec<&Array> = ug.cell_data.iter().filter(|a| !matches!(a.data, ArrayData::String(_))).collect();
            if !cd.is_empty() && list.is_some() {
                let (mname, mtype) = match unit {
                    1 => ("EdgeData", 1),
                    2 => ("FaceData", 2),
                    _ => ("CellData", 3),
                };
                dc.add(mname, matrix(mtype, &[ug.cells.len() as u64], &cd));
            }
            // Surface meshes of grains: features from the face labels.
            let nf = n_features(&ug.cell_data).or_else(|| {
                let f = ug.cell_data.get("FaceLabels")?;
                Some((0..f.len() * f.ncomp).map(|i| f.get(i / f.ncomp, i % f.ncomp) as i64).max().unwrap_or(0).max(0) as usize + 1)
            });
            let fm = if unit == 2 { ("FaceFeatureData", 6) } else { ("CellFeatureData", 7) };
            field_matrices(&mut dc, &ug.field_data, nf, fm);
            name
        }
        _ => bail!("a .dream3d file holds image data (volumes), meshes or points"),
    };
    Ok((name.to_string(), dc))
}

/// Write `ds` as a `.dream3d` file, recording `pipeline` (JSON text) when given.
pub fn write(ds: &DataSet, path: &Path, pipeline: Option<&str>) -> Result<()> {
    let mut root = Node::group();
    root.attr("DREAM3D Version", AttrValue::Str(format!("auraFX {}", env!("CARGO_PKG_VERSION"))));
    root.attr("FileVersion", AttrValue::Str("7.0".into()));
    root.add("DataContainerBundles", Node::group());
    let dcs = root.add("DataContainers", Node::group());
    let blocks: Vec<(String, DataSet)> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|(n, d)| (n, d.clone())).collect(),
        d => vec![(String::new(), d.clone())],
    };
    let mut used = std::collections::HashSet::new();
    for (bname, b) in &blocks {
        let (mut name, node) = container(b)?;
        if !bname.is_empty() {
            name = bname.chars().map(|c| if c == '/' { '_' } else { c }).collect();
        }
        while !used.insert(name.clone()) {
            name.push('_');
        }
        dcs.add(&name, node);
    }
    let mut p = Node::group();
    p.attr("Current Pipeline", AttrValue::Str("Pipeline".into()));
    p.attr("Pipeline Version", AttrValue::Data(ArrayData::I32(vec![2])));
    let json = pipeline.map(str::to_string).unwrap_or_else(|| r#"{"PipelineBuilder":{"Name":"auraFX","Number_Filters":0,"Version":6}}"#.into());
    p.add("Pipeline", Node::dataset(ArrayData::String(vec![json]), vec![]));
    root.add("Pipeline", p);
    crate::hdf5w::write(path, &root)
}
