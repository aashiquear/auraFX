//! VTK cell grid files (`.dg`, JSON): array groups, DG cell types and cell attributes.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::cellgrid::{ArrayRef, CellGrid, DgAttrInfo, DgAttribute, DgCells, DgSideSpec, Shape};
use aurafx_data::*;
use serde_json::Value;
use std::path::Path;

fn array_ref(v: &Value) -> Option<ArrayRef> {
    let a = v.as_array()?;
    Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string()))
}

pub fn read(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let j: Value = serde_json::from_str(&text).with_context(|| format!("{}: not valid JSON", path.display()))?;
    if j.get("data-type").and_then(|v| v.as_str()) != Some("cell-grid") {
        bail!("{}: not a cell grid (data-type is not \"cell-grid\")", path.display());
    }
    if j.get("format-version").and_then(|v| v.as_u64()).is_some_and(|v| v > 1) {
        bail!("{}: cell grid format version newer than 1", path.display());
    }
    let mut g = CellGrid::default();
    for (group, arrs) in j.get("arrays").and_then(|a| a.as_object()).ok_or_else(|| anyhow!("no arrays section"))? {
        let mut list = Vec::new();
        for a in arrs.as_array().into_iter().flatten() {
            let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let nc = a.get("components").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
            let ty = a.get("type").and_then(|v| v.as_str()).unwrap_or("double");
            let vals: Vec<f64> = a.get("data").and_then(|d| d.as_array()).map(|d| d.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
            let data = if ty.contains("int") || ty.contains("char") || ty.contains("long") || ty.contains("short") {
                ArrayData::I64(vals.iter().map(|&v| v as i64).collect())
            } else if ty == "float" {
                ArrayData::F32(vals.iter().map(|&v| v as f32).collect())
            } else {
                ArrayData::F64(vals)
            };
            list.push(Array::new(name, nc, data));
        }
        g.groups.insert(group.clone(), list);
    }
    for ct in j.get("cell-types").and_then(|c| c.as_array()).into_iter().flatten() {
        let ty = ct.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let Some(shape) = Shape::from_type(ty) else { continue };
        let conn = ct.pointer("/cell-spec/connectivity").and_then(array_ref).unwrap_or_else(|| (ty.to_string(), "connectivity".into()));
        let mut cells = DgCells::new(shape, conn);
        let spec = ct.get("cell-spec");
        let int = |v: Option<&Value>, k: &str, d: i64| v.and_then(|v| v.get(k)).and_then(|x| x.as_i64()).unwrap_or(d);
        let flag = |v: Option<&Value>, k: &str| v.and_then(|v| v.get(k)).and_then(|x| x.as_bool()).unwrap_or(false);
        cells.blanked = flag(spec, "blanked");
        cells.offset = int(spec, "offset", 0);
        // Side lists (as Compute Sides writes them): (cell, side) pairs of one side shape.
        for sp in ct.get("side-specs").and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(conn) = sp.get("connectivity").and_then(array_ref) else { continue };
            let Some(sh) = sp.get("shape").and_then(|v| v.as_str()).and_then(Shape::from_name) else { continue };
            let side_type = int(Some(sp), "side-type", shape.side_type_for(sh).map(|k| k as i64).unwrap_or(-1));
            cells.sides.push(DgSideSpec {
                connectivity: conn,
                shape: sh,
                side_type,
                offset: int(Some(sp), "offset", 0),
                blanked: flag(Some(sp), "blanked"),
                selection_type: int(Some(sp), "selection-type", -1),
            });
        }
        g.cell_types.insert(ty.to_string(), cells);
    }
    for a in j.get("attributes").and_then(|c| c.as_array()).into_iter().flatten() {
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut att = DgAttribute {
            name: name.clone(),
            components: a.get("components").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize,
            shape: a.get("shape").and_then(|v| v.as_bool()).unwrap_or(false),
            info: Default::default(),
        };
        for (tname, ci) in a.get("cell-info").and_then(|c| c.as_object()).into_iter().flatten() {
            let s = |k: &str| ci.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let sharing = s("dof-sharing");
            let arrays = ci.get("arrays").and_then(|v| v.as_object());
            let role = |r: &str| arrays.and_then(|m| m.get(r)).and_then(array_ref);
            // Files written without role names have one array under "": the connectivity for
            // values shared at points (named after the attribute in that group), else the
            // per-cell coefficients.
            let unnamed = role("");
            let (values, connectivity) = match (role("values"), role("connectivity"), unnamed, &sharing) {
                (Some(v), c, _, _) => (v, c),
                (None, c, Some(u), Some(grp)) => {
                    let grp = if g.groups.get(grp.as_str()).is_some_and(|l| l.iter().any(|x| x.name == name)) { grp.clone() } else { "points".into() };
                    let vname = if att.shape { g.groups.get(&grp).and_then(|l| l.first()).map(|x| x.name.clone()).unwrap_or_default() } else { name.clone() };
                    (( grp, vname), c.or(Some(u)))
                }
                (None, c, Some(u), None) => (u, c),
                _ => continue,
            };
            att.info.insert(
                tname.clone(),
                DgAttrInfo {
                    function_space: s("function-space").unwrap_or_else(|| "HGRAD".into()),
                    basis: s("basis").unwrap_or_else(|| "C".into()),
                    order: ci.get("order").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                    sharing,
                    values,
                    connectivity,
                },
            );
        }
        g.attributes.push(att);
    }
    if g.shape_attribute().is_none() {
        bail!("{}: the cell grid has no shape attribute", path.display());
    }
    Ok(DataSet::CellGrid(g))
}

