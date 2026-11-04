//! Surface mesh readers: PLY (ASCII and binary) and Wavefront OBJ.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;

fn cell_type(n: usize) -> CellType {
    match n {
        1 => CellType::Vertex,
        2 => CellType::Line,
        3 => CellType::Triangle,
        4 => CellType::Quad,
        _ => CellType::Polygon,
    }
}

pub fn read_obj(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut pts: Vec<f64> = Vec::new();
    let mut polys = Cells::new();
    let mut lines = Cells::new();
    let mut normals: Vec<f64> = Vec::new();
    for l in text.lines() {
        let t: Vec<&str> = l.split_whitespace().collect();
        match t.first().copied() {
            Some("v") if t.len() >= 4 => pts.extend(t[1..4].iter().map(|s| s.parse::<f64>().unwrap_or(0.0))),
            Some("vn") if t.len() >= 4 => normals.extend(t[1..4].iter().map(|s| s.parse::<f64>().unwrap_or(0.0))),
            Some("f") | Some("l") => {
                let nv = (pts.len() / 3) as i64;
                let ids: Vec<u32> = t[1..]
                    .iter()
                    .filter_map(|s| s.split('/').next()?.parse::<i64>().ok())
                    .map(|i| if i < 0 { (nv + i) as u32 } else { (i - 1) as u32 })
                    .collect();
                if t[0] == "f" && ids.len() >= 3 {
                    polys.push(cell_type(ids.len()), &ids);
                } else if ids.len() >= 2 {
                    lines.push(if ids.len() == 2 { CellType::Line } else { CellType::PolyLine }, &ids);
                }
            }
            _ => {}
        }
    }
    if pts.is_empty() {
        bail!("no vertices in {}", path.display());
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, lines, ..Default::default() };
    if normals.len() == pd.points.len() * 3 {
        pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(normals)));
    }
    Ok(DataSet::PolyData(pd))
}

#[derive(Debug)]
struct PlyProp {
    name: String,
    ty: String,
    list: Option<(String, String)>,
}

#[derive(Debug)]
struct PlyElem {
    name: String,
    count: usize,
    props: Vec<PlyProp>,
}

fn ply_size(ty: &str) -> usize {
    match ty {
        "char" | "uchar" | "int8" | "uint8" => 1,
        "short" | "ushort" | "int16" | "uint16" => 2,
        "int" | "uint" | "float" | "int32" | "uint32" | "float32" => 4,
        _ => 8,
    }
}

fn ply_read(b: &[u8], ty: &str, le: bool) -> f64 {
    macro_rules! r {
        ($t:ty) => {{
            let a: [u8; std::mem::size_of::<$t>()] = b[..std::mem::size_of::<$t>()].try_into().unwrap();
            (if le { <$t>::from_le_bytes(a) } else { <$t>::from_be_bytes(a) }) as f64
        }};
    }
    match ty {
        "char" | "int8" => b[0] as i8 as f64,
        "uchar" | "uint8" => b[0] as f64,
        "short" | "int16" => r!(i16),
        "ushort" | "uint16" => r!(u16),
        "int" | "int32" => r!(i32),
        "uint" | "uint32" => r!(u32),
        "float" | "float32" => r!(f32),
        _ => r!(f64),
    }
}

