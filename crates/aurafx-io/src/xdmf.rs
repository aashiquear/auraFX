//! Xdmf 2/3 (`.xmf`, `.xdmf`): XML light data with heavy data inline, in HDF5 or raw binary
//! files. Unstructured (mixed and single-type, linear and quadratic), curvilinear, rectilinear
//! and image topologies; node/cell/grid attributes; spatial collections (blocks) and temporal
//! collections (time steps); `xi:include` and `Reference` XPath pointers (as FEniCS/Dolfinx
//! write them); HyperSlab data items.

use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---- a small DOM ----

#[derive(Debug, Clone, Default)]
pub(crate) struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<El>,
    pub text: String,
}

impl El {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    pub fn kid(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == name)
    }
}

fn parse_xml(text: &str) -> Result<El> {
    let root = parse_dom(text)?;
    root.children.into_iter().find(|c| c.name == "Xdmf").ok_or_else(|| anyhow!("no <Xdmf> element"))
}

/// Parse XML into a tree; the returned element is a "#root" holding the document element.
pub(crate) fn parse_dom(text: &str) -> Result<El> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(text);
    let mut stack: Vec<El> = vec![El { name: "#root".into(), ..Default::default() }];
    let attrs = |e: &quick_xml::events::BytesStart| -> Vec<(String, String)> {
        e.attributes()
            .flatten()
            .map(|a| {
                (
                    a.key.as_ref().to_string(),
                    a.normalized_value(quick_xml::XmlVersion::default()).map(|v| v.to_string()).unwrap_or_else(|_| a.value.to_string()),
                )
            })
            .collect()
    };
    loop {
        match r.read_event().map_err(|e| anyhow!("XML: {e}"))? {
            Event::Start(e) => stack.push(El { name: e.name().as_ref().to_string(), attrs: attrs(&e), ..Default::default() }),
            Event::Empty(e) => {
                let el = El { name: e.name().as_ref().to_string(), attrs: attrs(&e), ..Default::default() };
                stack.last_mut().unwrap().children.push(el);
            }
            Event::End(_) => {
                let el = stack.pop().unwrap();
                stack.last_mut().ok_or_else(|| anyhow!("unbalanced XML"))?.children.push(el);
            }
            Event::Text(t) => {
                let s = t.xml_content(quick_xml::XmlVersion::default()).to_string();
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Event::CData(t) => stack.last_mut().unwrap().text.push_str(t.as_ref()),
            Event::GeneralRef(r) => {
                let s: &str = r.as_ref();
                let c = match s {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => r.resolve_char_ref().ok().flatten(),
                };
                if let Some(c) = c {
                    stack.last_mut().unwrap().text.push(c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack.pop().ok_or_else(|| anyhow!("empty XML"))
}

/// Resolve a small XPath subset: `/Xdmf/Domain/Grid[1]/Geometry`, `//Grid[@Name='mesh']/Topology`,
/// `/Xdmf/Domain/Grid[@GridType='Uniform'][1]/Geometry`.
fn xpath<'a>(root: &'a El, path: &str) -> Option<&'a El> {
    let path = path.trim().trim_start_matches("xpointer(").trim_end_matches(')');
    let (deep, rest) = if let Some(r) = path.strip_prefix("//") { (true, r) } else { (false, path.trim_start_matches('/')) };
    let steps: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    fn step_matches(e: &El, step: &str) -> (bool, Option<usize>) {
        let name_end = step.find('[').unwrap_or(step.len());
        if e.name != step[..name_end] {
            return (false, None);
        }
        let mut index = None;
        let mut rest = &step[name_end..];
        while let Some(open) = rest.find('[') {
            let close = rest[open..].find(']').map(|c| open + c).unwrap_or(rest.len());
            let pred = &rest[open + 1..close];
            if let Some(cond) = pred.strip_prefix('@') {
                let (k, v) = cond.split_once('=').unwrap_or((cond, ""));
                if e.attr(k) != Some(v.trim_matches(|c| c == '\'' || c == '"')) {
                    return (false, None);
                }
            } else if let Ok(n) = pred.parse::<usize>() {
                index = Some(n);
            }
            rest = &rest[(close + 1).min(rest.len())..];
        }
        (true, index)
    }
    fn walk<'a>(e: &'a El, steps: &[&str]) -> Option<&'a El> {
        let Some((step, more)) = steps.split_first() else { return Some(e) };
        let mut count = 0;
        for c in &e.children {
            let (ok, index) = step_matches(c, step);
            if ok {
                count += 1;
                if index.is_none_or(|i| i == count) {
                    if let Some(found) = walk(c, more) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    fn deep_walk<'a>(e: &'a El, steps: &[&str]) -> Option<&'a El> {
        walk(e, steps).or_else(|| e.children.iter().find_map(|c| deep_walk(c, steps)))
    }
    let wrapper = El { children: vec![root.clone()], ..Default::default() };
    let found = if deep { deep_walk(&wrapper, &steps) } else { walk(&wrapper, &steps) };
    // Find the same element in `root` (the wrapper is a temporary copy).
    found.and_then(|f| find_same(root, f))
}

fn find_same<'a>(e: &'a El, target: &El) -> Option<&'a El> {
    if e.name == target.name && e.attrs == target.attrs && e.children.len() == target.children.len() && e.text == target.text {
        return Some(e);
    }
    e.children.iter().find_map(|c| find_same(c, target))
}

/// Replace `xi:include` and `Reference` pointers by the elements they point at.
fn resolve(e: &El, root: &El, depth: usize) -> El {
    if depth > 32 {
        return e.clone();
    }
    let mut out = El { name: e.name.clone(), attrs: e.attrs.clone(), text: e.text.clone(), children: Vec::new() };
    for c in &e.children {
        if c.name == "xi:include" || c.name.ends_with(":include") {
            if let Some(t) = c.attr("xpointer").and_then(|p| xpath(root, p)) {
                out.children.push(resolve(t, root, depth + 1));
            }
            continue;
        }
        if c.name == "DataItem" {
            if let Some(r) = c.attr("Reference") {
                let path = if r.eq_ignore_ascii_case("XML") { c.text.trim() } else { r };
                if let Some(t) = xpath(root, path) {
                    out.children.push(resolve(t, root, depth + 1));
                    continue;
                }
            }
        }
        out.children.push(resolve(c, root, depth + 1));
    }
    out
}

// ---- data items ----

struct Ctx {
    dir: PathBuf,
    hdf: HashMap<PathBuf, hdf5_reader::Hdf5File>,
}

fn dims(e: &El) -> Vec<usize> {
    e.attr("Dimensions").map(|d| d.split_whitespace().filter_map(|v| v.parse().ok()).collect()).unwrap_or_default()
}

/// A data item's values as f64 (int types too) and its dimensions.
fn data(e: &El, cx: &mut Ctx) -> Result<(Vec<f64>, Vec<usize>)> {
    let d = dims(e);
    if e.attr("ItemType").is_some_and(|t| t.eq_ignore_ascii_case("HyperSlab")) {
        let items: Vec<&El> = e.kids("DataItem").collect();
        if items.len() != 2 {
            bail!("HyperSlab needs two data items");
        }
        let (sel, _) = data(items[0], cx)?;
        let (src, sd) = data(items[1], cx)?;
        let rank = sd.len().max(1);
        let (start, stride, count) = (&sel[..rank], &sel[rank..2 * rank], &sel[2 * rank..3 * rank]);
        let mut out = Vec::new();
        let total: usize = count.iter().map(|&c| c as usize).product();
        for flat in 0..total {
            let mut rem = flat;
            let mut idx = 0usize;
            let mut mul = 1usize;
            for k in (0..rank).rev() {
                let c = count[k] as usize;
                let i = rem % c;
                rem /= c;
                idx += (start[k] as usize + i * stride[k] as usize) * mul;
                mul *= sd[k];
            }
            out.push(*src.get(idx).ok_or_else(|| anyhow!("HyperSlab out of range"))?);
        }
        return Ok((out, count.iter().map(|&c| c as usize).collect()));
    }
    if e.attr("ItemType").is_some_and(|t| t.eq_ignore_ascii_case("Function")) {
        // JOIN($0, $1, $2): interleave components.
        let parts: Vec<(Vec<f64>, Vec<usize>)> = e.kids("DataItem").map(|c| data(c, cx)).collect::<Result<_>>()?;
        let f = e.attr("Function").unwrap_or("");
        if !f.to_ascii_uppercase().starts_with("JOIN") || parts.is_empty() {
            bail!("Xdmf function {f:?} is not supported");
        }
        let n = parts[0].0.len();
        let out = (0..n).flat_map(|i| parts.iter().map(move |p| p.0.get(i).copied().unwrap_or(0.0))).collect();
        return Ok((out, vec![n, parts.len()]));
    }
    let format = e.attr("Format").unwrap_or("XML").to_ascii_uppercase();
    let text = e.text.trim();
    let values = match format.as_str() {
        "XML" => text.split_whitespace().map(|t| t.parse::<f64>().map_err(|_| anyhow!("bad number {t:?}"))).collect::<Result<Vec<f64>>>()?,
        "HDF" => {
            let (file, path) = text.split_once(':').ok_or_else(|| anyhow!("HDF data item without file:path ({text:?})"))?;
            let fp = cx.dir.join(file.trim());
            if !cx.hdf.contains_key(&fp) {
                let f = hdf5_reader::Hdf5File::open(&fp).with_context(|| format!("opening {}", fp.display()))?;
                cx.hdf.insert(fp.clone(), f);
            }
            let ds = cx.hdf[&fp].dataset(path.trim()).with_context(|| format!("{} in {}", path, fp.display()))?;
            let (arr, _) = crate::vtkhdf::read_dataset(&ds)?;
            arr.to_f64_vec()
        }
        "BINARY" => {
            let fp = cx.dir.join(text);
            let bytes = std::fs::read(&fp).with_context(|| format!("reading {}", fp.display()))?;
            let seek: usize = e.attr("Seek").and_then(|s| s.parse().ok()).unwrap_or(0);
            let prec: usize = e.attr("Precision").and_then(|s| s.parse().ok()).unwrap_or(4);
            let ty = e.attr("NumberType").or(e.attr("DataType")).unwrap_or("Float").to_ascii_lowercase();
            let big = e.attr("Endian").is_some_and(|s| s.eq_ignore_ascii_case("Big"));
            let n: usize = d.iter().product();
            let b = bytes.get(seek..seek + n * prec).ok_or_else(|| anyhow!("{} is too short", fp.display()))?;
            b.chunks_exact(prec)
                .map(|c| {
                    let mut a = [0u8; 8];
                    a[..prec].copy_from_slice(c);
                    if big {
                        a[..prec].reverse();
                    }
                    match (ty.as_str(), prec) {
                        ("float", 4) => f32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                        ("float", _) => f64::from_le_bytes(a),
                        ("uint" | "uchar", _) => u64::from_le_bytes(a) as f64,
                        (_, 1) => a[0] as i8 as f64,
                        (_, 2) => i16::from_le_bytes([a[0], a[1]]) as f64,
                        (_, 4) => i32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                        _ => i64::from_le_bytes(a) as f64,
                    }
                })
                .collect()
        }
        f => bail!("Xdmf data format {f} is not supported"),
    };
    Ok((values, d))
}

// ---- grids ----

/// (Xdmf cell code or topology name, nodes per cell, VTK type)
fn topo_type(name: &str) -> Option<(usize, CellType)> {
    Some(match name.to_ascii_lowercase().replace('_', "").as_str() {
        "polyvertex" => (1, CellType::Vertex),
        "polyline" => (2, CellType::Line),
        "triangle" => (3, CellType::Triangle),
        "quadrilateral" => (4, CellType::Quad),
        "tetrahedron" => (4, CellType::Tetra),
        "pyramid" => (5, CellType::Pyramid),
        "wedge" => (6, CellType::Wedge),
        "hexahedron" => (8, CellType::Hexahedron),
        "edge3" => (3, CellType::QuadraticEdge),
        "triangle6" => (6, CellType::QuadraticTriangle),
        "quadrilateral8" => (8, CellType::QuadraticQuad),
        "quadrilateral9" => (9, CellType::BiquadraticQuad),
        "tetrahedron10" => (10, CellType::QuadraticTetra),
        "pyramid13" => (13, CellType::QuadraticPyramid),
        "wedge15" => (15, CellType::QuadraticWedge),
        "wedge18" => (18, CellType::BiquadraticQuadraticWedge),
        "hexahedron20" => (20, CellType::QuadraticHexahedron),
        "hexahedron27" => (27, CellType::TriquadraticHexahedron),
        _ => return None,
    })
}

/// Cell types of a Mixed topology stream, by Xdmf code (None: followed by a node count).
fn mixed_type(code: i64) -> Option<(Option<usize>, CellType)> {
    Some(match code {
        1 => (None, CellType::PolyVertex),
        2 => (None, CellType::PolyLine),
        3 => (None, CellType::Polygon),
        4 => (Some(3), CellType::Triangle),
        5 => (Some(4), CellType::Quad),
        6 => (Some(4), CellType::Tetra),
        7 => (Some(5), CellType::Pyramid),
        8 => (Some(6), CellType::Wedge),
        9 => (Some(8), CellType::Hexahedron),
        34 => (Some(3), CellType::QuadraticEdge),
        35 => (Some(9), CellType::BiquadraticQuad),
        36 => (Some(6), CellType::QuadraticTriangle),
        37 => (Some(8), CellType::QuadraticQuad),
        38 => (Some(10), CellType::QuadraticTetra),
        39 => (Some(13), CellType::QuadraticPyramid),
        40 => (Some(15), CellType::QuadraticWedge),
        41 => (Some(18), CellType::BiquadraticQuadraticWedge),
        48 => (Some(20), CellType::QuadraticHexahedron),
        50 => (Some(27), CellType::TriquadraticHexahedron),
        _ => return None,
    })
}

fn geometry(g: &El, cx: &mut Ctx) -> Result<Vec<f64>> {
    let ty = g.attr("GeometryType").or(g.attr("Type")).unwrap_or("XYZ").to_ascii_uppercase();
    let items: Vec<&El> = g.kids("DataItem").collect();
    let mut get = |k: usize| -> Result<Vec<f64>> { data(items.get(k).ok_or_else(|| anyhow!("geometry {ty} needs more data items"))?, cx).map(|d| d.0) };
    Ok(match ty.as_str() {
        "XYZ" => get(0)?,
        "XY" => get(0)?.chunks_exact(2).flat_map(|c| [c[0], c[1], 0.0]).collect(),
        "X_Y_Z" | "X_Y" => {
            let (x, y) = (get(0)?, get(1)?);
            let z = if ty == "X_Y_Z" { get(2)? } else { vec![0.0; x.len()] };
            (0..x.len()).flat_map(|i| [x[i], y[i], z[i]]).collect()
        }
        t => bail!("geometry type {t} here needs a structured topology"),
    })
}

fn attributes(grid: &El, cx: &mut Ctx, np: usize, nc: usize) -> Result<(Attributes, Attributes, Attributes)> {
    let (mut pd, mut cd, mut fd) = (Attributes::new(), Attributes::new(), Attributes::new());
    for a in grid.kids("Attribute") {
        let name = a.attr("Name").unwrap_or("data").to_string();
        let center = a.attr("Center").or(a.attr("AttributeCenter")).unwrap_or("Node").to_ascii_lowercase();
        let Some(item) = a.kid("DataItem") else { continue };
        let (vals, d) = match data(item, cx) {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!("Xdmf attribute {name}: {e:#}");
                continue;
            }
        };
        let n = match center.as_str() {
            "node" => np,
            "cell" => nc,
            _ => 1,
        };
        let ncomp = if n > 0 { (vals.len() / n.max(1)).max(1) } else { d.last().copied().unwrap_or(1) };
        let arr = Array::new(name, ncomp, ArrayData::F64(vals));
        match center.as_str() {
            "node" => pd.insert(arr),
            "cell" => cd.insert(arr),
            _ => fd.insert(arr),
        };
    }
    Ok((pd, cd, fd))
}