fn type_name(d: &ArrayData) -> &'static str {
    match d {
        ArrayData::F64(_) => "double",
        ArrayData::F32(_) | ArrayData::F16(_) => "float",
        ArrayData::I64(_) => "vtktypeint64",
        ArrayData::U64(_) => "vtktypeuint64",
        ArrayData::I32(_) => "int",
        ArrayData::U32(_) => "unsigned int",
        ArrayData::I16(_) => "short",
        ArrayData::U16(_) => "unsigned short",
        ArrayData::I8(_) => "signed char",
        ArrayData::U8(_) => "unsigned char",
        ArrayData::String(_) => "string",
    }
}

fn space_of(n: usize) -> String {
    const SUP: [&str; 10] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];
    if n == 1 {
        "ℝ".into()
    } else {
        format!("ℝ{}", n.to_string().chars().map(|c| SUP[c.to_digit(10).unwrap() as usize]).collect::<String>())
    }
}

fn shape_name(s: Shape) -> &'static str {
    match s {
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

/// Write a cell grid as a VTK `.dg` file (the JSON layout vtkCellGridWriter writes).
pub fn write(g: &CellGrid, path: &Path) -> Result<()> {
    use serde_json::{json, Map};
    let conn_refs: Vec<&ArrayRef> = g.cell_types.values().flat_map(|c| std::iter::once(&c.connectivity).chain(c.sides.iter().map(|s| &s.connectivity))).collect();
    let mut arrays = Map::new();
    for (group, list) in &g.groups {
        let items: Vec<Value> = list
            .iter()
            .map(|a| {
                let data: Vec<Value> = match &a.data {
                    ArrayData::String(s) => s.iter().map(|x| json!(x)).collect(),
                    d if d.scalar_type().is_float() => d.to_f64_vec().into_iter().map(|v| if v.is_finite() { json!(v) } else { Value::Null }).collect(),
                    d => d.to_f64_vec().into_iter().map(|v| json!(v as i64)).collect(),
                };
                let mut o = json!({"name": a.name, "type": type_name(&a.data), "components": a.ncomp, "tuples": a.len(), "data": data});
                if conn_refs.iter().any(|r| r.0 == *group && r.1 == a.name) {
                    o["default_scalars"] = json!(true);
                }
                o
            })
            .collect();
        arrays.insert(group.clone(), Value::Array(items));
    }
    let attributes: Vec<Value> = g
        .attributes
        .iter()
        .map(|a| {
            let mut info = Map::new();
            for (t, i) in &a.info {
                let mut arrs = json!({"values": [i.values.0, i.values.1]});
                let conn = i.connectivity.clone().or_else(|| i.sharing.as_ref().and_then(|_| g.cell_types.get(t).map(|c| c.connectivity.clone())));
                if let Some(c) = conn {
                    arrs["connectivity"] = json!([c.0, c.1]);
                }
                let mut e = json!({"function-space": i.function_space, "basis": i.basis, "order": i.order, "arrays": arrs});
                if let Some(s) = &i.sharing {
                    e["dof-sharing"] = json!(s);
                }
                info.insert(t.clone(), e);
            }
            let mut o = json!({"name": a.name, "components": a.components, "space": space_of(a.components), "cell-info": info});
            if a.shape {
                o["shape"] = json!(true);
            }
            o
        })
        .collect();
    let cell_types: Vec<Value> = g
        .cell_types
        .iter()
        .map(|(t, c)| {
            let mut spec = json!({"connectivity": [c.connectivity.0, c.connectivity.1], "shape": shape_name(c.shape)});
            if c.offset != 0 {
                spec["offset"] = json!(c.offset);
            }
            if c.blanked {
                spec["blanked"] = json!(true);
            }
            let mut o = json!({"type": t, "cell-spec": spec});
            if !c.sides.is_empty() {
                let sides: Vec<Value> = c
                    .sides
                    .iter()
                    .map(|s| {
                        let mut e = json!({"connectivity": [s.connectivity.0, s.connectivity.1], "shape": shape_name(s.shape)});
                        if s.offset != 0 {
                            e["offset"] = json!(s.offset);
                        }
                        if s.blanked {
                            e["blanked"] = json!(true);
                        }
                        if s.side_type >= 0 {
                            e["side-type"] = json!(s.side_type);
                        }
                        if s.selection_type != -1 {
                            e["selection-type"] = json!(s.selection_type);
                        }
                        e
                    })
                    .collect();
                o["side-specs"] = Value::Array(sides);
            }
            o
        })
        .collect();
    let doc = json!({
        "data-type": "cell-grid",
        "format-version": 1,
        "schema-name": "dg leaf",
        "schema-version": 1,
        "content-version": 1,
        "arrays": arrays,
        "attributes": attributes,
        "cell-types": cell_types,
    });
    std::fs::write(path, serde_json::to_string(&doc)?).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
