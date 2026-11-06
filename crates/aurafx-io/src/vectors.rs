//! GIS vector files without GDAL: ESRI Shapefiles (.shp + .dbf + .prj), GeoJSON, KML and KMZ,
//! GeoPackage, FlatGeobuf and GML, laid out as ParaView's GDAL vector reader does: polygonal data with points as
//! vertices, lines as polylines and polygon rings as closed polylines, the attributes as cell
//! data (integers as Int32, reals as Double, text as strings). Reader options choose one block
//! per layer (default) or per feature (ParaView's default), which layers, and filled polygons.

use crate::sqlite;
use crate::{option, option_bool, ReaderOption, ReaderOptionKind};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ model

#[derive(Debug, Clone, PartialEq)]
pub enum Geom {
    Point([f64; 3]),
    Line(Vec<[f64; 3]>),
    /// Exterior ring first, then holes; rings are closed (first point repeated last).
    Polygon(Vec<Vec<[f64; 3]>>),
    Collection(Vec<Geom>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// 32-bit integers (and booleans).
    Int,
    /// 64-bit integers (ParaView makes these text; here they stay numbers).
    Int64,
    Real,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub fid: i64,
    pub geom: Option<Geom>,
    pub values: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    pub fields: Vec<(String, FieldType)>,
    pub features: Vec<Feature>,
    /// Coordinate system (WKT or "EPSG:n").
    pub projection: Option<String>,
}

pub fn is_vector_ext(ext: &str) -> bool {
    matches!(ext, "shp" | "geojson" | "kml" | "kmz" | "gpkg" | "fgb" | "gml" | "tab" | "mif")
}

/// A `.json` file that holds GeoJSON.
pub fn is_geojson(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut head = vec![0u8; 4096];
    let n = std::io::Read::read(&mut f, &mut head).unwrap_or(0);
    let s = String::from_utf8_lossy(&head[..n]);
    s.contains("\"FeatureCollection\"") || s.contains("\"Feature\"") || (s.contains("\"coordinates\"") && s.contains("\"type\""))
}

/// Every layer of a vector file.
pub fn read_layers(path: &Path) -> Result<Vec<Layer>> {
    if let Some(dir) = gdb_dir(path) {
        return read_filegdb(&dir);
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "shp" => Ok(vec![read_shapefile(path)?]),
        "geojson" | "json" => Ok(vec![read_geojson(path)?]),
        "kml" => read_kml(path),
        "kmz" => read_kmz(path),
        "gpkg" => read_gpkg(path),
        "fgb" => Ok(vec![read_flatgeobuf(path)?]),
        "gml" => read_gml(path),
        "tab" => Ok(vec![read_mapinfo_tab(path)?]),
        "mif" => Ok(vec![read_mif(path)?]),
        _ => bail!("{}: not a vector file", path.display()),
    }
}

fn layer_names(path: &Path) -> Vec<String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "gpkg" => gpkg_tables(path).map(|t| t.into_iter().map(|(n, _)| n).collect()).unwrap_or_default(),
        "kml" | "kmz" | "gml" => read_layers(path).map(|l| l.into_iter().map(|l| l.name).collect()).unwrap_or_default(),
        _ if gdb_dir(path).is_some() => read_layers(path).map(|l| l.into_iter().map(|l| l.name).collect()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub const PER_LAYER: &str = "One per layer";
pub const PER_FEATURE: &str = "One per feature (ParaView)";
pub const OUTLINES: &str = "Outlines (ParaView)";
pub const FILLED: &str = "Filled";
pub const ALL_LAYERS: &str = "All";

pub fn options(path: &Path) -> Vec<ReaderOption> {
    let mut v = vec![
        ReaderOption::new(
            "blocks",
            "Blocks",
            ReaderOptionKind::Choice(vec![PER_LAYER.into(), PER_FEATURE.into()]),
            PER_LAYER,
            "One block per layer with a row of attributes per feature, or one block per feature as ParaView does.",
        ),
        ReaderOption::new(
            "polygons",
            "Polygons",
            ReaderOptionKind::Choice(vec![OUTLINES.into(), FILLED.into()]),
            OUTLINES,
            "Polygon outlines as lines (as ParaView), or filled areas (triangles, holes left open) to color by an attribute.",
        ),
        ReaderOption::new("feature_ids", "Feature ids", ReaderOptionKind::Bool, false, "Add each feature's id as the cell array _vtkPedigreeIds."),
    ];
    let names = layer_names(path);
    if names.len() > 1 {
        let mut c = vec![ALL_LAYERS.to_string()];
        c.extend(names);
        v.insert(0, ReaderOption::new("layer", "Layer", ReaderOptionKind::Choice(c), ALL_LAYERS, "Read every layer or only one."));
    }
    v
}

/// Read a vector file with the reader options in effect.
pub fn read(path: &Path) -> Result<DataSet> {
    let mut layers = read_layers(path)?;
    let want = option("layer").filter(|l| !l.is_empty() && l != ALL_LAYERS);
    if let Some(w) = &want {
        layers.retain(|l| &l.name == w);
        if layers.is_empty() {
            bail!("{}: no layer {w}", path.display());
        }
    }
    let per_feature = option("blocks").is_some_and(|b| b == PER_FEATURE);
    let filled = option("polygons").is_some_and(|b| b == FILLED);
    let ids = option_bool("feature_ids", false);
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    for layer in &layers {
        if per_feature {
            for f in &layer.features {
                if let Some(pd) = build(layer, std::slice::from_ref(f), filled, ids, true) {
                    mb.blocks.push((format!("{} {}", layer.name, f.fid), Some(DataSet::PolyData(pd))));
                }
            }
        } else if let Some(pd) = build(layer, &layer.features, filled, ids, false) {
            mb.blocks.push((layer.name.clone(), Some(DataSet::PolyData(pd))));
        }
    }
    if let Some(p) = layers.iter().find_map(|l| l.projection.clone()) {
        mb.field_data.insert(Array::new("MAP_PROJECTION", 1, ArrayData::String(vec![p])));
    }
    Ok(DataSet::MultiBlock(mb))
}

/// Polygonal data for some features of a layer (None when none has geometry, unless
/// `keep_empty`: then an empty block with empty attribute arrays, as ParaView gives a feature
/// without geometry in its one-block-per-feature output).
fn build(layer: &Layer, features: &[Feature], filled: bool, ids: bool, keep_empty: bool) -> Option<PolyData> {
    let mut pts: Vec<f64> = Vec::new();
    let (mut verts, mut lines, mut polys) = (Cells::new(), Cells::new(), Cells::new());
    // Feature index of each cell, per kind (cell data is ordered verts, lines, polys).
    let (mut fv, mut fl, mut fp) = (Vec::new(), Vec::new(), Vec::new());
    let add = |p: [f64; 3], pts: &mut Vec<f64>| {
        pts.extend_from_slice(&p);
        (pts.len() / 3 - 1) as u32
    };
    fn walk(g: &Geom, out: &mut Vec<Geom>) {
        match g {
            Geom::Collection(v) => v.iter().for_each(|x| walk(x, out)),
            g => out.push(g.clone()),
        }
    }
    for (k, f) in features.iter().enumerate() {
        let Some(g) = &f.geom else { continue };
        let mut parts = Vec::new();
        walk(g, &mut parts);
        for part in parts {
            match part {
                Geom::Point(p) => {
                    let i = add(p, &mut pts);
                    verts.push(CellType::Vertex, &[i]);
                    fv.push(k);
                }
                Geom::Line(l) if l.len() >= 2 => {
                    let ids: Vec<u32> = l.iter().map(|&p| add(p, &mut pts)).collect();
                    lines.push(if ids.len() == 2 { CellType::Line } else { CellType::PolyLine }, &ids);
                    fl.push(k);
                }
                Geom::Line(_) => {}
                Geom::Polygon(rings) if filled => {
                    let base = (pts.len() / 3) as u32;
                    let mut flat: Vec<[f64; 3]> = Vec::new();
                    let mut ring_starts = Vec::new();
                    for r in &rings {
                        let r = open_ring(r);
                        if r.len() < 3 {
                            continue;
                        }
                        ring_starts.push(flat.len());
                        flat.extend_from_slice(r);
                    }
                    if ring_starts.is_empty() {
                        continue;
                    }
                    for &p in &flat {
                        add(p, &mut pts);
                    }
                    for t in triangulate(&flat, &ring_starts) {
                        polys.push(CellType::Triangle, &[base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]);
                        fp.push(k);
                    }
                }
                Geom::Polygon(rings) => {
                    for r in rings.iter().filter(|r| r.len() >= 2) {
                        let ids: Vec<u32> = r.iter().map(|&p| add(p, &mut pts)).collect();
                        lines.push(if ids.len() == 2 { CellType::Line } else { CellType::PolyLine }, &ids);
                        fl.push(k);
                    }
                }
                Geom::Collection(_) => unreachable!(),
            }
        }
    }
    if verts.is_empty() && lines.is_empty() && polys.is_empty() && !keep_empty {
        return None;
    }
    let order: Vec<usize> = fv.into_iter().chain(fl).chain(fp).collect();
    let mut pd = PolyData { points: Points::new_f64(pts), verts, lines, polys, ..Default::default() };
    for (c, (name, ty)) in layer.fields.iter().enumerate() {
        let val = |k: usize| features[k].values.get(c).cloned().unwrap_or(Value::Null);
        let data = match ty {
            FieldType::Int => ArrayData::I32(order.iter().map(|&k| as_i64(&val(k)) as i32).collect()),
            FieldType::Int64 => ArrayData::I64(order.iter().map(|&k| as_i64(&val(k))).collect()),
            FieldType::Real => ArrayData::F64(order.iter().map(|&k| as_f64(&val(k))).collect()),
            FieldType::Text => ArrayData::String(order.iter().map(|&k| as_text(&val(k))).collect()),
        };
        pd.cell_data.insert(Array::new(name.clone(), 1, data));
    }
    if ids {
        pd.cell_data.insert_active(AttributeKind::PedigreeIds, Array::new("_vtkPedigreeIds", 1, ArrayData::I64(order.iter().map(|&k| features[k].fid).collect())));
    }
    Some(pd)
}

fn as_i64(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Real(r) => *r as i64,
        Value::Text(s) => s.trim().parse().unwrap_or(0),
        Value::Null => 0,
    }
}
fn as_f64(v: &Value) -> f64 {
    match v {
        Value::Int(i) => *i as f64,
        Value::Real(r) => *r,
        Value::Text(s) => s.trim().parse().unwrap_or(0.0),
        Value::Null => 0.0,
    }
}
fn as_text(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Real(r) => r.to_string(),
        Value::Text(s) => s.clone(),
        Value::Null => String::new(),
    }
}

fn open_ring(r: &[[f64; 3]]) -> &[[f64; 3]] {
    if r.len() > 1 && r[0] == r[r.len() - 1] {
        &r[..r.len() - 1]
    } else {
        r
    }
}

// ------------------------------------------------------------------ filling polygons

