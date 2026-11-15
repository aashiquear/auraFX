//! Surfaces written for other programs: Wavefront OBJ (vertices, normals, faces, lines and
//! points), binary glTF 2.0 (`.glb`: positions, normals, 8-bit colors, triangles, lines and points)
//! and Houdini's text geometry (`.geo`: points with their arrays as point attributes, polygons).

use anyhow::{bail, Result};
use aurafx_data::*;
use std::fmt::Write as _;
use std::path::Path;

fn polydata(ds: &DataSet) -> Result<PolyData> {
    match ds {
        DataSet::PolyData(p) => Ok(p.clone()),
        _ => bail!("this format holds a surface (polygonal data): Extract Surface first"),
    }
}

/// Wavefront OBJ: `v` (and `vn` from a 3-component `Normals` point array), `f` for polygons,
/// `l` for lines, `p` for vertices.
pub fn write_obj(ds: &DataSet, path: &Path) -> Result<()> {
    let pd = polydata(ds)?;
    let n = pd.points.len();
    let normals = pd.point_data.get("Normals").filter(|a| a.ncomp == 3 && a.len() == n);
    let mut s = String::from("# written by auraFX\n");
    for i in 0..n {
        let p = pd.points.get(i);
        let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
    }
    if let Some(nm) = normals {
        for i in 0..n {
            let _ = writeln!(s, "vn {} {} {}", nm.get(i, 0), nm.get(i, 1), nm.get(i, 2));
        }
    }
    for (_, c) in pd.polys.iter() {
        s += "f";
        for &v in c {
            if normals.is_some() {
                let _ = write!(s, " {}//{}", v + 1, v + 1);
            } else {
                let _ = write!(s, " {}", v + 1);
            }
        }
        s += "\n";
    }
    for (_, c) in pd.lines.iter() {
        s += "l";
        for &v in c {
            let _ = write!(s, " {}", v + 1);
        }
        s += "\n";
    }
    for (_, c) in pd.verts.iter() {
        for &v in c {
            let _ = writeln!(s, "p {}", v + 1);
        }
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Binary glTF 2.0: one mesh with up to three primitives (triangles from polygons as fans,
/// line segments, points), POSITION, NORMAL (a 3-component `Normals` point array) and COLOR_0
/// (an 8-bit point array of 3 or 4 components, or none).
pub fn write_glb(ds: &DataSet, path: &Path) -> Result<()> {
    let pd = polydata(ds)?;
    let n = pd.points.len();
    if n == 0 {
        bail!("no points to write");
    }
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let off = bin.len();
        bin.extend_from_slice(bytes);
        views.push(serde_json::json!({"buffer": 0, "byteOffset": off, "byteLength": bytes.len(), "target": target}));
        views.len() - 1
    };
    // Positions.
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let mut pos = Vec::with_capacity(n * 12);
    for i in 0..n {
        let p = pd.points.get(i);
        for k in 0..3 {
            let v = p[k] as f32;
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
            pos.extend_from_slice(&v.to_le_bytes());
        }
    }
    let v = push_view(&mut bin, &pos, 34962);
    accessors.push(serde_json::json!({"bufferView": v, "componentType": 5126, "count": n, "type": "VEC3", "min": lo, "max": hi}));
    let mut attrs = serde_json::json!({"POSITION": 0});
    if let Some(nm) = pd.point_data.get("Normals").filter(|a| a.ncomp == 3 && a.len() == n) {
        let mut b = Vec::with_capacity(n * 12);
        for i in 0..n {
            let (x, y, z) = (nm.get(i, 0), nm.get(i, 1), nm.get(i, 2));
            let l = (x * x + y * y + z * z).sqrt().max(1e-30);
            for c in [x / l, y / l, z / l] {
                b.extend_from_slice(&(c as f32).to_le_bytes());
            }
        }
        let v = push_view(&mut bin, &b, 34962);
        accessors.push(serde_json::json!({"bufferView": v, "componentType": 5126, "count": n, "type": "VEC3"}));
        attrs["NORMAL"] = serde_json::json!(accessors.len() - 1);
    }
    if let Some(col) = pd.point_data.iter().find(|a| matches!(a.data, ArrayData::U8(_)) && (a.ncomp == 3 || a.ncomp == 4) && a.len() == n) {
        let ArrayData::U8(c) = &col.data else { unreachable!() };
        // Four components keep the 4-byte stride glTF wants for normalized bytes.
        let mut b = Vec::with_capacity(n * 4);
        for i in 0..n {
            for k in 0..4 {
                b.push(if k < col.ncomp { c[i * col.ncomp + k] } else { 255 });
            }
        }
        let v = push_view(&mut bin, &b, 34962);
        accessors.push(serde_json::json!({"bufferView": v, "componentType": 5121, "normalized": true, "count": n, "type": "VEC4"}));
        attrs["COLOR_0"] = serde_json::json!(accessors.len() - 1);
    }
    let mut prims = Vec::new();
    let mut add = |bin: &mut Vec<u8>, idx: Vec<u32>, mode: u32, accessors: &mut Vec<serde_json::Value>, prims: &mut Vec<serde_json::Value>| {
        if idx.is_empty() {
            return;
        }
        let b: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
        let v = push_view(bin, &b, 34963);
        accessors.push(serde_json::json!({"bufferView": v, "componentType": 5125, "count": idx.len(), "type": "SCALAR"}));
        prims.push(serde_json::json!({"attributes": attrs.clone(), "indices": accessors.len() - 1, "mode": mode}));
    };
    let mut tris = Vec::new();
    for (_, c) in pd.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            tris.extend_from_slice(&[c[0], c[k], c[k + 1]]);
        }
    }
    let mut segs = Vec::new();
    for (_, c) in pd.lines.iter() {
        for w in c.windows(2) {
            segs.extend_from_slice(&[w[0], w[1]]);
        }
    }
    let pts: Vec<u32> = pd.verts.iter().flat_map(|(_, c)| c.to_vec()).collect();
    add(&mut bin, tris, 4, &mut accessors, &mut prims);
    add(&mut bin, segs, 1, &mut accessors, &mut prims);
    add(&mut bin, pts, 0, &mut accessors, &mut prims);
    if prims.is_empty() {
        bail!("nothing to draw: no polygons, lines or vertices");
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let doc = serde_json::json!({
        "asset": {"version": "2.0", "generator": "auraFX"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": path.file_stem().and_then(|s| s.to_str()).unwrap_or("mesh")}],
        "meshes": [{"primitives": prims}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
    });
    let mut json = serde_json::to_vec(&doc)?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    std::fs::write(path, out)?;
    Ok(())
}

/// Houdini text geometry (`.geo`, PGEOMETRY V5): the points with every point array of one to four
/// numeric components as a point attribute, and the polygons (closed) and lines (open) as Poly
/// primitives.
pub fn write_geo(ds: &DataSet, path: &Path) -> Result<()> {
    let pd = polydata(ds)?;
    let n = pd.points.len();
    let arrays: Vec<&Array> = pd.point_data.iter().filter(|a| a.len() == n && (1..=4).contains(&a.ncomp) && !matches!(a.data, ArrayData::String(_))).collect();
    let polys: Vec<(bool, Vec<u32>)> = pd.polys.iter().map(|(_, c)| (true, c.to_vec())).chain(pd.lines.iter().map(|(_, c)| (false, c.to_vec()))).collect();
    let mut s = String::from("PGEOMETRY V5\n");
    let _ = writeln!(s, "NPoints {} NPrims {}", n, polys.len());
    let _ = writeln!(s, "NPointGroups 0 NPrimGroups 0");
    let _ = writeln!(s, "NPointAttrib {} NVertexAttrib 0 NPrimAttrib 0 NAttrib 0", arrays.len());
    if !arrays.is_empty() {
        s += "PointAttrib\n";
        for a in &arrays {
            let name: String = a.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
            let _ = writeln!(s, "{} {} float{}", name, a.ncomp, " 0".repeat(a.ncomp));
        }
    }
    for i in 0..n {
        let p = pd.points.get(i);
        let _ = write!(s, "{} {} {} 1", p[0], p[1], p[2]);
        if !arrays.is_empty() {
            s += " (";
            let vals: Vec<String> = arrays.iter().flat_map(|a| (0..a.ncomp).map(move |c| format!("{}", a.get(i, c)))).collect();
            s += &vals.join(" ");
            s += ")";
        }
        s += "\n";
    }
    if !polys.is_empty() {
        let _ = writeln!(s, "Run {} Poly", polys.len());
        for (closed, c) in &polys {
            let _ = write!(s, " {} {}", c.len(), if *closed { "<" } else { ":" });
            for v in c {
                let _ = write!(s, " {v}");
            }
            s += "\n";
        }
    }
    s += "beginExtra\nendExtra\n";
    std::fs::write(path, s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> DataSet {
        let mut pd = PolyData::default();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]] {
            pd.points.push(p);
        }
        pd.polys.push(CellType::Quad, &[0, 1, 2, 3]);
        pd.lines.push(CellType::PolyLine, &[0, 2]);
        pd.point_data.insert(Array::new("Normals", 3, ArrayData::F32(vec![0.0, 0.0, 1.0].repeat(4))));
        pd.point_data.insert(Array::new("Colors", 3, ArrayData::U8(vec![255, 0, 0].repeat(4))));
        pd.point_data.insert(Array::scalars_f64("T", vec![1.0, 2.0, 3.0, 4.0]));
        DataSet::PolyData(pd)
    }

    #[test]
    fn obj_glb_and_geo_round_trip() {
        let dir = std::env::temp_dir().join(format!("aurafx_surface_writers_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sq = square();
        // OBJ and glTF read back by auraFX's readers: the same points and the square's area.
        write_obj(&sq, &dir.join("s.obj")).unwrap();
        write_glb(&sq, &dir.join("s.glb")).unwrap();
        for f in ["s.obj", "s.glb"] {
            let crate::Loaded::Static(d) = crate::read_file(dir.join(f)).unwrap() else { panic!() };
            let pd = match &*d {
                DataSet::PolyData(p) => p.clone(),
                DataSet::MultiBlock(mb) => match mb.leaves().first().map(|l| l.1.clone()) {
                    Some(DataSet::PolyData(p)) => p,
                    _ => panic!("{f}"),
                },
                _ => panic!("{f}"),
            };
            // (the glTF reader gives each primitive its own copy of the positions)
            assert!(pd.points.len() >= 4, "{f}");
            let area: f64 = pd
                .polys
                .iter()
                .map(|(_, c)| {
                    (1..c.len() - 1)
                        .map(|k| {
                            let (a, b, cc) = (pd.points.get(c[0] as usize), pd.points.get(c[k] as usize), pd.points.get(c[k + 1] as usize));
                            0.5 * ((b[0] - a[0]) * (cc[1] - a[1]) - (cc[0] - a[0]) * (b[1] - a[1])).abs()
                        })
                        .sum::<f64>()
                })
                .sum();
            assert!((area - 1.0).abs() < 1e-6, "{f}: {area}");
        }
        write_geo(&sq, &dir.join("s.geo")).unwrap();
        let g = std::fs::read_to_string(dir.join("s.geo")).unwrap();
        assert!(g.contains("NPoints 4 NPrims 2") && g.contains("T 1 float 0") && g.contains(" 4 < 0 1 2 3") && g.contains(" 2 : 0 2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