pub fn read_ply(path: &Path) -> Result<DataSet> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let end = memchr::memmem::find(&bytes, b"end_header").ok_or_else(|| anyhow!("PLY header has no end_header"))?;
    let header = String::from_utf8_lossy(&bytes[..end]).to_string();
    let mut body = end + "end_header".len();
    while body < bytes.len() && (bytes[body] == b'\r' || bytes[body] == b'\n') {
        body += 1;
        if bytes[body - 1] == b'\n' {
            break;
        }
    }
    let mut format = "ascii".to_string();
    let mut elems: Vec<PlyElem> = Vec::new();
    for l in header.lines() {
        let t: Vec<&str> = l.split_whitespace().collect();
        match t.first().copied() {
            Some("format") => format = t.get(1).unwrap_or(&"ascii").to_string(),
            Some("element") => elems.push(PlyElem { name: t[1].to_string(), count: t[2].parse()?, props: vec![] }),
            Some("property") => {
                let e = elems.last_mut().ok_or_else(|| anyhow!("property before element"))?;
                if t[1] == "list" {
                    e.props.push(PlyProp { name: t[4].to_string(), ty: String::new(), list: Some((t[2].to_string(), t[3].to_string())) });
                } else {
                    e.props.push(PlyProp { name: t[2].to_string(), ty: t[1].to_string(), list: None });
                }
            }
            _ => {}
        }
    }
    let le = format != "binary_big_endian";
    let ascii = format == "ascii";
    let mut tokens =
        if ascii { Some(String::from_utf8_lossy(&bytes[body..]).split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()) } else { None };
    let mut pos = body;
    let mut next = |ty: &str| -> Result<f64> {
        if let Some(t) = tokens.as_mut() {
            return t.next().ok_or_else(|| anyhow!("truncated PLY"))?.parse::<f64>().map_err(|e| anyhow!("{e}"));
        }
        let sz = ply_size(ty);
        if pos + sz > bytes.len() {
            bail!("truncated PLY");
        }
        let v = ply_read(&bytes[pos..], ty, le);
        pos += sz;
        Ok(v)
    };
    let mut pts: Vec<f64> = Vec::new();
    let mut extra: Vec<(String, Vec<f64>)> = Vec::new();
    let mut polys = Cells::new();
    for e in &elems {
        let is_vertex = e.name == "vertex";
        let xi = e.props.iter().position(|p| p.name == "x");
        if is_vertex {
            extra = e
                .props
                .iter()
                .filter(|p| !["x", "y", "z"].contains(&p.name.as_str()) && p.list.is_none())
                .map(|p| (p.name.clone(), Vec::with_capacity(e.count)))
                .collect();
        }
        for _ in 0..e.count {
            let mut row: Vec<f64> = Vec::with_capacity(e.props.len());
            let mut list: Vec<u32> = Vec::new();
            for p in &e.props {
                match &p.list {
                    Some((cty, ity)) => {
                        let n = next(cty)? as usize;
                        list = (0..n).map(|_| next(ity).map(|v| v as u32)).collect::<Result<_>>()?;
                        row.push(f64::NAN);
                    }
                    None => row.push(next(&p.ty)?),
                }
            }
            if is_vertex {
                let xi = xi.ok_or_else(|| anyhow!("vertex has no x"))?;
                pts.extend_from_slice(&[row[xi], row[xi + 1], row[xi + 2]]);
                let mut k = 0;
                for (j, p) in e.props.iter().enumerate() {
                    if !["x", "y", "z"].contains(&p.name.as_str()) && p.list.is_none() {
                        extra[k].1.push(row[j]);
                        k += 1;
                    }
                }
            } else if e.name == "face" && list.len() >= 3 {
                polys.push(cell_type(list.len()), &list);
            }
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    let (nx, ny, nz) = (extra.iter().position(|e| e.0 == "nx"), extra.iter().position(|e| e.0 == "ny"), extra.iter().position(|e| e.0 == "nz"));
    if let (Some(a), Some(b), Some(c)) = (nx, ny, nz) {
        let n: Vec<f64> = (0..pd.points.len()).flat_map(|i| [extra[a].1[i], extra[b].1[i], extra[c].1[i]]).collect();
        pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(n)));
    }
    let (r, g, bl) = (extra.iter().position(|e| e.0 == "red"), extra.iter().position(|e| e.0 == "green"), extra.iter().position(|e| e.0 == "blue"));
    if let (Some(a), Some(b), Some(c)) = (r, g, bl) {
        let rgb: Vec<u8> = (0..pd.points.len()).flat_map(|i| [extra[a].1[i] as u8, extra[b].1[i] as u8, extra[c].1[i] as u8]).collect();
        pd.point_data.insert(Array::new("RGB", 3, ArrayData::U8(rgb)));
    }
    for (name, v) in extra {
        if !["nx", "ny", "nz", "red", "green", "blue", "alpha"].contains(&name.as_str()) {
            pd.point_data.insert(Array::scalars_f64(name, v));
        }
    }
    Ok(DataSet::PolyData(pd))
}
