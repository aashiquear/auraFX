//! Surfaces from other programs: OFF (Object File Format, with colors), glTF 2.0 (`.gltf` with
//! embedded or separate buffers, `.glb`) and VRML 2.0 (`.wrl`: indexed face and line sets with
//! colors and normals, inside transforms). Each becomes polygonal data; glTF meshes and VRML
//! shapes come as one block each.

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

// ------------------------------------------------------------------ OFF

/// OFF: `OFF` (or `COFF`, `NOFF`, `CNOFF`), the counts, the vertices (with colors and normals as
/// the keyword says), then faces as a count and indices, optionally followed by a color.
pub fn read_off(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut tokens = text.lines().map(|l| l.split('#').next().unwrap_or("")).flat_map(|l| l.split_whitespace()).peekable();
    let key = tokens.next().ok_or_else(|| anyhow!("{} is empty", path.display()))?.to_ascii_uppercase();
    if !key.ends_with("OFF") {
        bail!("{} is not an OFF file (it starts with {key:?})", path.display());
    }
    let has_c = key.contains('C');
    let has_n = key.contains('N');
    let mut num = |what: &str| -> Result<f64> {
        tokens.next().ok_or_else(|| anyhow!("OFF: the file ends inside the {what}"))?.parse::<f64>().map_err(|_| anyhow!("OFF: bad number in the {what}"))
    };
    let (nv, nf, _ne) = (num("counts")? as usize, num("counts")? as usize, num("counts")? as usize);
    let mut pts = Vec::with_capacity(nv * 3);
    let mut normals = Vec::new();
    let mut colors: Vec<u8> = Vec::new();
    for _ in 0..nv {
        for _ in 0..3 {
            pts.push(num("vertices")?);
        }
        if has_n {
            for _ in 0..3 {
                normals.push(num("normals")?);
            }
        }
        if has_c {
            let c = [num("colors")?, num("colors")?, num("colors")?, num("colors")?];
            let scale = if c.iter().all(|x| *x <= 1.0) { 255.0 } else { 1.0 };
            colors.extend(c.iter().map(|x| (x * scale).round().clamp(0.0, 255.0) as u8));
        }
    }
    let mut polys = Cells::new();
    let mut lines = Cells::new();
    let mut verts = Cells::new();
    let mut face_colors: Vec<u8> = Vec::new();
    let mut lines_raw = text.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>().into_iter();
    // Faces may carry a color after their indices, so read them line by line: skip the header
    // lines and the vertex lines first.
    let mut header_tokens = 0usize;
    let need = 1 + 3 + nv * (3 + if has_n { 3 } else { 0 } + if has_c { 4 } else { 0 });
    for l in lines_raw.by_ref() {
        header_tokens += l.split_whitespace().count();
        if header_tokens >= need {
            break;
        }
    }
    for l in lines_raw.take(nf) {
        let t: Vec<f64> = l.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        let Some(&n) = t.first() else { continue };
        let n = n as usize;
        if t.len() < 1 + n {
            bail!("OFF: a face lists fewer than {n} points");
        }
        let ids: Vec<u32> = t[1..1 + n].iter().map(|&x| x as u32).collect();
        if ids.iter().any(|&i| i as usize >= nv) {
            bail!("OFF: a face refers to a point that does not exist");
        }
        match n {
            0 => {}
            1 => verts.push(CellType::Vertex, &ids),
            2 => lines.push(CellType::Line, &ids),
            _ => polys.push(cell_type(n), &ids),
        }
        let c = &t[1 + n..];
        if c.len() >= 3 {
            let scale = if c.iter().all(|x| *x <= 1.0) { 255.0 } else { 1.0 };
            face_colors.extend(c.iter().take(3).map(|x| (x * scale).round().clamp(0.0, 255.0) as u8));
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, lines, verts, ..Default::default() };
    if normals.len() == nv * 3 {
        pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(normals)));
    }
    if colors.len() == nv * 4 {
        pd.point_data.insert_active(AttributeKind::Scalars, Array::new("Colors", 4, ArrayData::U8(colors)));
    }
    if face_colors.len() == pd.n_cells() * 3 {
        pd.cell_data.insert(Array::new("FaceColors", 3, ArrayData::U8(face_colors)));
    }
    Ok(DataSet::PolyData(pd))
}

// ------------------------------------------------------------------ glTF