/// Triangles (indices into `pts`) covering a polygon with holes: `starts[0]` begins the outer
/// ring, the other starts begin holes. Holes are joined to the outer ring by bridges and the
/// result is cut into ears (in the x-y plane).
pub fn triangulate(pts: &[[f64; 3]], starts: &[usize]) -> Vec<[usize; 3]> {
    let ring = |r: usize| {
        let s = starts[r];
        let e = starts.get(r + 1).copied().unwrap_or(pts.len());
        (s..e).collect::<Vec<usize>>()
    };
    let area = |idx: &[usize]| -> f64 {
        let mut a = 0.0;
        for k in 0..idx.len() {
            let (p, q) = (pts[idx[k]], pts[idx[(k + 1) % idx.len()]]);
            a += p[0] * q[1] - q[0] * p[1];
        }
        a / 2.0
    };
    let mut outer = ring(0);
    if area(&outer) < 0.0 {
        outer.reverse();
    }
    // Holes clockwise, joined from their rightmost point, rightmost hole first.
    let mut holes: Vec<Vec<usize>> = (1..starts.len())
        .map(|r| {
            let mut h = ring(r);
            if area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .filter(|h| h.len() >= 3)
        .collect();
    let right = |h: &Vec<usize>| h.iter().map(|&i| pts[i][0]).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| right(b).total_cmp(&right(a)));
    for h in holes {
        let (hk, &hp) = h.iter().enumerate().max_by(|a, b| pts[*a.1][0].total_cmp(&pts[*b.1][0])).unwrap();
        let m = pts[hp];
        // The closest outer vertex the bridge to which crosses no edge.
        let mut best: Option<(f64, usize)> = None;
        for (k, &o) in outer.iter().enumerate() {
            let q = pts[o];
            let d = (q[0] - m[0]).powi(2) + (q[1] - m[1]).powi(2);
            if best.is_some_and(|(bd, _)| d >= bd) {
                continue;
            }
            let crosses = (0..outer.len()).any(|j| {
                let (a, b) = (outer[j], outer[(j + 1) % outer.len()]);
                a != o && b != o && segments_cross(m, q, pts[a], pts[b])
            }) || (0..h.len()).any(|j| {
                let (a, b) = (h[j], h[(j + 1) % h.len()]);
                a != hp && b != hp && segments_cross(m, q, pts[a], pts[b])
            });
            if !crosses {
                best = Some((d, k));
            }
        }
        let k = best.map(|b| b.1).unwrap_or(0);
        let mut merged = Vec::with_capacity(outer.len() + h.len() + 2);
        merged.extend_from_slice(&outer[..=k]);
        merged.extend(h[hk..].iter().chain(&h[..hk]));
        merged.push(hp);
        merged.extend_from_slice(&outer[k..]);
        outer = merged;
    }
    ear_clip(pts, outer)
}

fn cross2(o: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn segments_cross(p1: [f64; 3], p2: [f64; 3], q1: [f64; 3], q2: [f64; 3]) -> bool {
    let d1 = cross2(q1, q2, p1);
    let d2 = cross2(q1, q2, p2);
    let d3 = cross2(p1, p2, q1);
    let d4 = cross2(p1, p2, q2);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn ear_clip(pts: &[[f64; 3]], mut poly: Vec<usize>) -> Vec<[usize; 3]> {
    let mut tris = Vec::new();
    // Each pass removes one corner, so this many passes always suffice.
    let mut passes = poly.len();
    while poly.len() > 3 && passes > 0 {
        passes -= 1;
        let n = poly.len();
        let mut cut = None;
        for k in 0..n {
            let (a, b, c) = (poly[(k + n - 1) % n], poly[k], poly[(k + 1) % n]);
            if cross2(pts[a], pts[b], pts[c]) <= 0.0 {
                continue; // reflex or flat
            }
            let inside = poly.iter().any(|&p| {
                if p == a || p == b || p == c || pts[p] == pts[a] || pts[p] == pts[b] || pts[p] == pts[c] {
                    return false;
                }
                let x = pts[p];
                cross2(pts[a], pts[b], x) >= 0.0 && cross2(pts[b], pts[c], x) >= 0.0 && cross2(pts[c], pts[a], x) >= 0.0
            });
            if !inside {
                cut = Some(k);
                break;
            }
        }
        // Degenerate input: cut the flattest corner rather than loop.
        let k = cut.unwrap_or_else(|| {
            (0..n)
                .max_by(|&i, &j| {
                    let f = |k: usize| cross2(pts[poly[(k + n - 1) % n]], pts[poly[k]], pts[poly[(k + 1) % n]]);
                    f(i).total_cmp(&f(j))
                })
                .unwrap()
        });
        let (a, b, c) = (poly[(k + n - 1) % n], poly[k], poly[(k + 1) % n]);
        if cross2(pts[a], pts[b], pts[c]).abs() > 0.0 {
            tris.push([a, b, c]);
        }
        poly.remove(k);
    }
    if poly.len() == 3 && cross2(pts[poly[0]], pts[poly[1]], pts[poly[2]]).abs() > 0.0 {
        tris.push([poly[0], poly[1], poly[2]]);
    }
    tris
}

// ------------------------------------------------------------------ shapefile

fn sibling(path: &Path, ext: &str) -> Option<PathBuf> {
    for e in [ext.to_string(), ext.to_ascii_uppercase()] {
        let p = path.with_extension(e);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn le_f64(b: &[u8], i: usize) -> f64 {
    f64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
fn le_i32(b: &[u8], i: usize) -> i32 {
    i32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn be_i32(b: &[u8], i: usize) -> i32 {
    i32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}

pub fn read_shapefile(path: &Path) -> Result<Layer> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if b.len() < 100 || be_i32(&b, 0) != 9994 {
        bail!("{} is not a shapefile", path.display());
    }
    let mut shapes = Vec::new();
    let mut at = 100;
    while at + 8 <= b.len() {
        let len = be_i32(&b, at + 4) as usize * 2;
        let rec = b.get(at + 8..at + 8 + len).ok_or_else(|| anyhow!("{}: truncated record", path.display()))?;
        shapes.push(shape(rec).with_context(|| format!("{}: record {}", path.display(), shapes.len() + 1))?);
        at += 8 + len;
    }
    let (fields, rows) = match sibling(path, "dbf") {
        Some(d) => read_dbf(&d)?,
        None => (Vec::new(), vec![Some(Vec::new()); shapes.len()]),
    };
    let mut features = Vec::new();
    for (k, g) in shapes.into_iter().enumerate() {
        // Deleted rows of the table drop their feature, as GDAL does.
        let values = match rows.get(k) {
            Some(Some(v)) => v.clone(),
            Some(None) => continue,
            None => vec![Value::Null; fields.len()],
        };
        features.push(Feature { fid: k as i64, geom: g, values });
    }
    let projection = sibling(path, "prj").and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("layer").to_string();
    Ok(Layer { name, fields, features, projection })
}

/// One shape record (type and content).
fn shape(r: &[u8]) -> Result<Option<Geom>> {
    if r.len() < 4 {
        return Ok(None);
    }
    let t = le_i32(r, 0);
    let need = |n: usize| if r.len() < n { Err(anyhow!("short shape record")) } else { Ok(()) };
    Ok(match t {
        0 => None,
        1 | 11 | 21 => {
            need(20)?;
            let z = if t == 11 && r.len() >= 28 { le_f64(r, 20) } else { 0.0 };
            Some(Geom::Point([le_f64(r, 4), le_f64(r, 12), z]))
        }
        8 | 18 | 28 => {
            need(40)?;
            let n = le_i32(r, 36) as usize;
            need(40 + 16 * n)?;
            let zs = 40 + 16 * n + 16;
            let has_z = t == 18 && r.len() >= zs + 8 * n;
            Some(Geom::Collection((0..n).map(|i| Geom::Point([le_f64(r, 40 + 16 * i), le_f64(r, 48 + 16 * i), if has_z { le_f64(r, zs + 8 * i) } else { 0.0 }])).collect()))
        }
        3 | 13 | 23 | 5 | 15 | 25 => {
            need(44)?;
            let (np, n) = (le_i32(r, 36) as usize, le_i32(r, 40) as usize);
            let pts_at = 44 + 4 * np;
            need(pts_at + 16 * n)?;
            let zs = pts_at + 16 * n + 16;
            let has_z = (t == 13 || t == 15) && r.len() >= zs + 8 * n;
            let p = |i: usize| [le_f64(r, pts_at + 16 * i), le_f64(r, pts_at + 8 + 16 * i), if has_z { le_f64(r, zs + 8 * i) } else { 0.0 }];
            let parts: Vec<Vec<[f64; 3]>> = (0..np)
                .map(|k| {
                    let s = le_i32(r, 44 + 4 * k).max(0) as usize;
                    let e = if k + 1 < np { le_i32(r, 48 + 4 * k).max(0) as usize } else { n };
                    (s.min(n)..e.min(n)).map(p).collect()
                })
                .collect();
            if matches!(t, 3 | 13 | 23) {
                Some(if parts.len() == 1 { Geom::Line(parts.into_iter().next().unwrap()) } else { Geom::Collection(parts.into_iter().map(Geom::Line).collect()) })
            } else {
                Some(polygons_from_rings(parts))
            }
        }
        31 => None, // multipatch: not read (GDAL makes TINs, which ParaView skips)
        t => bail!("unknown shape type {t}"),
    })
}

/// Shapefile polygons: clockwise rings start polygons, counter-clockwise ones are holes of the
/// polygon before them (rings stay in file order).
fn polygons_from_rings(rings: Vec<Vec<[f64; 3]>>) -> Geom {
    let signed = |r: &[[f64; 3]]| -> f64 { (0..r.len()).map(|k| r[k][0] * r[(k + 1) % r.len()][1] - r[(k + 1) % r.len()][0] * r[k][1]).sum::<f64>() };
    let mut polys: Vec<Vec<Vec<[f64; 3]>>> = Vec::new();
    for r in rings {
        if signed(&r) <= 0.0 || polys.is_empty() {
            polys.push(vec![r]);
        } else {
            polys.last_mut().unwrap().push(r);
        }
    }
    if polys.len() == 1 {
        Geom::Polygon(polys.pop().unwrap())
    } else {
        Geom::Collection(polys.into_iter().map(Geom::Polygon).collect())
    }
}

type Rows = Vec<Option<Vec<Value>>>;

/// dBASE table: field names and types (GDAL's rules) and the rows (None = deleted).
pub fn read_dbf(path: &Path) -> Result<(Vec<(String, FieldType)>, Rows)> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if b.len() < 32 {
        bail!("{}: not a dBASE file", path.display());
    }
    let n = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let hlen = u16::from_le_bytes([b[8], b[9]]) as usize;
    let rlen = u16::from_le_bytes([b[10], b[11]]) as usize;
    let mut defs = Vec::new();
    let mut at = 32;
    while at + 32 <= b.len() && b[at] != 0x0d && at + 32 <= hlen {
        let d = &b[at..at + 32];
        let name = String::from_utf8_lossy(&d[..11]).trim_end_matches('\0').trim().to_string();
        defs.push((name, d[11] as char, d[16] as usize, d[17] as usize));
        at += 32;
    }
    let fields: Vec<(String, FieldType)> = defs
        .iter()
        .map(|(name, t, w, dec)| {
            let ty = match t {
                'N' | 'F' if *dec == 0 && *w < 10 => FieldType::Int,
                'N' | 'F' if *dec == 0 && *w < 19 => FieldType::Int64,
                'N' | 'F' | 'O' => FieldType::Real,
                'L' => FieldType::Int,
                'I' => FieldType::Int,
                '+' => FieldType::Int,
                _ => FieldType::Text,
            };
            (name.clone(), ty)
        })
        .collect();
    let mut rows = Vec::with_capacity(n);
    for r in 0..n {
        let s = hlen + r * rlen;
        let Some(rec) = b.get(s..s + rlen) else { break };
        if rec[0] == b'*' {
            rows.push(None);
            continue;
        }
        let mut off = 1;
        let mut vals = Vec::with_capacity(defs.len());
        for ((_, t, w, _), (_, ty)) in defs.iter().zip(&fields) {
            let raw = rec.get(off..off + w).unwrap_or(&[]);
            off += w;
            let text = String::from_utf8_lossy(raw).trim().to_string();
            let v = match (t, ty) {
                ('I', _) if raw.len() == 4 => Value::Int(i32::from_le_bytes(raw.try_into().unwrap()) as i64),
                ('O', _) if raw.len() == 8 => Value::Real(f64::from_le_bytes(raw.try_into().unwrap())),
                ('L', _) => match text.chars().next() {
                    Some('T' | 't' | 'Y' | 'y') => Value::Int(1),
                    Some('F' | 'f' | 'N' | 'n') => Value::Int(0),
                    _ => Value::Null,
                },
                ('D', _) if text.len() == 8 => Value::Text(format!("{}/{}/{}", &text[..4], &text[4..6], &text[6..8])),
                (_, FieldType::Int | FieldType::Int64) => text.parse::<i64>().map(Value::Int).or_else(|_| text.parse::<f64>().map(|f| Value::Int(f as i64))).unwrap_or(Value::Null),
                (_, FieldType::Real) => text.parse::<f64>().map(Value::Real).unwrap_or(Value::Null),
                _ if text.is_empty() => Value::Null,
                _ => Value::Text(text),
            };
            vals.push(v);
        }
        rows.push(Some(vals));
    }
    Ok((fields, rows))
}

// ------------------------------------------------------------------ GeoJSON

/// Field types from the values seen (GDAL's GeoJSON rules).
#[derive(Default, Clone, Copy)]
struct Seen {
    int32: bool,
    int64: bool,
    real: bool,
    text: bool,
}

impl Seen {
    fn ty(self) -> FieldType {
        if self.text {
            FieldType::Text
        } else if self.real {
            FieldType::Real
        } else if self.int64 {
            FieldType::Int64
        } else {
            FieldType::Int
        }
    }
}

fn json_coord(v: &serde_json::Value) -> Result<[f64; 3]> {
    let a = v.as_array().ok_or_else(|| anyhow!("a position is not an array"))?;
    let g = |k: usize| a.get(k).and_then(|x| x.as_f64());
    Ok([g(0).ok_or_else(|| anyhow!("bad position"))?, g(1).ok_or_else(|| anyhow!("bad position"))?, g(2).unwrap_or(0.0)])
}

fn json_line(v: &serde_json::Value) -> Result<Vec<[f64; 3]>> {
    v.as_array().ok_or_else(|| anyhow!("coordinates are not an array"))?.iter().map(json_coord).collect()
}

fn json_geom(g: &serde_json::Value) -> Result<Option<Geom>> {
    if g.is_null() {
        return Ok(None);
    }
    let t = g.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let c = g.get("coordinates").unwrap_or(&serde_json::Value::Null);
    let list = |v: &serde_json::Value| v.as_array().cloned().unwrap_or_default();
    Ok(Some(match t {
        "Point" => Geom::Point(json_coord(c)?),
        "MultiPoint" => Geom::Collection(list(c).iter().map(|p| json_coord(p).map(Geom::Point)).collect::<Result<_>>()?),
        "LineString" => Geom::Line(json_line(c)?),
        "MultiLineString" => Geom::Collection(list(c).iter().map(|l| json_line(l).map(Geom::Line)).collect::<Result<_>>()?),
        "Polygon" => Geom::Polygon(list(c).iter().map(json_line).collect::<Result<_>>()?),
        "MultiPolygon" => Geom::Collection(list(c).iter().map(|p| list(p).iter().map(json_line).collect::<Result<_>>().map(Geom::Polygon)).collect::<Result<_>>()?),
        "GeometryCollection" => Geom::Collection(list(g.get("geometries").unwrap_or(&serde_json::Value::Null)).iter().filter_map(|x| json_geom(x).transpose()).collect::<Result<_>>()?),
        t => bail!("unknown GeoJSON geometry type {t:?}"),
    }))
}

pub fn read_geojson(path: &Path) -> Result<Layer> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root: serde_json::Value = serde_json::from_str(&text).with_context(|| format!("{}: not valid JSON", path.display()))?;
    let feats: Vec<serde_json::Value> = match root.get("type").and_then(|t| t.as_str()) {
        Some("FeatureCollection") => root.get("features").and_then(|f| f.as_array()).cloned().unwrap_or_default(),
        Some("Feature") => vec![root.clone()],
        Some(_) => vec![serde_json::json!({"type": "Feature", "properties": {}, "geometry": root})],
        None => bail!("{}: not GeoJSON (no \"type\")", path.display()),
    };
    // Property names in file order (serde_json's maps are sorted).
    #[derive(serde::Deserialize)]
    struct Keys {
        #[serde(default)]
        features: Vec<Keys>,
        #[serde(default)]
        properties: Option<indexmap::IndexMap<String, serde::de::IgnoredAny>>,
    }
    let keys: Keys = serde_json::from_str(&text).unwrap_or(Keys { features: Vec::new(), properties: None });
    let key_lists: Vec<Vec<String>> = if keys.features.is_empty() {
        vec![keys.properties.map(|p| p.into_keys().collect()).unwrap_or_default()]
    } else {
        keys.features.into_iter().map(|f| f.properties.map(|p| p.into_keys().collect()).unwrap_or_default()).collect()
    };
    // Field names in order of first appearance, typed from all values.
    let mut names: Vec<String> = Vec::new();
    let mut seen: Vec<Seen> = Vec::new();
    for (fi, f) in feats.iter().enumerate() {
        if let Some(props) = f.get("properties").and_then(|p| p.as_object()) {
            let order: Vec<&String> = match key_lists.get(fi) {
                Some(l) if l.len() == props.len() => l.iter().collect(),
                _ => props.keys().collect(),
            };
            for k in order {
                let v = &props[k.as_str()];
                let i = match names.iter().position(|n| n == k) {
                    Some(i) => i,
                    None => {
                        names.push(k.clone());
                        seen.push(Seen::default());
                        names.len() - 1
                    }
                };
                let s = &mut seen[i];
                match v {
                    serde_json::Value::Null => {}
                    serde_json::Value::Bool(_) => s.int32 = true,
                    serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => {
                        let x = n.as_i64().unwrap_or(i64::MAX);
                        if i32::try_from(x).is_ok() {
                            s.int32 = true
                        } else {
                            s.int64 = true
                        }
                    }
                    serde_json::Value::Number(_) => s.real = true,
                    _ => s.text = true,
                }
            }
        }
    }
    let fields: Vec<(String, FieldType)> = names.iter().cloned().zip(seen.iter().map(|s| s.ty())).collect();
    let mut features = Vec::with_capacity(feats.len());
    for (k, f) in feats.iter().enumerate() {
        let props = f.get("properties").and_then(|p| p.as_object());
        let values = fields
            .iter()
            .map(|(n, ty)| match props.and_then(|p| p.get(n)) {
                None | Some(serde_json::Value::Null) => Value::Null,
                Some(serde_json::Value::Bool(b)) => Value::Int(*b as i64),
                Some(serde_json::Value::Number(x)) if *ty == FieldType::Real => Value::Real(x.as_f64().unwrap_or(0.0)),
                Some(serde_json::Value::Number(x)) if *ty == FieldType::Text => Value::Text(x.to_string()),
                Some(serde_json::Value::Number(x)) => Value::Int(x.as_i64().unwrap_or(0)),
                Some(serde_json::Value::String(s)) => Value::Text(s.clone()),
                Some(v) => Value::Text(v.to_string()),
            })
            .collect();
        let fid = f.get("id").and_then(|i| i.as_i64()).unwrap_or(k as i64);
        let geom = json_geom(f.get("geometry").unwrap_or(&serde_json::Value::Null)).with_context(|| format!("{}: feature {k}", path.display()))?;
        features.push(Feature { fid, geom, values });
    }
    let name = root.get("name").and_then(|n| n.as_str()).map(str::to_string).unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("layer").to_string());
    // Old-style named crs ("urn:ogc:def:crs:EPSG::3857"); RFC 7946 files are WGS 84.
    let projection = root
        .pointer("/crs/properties/name")
        .and_then(|n| n.as_str())
        .map(|n| match n.rsplit_once("EPSG::").or_else(|| n.rsplit_once("EPSG:")) {
            Some((_, code)) => format!("EPSG:{code}"),
            None if n.contains("CRS84") => "EPSG:4326".to_string(),
            None => n.to_string(),
        })
        .or(Some("EPSG:4326".to_string()));
    Ok(Layer { name, fields, features, projection })
}

// ------------------------------------------------------------------ KML

/// KML: a layer per Folder (placemarks outside folders form a layer named after the file),
/// fields Name and Description plus the extended data (typed by the document's Schemas).
pub fn read_kml(path: &Path) -> Result<Vec<Layer>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    kml_layers(&text, path)
}

/// KMZ: a zip archive holding a KML document (doc.kml, or the first .kml in it).
pub fn read_kmz(path: &Path) -> Result<Vec<Layer>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let entries = zip_entries(&bytes).with_context(|| format!("{}: not a zip archive", path.display()))?;
    let pick = entries
        .iter()
        .find(|e| e.0.eq_ignore_ascii_case("doc.kml"))
        .or_else(|| entries.iter().find(|e| e.0.to_ascii_lowercase().ends_with(".kml")))
        .ok_or_else(|| anyhow!("{}: no .kml document in the archive", path.display()))?;
    let data = zip_extract(&bytes, pick).with_context(|| format!("{}: {}", path.display(), pick.0))?;
    let text = String::from_utf8(data).map_err(|_| anyhow!("{}: {} is not UTF-8 text", path.display(), pick.0))?;
    kml_layers(&text, path)
}

/// A zip entry: name, compression method, compressed size, offset of its local header.
type ZipEntry = (String, u16, usize, usize);

/// The entries of a zip archive (from its central directory).
fn zip_entries(b: &[u8]) -> Result<Vec<ZipEntry>> {
    let u16_at = |i: usize| -> Result<u16> { Ok(u16::from_le_bytes(b.get(i..i + 2).ok_or_else(|| anyhow!("truncated"))?.try_into().unwrap())) };
    let u32_at = |i: usize| -> Result<u32> { Ok(u32::from_le_bytes(b.get(i..i + 4).ok_or_else(|| anyhow!("truncated"))?.try_into().unwrap())) };
    let eocd = (0..b.len().saturating_sub(21)).rev().take(65557).find(|&i| b[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or_else(|| anyhow!("no end of central directory"))?;
    let count = u16_at(eocd + 10)? as usize;
    let mut at = u32_at(eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            bail!("bad central directory entry");
        }
        let method = u16_at(at + 10)?;
        let size = u32_at(at + 20)? as usize;
        let (n, e, c) = (u16_at(at + 28)? as usize, u16_at(at + 30)? as usize, u16_at(at + 32)? as usize);
        let local = u32_at(at + 42)? as usize;
        let name = String::from_utf8_lossy(b.get(at + 46..at + 46 + n).ok_or_else(|| anyhow!("truncated"))?).into_owned();
        out.push((name, method, size, local));
        at += 46 + n + e + c;
    }
    Ok(out)
}

fn zip_extract(b: &[u8], e: &ZipEntry) -> Result<Vec<u8>> {
    let (_, method, size, local) = e;
    let le16 = |i: usize| b.get(i..i + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize).ok_or_else(|| anyhow!("truncated"));
    if b.get(*local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04][..]) {
        bail!("bad local header");
    }
    let start = local + 30 + le16(local + 26)? + le16(local + 28)?;
    let data = b.get(start..start + size).ok_or_else(|| anyhow!("truncated entry"))?;
    match method {
        0 => Ok(data.to_vec()),
        8 => {
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(data), &mut out)?;
            Ok(out)
        }
        m => bail!("compression method {m} is not supported (stored and deflate are)"),
    }
}