fn uniform(grid: &El, cx: &mut Ctx) -> Result<DataSet> {
    let topo = grid.kid("Topology").ok_or_else(|| anyhow!("grid without Topology"))?;
    let geom = grid.kid("Geometry").ok_or_else(|| anyhow!("grid without Geometry"))?;
    let tname = topo.attr("TopologyType").or(topo.attr("Type")).unwrap_or("").to_string();
    let tl = tname.to_ascii_lowercase();
    if tl.contains("mesh") {
        // Structured: dimensions slowest first (z y x).
        let mut dd = dims(topo);
        if dd.len() == 2 {
            dd.insert(0, 1);
        }
        let (nz, ny, nx) = (dd[0], dd[1], dd[2]);
        let extent = [0, nx as i64 - 1, 0, ny as i64 - 1, 0, nz as i64 - 1];
        let np = nx * ny * nz;
        let nc = nx.saturating_sub(1).max(1) * ny.saturating_sub(1).max(1) * nz.saturating_sub(1).max(1);
        let (pd, cd, fd) = attributes(grid, cx, np, nc)?;
        let gty = geom.attr("GeometryType").or(geom.attr("Type")).unwrap_or("").to_ascii_uppercase();
        let items: Vec<&El> = geom.kids("DataItem").collect();
        if tl.contains("corect") {
            let o = data(items.first().ok_or_else(|| anyhow!("ORIGIN_DXDYDZ needs data"))?, cx)?.0;
            let s = data(items.get(1).ok_or_else(|| anyhow!("ORIGIN_DXDYDZ needs spacing"))?, cx)?.0;
            // Values are given z, y, x.
            let (origin, spacing) = if o.len() == 2 { ([o[1], o[0], 0.0], [s[1], s[0], 1.0]) } else { ([o[2], o[1], o[0]], [s[2], s[1], s[0]]) };
            let _ = gty;
            return Ok(DataSet::ImageData(ImageData {
                extent,
                origin,
                spacing,
                direction: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                point_data: pd,
                cell_data: cd,
                field_data: fd,
            }));
        }
        if tl.contains("rectmesh") {
            let mut v: Vec<Vec<f64>> = items.iter().map(|i| data(i, cx).map(|d| d.0)).collect::<Result<_>>()?;
            while v.len() < 3 {
                v.push(vec![0.0]);
            }
            return Ok(DataSet::RectilinearGrid(RectilinearGrid {
                extent,
                x: v[0].clone(),
                y: v[1].clone(),
                z: v[2].clone(),
                point_data: pd,
                cell_data: cd,
                field_data: fd,
            }));
        }
        let pts = geometry(geom, cx)?;
        return Ok(DataSet::StructuredGrid(StructuredGrid { extent, points: Points::new_f64(pts), point_data: pd, cell_data: cd, field_data: fd }));
    }
    let pts = geometry(geom, cx)?;
    let np = pts.len() / 3;
    let base: i64 = topo.attr("BaseOffset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let item = topo.kid("DataItem").ok_or_else(|| anyhow!("topology without data"))?;
    let (conn, cd_dims) = data(item, cx)?;
    let conn: Vec<i64> = conn.into_iter().map(|v| v as i64 - base).collect();
    let mut cells = Cells::new();
    let check = |ids: &[i64]| -> Result<Vec<u32>> {
        ids.iter().map(|&i| if i >= 0 && (i as usize) < np { Ok(i as u32) } else { Err(anyhow!("node {i} out of range")) }).collect()
    };
    if tl == "mixed" {
        let mut k = 0;
        let mut faces: Vec<Vec<Vec<u32>>> = Vec::new();
        while k < conn.len() {
            if conn[k] == 16 {
                // Polyhedron: face count, then each face's node count and nodes.
                let nf = *conn.get(k + 1).ok_or_else(|| anyhow!("truncated mixed topology"))? as usize;
                k += 2;
                let mut fs = Vec::with_capacity(nf);
                let mut all: Vec<u32> = Vec::new();
                for _ in 0..nf {
                    let nn = *conn.get(k).ok_or_else(|| anyhow!("truncated mixed topology"))? as usize;
                    let ids = check(conn.get(k + 1..k + 1 + nn).ok_or_else(|| anyhow!("truncated mixed topology"))?)?;
                    k += 1 + nn;
                    all.extend(&ids);
                    fs.push(ids);
                }
                all.sort_unstable();
                all.dedup();
                cells.push(CellType::Polyhedron, &all);
                faces.resize(cells.len() - 1, Vec::new());
                faces.push(fs);
                continue;
            }
            let (n, ty) = mixed_type(conn[k]).ok_or_else(|| anyhow!("unknown Xdmf cell code {}", conn[k]))?;
            k += 1;
            let n = match n {
                Some(n) => n,
                None => {
                    k += 1;
                    conn[k - 1] as usize
                }
            };
            let ids = check(conn.get(k..k + n).ok_or_else(|| anyhow!("truncated mixed topology"))?)?;
            k += n;
            let ty = match (ty, n) {
                (CellType::PolyLine, 2) => CellType::Line,
                (CellType::PolyVertex, 1) => CellType::Vertex,
                (t, _) => t,
            };
            cells.push(ty, &ids);
        }
        if !faces.is_empty() {
            faces.resize(cells.len(), Vec::new());
            cells.polyhedron_faces = Some(faces);
        }
    } else {
        let (npe, ty) = topo_type(&tname).ok_or_else(|| anyhow!("topology type {tname} is not supported"))?;
        let npe = topo.attr("NodesPerElement").and_then(|v| v.parse().ok()).or_else(|| (cd_dims.len() == 2).then(|| cd_dims[1])).unwrap_or(npe);
        let ty = match (tl.as_str(), npe) {
            ("polyline", n) if n > 2 => CellType::PolyLine,
            ("polyvertex", n) if n > 1 => CellType::PolyVertex,
            ("polygon", _) => CellType::Polygon,
            _ => ty,
        };
        for e in conn.chunks_exact(npe.max(1)) {
            cells.push(ty, &check(e)?);
        }
    }
    let nc = cells.len();
    let (pd, cd, fd) = attributes(grid, cx, np, nc)?;
    Ok(DataSet::UnstructuredGrid(UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: pd, cell_data: cd, field_data: fd }))
}