type Mat4 = [f64; 16];

const I4: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Column-major 4x4 product a * b.
fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    m
}

fn apply(m: &Mat4, p: [f64; 3], w: f64) -> [f64; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12] * w,
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13] * w,
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14] * w,
    ]
}

/// Translation, rotation (quaternion x y z w) and scale as a column-major matrix.
fn trs(t: [f64; 3], q: [f64; 4], s: [f64; 3]) -> Mat4 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    [r[0] * s[0], r[1] * s[0], r[2] * s[0], 0.0, r[3] * s[1], r[4] * s[1], r[5] * s[1], 0.0, r[6] * s[2], r[7] * s[2], r[8] * s[2], 0.0, t[0], t[1], t[2], 1.0]
}

struct Gltf {
    json: serde_json::Value,
    buffers: Vec<Vec<u8>>,
}

impl Gltf {
    /// An accessor's values as f64, `ncomp` per element.
    fn accessor(&self, idx: usize) -> Result<(Vec<f64>, usize)> {
        let a = &self.json["accessors"][idx];
        let count = a["count"].as_u64().ok_or_else(|| anyhow!("glTF: accessor {idx} has no count"))? as usize;
        let ncomp = match a["type"].as_str().unwrap_or("SCALAR") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            other => bail!("glTF accessor type {other} is not supported"),
        };
        let ctype = a["componentType"].as_u64().unwrap_or(5126);
        let csize = match ctype {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            other => bail!("glTF component type {other}"),
        };
        let normalized = a["normalized"].as_bool().unwrap_or(false);
        let Some(bv) = a["bufferView"].as_u64() else { return Ok((vec![0.0; count * ncomp], ncomp)) };
        let view = &self.json["bufferViews"][bv as usize];
        let buf = &self.buffers[view["buffer"].as_u64().unwrap_or(0) as usize];
        let off = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = view["byteStride"].as_u64().map(|s| s as usize).unwrap_or(ncomp * csize);
        let mut out = Vec::with_capacity(count * ncomp);
        for e in 0..count {
            for c in 0..ncomp {
                let p = off + e * stride + c * csize;
                let b = buf.get(p..p + csize).ok_or_else(|| anyhow!("glTF: accessor {idx} runs past its buffer"))?;
                let v = match ctype {
                    5120 => {
                        let x = b[0] as i8 as f64;
                        if normalized {
                            (x / 127.0).max(-1.0)
                        } else {
                            x
                        }
                    }
                    5121 => {
                        let x = b[0] as f64;
                        if normalized {
                            x / 255.0
                        } else {
                            x
                        }
                    }
                    5122 => {
                        let x = i16::from_le_bytes([b[0], b[1]]) as f64;
                        if normalized {
                            (x / 32767.0).max(-1.0)
                        } else {
                            x
                        }
                    }
                    5123 => {
                        let x = u16::from_le_bytes([b[0], b[1]]) as f64;
                        if normalized {
                            x / 65535.0
                        } else {
                            x
                        }
                    }
                    5125 => u32::from_le_bytes(b.try_into().unwrap()) as f64,
                    _ => f32::from_le_bytes(b.try_into().unwrap()) as f64,
                };
                out.push(v);
            }
        }
        Ok((out, ncomp))
    }
}

fn load_gltf(path: &Path) -> Result<Gltf> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let (json, bin) = if bytes.starts_with(b"glTF") {
        let total = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        let mut pos = 12;
        let mut json = None;
        let mut bin = None;
        while pos + 8 <= total.min(bytes.len()) {
            let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize;
            let kind = &bytes[pos + 4..pos + 8];
            let data = &bytes[pos + 8..(pos + 8 + len).min(bytes.len())];
            match kind {
                b"JSON" => json = Some(serde_json::from_slice::<serde_json::Value>(data)?),
                b"BIN\0" => bin = Some(data.to_vec()),
                _ => {}
            }
            pos += 8 + len;
        }
        (json.ok_or_else(|| anyhow!("glb without JSON"))?, bin)
    } else {
        (serde_json::from_slice::<serde_json::Value>(&bytes).with_context(|| format!("{} is not glTF JSON", path.display()))?, None)
    };
    let mut buffers = Vec::new();
    for (k, b) in json["buffers"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let data = match b["uri"].as_str() {
            None if k == 0 => bin.clone().ok_or_else(|| anyhow!("glTF buffer 0 has no data"))?,
            None => bail!("glTF buffer {k} has no data"),
            Some(uri) if uri.starts_with("data:") => {
                use base64::Engine;
                let payload = uri.split_once(",").map(|x| x.1).unwrap_or("");
                base64::engine::general_purpose::STANDARD.decode(payload)?
            }
            Some(uri) => {
                let p = crate::sibling(path, uri);
                std::fs::read(&p).with_context(|| format!("reading glTF buffer {}", p.display()))?
            }
        };
        buffers.push(data);
    }
    Ok(Gltf { json, buffers })
}