fn kml_layers(text: &str, path: &Path) -> Result<Vec<Layer>> {
    use crate::xdmf::El;
    let root = crate::xdmf::parse_dom(text).with_context(|| format!("{}: not valid XML", path.display()))?;
    fn local(n: &str) -> &str {
        n.rsplit(':').next().unwrap_or(n)
    }
    fn all<'a>(e: &'a El, name: &str, out: &mut Vec<&'a El>) {
        for c in &e.children {
            if local(&c.name) == name {
                out.push(c);
            }
            all(c, name, out);
        }
    }
    let kid = |e: &El, n: &str| e.children.iter().find(|c| local(&c.name) == n).map(|c| c.text.trim().to_string());
    // Schema field types.
    let mut schema_types: std::collections::HashMap<String, FieldType> = std::collections::HashMap::new();
    let mut fields_el = Vec::new();
    all(&root, "SimpleField", &mut fields_el);
    for f in fields_el {
        if let Some(n) = f.attr("name") {
            schema_types.insert(n.to_string(), kml_type(&f.attr("type").unwrap_or("").to_ascii_lowercase()));
        }
    }
    fn coords(e: &El) -> Vec<[f64; 3]> {
        let t = e.children.iter().find(|c| c.name.rsplit(':').next() == Some("coordinates")).map(|c| c.text.as_str()).unwrap_or("");
        t.split_whitespace()
            .filter_map(|t| {
                let v: Vec<f64> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                (v.len() >= 2).then(|| [v[0], v[1], v.get(2).copied().unwrap_or(0.0)])
            })
            .collect()
    }
    fn geom(e: &El) -> Option<Geom> {
        match e.name.rsplit(':').next().unwrap_or("") {
            "Point" => coords(e).first().map(|&p| Geom::Point(p)),
            "LineString" | "LinearRing" => Some(Geom::Line(coords(e))),
            "Polygon" => {
                let mut rings = Vec::new();
                for b in ["outerBoundaryIs", "innerBoundaryIs"] {
                    for bound in e.children.iter().filter(|c| c.name.rsplit(':').next() == Some(b)) {
                        for r in bound.children.iter().filter(|c| c.name.rsplit(':').next() == Some("LinearRing")) {
                            rings.push(coords(r));
                        }
                    }
                }
                Some(Geom::Polygon(rings))
            }
            "MultiGeometry" => Some(Geom::Collection(e.children.iter().filter_map(geom).collect())),
            _ => None,
        }
    }
    // Placemarks grouped by their closest Folder.
    let mut groups: Vec<(String, Vec<&El>)> = Vec::new();
    fn walk<'a>(e: &'a El, folder: usize, groups: &mut Vec<(String, Vec<&'a El>)>) {
        for c in &e.children {
            match c.name.rsplit(':').next().unwrap_or("") {
                "Folder" => {
                    let name = c.children.iter().find(|x| x.name.rsplit(':').next() == Some("name")).map(|x| x.text.trim().to_string());
                    groups.push((name.unwrap_or_else(|| format!("Layer {}", groups.len())), Vec::new()));
                    let k = groups.len() - 1;
                    walk(c, k, groups);
                }
                "Placemark" => groups[folder].1.push(c),
                _ => walk(c, folder, groups),
            }
        }
    }
    groups.push((path.file_stem().and_then(|s| s.to_str()).unwrap_or("layer").to_string(), Vec::new()));
    walk(&root, 0, &mut groups);
    let mut out = Vec::new();
    for (name, items) in groups.into_iter().filter(|(_, i)| !i.is_empty()) {
        let data = |p: &El| -> Vec<(String, String)> {
            let mut v = Vec::new();
            let mut sd = Vec::new();
            all(p, "SimpleData", &mut sd);
            v.extend(sd.into_iter().filter_map(|d| d.attr("name").map(|n| (n.to_string(), d.text.trim().to_string()))));
            let mut dd = Vec::new();
            all(p, "Data", &mut dd);
            v.extend(dd.into_iter().filter_map(|d| d.attr("name").map(|n| (n.to_string(), kid(d, "value").unwrap_or_default()))));
            v
        };
        let mut fields = vec![("Name".to_string(), FieldType::Text), ("Description".to_string(), FieldType::Text)];
        for p in &items {
            for (k, _) in data(p) {
                if !fields.iter().any(|(n, _)| *n == k) {
                    let ty = schema_types.get(&k).copied().unwrap_or(FieldType::Text);
                    fields.push((k, ty));
                }
            }
        }
        let features = items
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let d = data(p);
                let values = fields
                    .iter()
                    .enumerate()
                    .map(|(i, (n, ty))| {
                        let raw = match i {
                            0 => kid(p, "name"),
                            1 => kid(p, "description"),
                            _ => d.iter().find(|(x, _)| x == n).map(|(_, v)| v.clone()),
                        };
                        match (raw, ty) {
                            (None, _) => Value::Null,
                            (Some(s), FieldType::Int | FieldType::Int64) => s.trim().parse().map(Value::Int).unwrap_or(Value::Null),
                            (Some(s), FieldType::Real) => s.trim().parse().map(Value::Real).unwrap_or(Value::Null),
                            (Some(s), FieldType::Text) => Value::Text(s),
                        }
                    })
                    .collect();
                let gs: Vec<Geom> = p.children.iter().filter_map(geom).collect();
                let geom = match gs.len() {
                    0 => None,
                    1 => gs.into_iter().next(),
                    _ => Some(Geom::Collection(gs)),
                };
                Feature { fid: k as i64, geom, values }
            })
            .collect();
        out.push(Layer { name, fields, features, projection: Some("EPSG:4326".into()) });
    }
    Ok(out)
}

fn kml_type(t: &str) -> FieldType {
    match t {
        "int" | "uint" | "short" | "ushort" | "bool" => FieldType::Int,
        "float" | "double" => FieldType::Real,
        _ => FieldType::Text,
    }
}

// ------------------------------------------------------------------ GeoPackage

/// Feature tables of a GeoPackage: (name, geometry column).
fn gpkg_tables(path: &Path) -> Result<Vec<(String, String)>> {
    let db = sqlite::Database::open(path)?;
    let contents = db.rows("gpkg_contents").context("not a GeoPackage (no gpkg_contents table)")?;
    let ccols = db.columns("gpkg_contents")?;
    let ci = |n: &str| ccols.iter().position(|(c, _)| c.eq_ignore_ascii_case(n));
    let (tn, dt) = (ci("table_name").ok_or_else(|| anyhow!("gpkg_contents has no table_name"))?, ci("data_type").ok_or_else(|| anyhow!("gpkg_contents has no data_type"))?);
    let geo = db.rows("gpkg_geometry_columns")?;
    let gcols = db.columns("gpkg_geometry_columns")?;
    let gi = |n: &str| gcols.iter().position(|(c, _)| c.eq_ignore_ascii_case(n));
    let (gt, gc) = (gi("table_name").unwrap_or(0), gi("column_name").unwrap_or(1));
    let mut out = Vec::new();
    for (_, row) in contents {
        if row.get(dt).and_then(|v| v.as_str()) != Some("features") {
            continue;
        }
        let name = row.get(tn).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let col = geo.iter().find(|(_, g)| g.get(gt).and_then(|v| v.as_str()).is_some_and(|t| t.eq_ignore_ascii_case(&name))).and_then(|(_, g)| g.get(gc).and_then(|v| v.as_str()).map(str::to_string));
        if let Some(c) = col {
            out.push((name, c));
        }
    }
    Ok(out)
}

/// GDAL's field types for GeoPackage column types.
fn gpkg_type(decl: &str) -> Option<FieldType> {
    let t = decl.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    let t = t.split('(').next().unwrap_or("");
    Some(match t {
        "BOOLEAN" | "TINYINT" | "SMALLINT" | "MEDIUMINT" => FieldType::Int,
        "INT" | "INTEGER" => FieldType::Int64,
        "FLOAT" | "DOUBLE" | "REAL" => FieldType::Real,
        "BLOB" => return None,
        _ => FieldType::Text,
    })
}