fn is_collection(g: &El) -> bool {
    g.attr("GridType").is_some_and(|t| t.eq_ignore_ascii_case("Collection") || t.eq_ignore_ascii_case("Tree"))
}

fn is_temporal(g: &El) -> bool {
    is_collection(g) && g.attr("CollectionType").is_some_and(|t| t.eq_ignore_ascii_case("Temporal"))
}

fn read_grid(g: &El, cx: &mut Ctx) -> Result<DataSet> {
    if is_temporal(g) {
        // A temporal collection read as one dataset: its first step.
        let first = g.kids("Grid").next().ok_or_else(|| anyhow!("empty temporal collection"))?;
        return read_grid(first, cx);
    }
    if is_collection(g) {
        let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
        for (i, c) in g.kids("Grid").enumerate() {
            let name = c.attr("Name").map(|s| s.to_string()).unwrap_or_else(|| format!("Block {i}"));
            mb.blocks.push((name, Some(read_grid(c, cx)?)));
        }
        return Ok(DataSet::MultiBlock(mb));
    }
    uniform(g, cx)
}

/// The time steps of a temporal collection: (time, grid).
fn time_steps(g: &El) -> Vec<(f64, &El)> {
    let grids: Vec<&El> = g.kids("Grid").collect();
    // Times on the collection (Xdmf 2: TimeType List) or on each grid.
    let listed: Option<Vec<f64>> = g.kid("Time").and_then(|t| t.kid("DataItem")).map(|d| d.text.split_whitespace().filter_map(|v| v.parse().ok()).collect());
    grids
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let t = c
                .kid("Time")
                .and_then(|t| t.attr("Value"))
                .and_then(|v| v.parse().ok())
                .or_else(|| listed.as_ref().and_then(|l| l.get(i).copied()))
                .unwrap_or(i as f64);
            (t, *c)
        })
        .collect()
}