/// glTF 2.0 (`.gltf`, `.glb`): the triangles, lines and points of every mesh the scene places,
/// moved by its nodes' transforms, with normals, vertex colors and texture coordinates. One block
/// per mesh node.
pub fn read_gltf(path: &Path) -> Result<DataSet> {
    let g = load_gltf(path)?;
    let j = &g.json;
    let mut mb = MultiBlock::default();
    // Nodes of the default scene (else every root node), walked with their transforms.
    let mut stack: Vec<(usize, Mat4)> = Vec::new();
    let scene = j["scene"].as_u64().unwrap_or(0) as usize;
    let roots: Vec<usize> = match j["scenes"][scene]["nodes"].as_array() {
        Some(n) => n.iter().filter_map(|v| v.as_u64().map(|x| x as usize)).collect(),
        None => (0..j["nodes"].as_array().map(|a| a.len()).unwrap_or(0)).collect(),
    };
    for r in roots.into_iter().rev() {
        stack.push((r, I4));
    }
    let mut guard = 0;
    while let Some((ni, parent)) = stack.pop() {
        guard += 1;
        if guard > 100_000 {
            bail!("glTF: the node tree loops");
        }
        let n = &j["nodes"][ni];
        let local = if let Some(m) = n["matrix"].as_array() {
            let mut a = [0.0; 16];
            for (k, v) in m.iter().take(16).enumerate() {
                a[k] = v.as_f64().unwrap_or(0.0);
            }
            a
        } else {
            let v3 = |key: &str, d: [f64; 3]| {
                n[key].as_array().map(|a| [a[0].as_f64().unwrap_or(d[0]), a[1].as_f64().unwrap_or(d[1]), a[2].as_f64().unwrap_or(d[2])]).unwrap_or(d)
            };
            let q = n["rotation"]
                .as_array()
                .map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0), a[3].as_f64().unwrap_or(1.0)])
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            trs(v3("translation", [0.0; 3]), q, v3("scale", [1.0; 3]))
        };
        let world = mul(&parent, &local);
        if let Some(mi) = n["mesh"].as_u64() {
            let name = n["name"].as_str().or(j["meshes"][mi as usize]["name"].as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("mesh {mi}"));
            mb.blocks.push((name, Some(DataSet::PolyData(gltf_mesh(&g, mi as usize, &world)?))));
        }
        if let Some(kids) = n["children"].as_array() {
            for k in kids.iter().rev().filter_map(|v| v.as_u64()) {
                stack.push((k as usize, world));
            }
        }
    }
    if mb.blocks.is_empty() {
        // Meshes not placed by any node.
        for mi in 0..j["meshes"].as_array().map(|a| a.len()).unwrap_or(0) {
            mb.blocks.push((format!("mesh {mi}"), Some(DataSet::PolyData(gltf_mesh(&g, mi, &I4)?))));
        }
    }
    if mb.blocks.is_empty() {
        bail!("{}: no meshes", path.display());
    }
    Ok(if mb.blocks.len() == 1 { mb.blocks.pop().unwrap().1.unwrap() } else { DataSet::MultiBlock(mb) })
}