pub fn read_gpkg(path: &Path) -> Result<Vec<Layer>> {
    let db = sqlite::Database::open(path)?;
    let srs: Vec<(i64, String)> = {
        let cols = db.columns("gpkg_spatial_ref_sys")?;
        let id = cols.iter().position(|(c, _)| c.eq_ignore_ascii_case("srs_id")).unwrap_or(1);
        let def = cols.iter().position(|(c, _)| c.eq_ignore_ascii_case("definition")).unwrap_or(4);
        let org = cols.iter().position(|(c, _)| c.eq_ignore_ascii_case("organization"));
        let code = cols.iter().position(|(c, _)| c.eq_ignore_ascii_case("organization_coordsys_id"));
        db.rows("gpkg_spatial_ref_sys")?
            .into_iter()
            .map(|(_, r)| {
                let epsg = org
                    .and_then(|o| r.get(o).and_then(|v| v.as_str()))
                    .filter(|o| o.eq_ignore_ascii_case("EPSG"))
                    .and_then(|_| code.and_then(|c| r.get(c).and_then(|v| v.as_i64())))
                    .filter(|c| *c > 0)
                    .map(|c| format!("EPSG:{c}"));
                let wkt = r.get(def).and_then(|v| v.as_str()).filter(|s| *s != "undefined").map(str::to_string);
                (r.get(id).and_then(|v| v.as_i64()).unwrap_or(0), epsg.or(wkt).unwrap_or_default())
            })
            .collect()
    };
    let mut layers = Vec::new();
    for (table, gcol) in gpkg_tables(path)? {
        let cols = db.columns(&table)?;
        let gi = cols.iter().position(|(c, _)| c.eq_ignore_ascii_case(&gcol)).ok_or_else(|| anyhow!("{table}: no column {gcol}"))?;
        let pk = cols.iter().position(|(_, t)| t.to_ascii_uppercase().contains("PRIMARY KEY"));
        let fields: Vec<(usize, String, FieldType)> =
            cols.iter().enumerate().filter(|(i, _)| *i != gi && Some(*i) != pk).filter_map(|(i, (n, t))| gpkg_type(t).map(|ty| (i, n.clone(), ty))).collect();
        let mut projection = None;
        let mut features = Vec::new();
        for (rowid, row) in db.rows(&table)? {
            let geom = match row.get(gi) {
                Some(sqlite::Value::Blob(b)) => {
                    let (g, srs_id) = gpkg_geometry(b).with_context(|| format!("{table}: feature {rowid}"))?;
                    if projection.is_none() {
                        projection = srs.iter().find(|(i, _)| *i == srs_id as i64).map(|(_, p)| p.clone()).filter(|p| !p.is_empty());
                    }
                    g
                }
                _ => None,
            };
            let values = fields
                .iter()
                .map(|(i, _, ty)| match (row.get(*i), ty) {
                    (None | Some(sqlite::Value::Null), _) => Value::Null,
                    (Some(v), FieldType::Int | FieldType::Int64) => v.as_i64().map(Value::Int).unwrap_or(Value::Null),
                    (Some(v), FieldType::Real) => v.as_f64().map(Value::Real).unwrap_or(Value::Null),
                    (Some(sqlite::Value::Text(s)), FieldType::Text) => Value::Text(s.clone()),
                    (Some(sqlite::Value::Int(x)), FieldType::Text) => Value::Text(x.to_string()),
                    (Some(sqlite::Value::Real(x)), FieldType::Text) => Value::Text(x.to_string()),
                    (Some(_), FieldType::Text) => Value::Null,
                })
                .collect();
            let fid = pk.and_then(|k| row.get(k).and_then(|v| v.as_i64())).unwrap_or(rowid);
            features.push(Feature { fid, geom, values });
        }
        layers.push(Layer { name: table, fields: fields.into_iter().map(|(_, n, t)| (n, t)).collect(), features, projection });
    }
    Ok(layers)
}

/// A GeoPackage geometry blob: header ("GP", version, flags, srs id, envelope) and WKB.
fn gpkg_geometry(b: &[u8]) -> Result<(Option<Geom>, i32)> {
    if b.len() < 8 || &b[..2] != b"GP" {
        bail!("not a GeoPackage geometry");
    }
    let flags = b[3];
    let le = flags & 1 == 1;
    let srs = if le { i32::from_le_bytes(b[4..8].try_into().unwrap()) } else { i32::from_be_bytes(b[4..8].try_into().unwrap()) };
    let env = [0, 32, 48, 48, 64].get(((flags >> 1) & 7) as usize).copied().unwrap_or(0);
    if flags & 0x10 != 0 {
        return Ok((None, srs));
    }
    let mut at = 8 + env;
    let g = wkb(b, &mut at)?;
    Ok((Some(g), srs))
}

/// Well-known binary (ISO and extended forms; Z kept, M dropped).
pub fn wkb(b: &[u8], at: &mut usize) -> Result<Geom> {
    let take = |at: &mut usize, n: usize| -> Result<&[u8]> {
        let s = b.get(*at..*at + n).ok_or_else(|| anyhow!("truncated WKB"))?;
        *at += n;
        Ok(s)
    };
    let le = take(at, 1)?[0] == 1;
    let u32_ = |at: &mut usize| -> Result<u32> {
        let s = take(at, 4)?;
        Ok(if le { u32::from_le_bytes(s.try_into().unwrap()) } else { u32::from_be_bytes(s.try_into().unwrap()) })
    };
    let f64_ = |at: &mut usize| -> Result<f64> {
        let s = take(at, 8)?;
        Ok(if le { f64::from_le_bytes(s.try_into().unwrap()) } else { f64::from_be_bytes(s.try_into().unwrap()) })
    };
    let code = u32_(at)?;
    let (mut has_z, mut has_m) = (code & 0x8000_0000 != 0, code & 0x4000_0000 != 0);
    if code & 0x2000_0000 != 0 {
        u32_(at)?; // EWKB srid
    }
    let mut t = code & 0x0fff_ffff;
    match t / 1000 {
        1 => has_z = true,
        2 => has_m = true,
        3 => {
            has_z = true;
            has_m = true
        }
        _ => {}
    }
    t %= 1000;
    let point = |at: &mut usize| -> Result<[f64; 3]> {
        let (x, y) = (f64_(at)?, f64_(at)?);
        let z = if has_z { f64_(at)? } else { 0.0 };
        if has_m {
            f64_(at)?;
        }
        Ok([x, y, z])
    };
    let line = |at: &mut usize| -> Result<Vec<[f64; 3]>> {
        let n = u32_(at)? as usize;
        if n > b.len() {
            bail!("bad WKB point count");
        }
        (0..n).map(|_| point(at)).collect()
    };
    Ok(match t {
        1 => Geom::Point(point(at)?),
        2 => Geom::Line(line(at)?),
        3 => {
            let n = u32_(at)? as usize;
            if n > b.len() {
                bail!("bad WKB ring count");
            }
            Geom::Polygon((0..n).map(|_| line(at)).collect::<Result<_>>()?)
        }
        4..=7 => {
            let n = u32_(at)? as usize;
            if n > b.len() {
                bail!("bad WKB part count");
            }
            Geom::Collection((0..n).map(|_| wkb(b, at)).collect::<Result<_>>()?)
        }
        t => bail!("WKB geometry type {t} is not supported"),
    })
}

// ------------------------------------------------------------------ FlatGeobuf

/// A FlatBuffers table: the buffer and the table's position.
#[derive(Clone, Copy)]
struct Fb<'a> {
    b: &'a [u8],
    t: usize,
}

impl<'a> Fb<'a> {
    fn u32_at(b: &[u8], i: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(b.get(i..i + 4).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?.try_into().unwrap()))
    }
    fn root(b: &'a [u8]) -> Result<Self> {
        Ok(Fb { b, t: Self::u32_at(b, 0)? as usize })
    }
    /// Position of field `i`, if present.
    fn field(&self, i: usize) -> Result<Option<usize>> {
        let soff = i32::from_le_bytes(self.b.get(self.t..self.t + 4).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?.try_into().unwrap());
        let vt = (self.t as i64 - soff as i64) as usize;
        let le16 = |i: usize| -> Result<usize> { Ok(u16::from_le_bytes(self.b.get(i..i + 2).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?.try_into().unwrap()) as usize) };
        let vsize = le16(vt)?;
        if 4 + 2 * i + 2 > vsize {
            return Ok(None);
        }
        let off = le16(vt + 4 + 2 * i)?;
        Ok((off != 0).then_some(self.t + off))
    }
    fn u8(&self, i: usize, default: u8) -> Result<u8> {
        Ok(match self.field(i)? {
            Some(p) => *self.b.get(p).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?,
            None => default,
        })
    }
    fn u16(&self, i: usize, default: u16) -> Result<u16> {
        Ok(match self.field(i)? {
            Some(p) => u16::from_le_bytes(self.b.get(p..p + 2).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?.try_into().unwrap()),
            None => default,
        })
    }
    fn i32(&self, i: usize, default: i32) -> Result<i32> {
        Ok(match self.field(i)? {
            Some(p) => Self::u32_at(self.b, p)? as i32,
            None => default,
        })
    }
    fn u64(&self, i: usize) -> Result<u64> {
        Ok(match self.field(i)? {
            Some(p) => u64::from_le_bytes(self.b.get(p..p + 8).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?.try_into().unwrap()),
            None => 0,
        })
    }
    /// Start and length of a vector (or string) field.
    fn vector(&self, i: usize) -> Result<Option<(usize, usize)>> {
        let Some(p) = self.field(i)? else { return Ok(None) };
        let v = p + Self::u32_at(self.b, p)? as usize;
        Ok(Some((v + 4, Self::u32_at(self.b, v)? as usize)))
    }
    fn string(&self, i: usize) -> Result<Option<String>> {
        Ok(match self.vector(i)? {
            Some((s, n)) => Some(String::from_utf8_lossy(self.b.get(s..s + n).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?).into_owned()),
            None => None,
        })
    }
    fn f64s(&self, i: usize) -> Result<Vec<f64>> {
        let Some((s, n)) = self.vector(i)? else { return Ok(Vec::new()) };
        let d = self.b.get(s..s + 8 * n).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?;
        Ok(d.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect())
    }
    fn u32s(&self, i: usize) -> Result<Vec<u32>> {
        let Some((s, n)) = self.vector(i)? else { return Ok(Vec::new()) };
        let d = self.b.get(s..s + 4 * n).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))?;
        Ok(d.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
    }
    fn bytes(&self, i: usize) -> Result<&'a [u8]> {
        let Some((s, n)) = self.vector(i)? else { return Ok(&[]) };
        self.b.get(s..s + n).ok_or_else(|| anyhow!("FlatBuffer out of bounds"))
    }
    fn tables(&self, i: usize) -> Result<Vec<Fb<'a>>> {
        let Some((s, n)) = self.vector(i)? else { return Ok(Vec::new()) };
        (0..n).map(|k| Ok(Fb { b: self.b, t: s + 4 * k + Self::u32_at(self.b, s + 4 * k)? as usize })).collect()
    }
    fn table(&self, i: usize) -> Result<Option<Fb<'a>>> {
        let Some(p) = self.field(i)? else { return Ok(None) };
        Ok(Some(Fb { b: self.b, t: p + Self::u32_at(self.b, p)? as usize }))
    }
}

/// A FlatGeobuf geometry of type `ty` (GeometryType: 1 point ... 7 collection).
fn fgb_geometry(g: &Fb, ty: u8, has_z: bool) -> Result<Option<Geom>> {
    let ty = match g.u8(6, 0)? {
        0 => ty,
        t => t,
    };
    let xy = g.f64s(1)?;
    let z = if has_z { g.f64s(2)? } else { Vec::new() };
    let pts: Vec<[f64; 3]> = (0..xy.len() / 2).map(|k| [xy[2 * k], xy[2 * k + 1], z.get(k).copied().unwrap_or(0.0)]).collect();
    let ends = g.u32s(0)?;
    let split = |pts: &[[f64; 3]]| -> Vec<Vec<[f64; 3]>> {
        if ends.is_empty() {
            return vec![pts.to_vec()];
        }
        let mut start = 0;
        ends.iter()
            .map(|&e| {
                let r = pts[start.min(pts.len())..(e as usize).min(pts.len())].to_vec();
                start = e as usize;
                r
            })
            .collect()
    };
    Ok(Some(match ty {
        1 => match pts.first() {
            Some(&p) => Geom::Point(p),
            None => return Ok(None),
        },
        2 => Geom::Line(pts),
        3 => Geom::Polygon(split(&pts)),
        4 => Geom::Collection(pts.into_iter().map(Geom::Point).collect()),
        5 => Geom::Collection(split(&pts).into_iter().map(Geom::Line).collect()),
        6 => Geom::Collection(g.tables(7)?.iter().map(|p| fgb_geometry(p, 3, has_z)).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect()),
        7 => Geom::Collection(g.tables(7)?.iter().map(|p| fgb_geometry(p, 0, has_z)).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect()),
        t => bail!("FlatGeobuf geometry type {t} is not supported (points, lines, polygons and their collections are)"),
    }))
}

/// FlatGeobuf (`.fgb`): one layer; the header's columns as fields, features in file order
/// (as GDAL reads them; the spatial index is skipped).
pub fn read_flatgeobuf(path: &Path) -> Result<Layer> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if b.len() < 12 || &b[0..3] != b"fgb" || &b[4..7] != b"fgb" {
        bail!("{}: not a FlatGeobuf file", path.display());
    }
    let hsize = Fb::u32_at(&b, 8)? as usize;
    let hb = b.get(12..12 + hsize).ok_or_else(|| anyhow!("{}: truncated header", path.display()))?;
    let h = Fb::root(hb)?;
    let name = h.string(0)?.filter(|n| !n.is_empty()).unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("layer").to_string());
    let gtype = h.u8(2, 0)?;
    let has_z = h.u8(3, 0)? != 0;
    let columns: Vec<(String, u8)> = h.tables(7)?.iter().map(|c| Ok((c.string(0)?.unwrap_or_default(), c.u8(1, 0)?))).collect::<Result<_>>()?;
    let count = h.u64(8)? as usize;
    let node_size = h.u16(9, 16)? as usize;
    let projection = match h.table(10)? {
        Some(c) => {
            let code = c.i32(1, 0)?;
            let org = c.string(0)?.unwrap_or_else(|| "EPSG".into());
            if code != 0 {
                Some(format!("{org}:{code}"))
            } else {
                c.string(4)?
            }
        }
        None => None,
    };
    // Packed Hilbert R-tree: 40-byte nodes on every level.
    let index_size = if node_size > 0 && count > 0 {
        let ns = node_size.clamp(2, 65535);
        let (mut n, mut nodes) = (count, count);
        loop {
            n = n.div_ceil(ns);
            nodes += n;
            if n == 1 {
                break;
            }
        }
        nodes * 40
    } else {
        0
    };
    let fields: Vec<(String, FieldType)> = columns
        .iter()
        .map(|(n, t)| {
            (
                n.clone(),
                match t {
                    0..=2 | 3 | 4 | 5 => FieldType::Int,
                    6 | 7 | 8 => FieldType::Int64,
                    9 | 10 => FieldType::Real,
                    _ => FieldType::Text,
                },
            )
        })
        .collect();
    let mut at = 12 + hsize + index_size;
    let mut features = Vec::new();
    while at + 4 <= b.len() {
        let size = Fb::u32_at(&b, at)? as usize;
        let fbuf = b.get(at + 4..at + 4 + size).ok_or_else(|| anyhow!("{}: truncated feature {}", path.display(), features.len()))?;
        at += 4 + size;
        let f = Fb::root(fbuf)?;
        let geom = match f.table(0)? {
            Some(g) => fgb_geometry(&g, gtype, has_z)?,
            None => None,
        };
        let mut values = vec![Value::Null; fields.len()];
        let props = f.bytes(1)?;
        let mut p = 0;
        while p + 2 <= props.len() {
            let col = u16::from_le_bytes([props[p], props[p + 1]]) as usize;
            p += 2;
            let ty = columns.get(col).map(|c| c.1).ok_or_else(|| anyhow!("{}: property of unknown column {col}", path.display()))?;
            let take = |p: &mut usize, n: usize| -> Result<&[u8]> {
                let s = props.get(*p..*p + n).ok_or_else(|| anyhow!("truncated properties"))?;
                *p += n;
                Ok(s)
            };
            let v = match ty {
                0 => Value::Int(take(&mut p, 1)?[0] as i8 as i64),
                1 | 2 => Value::Int(take(&mut p, 1)?[0] as i64),
                3 => Value::Int(i16::from_le_bytes(take(&mut p, 2)?.try_into().unwrap()) as i64),
                4 => Value::Int(u16::from_le_bytes(take(&mut p, 2)?.try_into().unwrap()) as i64),
                5 => Value::Int(i32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()) as i64),
                6 => Value::Int(u32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()) as i64),
                7 | 8 => Value::Int(i64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap())),
                9 => Value::Real(f32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()) as f64),
                10 => Value::Real(f64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap())),
                _ => {
                    let n = u32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
                    Value::Text(String::from_utf8_lossy(take(&mut p, n)?).into_owned())
                }
            };
            values[col] = v;
        }
        features.push(Feature { fid: features.len() as i64, geom, values });
    }
    Ok(Layer { name, fields, features, projection })
}

