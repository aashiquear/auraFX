//! GIS vector files without GDAL: ESRI Shapefiles (.shp + .dbf + .prj), GeoJSON, KML and
//! GeoPackage, laid out as ParaView's GDAL vector reader does: polygonal data with points as
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
    matches!(ext, "shp" | "geojson" | "kml" | "gpkg")
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
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "shp" => Ok(vec![read_shapefile(path)?]),
        "geojson" | "json" => Ok(vec![read_geojson(path)?]),
        "kml" => read_kml(path),
        "gpkg" => read_gpkg(path),
        _ => bail!("{}: not a vector file", path.display()),
    }
}

fn layer_names(path: &Path) -> Vec<String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "gpkg" => gpkg_tables(path).map(|t| t.into_iter().map(|(n, _)| n).collect()).unwrap_or_default(),
        "kml" => read_kml(path).map(|l| l.into_iter().map(|l| l.name).collect()).unwrap_or_default(),
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
                if let Some(pd) = build(layer, std::slice::from_ref(f), filled, ids) {
                    mb.blocks.push((format!("{} {}", layer.name, f.fid), Some(DataSet::PolyData(pd))));
                }
            }
        } else if let Some(pd) = build(layer, &layer.features, filled, ids) {
            mb.blocks.push((layer.name.clone(), Some(DataSet::PolyData(pd))));
        }
    }
    if let Some(p) = layers.iter().find_map(|l| l.projection.clone()) {
        mb.field_data.insert(Array::new("MAP_PROJECTION", 1, ArrayData::String(vec![p])));
    }
    Ok(DataSet::MultiBlock(mb))
}

/// Polygonal data for some features of a layer (None when none has geometry).
fn build(layer: &Layer, features: &[Feature], filled: bool, ids: bool) -> Option<PolyData> {
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
    if verts.is_empty() && lines.is_empty() && polys.is_empty() {
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
    use crate::xdmf::El;
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root = crate::xdmf::parse_dom(&text).with_context(|| format!("{}: not valid XML", path.display()))?;
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