fn gltf_mesh(g: &Gltf, mi: usize, m: &Mat4) -> Result<PolyData> {
    let mut pd = PolyData::default();
    let (mut normals, mut colors, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    let mut all_normals = true;
    let mut all_colors = true;
    let mut all_uvs = true;
    for prim in g.json["meshes"][mi]["primitives"].as_array().cloned().unwrap_or_default() {
        let attrs = &prim["attributes"];
        let Some(pos) = attrs["POSITION"].as_u64() else { continue };
        let (p, _) = g.accessor(pos as usize)?;
        let base = pd.points.len() as u32;
        let n = p.len() / 3;
        for k in 0..n {
            pd.points.push(apply(m, [p[3 * k], p[3 * k + 1], p[3 * k + 2]], 1.0));
        }
        match attrs["NORMAL"].as_u64() {
            Some(a) => {
                let (v, _) = g.accessor(a as usize)?;
                for k in 0..n {
                    let q = apply(m, [v[3 * k], v[3 * k + 1], v[3 * k + 2]], 0.0);
                    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt().max(1e-300);
                    normals.extend(q.map(|x| x / l));
                }
            }
            None => all_normals = false,
        }
        match attrs["COLOR_0"].as_u64() {
            Some(a) => {
                let (v, nc) = g.accessor(a as usize)?;
                for k in 0..n {
                    for c in 0..3 {
                        colors.push((v[k * nc + c] * 255.0).round().clamp(0.0, 255.0) as u8);
                    }
                }
            }
            None => all_colors = false,
        }
        match attrs["TEXCOORD_0"].as_u64() {
            Some(a) => uvs.extend(g.accessor(a as usize)?.0),
            None => all_uvs = false,
        }
        let idx: Vec<u32> = match prim["indices"].as_u64() {
            Some(a) => g.accessor(a as usize)?.0.iter().map(|&x| x as u32 + base).collect(),
            None => (base..base + n as u32).collect(),
        };
        match prim["mode"].as_u64().unwrap_or(4) {
            0 => idx.iter().for_each(|&i| pd.verts.push(CellType::Vertex, &[i])),
            1 => idx.chunks_exact(2).for_each(|c| pd.lines.push(CellType::Line, c)),
            3 => pd.lines.push(CellType::PolyLine, &idx),
            2 => {
                let mut l = idx.clone();
                l.push(idx[0]);
                pd.lines.push(CellType::PolyLine, &l);
            }
            4 => idx.chunks_exact(3).for_each(|c| pd.polys.push(CellType::Triangle, c)),
            5 => {
                for k in 0..idx.len().saturating_sub(2) {
                    let t = if k % 2 == 0 { [idx[k], idx[k + 1], idx[k + 2]] } else { [idx[k + 1], idx[k], idx[k + 2]] };
                    pd.polys.push(CellType::Triangle, &t);
                }
            }
            6 => {
                for k in 1..idx.len().saturating_sub(1) {
                    pd.polys.push(CellType::Triangle, &[idx[0], idx[k], idx[k + 1]]);
                }
            }
            other => bail!("glTF primitive mode {other}"),
        }
    }
    let np = pd.points.len();
    if all_normals && normals.len() == np * 3 && np > 0 {
        pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(normals)));
    }
    if all_colors && colors.len() == np * 3 && np > 0 {
        pd.point_data.insert(Array::new("Colors", 3, ArrayData::U8(colors)));
    }
    if all_uvs && uvs.len() == np * 2 && np > 0 {
        pd.point_data.insert_active(AttributeKind::TCoords, Array::new("TEXCOORD_0", 2, ArrayData::F64(uvs)));
    }
    Ok(pd)
}

// ------------------------------------------------------------------ VRML 2.0

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Num(f64),
    Str(String),
    Open,
    Close,
    LBr,
    RBr,
}

fn vrml_tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = text.chars().peekable();
    while let Some(&c) = it.peek() {
        if c == '#' {
            while let Some(c) = it.next() {
                if c == '\n' {
                    break;
                }
            }
        } else if c.is_whitespace() || c == ',' {
            it.next();
        } else if c == '{' || c == '}' || c == '[' || c == ']' {
            it.next();
            out.push(match c {
                '{' => Tok::Open,
                '}' => Tok::Close,
                '[' => Tok::LBr,
                _ => Tok::RBr,
            });
        } else if c == '"' {
            it.next();
            let mut s = String::new();
            while let Some(c) = it.next() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            out.push(Tok::Str(s));
        } else {
            let mut s = String::new();
            while let Some(&c) = it.peek() {
                if c.is_whitespace() || "{}[],#\"".contains(c) {
                    break;
                }
                s.push(c);
                it.next();
            }
            match s.parse::<f64>() {
                Ok(v) if s.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') => out.push(Tok::Num(v)),
                _ => out.push(Tok::Word(s)),
            }
        }
    }
    out
}