// ------------------------------------------------------------------ GML

/// Geographic CRSs whose EPSG definition puts latitude first: GML in the URN/URI forms
/// (urn:ogc:def:crs:EPSG::4326, http://www.opengis.net/def/crs/EPSG/0/4326) follows it, and
/// coordinates are swapped back to longitude, latitude (as GDAL does).
const LAT_FIRST: &[u32] = &[4326, 4258, 4269, 4267, 4283, 4230, 4277, 4612, 6318, 4171, 4674, 4019, 4979, 4937, 4617, 4148, 4152, 4170, 4189, 4190, 4167, 4314, 4807];

fn gml_srs(name: &str) -> (Option<String>, bool) {
    let code = |s: &str| s.trim().parse::<u32>().ok();
    if let Some(c) = name.rsplit_once("EPSG::").and_then(|(_, c)| code(c)).or_else(|| name.split("/EPSG/").nth(1).and_then(|r| r.rsplit('/').next()).and_then(code)) {
        return (Some(format!("EPSG:{c}")), LAT_FIRST.contains(&c));
    }
    if let Some(c) = name.rsplit_once("EPSG:").and_then(|(_, c)| code(c)).or_else(|| name.rsplit_once('#').and_then(|(_, c)| code(c))) {
        return (Some(format!("EPSG:{c}")), false);
    }
    (None, false)
}

/// GML (`.gml`, versions 2 to 3.2 as GDAL writes and reads simple features): a layer per
/// feature type in the collection; fields typed from the `.xsd` schema beside the file when
/// there is one, else from the values. Coordinates in a latitude-first CRS are swapped.
pub fn read_gml(path: &Path) -> Result<Vec<Layer>> {
    use crate::xdmf::El;
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root = crate::xdmf::parse_dom(&text).with_context(|| format!("{}: not valid XML", path.display()))?;
    fn local(n: &str) -> &str {
        n.rsplit(':').next().unwrap_or(n)
    }
    const GEOMS: &[&str] = &[
        "Point", "LineString", "LinearRing", "Polygon", "MultiPoint", "MultiLineString", "MultiCurve", "MultiPolygon", "MultiSurface", "MultiGeometry", "Curve", "Surface", "CompositeCurve", "CompositeSurface",
    ];
    // Field types from the schema: element name -> type, per feature type.
    let mut schema: std::collections::HashMap<String, Vec<(String, FieldType)>> = std::collections::HashMap::new();
    if let Some(xsd) = sibling(path, "xsd").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| crate::xdmf::parse_dom(&t).ok()) {
        for ct in xsd.children.iter().filter(|c| local(&c.name) == "complexType") {
            let Some(tname) = ct.attr("name") else { continue };
            let mut fields = Vec::new();
            fn elements<'a>(e: &'a El, out: &mut Vec<&'a El>) {
                for c in &e.children {
                    if c.name.rsplit(':').next() == Some("element") {
                        out.push(c);
                    } else {
                        elements(c, out);
                    }
                }
            }
            let mut els = Vec::new();
            elements(ct, &mut els);
            for el in els {
                let Some(n) = el.attr("name") else { continue };
                if el.attr("type").is_some_and(|t| t.starts_with("gml:")) {
                    continue;
                }
                let mut base = el.attr("type").map(|t| local(t).to_string()).unwrap_or_default();
                let mut digits = None;
                fn find<'a>(e: &'a El, name: &str) -> Option<&'a El> {
                    e.children.iter().find_map(|c| if c.name.rsplit(':').next() == Some(name) { Some(c) } else { find(c, name) })
                }
                if let Some(r) = find(el, "restriction") {
                    base = r.attr("base").map(|t| local(t).to_string()).unwrap_or_default();
                    digits = find(r, "totalDigits").and_then(|d| d.attr("value")).and_then(|v| v.parse::<u32>().ok());
                }
                let ty = match base.as_str() {
                    "integer" | "int" | "short" | "byte" | "long" | "boolean" => {
                        if base == "long" || digits.is_some_and(|d| d >= 10) {
                            FieldType::Int64
                        } else {
                            FieldType::Int
                        }
                    }
                    "decimal" | "double" | "float" => FieldType::Real,
                    _ => FieldType::Text,
                };
                fields.push((n.to_string(), ty));
            }
            schema.insert(tname.trim_end_matches("_Type").trim_end_matches("Type").to_string(), fields);
        }
    }
    // Feature members (featureMember / featureMembers / member of a collection).
    let mut members: Vec<&El> = Vec::new();
    fn collect<'a>(e: &'a El, out: &mut Vec<&'a El>) {
        for c in &e.children {
            match c.name.rsplit(':').next().unwrap_or("") {
                "featureMember" | "member" => out.extend(c.children.iter()),
                "featureMembers" => out.extend(c.children.iter()),
                "boundedBy" => {}
                _ => collect(c, out),
            }
        }
    }
    collect(&root, &mut members);
    let mut projection = None;
    let mut swap_all = None;
    if let Some(srs) = root.children.iter().find(|c| local(&c.name) == "boundedBy").and_then(|b| b.children.first()).and_then(|e| e.attr("srsName")) {
        let (p, s) = gml_srs(srs);
        projection = p;
        swap_all = Some(s);
    }
    fn coords(e: &El, dim: usize, swap: bool) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        let mk = |v: &[f64]| -> [f64; 3] {
            let (x, y) = if swap { (v[1], v[0]) } else { (v[0], v[1]) };
            [x, y, v.get(2).copied().unwrap_or(0.0)]
        };
        for c in &e.children {
            match c.name.rsplit(':').next().unwrap_or("") {
                "posList" | "pos" => {
                    let d = c.attr("srsDimension").or_else(|| c.attr("dimension")).and_then(|d| d.parse().ok()).unwrap_or(if c.name.ends_with("pos") { 0 } else { dim });
                    let v: Vec<f64> = c.text.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                    let d = if d == 0 { v.len().clamp(2, 3) } else { d.max(2) };
                    out.extend(v.chunks_exact(d).map(mk));
                }
                "coordinates" => {
                    let (cs, ts) = (c.attr("cs").unwrap_or(","), c.attr("ts").unwrap_or(" "));
                    for t in c.text.split(|ch: char| ts.contains(ch) || ch.is_whitespace()).filter(|t| !t.is_empty()) {
                        let v: Vec<f64> = t.split(cs).filter_map(|x| x.trim().parse().ok()).collect();
                        if v.len() >= 2 {
                            out.push(mk(&v));
                        }
                    }
                }
                "coord" => {
                    let g = |n: &str| c.children.iter().find(|x| x.name.rsplit(':').next() == Some(n)).and_then(|x| x.text.trim().parse::<f64>().ok());
                    if let (Some(x), Some(y)) = (g("X"), g("Y")) {
                        out.push(mk(&[x, y, g("Z").unwrap_or(0.0)]));
                    }
                }
                "Point" => out.extend(coords(c, dim, swap)),
                _ => {}
            }
        }
        out
    }
    fn rings(e: &El, dim: usize, swap: bool) -> Vec<Vec<[f64; 3]>> {
        // LinearRing, or a Ring of curve members.
        let mut r = Vec::new();
        for c in &e.children {
            match c.name.rsplit(':').next().unwrap_or("") {
                "LinearRing" => r.push(coords(c, dim, swap)),
                "Ring" => {
                    let mut pts = Vec::new();
                    fn lines(e: &El, dim: usize, swap: bool, out: &mut Vec<[f64; 3]>) {
                        let p = coords(e, dim, swap);
                        if !p.is_empty() {
                            out.extend(p);
                        }
                        for c in &e.children {
                            lines(c, dim, swap, out);
                        }
                    }
                    lines(c, dim, swap, &mut pts);
                    pts.dedup();
                    r.push(pts);
                }
                _ => {}
            }
        }
        r
    }
    fn geom(e: &El, dim: usize, swap: Option<bool>) -> Option<Geom> {
        let (dim, swap) = {
            let d = e.attr("srsDimension").and_then(|d| d.parse().ok()).unwrap_or(dim);
            let s = e.attr("srsName").map(|s| gml_srs(s).1).or(swap).unwrap_or(false);
            (d, s)
        };
        let kids = |names: &[&str]| -> Vec<&El> {
            let mut v = Vec::new();
            for c in &e.children {
                let n = c.name.rsplit(':').next().unwrap_or("");
                if names.contains(&n) {
                    // A member holds geometries; a members element holds several.
                    if n.ends_with("Members") {
                        v.extend(c.children.iter());
                    } else {
                        v.extend(c.children.iter());
                    }
                }
            }
            v
        };
        let swap_o = Some(swap);
        match e.name.rsplit(':').next().unwrap_or("") {
            "Point" => coords(e, dim, swap).first().map(|&p| Geom::Point(p)),
            "LineString" | "LinearRing" => Some(Geom::Line(coords(e, dim, swap))),
            "Curve" => {
                let mut pts = Vec::new();
                for seg in kids(&["segments"]) {
                    let p = coords(seg, dim, swap);
                    if pts.last().is_some() && p.first() == pts.last() {
                        pts.extend_from_slice(&p[1..]);
                    } else {
                        pts.extend(p);
                    }
                }
                Some(Geom::Line(pts))
            }
            "Polygon" | "PolygonPatch" => {
                let mut r = Vec::new();
                for b in ["exterior", "outerBoundaryIs", "interior", "innerBoundaryIs"] {
                    for bound in e.children.iter().filter(|c| c.name.rsplit(':').next() == Some(b)) {
                        r.extend(rings(bound, dim, swap));
                    }
                }
                Some(Geom::Polygon(r))
            }
            "Surface" => {
                let p: Vec<Geom> = kids(&["patches"]).into_iter().filter_map(|c| geom(c, dim, swap_o)).collect();
                if p.len() == 1 { p.into_iter().next() } else { Some(Geom::Collection(p)) }
            }
            "MultiPoint" => Some(Geom::Collection(kids(&["pointMember", "pointMembers"]).into_iter().filter_map(|c| geom(c, dim, swap_o)).collect())),
            "MultiLineString" | "MultiCurve" | "CompositeCurve" => Some(Geom::Collection(kids(&["lineStringMember", "curveMember", "curveMembers"]).into_iter().filter_map(|c| geom(c, dim, swap_o)).collect())),
            "MultiPolygon" | "MultiSurface" | "CompositeSurface" => Some(Geom::Collection(kids(&["polygonMember", "surfaceMember", "surfaceMembers"]).into_iter().filter_map(|c| geom(c, dim, swap_o)).collect())),
            "MultiGeometry" => Some(Geom::Collection(kids(&["geometryMember", "geometryMembers"]).into_iter().filter_map(|c| geom(c, dim, swap_o)).collect())),
            _ => None,
        }
    }
    // Features grouped by type, in order of first appearance.
    let mut order: Vec<String> = Vec::new();
    let mut by_type: std::collections::HashMap<String, Vec<&El>> = std::collections::HashMap::new();
    for m in members {
        let t = local(&m.name).to_string();
        if !by_type.contains_key(&t) {
            order.push(t.clone());
        }
        by_type.entry(t).or_default().push(m);
    }
    let mut out = Vec::new();
    for t in order {
        let items = &by_type[&t];
        // Attributes: child elements that are not geometry properties or GML metadata.
        let is_geom_prop = |c: &El| c.children.iter().any(|g| GEOMS.contains(&local(&g.name)));
        let skip = |c: &El| c.name.starts_with("gml:") || is_geom_prop(c);
        let mut fields: Vec<(String, FieldType)> = schema.get(&t).cloned().unwrap_or_default();
        if fields.is_empty() {
            let mut names: Vec<String> = Vec::new();
            let mut seen: Vec<Seen> = Vec::new();
            for f in items {
                for c in f.children.iter().filter(|c| !skip(c)) {
                    let n = local(&c.name).to_string();
                    let i = names.iter().position(|x| *x == n).unwrap_or_else(|| {
                        names.push(n.clone());
                        seen.push(Seen::default());
                        names.len() - 1
                    });
                    let v = c.text.trim();
                    if v.is_empty() {
                        continue;
                    }
                    if let Ok(x) = v.parse::<i64>() {
                        if i32::try_from(x).is_ok() {
                            seen[i].int32 = true
                        } else {
                            seen[i].int64 = true
                        }
                    } else if v.parse::<f64>().is_ok() {
                        seen[i].real = true
                    } else {
                        seen[i].text = true
                    }
                }
            }
            fields = names.into_iter().zip(seen.into_iter().map(|s| s.ty())).collect();
        }
        let features = items
            .iter()
            .enumerate()
            .map(|(k, f)| {
                let values = fields
                    .iter()
                    .map(|(n, ty)| match f.children.iter().find(|c| local(&c.name) == n && !skip(c)).map(|c| c.text.trim().to_string()) {
                        None => Value::Null,
                        Some(s) if s.is_empty() && *ty != FieldType::Text => Value::Null,
                        Some(s) => match ty {
                            FieldType::Int | FieldType::Int64 => s.parse().map(Value::Int).unwrap_or(Value::Null),
                            FieldType::Real => s.parse().map(Value::Real).unwrap_or(Value::Null),
                            FieldType::Text => Value::Text(s),
                        },
                    })
                    .collect();
                let gs: Vec<Geom> = f.children.iter().filter(|c| is_geom_prop(c)).flat_map(|c| c.children.iter()).filter_map(|g| geom(g, 2, swap_all)).collect();
                let geom = match gs.len() {
                    0 => None,
                    1 => gs.into_iter().next(),
                    _ => Some(Geom::Collection(gs)),
                };
                Feature { fid: k as i64, geom, values }
            })
            .collect();
        out.push(Layer { name: t, fields, features, projection: projection.clone() });
    }
    if out.is_empty() {
        bail!("{}: no features found in the GML file", path.display());
    }
    Ok(out)
}