fn load(path: &Path) -> Result<(El, Ctx)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root = parse_xml(&text)?;
    let resolved = resolve(&root, &root, 0);
    Ok((resolved, Ctx { dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(), hdf: HashMap::new() }))
}

/// The domain's grids: a single temporal collection becomes a time series; otherwise all grids
/// of the domain (one, or several as blocks).
pub fn open(path: &Path) -> Result<Loaded> {
    let (root, _) = load(path)?;
    let domain = root.kid("Domain").ok_or_else(|| anyhow!("no <Domain>"))?;
    let grids: Vec<&El> = domain.kids("Grid").collect();
    if grids.is_empty() {
        bail!("{}: the Xdmf domain has no grids", path.display());
    }
    if grids.len() == 1 && is_temporal(grids[0]) {
        let steps = time_steps(grids[0]);
        if steps.len() > 1 {
            return Ok(Loaded::Series(TimeSeries {
                times: steps.iter().map(|s| s.0).collect(),
                steps: (0..steps.len()).map(|i| TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i }).collect(),
            }));
        }
    }
    Ok(Loaded::Static(Arc::new(read_step(path, 0)?)))
}

pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    let (root, mut cx) = load(path)?;
    let domain = root.kid("Domain").ok_or_else(|| anyhow!("no <Domain>"))?;
    let grids: Vec<&El> = domain.kids("Grid").collect();
    if grids.len() == 1 && is_temporal(grids[0]) {
        let steps = time_steps(grids[0]);
        let g = steps.get(step).or(steps.first()).ok_or_else(|| anyhow!("empty temporal collection"))?.1;
        return read_grid(g, &mut cx);
    }
    if grids.len() == 1 {
        return read_grid(grids[0], &mut cx);
    }
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    for (i, g) in grids.iter().enumerate() {
        mb.blocks.push((g.attr("Name").map(|s| s.to_string()).unwrap_or_else(|| format!("Grid {i}")), Some(read_grid(g, &mut cx)?)));
    }
    Ok(DataSet::MultiBlock(mb))
}

pub fn is_xdmf_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "xmf" | "xdmf" | "xdmf3"))
}