/// A parsed VRML node: its type, plain fields (numbers) and child nodes by field name.
#[derive(Debug, Default, Clone)]
struct VNode {
    ty: String,
    nums: std::collections::HashMap<String, Vec<f64>>,
    kids: Vec<(String, VNode)>,
}

fn parse_vnode(t: &[Tok], pos: &mut usize, defs: &mut std::collections::HashMap<String, VNode>) -> Result<VNode> {
    // [DEF name] Type { fields } | USE name
    let mut def = None;
    if let Some(Tok::Word(w)) = t.get(*pos) {
        if w == "USE" {
            *pos += 1;
            if let Some(Tok::Word(n)) = t.get(*pos) {
                *pos += 1;
                return Ok(defs.get(n).cloned().unwrap_or_default());
            }
        }
        if w == "DEF" {
            if let Some(Tok::Word(n)) = t.get(*pos + 1) {
                def = Some(n.clone());
            }
            *pos += 2;
        }
    }
    let ty = match t.get(*pos) {
        Some(Tok::Word(w)) => w.clone(),
        other => bail!("VRML: a node type was expected, found {other:?}"),
    };
    *pos += 1;
    if t.get(*pos) != Some(&Tok::Open) {
        bail!("VRML: {{ expected after {ty}");
    }
    *pos += 1;
    let mut node = VNode { ty, ..Default::default() };
    while *pos < t.len() && t[*pos] != Tok::Close {
        let field = match &t[*pos] {
            Tok::Word(w) => w.clone(),
            _ => {
                *pos += 1;
                continue;
            }
        };
        *pos += 1;
        match t.get(*pos) {
            Some(Tok::LBr) => {
                *pos += 1;
                // A list of numbers, or of nodes.
                let mut nums = Vec::new();
                while *pos < t.len() && t[*pos] != Tok::RBr {
                    match &t[*pos] {
                        Tok::Num(v) => {
                            nums.push(*v);
                            *pos += 1;
                        }
                        Tok::Word(_) => {
                            let k = parse_vnode(t, pos, defs)?;
                            node.kids.push((field.clone(), k));
                        }
                        _ => *pos += 1,
                    }
                }
                *pos += 1;
                node.nums.insert(field, nums);
            }
            Some(Tok::Word(w)) if matches!(t.get(*pos + 1), Some(Tok::Open)) || w == "DEF" || w == "USE" => {
                let k = parse_vnode(t, pos, defs)?;
                node.kids.push((field, k));
            }
            _ => {
                let mut nums = Vec::new();
                while let Some(tok) = t.get(*pos) {
                    match tok {
                        Tok::Num(v) => {
                            nums.push(*v);
                            *pos += 1;
                        }
                        Tok::Word(w) if w == "TRUE" || w == "FALSE" => {
                            nums.push(if w == "TRUE" { 1.0 } else { 0.0 });
                            *pos += 1;
                            break;
                        }
                        Tok::Str(_) => {
                            *pos += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                node.nums.insert(field, nums);
            }
        }
    }
    *pos += 1;
    if let Some(d) = def {
        defs.insert(d, node.clone());
    }
    Ok(node)
}

fn vrml_transform(n: &VNode) -> Mat4 {
    let g = |k: &str, d: Vec<f64>| n.nums.get(k).cloned().filter(|v| v.len() >= d.len()).unwrap_or(d);
    let t = g("translation", vec![0.0; 3]);
    let s = g("scale", vec![1.0; 3]);
    let r = g("rotation", vec![0.0, 0.0, 1.0, 0.0]);
    let c = g("center", vec![0.0; 3]);
    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(1e-300);
    let h = 0.5 * r[3];
    let q = [r[0] / l * h.sin(), r[1] / l * h.sin(), r[2] / l * h.sin(), h.cos()];
    // T * C * R * S * -C
    let tc = trs([t[0] + c[0], t[1] + c[1], t[2] + c[2]], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
    let rs = trs([0.0; 3], q, [s[0], s[1], s[2]]);
    let mc = trs([-c[0], -c[1], -c[2]], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
    mul(&mul(&tc, &rs), &mc)
}

/// VRML 2.0 (`.wrl`): the IndexedFaceSet, IndexedLineSet and PointSet geometry of every Shape,
/// moved by the Transforms around it, with per-vertex or per-face colors and normals. One block
/// per shape.
pub fn read_vrml(path: &Path) -> Result<DataSet> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let raw = if raw.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut out)?;
        out
    } else {
        raw
    };
    let text = String::from_utf8_lossy(&raw);
    if !text.starts_with("#VRML V2.0") {
        bail!("{}: only VRML 2.0 files are read", path.display());
    }
    let toks = vrml_tokens(&text);
    let mut defs = std::collections::HashMap::new();
    let mut pos = 0;
    let mut roots = Vec::new();
    while pos < toks.len() {
        match &toks[pos] {
            Tok::Word(w) if w == "ROUTE" => pos += 4,
            Tok::Word(w) if w == "PROTO" || w == "EXTERNPROTO" => bail!("{}: VRML PROTO definitions are not supported", path.display()),
            Tok::Word(_) => roots.push(parse_vnode(&toks, &mut pos, &mut defs)?),
            _ => pos += 1,
        }
    }
    let mut mb = MultiBlock::default();
    let mut stack: Vec<(VNode, Mat4)> = roots.into_iter().rev().map(|n| (n, I4)).collect();
    while let Some((n, m)) = stack.pop() {
        let m = if n.ty == "Transform" { mul(&m, &vrml_transform(&n)) } else { m };
        if n.ty == "Shape" {
            if let Some((_, geom)) = n.kids.iter().find(|(f, _)| f == "geometry") {
                if let Some(pd) = vrml_geometry(geom, &m)? {
                    mb.blocks.push((format!("{} {}", geom.ty, mb.blocks.len() + 1), Some(DataSet::PolyData(pd))));
                }
            }
            continue;
        }
        for (f, k) in n.kids.iter().rev() {
            if f == "children" || f == "level" || f == "choice" {
                stack.push((k.clone(), m));
            }
        }
    }
    if mb.blocks.is_empty() {
        bail!("{}: no indexed face sets, line sets or point sets", path.display());
    }
    Ok(if mb.blocks.len() == 1 { mb.blocks.pop().unwrap().1.unwrap() } else { DataSet::MultiBlock(mb) })
}

fn vrml_geometry(g: &VNode, m: &Mat4) -> Result<Option<PolyData>> {
    let kid = |name: &str| g.kids.iter().find(|(f, _)| f == name).map(|(_, k)| k);
    let Some(coord) = kid("coord").and_then(|c| c.nums.get("point")) else { return Ok(None) };
    let mut pd = PolyData::default();
    for p in coord.chunks_exact(3) {
        pd.points.push(apply(m, [p[0], p[1], p[2]], 1.0));
    }
    let np = pd.points.len();
    let index = g.nums.get("coordIndex").cloned().unwrap_or_default();
    let faces: Vec<Vec<u32>> = index.split(|&x| x < 0.0).filter(|f| !f.is_empty()).map(|f| f.iter().map(|&x| x as u32).collect()).collect();
    if faces.iter().flatten().any(|&i| i as usize >= np) {
        bail!("VRML: an index refers to a point that does not exist");
    }
    match g.ty.as_str() {
        "IndexedFaceSet" => faces.iter().for_each(|f| pd.polys.push(cell_type(f.len()), f)),
        "IndexedLineSet" => faces.iter().for_each(|f| pd.lines.push(if f.len() == 2 { CellType::Line } else { CellType::PolyLine }, f)),
        "PointSet" => (0..np as u32).for_each(|i| pd.verts.push(CellType::Vertex, &[i])),
        _ => return Ok(None),
    }
    let per_vertex = g.nums.get("colorPerVertex").map(|v| v.first().copied().unwrap_or(1.0) != 0.0).unwrap_or(true);
    if let Some(cols) = kid("color").and_then(|c| c.nums.get("color")) {
        let rgb: Vec<u8> = cols.iter().map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8).collect();
        let cidx = g.nums.get("colorIndex").cloned().unwrap_or_default();
        let ncell = pd.n_cells();
        if per_vertex || g.ty == "PointSet" {
            if cidx.is_empty() && rgb.len() == np * 3 {
                pd.point_data.insert(Array::new("Colors", 3, ArrayData::U8(rgb)));
            }
        } else {
            let pick: Vec<usize> = if cidx.is_empty() { (0..ncell).collect() } else { cidx.iter().filter(|&&x| x >= 0.0).map(|&x| x as usize).collect() };
            if pick.len() >= ncell && pick.iter().take(ncell).all(|&i| i * 3 + 2 < rgb.len()) {
                let v: Vec<u8> = pick.iter().take(ncell).flat_map(|&i| rgb[i * 3..i * 3 + 3].to_vec()).collect();
                pd.cell_data.insert(Array::new("Colors", 3, ArrayData::U8(v)));
            }
        }
    }
    if let Some(nv) = kid("normal").and_then(|c| c.nums.get("vector")) {
        if nv.len() == np * 3 && g.nums.get("normalIndex").is_none() {
            let n: Vec<f64> = nv.chunks_exact(3).flat_map(|v| apply(m, [v[0], v[1], v[2]], 0.0)).collect();
            pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(n)));
        }
    }
    Ok(Some(pd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, body: &[u8]) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("afx-scenes-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn off_with_colors() {
        let p = tmp(
            "t.off",
            b"COFF\n# a tetrahedron\n4 4 6\n0 0 0 1 0 0 1\n1 0 0 0 1 0 1\n0 1 0 0 0 1 1\n0 0 1 1 1 1 1\n3 0 2 1\n3 0 1 3 255 0 0\n3 1 2 3\n3 0 3 2\n",
        );
        let DataSet::PolyData(pd) = read_off(&p).unwrap() else { panic!() };
        assert_eq!((pd.points.len(), pd.polys.len()), (4, 4));
        assert_eq!(pd.point_data.get("Colors").unwrap().get(1, 1), 255.0);
        assert_eq!(pd.polys.cell(1), &[0, 1, 3]);
    }

    #[test]
    fn vrml_nested_transforms() {
        let p = tmp(
            "s.wrl",
            br#"#VRML V2.0 utf8
Transform { translation 10 0 0 children [
  Transform { scale 2 2 2 children [
    Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
      geometry DEF Q IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] }
        coordIndex [ 0 1 2 3 -1 ] color Color { color [ 1 0 0 ] } colorPerVertex FALSE } }
  ] }
  Shape { geometry IndexedLineSet { coord Coordinate { point [ 0 0 0 0 0 1 ] } coordIndex [ 0 1 -1 ] } }
] }
"#,
        );
        let DataSet::MultiBlock(mb) = read_vrml(&p).unwrap() else { panic!() };
        assert_eq!(mb.blocks.len(), 2);
        let Some(DataSet::PolyData(q)) = &mb.blocks[0].1 else { panic!() };
        assert_eq!(q.points.get(2), [12.0, 2.0, 0.0]);
        assert_eq!(q.cell_data.get("Colors").unwrap().get(0, 0), 255.0);
        let Some(DataSet::PolyData(l)) = &mb.blocks[1].1 else { panic!() };
        assert_eq!((l.lines.len(), l.points.get(1)), (1, [10.0, 0.0, 1.0]));
    }

    #[test]
    fn gltf_embedded_and_binary() {
        // One triangle (positions as floats, indices as u16) placed by a node moved by (0, 0, 5).
        let mut bin = Vec::new();
        for v in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2, 0] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        use base64::Engine;
        let uri = format!("data:application/octet-stream;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bin));
        let json = |buf: &str| {
            format!(
                r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"translation":[0,0,5],"name":"tri"}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"buffers":[{{"byteLength":{}{buf}}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":6}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
                bin.len()
            )
        };
        let p = tmp("t.gltf", json(&format!(",\"uri\":\"{uri}\"")).as_bytes());
        let DataSet::PolyData(pd) = read_gltf(&p).unwrap() else { panic!() };
        assert_eq!((pd.points.len(), pd.polys.len(), pd.points.get(1)), (3, 1, [1.0, 0.0, 5.0]));
        // The same as .glb.
        let mut j = json("").into_bytes();
        while j.len() % 4 != 0 {
            j.push(b' ');
        }
        let mut b = bin.clone();
        while b.len() % 4 != 0 {
            b.push(0);
        }
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&((12 + 8 + j.len() + 8 + b.len()) as u32).to_le_bytes());
        glb.extend_from_slice(&(j.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&j);
        glb.extend_from_slice(&(b.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&b);
        let p = tmp("t.glb", &glb);
        let DataSet::PolyData(pd) = read_gltf(&p).unwrap() else { panic!() };
        assert_eq!(pd.points.get(2), [0.0, 1.0, 5.0]);
    }
}