// ------------------------------------------------------------------ MapInfo

/// Field types of a MapInfo table definition (`Char (n)`, `Integer`, `Float`...).
fn mapinfo_type(t: &str) -> FieldType {
    let t = t.trim().to_ascii_lowercase();
    if t.starts_with("integer") || t.starts_with("smallint") {
        FieldType::Int
    } else if t.starts_with("largeint") {
        FieldType::Int64
    } else if t.starts_with("float") || t.starts_with("decimal") {
        FieldType::Real
    } else {
        FieldType::Text
    }
}

/// Text in a MapInfo charset: Neutral and UTF-8 as they are, the Windows/ISO Latin ones
/// converted from Latin-1 (as GDAL recodes them).
fn mapinfo_text(raw: &[u8], latin1: bool) -> String {
    if latin1 {
        raw.iter().map(|&b| b as char).collect()
    } else {
        String::from_utf8_lossy(raw).into_owned()
    }
}

/// The `Fields` of a `.tab` file's table definition: (name, MapInfo type text).
fn tab_definition(text: &str) -> Result<(Vec<(String, String)>, bool)> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    let latin1 = lines.iter().any(|l| l.to_ascii_lowercase().contains("charset") && l.to_ascii_lowercase().contains("latin"));
    if let Some(t) = lines.iter().find(|l| l.to_ascii_lowercase().starts_with("type ")) {
        let kind = t.split_whitespace().nth(1).unwrap_or("").to_ascii_uppercase();
        if kind != "NATIVE" && kind != "DBF" {
            bail!("MapInfo {kind} tables are not supported (native ones are)");
        }
    }
    let start = lines.iter().position(|l| l.to_ascii_lowercase().starts_with("fields")).ok_or_else(|| anyhow!("no Fields in the table definition"))?;
    let n: usize = lines[start].split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut fields = Vec::new();
    for l in lines.iter().skip(start + 1).take(n) {
        let l = l.trim_end_matches(';').trim();
        let (name, ty) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
        fields.push((name.trim_matches('"').to_string(), ty.trim().to_string()));
    }
    Ok((fields, latin1))
}

/// A byte reader over a `.map` file's chained coordinate blocks (GDAL's TABMAPCoordBlock):
/// at the end of a block's data the stream continues past the next block's 8-byte header.
struct CoordStream<'a> {
    b: &'a [u8],
    pos: usize,
    /// Start of the coordinate block being read (None: a plain byte stream).
    start: Option<usize>,
}

impl<'a> CoordStream<'a> {
    fn new(b: &'a [u8], at: usize, block: usize) -> Self {
        CoordStream { b, pos: at, start: (block != usize::MAX).then(|| at / block * block) }
    }
    fn byte(&mut self) -> Result<u8> {
        if let Some(start) = self.start {
            let used = self.b.get(start + 2..start + 4).map(|x| i16::from_le_bytes([x[0], x[1]]) as usize).unwrap_or(0) + 8;
            if self.pos >= start + used {
                let next = self.b.get(start + 4..start + 8).map(|x| i32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0);
                if next > 0 {
                    self.pos = next as usize + 8;
                    self.start = Some(next as usize);
                }
            }
        }
        let v = *self.b.get(self.pos).ok_or_else(|| anyhow!("MapInfo coordinates run past the end of the .map file"))?;
        self.pos += 1;
        Ok(v)
    }
    fn i16(&mut self) -> Result<i32> {
        Ok(i16::from_le_bytes([self.byte()?, self.byte()?]) as i32)
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes([self.byte()?, self.byte()?, self.byte()?, self.byte()?]))
    }
    /// A coordinate pair, 16-bit relative to `org` when compressed.
    fn coord(&mut self, compressed: bool, org: (i32, i32)) -> Result<(i32, i32)> {
        Ok(if compressed { (self.i16()?.saturating_add(org.0), self.i16()?.saturating_add(org.1)) } else { (self.i32()?, self.i32()?) })
    }
}

/// The `.map` header's integer-to-coordinate transform (GDAL's TABMAPHeaderBlock::Int2Coordsys,
/// including its rounding to the scale's power of ten).
struct MapTransform {
    scale: [f64; 2],
    displ: [f64; 2],
    quadrant: u8,
    precision: [f64; 2],
}

impl MapTransform {
    fn apply(&self, x: i32, y: i32) -> [f64; 3] {
        let q = self.quadrant;
        let mut dx = if matches!(q, 2 | 3 | 0) { -(x as f64 + self.displ[0]) / self.scale[0] } else { (x as f64 - self.displ[0]) / self.scale[0] };
        let mut dy = if matches!(q, 3 | 4 | 0) { -(y as f64 + self.displ[1]) / self.scale[1] } else { (y as f64 - self.displ[1]) / self.scale[1] };
        if self.precision[0] > 0.0 && self.precision[1] > 0.0 {
            dx = (dx * self.precision[0]).round() / self.precision[0];
            dy = (dy * self.precision[1]).round() / self.precision[1];
        }
        [dx, dy, 0.0]
    }
}

/// One object of a `.map` file at `ptr` (GDAL's TABMAPObjHdr readers and the features'
/// ReadGeometryFromMAPFile). Text, arcs, rectangles, ellipses and collections give no geometry.
fn tab_object(b: &[u8], ptr: usize, block: usize, t: &MapTransform) -> Result<Option<Geom>> {
    let start = ptr / block * block;
    let hdr = b.get(start..start + 12).ok_or_else(|| anyhow!("MapInfo object outside the .map file"))?;
    let center = (i32::from_le_bytes(hdr[4..8].try_into().unwrap()), i32::from_le_bytes(hdr[8..12].try_into().unwrap()));
    let mut s = CoordStream::new(b, ptr, usize::MAX);
    let ty = s.byte()?;
    let _id = s.i32()?;
    let compressed = ty % 3 == 1;
    let pt = |s: &mut CoordStream, org: (i32, i32)| -> Result<[f64; 3]> {
        let (x, y) = s.coord(compressed, org)?;
        Ok(t.apply(x, y))
    };
    Ok(match ty {
        // Points (plain, font and custom symbols).
        0x01 | 0x02 => Some(Geom::Point(pt(&mut s, center)?)),
        0x28 | 0x29 => {
            for _ in 0..12 {
                s.byte()?;
            }
            Some(Geom::Point(pt(&mut s, center)?))
        }
        0x2b | 0x2c => {
            s.byte()?;
            s.byte()?;
            Some(Geom::Point(pt(&mut s, center)?))
        }
        // Two-point line.
        0x04 | 0x05 => {
            let a = pt(&mut s, center)?;
            let c = pt(&mut s, center)?;
            Some(Geom::Line(vec![a, c]))
        }
        // Polylines, multi-polylines and regions.
        0x07 | 0x08 | 0x25 | 0x26 | 0x31 | 0x32 | 0x40 | 0x41 | 0x0d | 0x0e | 0x2e | 0x2f | 0x3d | 0x3e => {
            let coord_ptr = s.i32()? as usize;
            let data_size = (s.i32()? as u32 & 0x7fff_ffff) as usize;
            let sections = match ty {
                0x07 | 0x08 => 1,
                0x3d | 0x3e | 0x40 | 0x41 => {
                    let n = s.i32()?;
                    for _ in 0..33 {
                        s.byte()?;
                    }
                    n
                }
                _ => s.i16()?,
            };
            let org = if compressed {
                s.i16()?;
                s.i16()?;
                (s.i32()?, s.i32()?)
            } else {
                (0, 0)
            };
            let version = if ty < 0x2e { 300 } else if ty < 0x34 { 450 } else if ty < 0x3a { 650 } else { 800 };
            let mut c = CoordStream::new(b, coord_ptr, block);
            if matches!(ty, 0x07 | 0x08) {
                let n = data_size / if compressed { 4 } else { 8 };
                let line = (0..n).map(|_| pt(&mut c, org)).collect::<Result<Vec<_>>>()?;
                return Ok(Some(Geom::Line(line)));
            }
            // Section headers, then every section's vertices.
            let mut secs = Vec::new();
            let hdr_size = if version >= 450 { 28 } else { 24 } * sections.max(0) as usize;
            for _ in 0..sections.max(0) {
                let nv = if version >= 450 { c.i32()? } else { c.i16()? };
                let holes = if version >= 800 { c.i32()? } else { c.i16()? };
                c.coord(compressed, org)?;
                c.coord(compressed, org)?;
                let data_off = c.i32()? as usize;
                secs.push((nv.max(0) as usize, holes.max(0) as usize, data_off.saturating_sub(hdr_size) / 8));
            }
            let total: usize = secs.iter().map(|s| s.0).sum();
            let xy = (0..total).map(|_| pt(&mut c, org)).collect::<Result<Vec<_>>>()?;
            let ring = |(nv, _, off): (usize, usize, usize)| -> Vec<[f64; 3]> { xy.get(off..off + nv).map(|r| r.to_vec()).unwrap_or_default() };
            if matches!(ty, 0x0d | 0x0e | 0x2e | 0x2f | 0x3d | 0x3e) {
                // Each outer ring is followed by its holes.
                let mut polys: Vec<Vec<Vec<[f64; 3]>>> = Vec::new();
                let mut holes_left = 0;
                let mut cur: Vec<Vec<[f64; 3]>> = Vec::new();
                for &sec in &secs {
                    if holes_left < 1 {
                        holes_left = sec.1;
                    } else {
                        holes_left -= 1;
                    }
                    cur.push(ring(sec));
                    if holes_left < 1 {
                        polys.push(std::mem::take(&mut cur));
                    }
                }
                match polys.len() {
                    0 => None,
                    1 => polys.pop().map(Geom::Polygon),
                    _ => Some(Geom::Collection(polys.into_iter().map(Geom::Polygon).collect())),
                }
            } else {
                let mut lines: Vec<Geom> = secs.iter().map(|&sec| Geom::Line(ring(sec))).collect();
                if lines.len() == 1 {
                    lines.pop()
                } else {
                    Some(Geom::Collection(lines))
                }
            }
        }
        // Multipoints.
        0x34 | 0x35 | 0x43 | 0x44 => {
            let coord_ptr = s.i32()? as usize;
            let n = s.i32()?.max(0) as usize;
            for _ in 0..15 {
                s.byte()?;
            }
            if matches!(ty, 0x43 | 0x44) {
                for _ in 0..33 {
                    s.byte()?;
                }
            }
            s.byte()?; // symbol
            s.byte()?;
            let org = if compressed {
                s.i16()?;
                s.i16()?;
                (s.i32()?, s.i32()?)
            } else {
                (0, 0)
            };
            let mut c = CoordStream::new(b, coord_ptr, block);
            Some(Geom::Collection((0..n).map(|_| pt(&mut c, org).map(Geom::Point)).collect::<Result<_>>()?))
        }
        _ => None,
    })
}

