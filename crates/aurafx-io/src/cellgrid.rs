//! VTK cell grid files (`.dg`, JSON): array groups, DG cell types and cell attributes.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::cellgrid::{ArrayRef, CellGrid, DgAttrInfo, DgAttribute, DgCells, Shape};
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
        g.cell_types.insert(ty.to_string(), DgCells { shape, connectivity: conn });
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