/// A MapInfo `.tab` table (native: `.map` geometry, `.id` index, `.dat` attributes), as GDAL's
/// MITAB driver reads it: integer coordinates through the header's scale and displacement,
/// rounded to the scale's power of ten.
pub fn read_mapinfo_tab(path: &Path) -> Result<Layer> {
    let text = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let (defs, latin1) = tab_definition(&String::from_utf8_lossy(&text)).with_context(|| path.display().to_string())?;
    let fields: Vec<(String, FieldType)> = defs.iter().map(|(n, t)| (n.clone(), mapinfo_type(t))).collect();
    // Attributes (.dat: a dBASE-like header, binary values in native tables).
    let dat_path = sibling(path, "dat").ok_or_else(|| anyhow!("{}: no .dat file beside it", path.display()))?;
    let d = std::fs::read(&dat_path)?;
    if d.len() < 32 {
        bail!("{}: not a MapInfo .dat file", dat_path.display());
    }
    let nrec = u32::from_le_bytes(d[4..8].try_into().unwrap()) as usize;
    let hlen = u16::from_le_bytes([d[8], d[9]]) as usize;
    let rlen = u16::from_le_bytes([d[10], d[11]]) as usize;
    let mut widths = Vec::new();
    let mut at = 32;
    while at + 32 <= hlen && d.get(at) != Some(&0x0d) {
        widths.push(d[at + 16] as usize);
        at += 32;
    }
    let rows: Vec<Option<Vec<Value>>> = (0..nrec)
        .map(|r| {
            let rec = d.get(hlen + r * rlen..hlen + (r + 1) * rlen)?;
            if rec[0] == b'*' {
                return None;
            }
            let mut off = 1;
            let vals = defs
                .iter()
                .zip(&widths)
                .map(|((_, ty), &w)| {
                    let raw = rec.get(off..off + w).unwrap_or(&[]);
                    off += w;
                    let ty = ty.to_ascii_lowercase();
                    let num = |n: usize| raw.len() >= n;
                    if ty.starts_with("integer") && num(4) {
                        Value::Int(i32::from_le_bytes(raw[..4].try_into().unwrap()) as i64)
                    } else if ty.starts_with("smallint") && num(2) {
                        Value::Int(i16::from_le_bytes(raw[..2].try_into().unwrap()) as i64)
                    } else if ty.starts_with("largeint") && num(8) {
                        Value::Int(i64::from_le_bytes(raw[..8].try_into().unwrap()))
                    } else if ty.starts_with("float") && num(8) {
                        Value::Real(f64::from_le_bytes(raw[..8].try_into().unwrap()))
                    } else if ty.starts_with("date") && !ty.starts_with("datetime") && num(4) {
                        let y = i16::from_le_bytes([raw[0], raw[1]]);
                        if y == 0 {
                            Value::Null
                        } else {
                            Value::Text(format!("{:04}/{:02}/{:02}", y, raw[2], raw[3]))
                        }
                    } else {
                        let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
                        let s = mapinfo_text(&raw[..end], latin1);
                        let s = s.trim_end().to_string();
                        if ty.starts_with("decimal") {
                            s.trim().parse().map(Value::Real).unwrap_or(Value::Null)
                        } else {
                            Value::Text(s)
                        }
                    }
                })
                .collect();
            Some(vals)
        })
        .collect();
    // Geometry (.map objects found through the .id offsets).
    let map = sibling(path, "map").map(std::fs::read).transpose()?;
    let ids = sibling(path, "id").map(std::fs::read).transpose()?;
    let mut tr = MapTransform { scale: [1.0; 2], displ: [0.0; 2], quadrant: 1, precision: [0.0; 2] };
    let mut block = 512;
    if let Some(m) = &map {
        let h = m.get(..512).ok_or_else(|| anyhow!("{}: short .map header", path.display()))?;
        if i32::from_le_bytes(h[0x100..0x104].try_into().unwrap()) != 42424242 {
            bail!("{}: not a MapInfo .map file", path.display());
        }
        let version = i16::from_le_bytes([h[0x104], h[0x105]]);
        block = (i16::from_le_bytes([h[0x106], h[0x107]]) as usize).max(512);
        let f64_at = |o: usize| f64::from_le_bytes(h[o..o + 8].try_into().unwrap());
        // 0x15e.. : units, index depth, precision, quadrant, ... then datum, ids, then scales.
        let precision_digits = h[0x160];
        tr.quadrant = h[0x161];
        tr.scale = [f64_at(0x170), f64_at(0x178)];
        tr.displ = [f64_at(0x180), f64_at(0x188)];
        if version <= 100 {
            let s = 10f64.powi(precision_digits as i32);
            tr.scale = [s, s];
            tr.displ = [0.0, 0.0];
        }
        if tr.scale[0] == 0.0 || tr.scale[1] == 0.0 {
            bail!("{}: the .map header has no scale", path.display());
        }
        tr.precision = [10f64.powf(tr.scale[0].log10().round()), 10f64.powf(tr.scale[1].log10().round())];
    }
    let mut features = Vec::new();
    for (k, row) in rows.into_iter().enumerate() {
        let Some(values) = row else { continue };
        let ptr = ids.as_ref().and_then(|i| i.get(4 * k..4 * k + 4)).map(|x| i32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0);
        let geom = match (&map, ptr) {
            (Some(m), p) if p > 0 => tab_object(m, p as usize, block, &tr)?,
            _ => None,
        };
        features.push(Feature { fid: k as i64 + 1, geom, values });
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("layer").to_string();
    Ok(Layer { name, fields, features, projection: None })
}

/// Rings of a MIF region grouped into polygons as GDAL's organizePolygons does: rings by
/// decreasing area, each a hole of the smallest larger ring around it (or a new polygon when
/// that ring is itself a hole or there is none).
fn organize_rings(rings: Vec<Vec<[f64; 3]>>) -> Option<Geom> {
    if rings.len() <= 1 {
        return rings.into_iter().next().map(|r| Geom::Polygon(vec![r]));
    }
    let area = |r: &[[f64; 3]]| -> f64 { 0.5 * r.windows(2).map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1]).sum::<f64>().abs() };
    let inside = |p: [f64; 3], r: &[[f64; 3]]| -> bool {
        let mut c = false;
        for w in r.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
                c = !c;
            }
        }
        c
    };
    let mut order: Vec<usize> = (0..rings.len()).collect();
    order.sort_by(|&a, &b| area(&rings[b]).total_cmp(&area(&rings[a])));
    // (outer?, the polygon index it belongs to)
    let mut role: Vec<(bool, usize)> = Vec::new();
    let mut polys: Vec<Vec<usize>> = Vec::new();
    for (i, &ri) in order.iter().enumerate() {
        let probe = rings[ri].iter().copied().find(|&p| (0..i).all(|j| !rings[order[j]].contains(&p))).unwrap_or(rings[ri][0]);
        let parent = (0..i).rev().find(|&j| inside(probe, &rings[order[j]]));
        match parent {
            Some(j) if role[j].0 => {
                polys[role[j].1].push(ri);
                role.push((false, role[j].1));
            }
            _ => {
                polys.push(vec![ri]);
                role.push((true, polys.len() - 1));
            }
        }
    }
    let mut rings: Vec<Option<Vec<[f64; 3]>>> = rings.into_iter().map(Some).collect();
    let mut out: Vec<Geom> = polys.into_iter().map(|p| Geom::Polygon(p.into_iter().filter_map(|i| rings[i].take()).collect())).collect();
    Some(if out.len() == 1 { out.pop().unwrap() } else { Geom::Collection(out) })
}

/// Tokens of a MIF data section: words, numbers and quoted strings (with `""` inside).
fn mif_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == ',' || c == '(' || c == ')' {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::from("\"");
            while let Some(c) = chars.next() {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        s.push('"');
                    } else {
                        break;
                    }
                } else {
                    s.push(c);
                }
            }
            out.push(s);
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == '"' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(s);
        }
    }
    out
}

/// One `.mid` row split at the delimiter (quoted values may hold it, `""` is a quote).
fn mid_row(line: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            quoted = true;
        } else if c == delim {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

/// A MapInfo interchange file (`.mif` geometry and header, `.mid` attributes), as GDAL reads it.
pub fn read_mif(path: &Path) -> Result<Layer> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let head_text = String::from_utf8_lossy(&raw);
    let latin1 = head_text.lines().take(20).any(|l| l.to_ascii_lowercase().starts_with("charset") && l.to_ascii_lowercase().contains("latin"));
    let text = if latin1 { mapinfo_text(&raw, true) } else { head_text.into_owned() };
    let lines: Vec<&str> = text.lines().collect();
    let mut delim = '\t';
    let mut fields = Vec::new();
    let mut transform = [1.0, 1.0, 0.0, 0.0];
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();
        let low = l.to_ascii_lowercase();
        if low.starts_with("delimiter") {
            delim = l.split('"').nth(1).and_then(|d| d.chars().next()).unwrap_or('\t');
        } else if low.starts_with("transform") {
            let v: Vec<f64> = l.split(|c: char| c.is_whitespace() || c == ',').skip(1).filter_map(|x| x.parse().ok()).collect();
            if v.len() == 4 {
                transform = [v[0], v[1], v[2], v[3]];
            }
        } else if low.starts_with("columns") {
            let n: usize = l.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            for c in lines.iter().skip(i + 1).take(n) {
                let c = c.trim();
                let (name, ty) = c.split_once(char::is_whitespace).unwrap_or((c, ""));
                fields.push((name.trim_matches('"').to_string(), mapinfo_type(ty)));
            }
            i += n;
        } else if low == "data" {
            i += 1;
            break;
        }
        i += 1;
    }
    let toks = mif_tokens(&lines[i.min(lines.len())..].join("\n"));
    let num = |t: &str| t.parse::<f64>().ok();
    let mut geoms: Vec<Option<Geom>> = Vec::new();
    let mut k = 0;
    let xy = |toks: &[String], k: &mut usize| -> Result<[f64; 3]> {
        let x = toks.get(*k).and_then(|t| num(t)).ok_or_else(|| anyhow!("MIF: a coordinate is missing"))?;
        let y = toks.get(*k + 1).and_then(|t| num(t)).ok_or_else(|| anyhow!("MIF: a coordinate is missing"))?;
        *k += 2;
        Ok([x * transform[0] + transform[2], y * transform[1] + transform[3], 0.0])
    };
    let count = |toks: &[String], k: &mut usize| -> Result<usize> {
        let n = toks.get(*k).and_then(|t| t.parse::<usize>().ok()).ok_or_else(|| anyhow!("MIF: a count is missing"))?;
        *k += 1;
        Ok(n)
    };
    const KEYWORDS: [&str; 12] = ["none", "point", "line", "pline", "region", "multipoint", "collection", "arc", "text", "rect", "roundrect", "ellipse"];
    while k < toks.len() {
        let w = toks[k].to_ascii_lowercase();
        if !KEYWORDS.contains(&w.as_str()) || toks[k].starts_with('"') {
            k += 1; // styles (Pen, Brush, Symbol, Smooth, Center...) and their values
            continue;
        }
        k += 1;
        let g = match w.as_str() {
            "point" => Some(Geom::Point(xy(&toks, &mut k)?)),
            "line" => Some(Geom::Line(vec![xy(&toks, &mut k)?, xy(&toks, &mut k)?])),
            "pline" => {
                if toks.get(k).is_some_and(|t| t.eq_ignore_ascii_case("multiple")) {
                    k += 1;
                    let n = count(&toks, &mut k)?;
                    let mut parts = Vec::new();
                    for _ in 0..n {
                        let m = count(&toks, &mut k)?;
                        parts.push(Geom::Line((0..m).map(|_| xy(&toks, &mut k)).collect::<Result<_>>()?));
                    }
                    if parts.len() == 1 {
                        parts.pop()
                    } else {
                        Some(Geom::Collection(parts))
                    }
                } else {
                    let m = count(&toks, &mut k)?;
                    Some(Geom::Line((0..m).map(|_| xy(&toks, &mut k)).collect::<Result<_>>()?))
                }
            }
            "region" => {
                let n = count(&toks, &mut k)?;
                let mut rings = Vec::new();
                for _ in 0..n {
                    let m = count(&toks, &mut k)?;
                    let mut r: Vec<[f64; 3]> = (0..m).map(|_| xy(&toks, &mut k)).collect::<Result<_>>()?;
                    if r.first() != r.last() {
                        r.push(r[0]);
                    }
                    rings.push(r);
                }
                organize_rings(rings)
            }
            "multipoint" => {
                let n = count(&toks, &mut k)?;
                Some(Geom::Collection((0..n).map(|_| xy(&toks, &mut k).map(Geom::Point)).collect::<Result<_>>()?))
            }
            _ => None,
        };
        geoms.push(g);
    }
    // Attributes.
    let mid = sibling(path, "mid").map(std::fs::read).transpose()?.map(|b| mapinfo_text(&b, latin1)).unwrap_or_default();
    let rows: Vec<Vec<String>> = mid.lines().map(|l| mid_row(l, delim)).collect();
    let features = geoms
        .into_iter()
        .enumerate()
        .map(|(k, geom)| {
            let row = rows.get(k);
            let values = fields
                .iter()
                .enumerate()
                .map(|(j, (_, ty))| {
                    let s = row.and_then(|r| r.get(j)).map(|s| s.trim()).unwrap_or("");
                    match ty {
                        _ if s.is_empty() && *ty != FieldType::Text => Value::Null,
                        FieldType::Int | FieldType::Int64 => s.parse().map(Value::Int).unwrap_or(Value::Null),
                        FieldType::Real => s.parse().map(Value::Real).unwrap_or(Value::Null),
                        FieldType::Text => Value::Text(s.to_string()),
                    }
                })
                .collect();
            Feature { fid: k as i64 + 1, geom, values }
        })
        .collect();
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("layer").to_string();
    Ok(Layer { name, fields, features, projection: None })
}

// ------------------------------------------------------------------ Esri file geodatabase

/// The `.gdb` folder of a file geodatabase: the folder itself, or a file inside it.
pub fn gdb_dir(path: &Path) -> Option<PathBuf> {
    let is_gdb = |p: &Path| p.is_dir() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gdb")) && p.join("a00000001.gdbtable").is_file();
    if is_gdb(path) {
        return Some(path.to_path_buf());
    }
    path.parent().filter(|p| is_gdb(p)).map(|p| p.to_path_buf())
}

/// An unsigned variable-length integer (7 bits per byte, low first).
fn gdb_varuint(b: &[u8], at: &mut usize) -> Result<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let c = *b.get(*at).ok_or_else(|| anyhow!("file geodatabase: truncated value"))?;
        *at += 1;
        v |= ((c & 0x7f) as u64) << shift;
        if c & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        if shift > 63 {
            bail!("file geodatabase: bad variable-length integer");
        }
    }
}

/// A signed variable-length integer (sign in bit 6 of the first byte, 6 bits then 7 per byte).
fn gdb_varint(b: &[u8], at: &mut usize) -> Result<i64> {
    let c = *b.get(*at).ok_or_else(|| anyhow!("file geodatabase: truncated value"))?;
    *at += 1;
    let neg = c & 0x40 != 0;
    let mut v = (c & 0x3f) as u64;
    if c & 0x80 != 0 {
        let mut shift = 6;
        loop {
            let c = *b.get(*at).ok_or_else(|| anyhow!("file geodatabase: truncated value"))?;
            *at += 1;
            v |= ((c & 0x7f) as u64) << shift;
            if c & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                bail!("file geodatabase: bad variable-length integer");
            }
        }
    }
    Ok(if neg { -(v as i64) } else { v as i64 })
}

fn gdb_utf16(b: &[u8], at: &mut usize, chars: usize) -> Result<String> {
    let raw = b.get(*at..*at + 2 * chars).ok_or_else(|| anyhow!("file geodatabase: truncated text"))?;
    *at += 2 * chars;
    Ok(String::from_utf16_lossy(&raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
}

#[derive(Debug, Clone)]
struct GdbField {
    name: String,
    ty: u8,
    nullable: bool,
}

/// The geometry field's origin and scales (x/y, z) and coordinate system.
#[derive(Debug, Clone, Default)]
struct GdbGeomField {
    wkt: String,
    origin: [f64; 3],
    scale: [f64; 2],
}

/// One table of a file geodatabase (`aXXXXXXXX.gdbtable` with its `.gdbtablx` row index), as
/// GDAL's OpenFileGDB driver reads it: every row with its field values.
struct GdbTable {
    fields: Vec<GdbField>,
    geom: Option<GdbGeomField>,
    utf8: bool,
    rows: Vec<Option<Vec<u8>>>,
}

fn gdb_table(path: &Path) -> Result<GdbTable> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if b.len() < 40 {
        bail!("{}: not a file geodatabase table", path.display());
    }
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let desc = u64::from_le_bytes(b[32..40].try_into().unwrap()) as usize;
    let h = b.get(desc..desc + 14).ok_or_else(|| anyhow!("{}: no field descriptions", path.display()))?;
    let utf8 = h[9] & 1 != 0;
    let has_z = h[11] & 0x80 != 0;
    let has_m = h[11] & 0x40 != 0;
    let nfields = u16::from_le_bytes([h[12], h[13]]) as usize;
    let mut at = desc + 14;
    let mut fields = Vec::new();
    let mut geom = None;
    for _ in 0..nfields {
        let n = *b.get(at).ok_or_else(|| anyhow!("truncated field"))? as usize;
        at += 1;
        let name = gdb_utf16(&b, &mut at, n)?;
        let n = *b.get(at).ok_or_else(|| anyhow!("truncated field"))? as usize;
        at += 1;
        gdb_utf16(&b, &mut at, n)?; // alias
        let ty = *b.get(at).ok_or_else(|| anyhow!("truncated field"))?;
        at += 1;
        let flags;
        match ty {
            7 | 9 => {
                flags = b[at + 1];
                at += 2;
                if ty == 9 {
                    let n = b[at] as usize;
                    at += 1;
                    gdb_utf16(&b, &mut at, n)?;
                }
                let wkt_len = u16::from_le_bytes([b[at], b[at + 1]]) as usize;
                at += 2;
                let wkt = gdb_utf16(&b, &mut at, wkt_len / 2)?;
                let gflags = b[at];
                at += 1;
                let (has_mo, has_zo) = (gflags & 2 != 0, gflags & 4 != 0);
                let f64_at = |o: &mut usize| -> f64 {
                    let v = f64::from_le_bytes(b[*o..*o + 8].try_into().unwrap());
                    *o += 8;
                    v
                };
                let mut g = GdbGeomField { wkt, ..Default::default() };
                if ty == 7 || gflags > 0 {
                    g.origin[0] = f64_at(&mut at);
                    g.origin[1] = f64_at(&mut at);
                    g.scale[0] = f64_at(&mut at);
                    if has_mo {
                        f64_at(&mut at);
                        f64_at(&mut at);
                    }
                    if has_zo {
                        g.origin[2] = f64_at(&mut at);
                        g.scale[1] = f64_at(&mut at);
                    }
                    f64_at(&mut at); // xy tolerance
                    if has_mo {
                        f64_at(&mut at);
                    }
                    if has_zo {
                        f64_at(&mut at);
                    }
                }
                if ty == 9 {
                    at += 1;
                } else {
                    at += 32; // extent
                    if has_z {
                        at += 16;
                    }
                    if has_m {
                        at += 16;
                    }
                    at += 1;
                    let grids = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
                    at += 4 + 8 * grids;
                    geom = Some(g);
                }
            }
            4 => {
                flags = b[at + 4];
                at += 5;
                let dl = gdb_varuint(&b, &mut at)? as usize;
                if flags & 4 != 0 {
                    at += dl;
                }
            }
            6 | 8 | 10 | 11 | 12 => {
                flags = b[at + 1];
                at += 2;
            }
            _ => {
                flags = b[at + 1];
                let dl = b[at + 2] as usize;
                at += 3;
                if flags & 4 != 0 {
                    at += dl;
                }
            }
        }
        fields.push(GdbField { name, ty, nullable: flags & 1 != 0 });
    }
    // Row offsets from the .gdbtablx file (with its map of present 1024-row blocks).
    let x = std::fs::read(path.with_extension("gdbtablx")).unwrap_or_default();
    let mut rows = Vec::new();
    if x.len() >= 16 {
        let blocks = u32::from_le_bytes(x[4..8].try_into().unwrap()) as usize;
        let total = u32::from_le_bytes(x[8..12].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(x[12..16].try_into().unwrap()) as usize;
        let trailer = 16 + size * 1024 * blocks;
        let mut map: Option<Vec<u8>> = None;
        if blocks > 0 && x.len() >= trailer + 16 {
            let words = u32::from_le_bytes(x[trailer..trailer + 4].try_into().unwrap()) as usize;
            let bits = u32::from_le_bytes(x[trailer + 4..trailer + 8].try_into().unwrap()) as usize;
            if words > 0 {
                map = x.get(trailer + 16..trailer + 16 + bits.div_ceil(8)).map(|m| m.to_vec());
            }
        }
        for r in 0..total {
            // The row's slot: blocks before it that are present.
            let slot = match &map {
                None => Some(r),
                Some(m) => {
                    let blk = r / 1024;
                    if m.get(blk / 8).is_some_and(|v| v >> (blk % 8) & 1 != 0) {
                        let before = (0..blk).filter(|&k| m.get(k / 8).is_some_and(|v| v >> (k % 8) & 1 != 0)).count();
                        Some(before * 1024 + r % 1024)
                    } else {
                        None
                    }
                }
            };
            let off = slot.and_then(|s| x.get(16 + s * size..16 + (s + 1) * size)).map(|o| {
                let mut v = [0u8; 8];
                v[..size].copy_from_slice(o);
                u64::from_le_bytes(v) as usize
            });
            rows.push(match off {
                Some(o) if o > 0 && o + 4 <= b.len() => {
                    let len = u32_at(o) as usize;
                    b.get(o + 4..o + 4 + len).map(|r| r.to_vec())
                }
                _ => None,
            });
        }
    }
    Ok(GdbTable { fields, geom, utf8, rows })
}

/// A field value of a row (None for null, the object id and unsupported kinds), the geometry
/// as its raw blob.
enum GdbValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

fn gdb_row(t: &GdbTable, row: &[u8]) -> Result<Vec<GdbValue>> {
    let nullable = t.fields.iter().filter(|f| f.nullable).count();
    let mut at = nullable.div_ceil(8);
    let mut k = 0;
    let mut out = Vec::new();
    for f in &t.fields {
        if f.nullable {
            let null = row.get(k / 8).is_some_and(|v| v >> (k % 8) & 1 != 0);
            k += 1;
            if null {
                out.push(GdbValue::Null);
                continue;
            }
        }
        let take = |at: &mut usize, n: usize| -> Result<&[u8]> {
            let s = row.get(*at..*at + n).ok_or_else(|| anyhow!("file geodatabase: truncated row"))?;
            *at += n;
            Ok(s)
        };
        let v = match f.ty {
            6 => GdbValue::Null,
            0 => GdbValue::Int(i16::from_le_bytes(take(&mut at, 2)?.try_into().unwrap()) as i64),
            1 => GdbValue::Int(i32::from_le_bytes(take(&mut at, 4)?.try_into().unwrap()) as i64),
            2 => GdbValue::Real(f32::from_le_bytes(take(&mut at, 4)?.try_into().unwrap()) as f64),
            3 => GdbValue::Real(f64::from_le_bytes(take(&mut at, 8)?.try_into().unwrap())),
            5 => {
                // Days since 1899-12-30, as GDAL's date and time text.
                let d = f64::from_le_bytes(take(&mut at, 8)?.try_into().unwrap());
                let secs = ((d - 25569.0) * 86400.0 + 0.5).floor() as i64;
                let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
                let (y, m, dd) = civil_from_days(days);
                GdbValue::Text(format!("{y:04}/{m:02}/{dd:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60))
            }
            4 | 12 => {
                let n = gdb_varuint(row, &mut at)? as usize;
                let s = take(&mut at, n)?;
                GdbValue::Text(if t.utf8 || f.ty == 12 { String::from_utf8_lossy(s).into_owned() } else { String::from_utf16_lossy(&s.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>()) })
            }
            7 | 8 => {
                let n = gdb_varuint(row, &mut at)? as usize;
                GdbValue::Blob(take(&mut at, n)?.to_vec())
            }
            10 | 11 => {
                let g = take(&mut at, 16)?;
                GdbValue::Text(format!(
                    "{{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
                    g[3], g[2], g[1], g[0], g[5], g[4], g[7], g[6], g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
                ))
            }
            _ => bail!("file geodatabase: field type {} is not supported", f.ty),
        };
        out.push(v);
    }
    Ok(out)
}

/// Year, month, day of a day count from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A geometry blob (Esri's compressed shape buffer: varint counts, zig-zag deltas on the
/// field's grid), as GDAL's FileGDBOGRGeometryConverter decodes it. Curves keep only their
/// vertices.
fn gdb_geometry(blob: &[u8], g: &GdbGeomField) -> Result<Option<Geom>> {
    let mut at = 0;
    let gt = gdb_varuint(blob, &mut at)? as u32;
    let base = gt & 0xff;
    let has_z = gt & 0x8000_0000 != 0 || matches!(base, 9 | 11 | 18 | 20 | 10 | 13 | 15 | 19);
    let xy = |dx: i64, dy: i64| -> [f64; 3] { [dx as f64 / g.scale[0] + g.origin[0], dy as f64 / g.scale[0] + g.origin[1], 0.0] };
    let zscale = if g.scale[1] == 0.0 { 1.0 } else { g.scale[1] };
    Ok(match base {
        0 => None,
        1 | 9 | 11 | 21 | 52 => {
            let x = gdb_varuint(blob, &mut at)?;
            let y = gdb_varuint(blob, &mut at)?;
            let f = |v: u64, o: f64| if v == 0 { f64::NAN } else { (v - 1) as f64 / g.scale[0] + o };
            let mut p = [f(x, g.origin[0]), f(y, g.origin[1]), 0.0];
            if has_z {
                let z = gdb_varuint(blob, &mut at)?;
                p[2] = if z == 0 { f64::NAN } else { (z - 1) as f64 / zscale + g.origin[2] };
            }
            Some(Geom::Point(p))
        }
        8 | 18 | 20 | 28 | 53 => {
            let n = gdb_varuint(blob, &mut at)? as usize;
            if n == 0 {
                return Ok(Some(Geom::Collection(Vec::new())));
            }
            for _ in 0..4 {
                gdb_varuint(blob, &mut at)?;
            }
            let (mut dx, mut dy) = (0i64, 0i64);
            let mut pts: Vec<[f64; 3]> = Vec::with_capacity(n);
            for _ in 0..n {
                dx += gdb_varint(blob, &mut at)?;
                dy += gdb_varint(blob, &mut at)?;
                pts.push(xy(dx, dy));
            }
            if has_z {
                let mut dz = 0i64;
                for p in &mut pts {
                    dz += gdb_varint(blob, &mut at)?;
                    p[2] = dz as f64 / zscale + g.origin[2];
                }
            }
            Some(Geom::Collection(pts.into_iter().map(Geom::Point).collect()))
        }
        3 | 10 | 13 | 23 | 50 | 5 | 15 | 19 | 25 | 51 => {
            let n = gdb_varuint(blob, &mut at)? as usize;
            if n == 0 {
                return Ok(None);
            }
            let parts = gdb_varuint(blob, &mut at)? as usize;
            if gt & 0x2000_0000 != 0 {
                gdb_varuint(blob, &mut at)?; // number of curves
            }
            if parts == 0 {
                return Ok(None);
            }
            for _ in 0..4 {
                gdb_varuint(blob, &mut at)?;
            }
            let mut counts = Vec::with_capacity(parts);
            let mut sum = 0;
            for _ in 0..parts - 1 {
                let c = gdb_varuint(blob, &mut at)? as usize;
                counts.push(c);
                sum += c;
            }
            counts.push(n.saturating_sub(sum));
            let (mut dx, mut dy) = (0i64, 0i64);
            let mut rings: Vec<Vec<[f64; 3]>> = Vec::with_capacity(parts);
            for &c in &counts {
                let mut r = Vec::with_capacity(c);
                for _ in 0..c {
                    dx += gdb_varint(blob, &mut at)?;
                    dy += gdb_varint(blob, &mut at)?;
                    r.push(xy(dx, dy));
                }
                rings.push(r);
            }
            if has_z {
                let mut dz = 0i64;
                for r in &mut rings {
                    for p in r.iter_mut() {
                        dz += gdb_varint(blob, &mut at)?;
                        p[2] = dz as f64 / zscale + g.origin[2];
                    }
                }
            }
            if matches!(base, 3 | 10 | 13 | 23 | 50) {
                let mut lines: Vec<Geom> = rings.into_iter().map(Geom::Line).collect();
                if lines.len() == 1 {
                    lines.pop()
                } else {
                    Some(Geom::Collection(lines))
                }
            } else {
                organize_rings(rings)
            }
        }
        _ => None, // multipatches
    })
}

/// The feature classes and tables of a file geodatabase (a `.gdb` folder), in the order the
/// system catalog lists them, as GDAL's OpenFileGDB driver reads them.
pub fn read_filegdb(dir: &Path) -> Result<Vec<Layer>> {
    let catalog = gdb_table(&dir.join("a00000001.gdbtable"))?;
    let name_col = catalog.fields.iter().position(|f| f.name.eq_ignore_ascii_case("Name")).ok_or_else(|| anyhow!("{}: no catalog", dir.display()))?;
    let mut layers = Vec::new();
    for (k, row) in catalog.rows.iter().enumerate() {
        let Some(row) = row else { continue };
        let vals = gdb_row(&catalog, row)?;
        let GdbValue::Text(name) = &vals[name_col] else { continue };
        if name.starts_with("GDB_") {
            continue;
        }
        let file = dir.join(format!("a{:08x}.gdbtable", k + 1));
        if !file.is_file() {
            continue;
        }
        let t = gdb_table(&file).with_context(|| format!("{}: table {name}", dir.display()))?;
        let attr: Vec<usize> = (0..t.fields.len()).filter(|&i| matches!(t.fields[i].ty, 0..=5 | 10..=12)).collect();
        let fields: Vec<(String, FieldType)> = attr
            .iter()
            .map(|&i| {
                let ty = match t.fields[i].ty {
                    0 | 1 => FieldType::Int,
                    2 | 3 => FieldType::Real,
                    _ => FieldType::Text,
                };
                (t.fields[i].name.clone(), ty)
            })
            .collect();
        let gcol = t.fields.iter().position(|f| f.ty == 7);
        let mut features = Vec::new();
        for (r, row) in t.rows.iter().enumerate() {
            let Some(row) = row else { continue };
            let vals = gdb_row(&t, row)?;
            let geom = match (gcol.map(|c| &vals[c]), &t.geom) {
                (Some(GdbValue::Blob(b)), Some(g)) => gdb_geometry(b, g)?,
                _ => None,
            };
            let values = attr
                .iter()
                .map(|&i| match &vals[i] {
                    GdbValue::Int(v) => Value::Int(*v),
                    GdbValue::Real(v) => Value::Real(*v),
                    GdbValue::Text(s) => Value::Text(s.clone()),
                    _ => Value::Null,
                })
                .collect();
            features.push(Feature { fid: r as i64 + 1, geom, values });
        }
        let projection = t.geom.as_ref().map(|g| g.wkt.clone()).filter(|w| !w.is_empty() && w != "{B286C06B-0879-11D2-AACA-00C04FA33C20}");
        layers.push(Layer { name: name.clone(), fields, features, projection });
    }
    if layers.is_empty() {
        bail!("{}: no feature classes or tables in the file geodatabase", dir.display());
    }
    Ok(layers)
}
