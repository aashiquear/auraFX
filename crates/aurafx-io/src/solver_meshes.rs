//! Solver meshes: Nastran bulk data (`.bdf`, `.nas`, `.dat` with `BEGIN BULK`) and Fluent mesh
//! and case files (`.msh`, `.cas`, ASCII and binary sections), read and written.
//!
//! Read meshes carry the auraFX mesh conventions: an `ElementBlock` cell array (the Nastran
//! property, the Fluent cell zone) named by the field arrays `ElementBlockIds` and
//! `ElementBlockNames`; Fluent boundary zones become side sets `SideSet <name>` (cell, Exodus
//! side). Written meshes take the same arrays (from the mesh generator, a read Exodus file, ...).

use crate::mesh_writers::{clean, mesh_of, Mesh};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

/// Exodus side corners per cell type (also the faces Fluent zones are made of).
fn exodus_sides(t: CellType) -> &'static [&'static [usize]] {
    match t {
        CellType::Hexahedron | CellType::Voxel => &[&[0, 1, 5, 4], &[1, 2, 6, 5], &[2, 3, 7, 6], &[3, 0, 4, 7], &[0, 3, 2, 1], &[4, 5, 6, 7]],
        CellType::Tetra => &[&[0, 1, 3], &[1, 2, 3], &[0, 3, 2], &[0, 2, 1]],
        CellType::Wedge => &[&[0, 1, 4, 3], &[1, 2, 5, 4], &[0, 3, 5, 2], &[0, 2, 1], &[3, 4, 5]],
        CellType::Pyramid => &[&[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4], &[0, 3, 2, 1]],
        CellType::Quad => &[&[0, 1], &[1, 2], &[2, 3], &[3, 0]],
        CellType::Triangle => &[&[0, 1], &[1, 2], &[2, 0]],
        _ => &[],
    }
}

fn block_fields(ug: &mut UnstructuredGrid, blocks: &BTreeMap<i64, String>) {
    ug.field_data.insert(Array::new("ElementBlockIds", 1, ArrayData::I64(blocks.keys().copied().collect())));
    ug.field_data.insert(Array::new("ElementBlockNames", 1, ArrayData::String(blocks.values().cloned().collect())));
}

// ================================================================== Nastran

/// Is this a Nastran bulk-data file (a GRID card, or BEGIN BULK)?
pub fn is_nastran(path: &Path) -> bool {
    let Ok(b) = std::fs::read(path) else { return false };
    let head = String::from_utf8_lossy(&b[..b.len().min(1 << 16)]).to_ascii_uppercase();
    head.contains("BEGIN BULK") || head.lines().any(|l| l.starts_with("GRID"))
}

/// The cards of a bulk-data file: name and fields (small, large and free fields; continuations
/// joined).
fn bdf_cards(text: &str) -> Vec<(String, Vec<String>)> {
    let mut cards: Vec<(String, Vec<String>)> = Vec::new();
    let mut in_bulk = !text.to_ascii_uppercase().contains("BEGIN BULK");
    for raw in text.lines() {
        let line = raw.split('$').next().unwrap_or("").trim_end();
        let up = line.to_ascii_uppercase();
        if up.trim_start().starts_with("BEGIN BULK") {
            in_bulk = true;
            continue;
        }
        if up.trim_start().starts_with("ENDDATA") {
            break;
        }
        if !in_bulk || line.trim().is_empty() {
            continue;
        }
        let (name_field, fields): (String, Vec<String>) = if line.contains(',') {
            let mut parts = line.split(',').map(|s| s.trim().to_string());
            (parts.next().unwrap_or_default(), parts.collect())
        } else if up.split_whitespace().next().is_some_and(|w| w.ends_with('*')) || line.starts_with('*') {
            // Large field: 8 + 4 x 16 (+ 8).
            let cols: Vec<String> = (0..4).map(|k| line.get(8 + 16 * k..(24 + 16 * k).min(line.len())).unwrap_or("").trim().to_string()).collect();
            (line.get(..8.min(line.len())).unwrap_or("").trim().to_string(), cols)
        } else {
            let cols: Vec<String> = (0..8).map(|k| line.get(8 + 8 * k..(16 + 8 * k).min(line.len())).unwrap_or("").trim().to_string()).collect();
            (line.get(..8.min(line.len())).unwrap_or("").trim().to_string(), cols)
        };
        let is_cont = name_field.is_empty() || name_field.starts_with('+') || name_field.starts_with('*');
        let mut fields = fields;
        // Free-field continuation markers at the end of a line.
        if line.contains(',') {
            if let Some(last) = fields.last() {
                if last.starts_with('+') {
                    fields.pop();
                }
            }
        }
        if is_cont {
            if let Some(c) = cards.last_mut() {
                c.1.extend(fields);
            }
        } else {
            let name = name_field.trim_end_matches('*').to_ascii_uppercase();
            cards.push((name, fields));
        }
    }
    cards
}

/// A Nastran number: `1.5`, `1.5E3`, `1.5+3`, `-2.-4` (an exponent without the E).
fn nas_f64(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Ok(v) = t.parse::<f64>() {
        return v;
    }
    let t = t.replace('D', "E").replace('d', "E");
    if let Ok(v) = t.parse::<f64>() {
        return v;
    }
    // Implicit exponent: the last + or - after the first character.
    if let Some(p) = t[1..].rfind(['+', '-']).map(|p| p + 1) {
        if let (Ok(m), Ok(e)) = (t[..p].parse::<f64>(), t[p..].parse::<i32>()) {
            return m * 10f64.powi(e);
        }
    }
    f64::NAN
}

fn nas_i64(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// Rectangular, cylindrical and spherical coordinate systems (CORD2R/C/S), as origin and axes.
#[derive(Clone, Copy)]
struct Cord {
    kind: u8,
    o: [f64; 3],
    e: [[f64; 3]; 3],
}

impl Cord {
    fn to_global(&self, p: [f64; 3]) -> [f64; 3] {
        let l = match self.kind {
            b'C' => {
                let t = p[1].to_radians();
                [p[0] * t.cos(), p[0] * t.sin(), p[2]]
            }
            b'S' => {
                let (th, ph) = (p[1].to_radians(), p[2].to_radians());
                [p[0] * th.sin() * ph.cos(), p[0] * th.sin() * ph.sin(), p[0] * th.cos()]
            }
            _ => p,
        };
        [0, 1, 2].map(|a| self.o[a] + l[0] * self.e[0][a] + l[1] * self.e[1][a] + l[2] * self.e[2][a])
    }
}

fn normalise(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-300);
    v.map(|x| x / l)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Read a Nastran bulk-data file: GRID points (in their coordinate systems), solid, shell, bar
/// and point elements, with their property numbers as blocks (named after PSOLID/PSHELL/...
/// comments when present, else `property <n>`), `ElementId` and `NodeId` arrays.
pub fn read_nastran(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cards = bdf_cards(&text);
    // Coordinate systems first (CORD2* refer to others; resolve in a few passes).
    let mut cords: HashMap<i64, Cord> = HashMap::new();
    cords.insert(0, Cord { kind: b'R', o: [0.0; 3], e: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] });
    for _ in 0..8 {
        for (name, f) in &cards {
            let kind = match name.as_str() {
                "CORD2R" => b'R',
                "CORD2C" => b'C',
                "CORD2S" => b'S',
                _ => continue,
            };
            let Some(cid) = f.first().and_then(|s| nas_i64(s)) else { continue };
            if cords.contains_key(&cid) {
                continue;
            }
            let rid = f.get(1).and_then(|s| nas_i64(s)).unwrap_or(0);
            let Some(r) = cords.get(&rid).copied() else { continue };
            let g = |k: usize| {
                [
                    nas_f64(f.get(2 + k).map(|s| s.as_str()).unwrap_or("")),
                    nas_f64(f.get(3 + k).map(|s| s.as_str()).unwrap_or("")),
                    nas_f64(f.get(4 + k).map(|s| s.as_str()).unwrap_or("")),
                ]
            };
            let (a, bb, c) = (r.to_global(g(0)), r.to_global(g(3)), r.to_global(g(6)));
            let ez = normalise([bb[0] - a[0], bb[1] - a[1], bb[2] - a[2]]);
            let ey = normalise(cross(ez, [c[0] - a[0], c[1] - a[1], c[2] - a[2]]));
            let ex = cross(ey, ez);
            cords.insert(cid, Cord { kind, o: a, e: [ex, ey, ez] });
        }
    }
    let mut node_index: HashMap<i64, u32> = HashMap::new();
    let mut pts = Points::empty();
    let mut node_ids = Vec::new();
    for (name, f) in &cards {
        if name != "GRID" {
            continue;
        }
        let id = f.first().and_then(|s| nas_i64(s)).ok_or_else(|| anyhow!("GRID without an id"))?;
        let cp = f.get(1).and_then(|s| nas_i64(s)).unwrap_or(0);
        let x = [2, 3, 4].map(|k| nas_f64(f.get(k).map(|s| s.as_str()).unwrap_or("")));
        let c = cords.get(&cp).ok_or_else(|| anyhow!("GRID {id}: coordinate system {cp} is not defined (or not CORD2R/C/S)"))?;
        node_index.insert(id, pts.len() as u32);
        pts.push(c.to_global(x));
        node_ids.push(id);
    }
    if pts.is_empty() {
        bail!("{}: no GRID points", path.display());
    }
    // Property names from comments like "$ PSOLID 1 steel" are rare; use the property number.
    let mut cells = Cells::new();
    let (mut eids, mut pids) = (Vec::new(), Vec::new());
    let elem = |name: &str| -> Option<(CellType, usize, bool)> {
        Some(match name {
            "CTETRA" => (CellType::Tetra, 4, true),
            "CPENTA" => (CellType::Wedge, 6, true),
            "CHEXA" => (CellType::Hexahedron, 8, true),
            "CPYRAM" => (CellType::Pyramid, 5, true),
            "CTRIA3" | "CTRIAR" => (CellType::Triangle, 3, true),
            "CQUAD4" | "CQUADR" => (CellType::Quad, 4, true),
            "CTRIA6" => (CellType::QuadraticTriangle, 6, true),
            "CQUAD8" => (CellType::QuadraticQuad, 8, true),
            "CBAR" | "CBEAM" | "CROD" | "CTUBE" => (CellType::Line, 2, true),
            "CONROD" => (CellType::Line, 2, false),
            "CONM2" => (CellType::Vertex, 1, false),
            _ => return None,
        })
    };
    for (name, f) in &cards {
        let Some((mut ty, n, has_pid)) = elem(name) else { continue };
        let eid = f.first().and_then(|s| nas_i64(s)).unwrap_or(0);
        let pid = if has_pid { f.get(1).and_then(|s| nas_i64(s)).unwrap_or(0) } else { 0 };
        let start = if has_pid { 2 } else { 1 };
        let given: Vec<i64> = f[start.min(f.len())..].iter().map_while(|s| nas_i64(s)).collect();
        // Higher-order solids (10, 15, 20, 13 nodes) when that many are given.
        let n = match (name.as_str(), given.len()) {
            ("CTETRA", k) if k >= 10 => {
                ty = CellType::QuadraticTetra;
                10
            }
            ("CHEXA", k) if k >= 20 => {
                ty = CellType::QuadraticHexahedron;
                20
            }
            ("CPENTA", k) if k >= 15 => {
                ty = CellType::QuadraticWedge;
                15
            }
            ("CPYRAM", k) if k >= 13 => {
                ty = CellType::QuadraticPyramid;
                13
            }
            _ => n,
        };
        if given.len() < n {
            bail!("{name} {eid}: {} node numbers, {n} needed", given.len());
        }
        let ids: Vec<u32> =
            given[..n].iter().map(|g| node_index.get(g).copied().ok_or_else(|| anyhow!("{name} {eid}: GRID {g} is not defined"))).collect::<Result<_>>()?;
        cells.push(ty, &ids);
        eids.push(eid);
        pids.push(pid);
    }
    let mut ug = UnstructuredGrid { points: pts, cells, ..Default::default() };
    let mut blocks: BTreeMap<i64, String> = BTreeMap::new();
    for (name, f) in &cards {
        if matches!(name.as_str(), "PSOLID" | "PSHELL" | "PBAR" | "PBEAM" | "PROD" | "PCOMP" | "PBARL" | "PBEAML") {
            if let Some(pid) = f.first().and_then(|s| nas_i64(s)) {
                blocks.insert(pid, format!("{} {pid}", name.to_ascii_lowercase()));
            }
        }
    }
    for p in &pids {
        blocks.entry(*p).or_insert_with(|| format!("property {p}"));
    }
    ug.point_data.insert(Array::new("NodeId", 1, ArrayData::I64(node_ids)));
    ug.cell_data.insert(Array::new("ElementId", 1, ArrayData::I64(eids)));
    ug.cell_data.insert_active(AttributeKind::Scalars, Array::new("ElementBlock", 1, ArrayData::I64(pids)));
    block_fields(&mut ug, &blocks);
    Ok(DataSet::UnstructuredGrid(ug))
}

/// Free-field number with full precision.
fn nas_num(v: f64) -> String {
    let s = format!("{v:.16e}");
    s.replace("e", "E")
}

/// Write a Nastran bulk-data file: GRID points, elements (property = block), a PSOLID or PSHELL
/// per block (with a MAT1 of steel-like placeholder values), node sets as SPC1 cards with set
/// numbers, in free field format with full precision.
pub fn write_nastran(ds: &DataSet, path: &Path) -> Result<()> {
    let (ug, _) = mesh_of(ds)?;
    let m = Mesh::new(&ug);
    let mut s = String::new();
    writeln!(s, "$ Written by auraFX {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(s, "BEGIN BULK")?;
    for i in 0..ug.points.len() {
        let p = ug.points.get(i);
        writeln!(s, "GRID,{},,{},{},{}", i + 1, nas_num(p[0]), nas_num(p[1]), nas_num(p[2]))?;
    }
    let mut solid_blocks = std::collections::BTreeSet::new();
    let mut shell_blocks = std::collections::BTreeSet::new();
    let mut bar_blocks = std::collections::BTreeSet::new();
    for (c, (t, ids)) in ug.cells.iter().enumerate() {
        let b = m.block[c].max(1);
        let (card, n) = match (t, ids.len()) {
            (CellType::Tetra, 4) => ("CTETRA", 4),
            (CellType::QuadraticTetra, 10) => ("CTETRA", 10),
            (CellType::Hexahedron, 8) => ("CHEXA", 8),
            (CellType::Voxel, 8) => ("CHEXA", 8),
            (CellType::QuadraticHexahedron, 20) => ("CHEXA", 20),
            (CellType::Wedge, 6) => ("CPENTA", 6),
            (CellType::QuadraticWedge, 15) => ("CPENTA", 15),
            (CellType::Pyramid, 5) => ("CPYRAM", 5),
            (CellType::Triangle, 3) => ("CTRIA3", 3),
            (CellType::Quad, 4) => ("CQUAD4", 4),
            (CellType::Line, 2) => ("CBAR", 2),
            (other, k) => bail!("{other:?} cells with {k} points cannot be written to a Nastran file"),
        };
        let ids: Vec<u32> = if t == CellType::Voxel { vec![ids[0], ids[1], ids[3], ids[2], ids[4], ids[5], ids[7], ids[6]] } else { ids.to_vec() };
        match card {
            "CTRIA3" | "CQUAD4" => shell_blocks.insert(b),
            "CBAR" => bar_blocks.insert(b),
            _ => solid_blocks.insert(b),
        };
        let mut fields = vec![card.to_string(), (c + 1).to_string(), b.to_string()];
        fields.extend(ids[..n].iter().map(|i| (i + 1).to_string()));
        if card == "CBAR" {
            // An orientation vector (x) for the bar.
            fields.extend(["1.0".into(), "0.0".into(), "0.0".into()]);
        }
        // Free field, at most 8 data fields per line after the name: continue with "+".
        let mut line = fields[0].clone();
        for (k, f) in fields[1..].iter().enumerate() {
            if k > 0 && k % 8 == 0 {
                line += ",+\n+";
            }
            line += ",";
            line += f;
        }
        writeln!(s, "{line}")?;
    }
    for b in &solid_blocks {
        writeln!(s, "$ block {b}: {}", m.names.get(b).cloned().unwrap_or_default())?;
        writeln!(s, "PSOLID,{b},1")?;
    }
    for b in &shell_blocks {
        writeln!(s, "$ block {b}: {}", m.names.get(b).cloned().unwrap_or_default())?;
        writeln!(s, "PSHELL,{b},1,1.0")?;
    }
    for b in &bar_blocks {
        writeln!(s, "$ block {b}: {}", m.names.get(b).cloned().unwrap_or_default())?;
        writeln!(s, "PBAR,{b},1,1.0")?;
    }
    writeln!(s, "MAT1,1,2.1E11,,0.3,7850.0")?;
    for (k, (name, nodes)) in m.node_sets.iter().enumerate() {
        writeln!(s, "$ node set {}: {}", k + 1, clean(name))?;
        let mut line = format!("SPC1,{},123456", k + 1);
        for (j, n) in nodes.iter().enumerate() {
            if j > 0 && (j + 2) % 8 == 0 {
                line += ",+\n+";
            }
            line += &format!(",{}", n + 1);
        }
        writeln!(s, "{line}")?;
    }
    writeln!(s, "ENDDATA")?;
    std::fs::write(path, s).with_context(|| format!("writing {}", path.display()))
}

// ================================================================== Fluent

/// Is this a Fluent mesh or case file?
pub fn is_fluent(path: &Path) -> bool {
    let Ok(b) = std::fs::read(path) else { return false };
    let head = &b[..b.len().min(4096)];
    let t = String::from_utf8_lossy(head);
    // Sections like (0 "...") (2 3) (10 (0 1 ...
    t.trim_start().starts_with('(') && (t.contains("(10 ") || t.contains("(2 ") || t.contains("(0 \""))
}

/// One section: index, header text inside the first parentheses, and the body (text or bytes).
struct Section {
    index: i64,
    header: Vec<i64>,
    header_words: Vec<String>,
    body: Vec<u8>,
}

/// Split a Fluent file into its top-level sections.
fn fluent_sections(b: &[u8]) -> Result<Vec<Section>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < b.len() {
        // Find "(<index>".
        let Some(open) = b[pos..].iter().position(|&c| c == b'(') else { break };
        pos += open + 1;
        let start = pos;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
        let Ok(index) = std::str::from_utf8(&b[start..pos]).unwrap_or("").parse::<i64>() else {
            // Not a section: skip to its matching close.
            pos = skip_group(b, start - 1)?;
            continue;
        };
        // Header: the next (...) group, or the rest up to the close for short sections.
        while pos < b.len() && b[pos] == b' ' {
            pos += 1;
        }
        let binary = matches!(index, 2010 | 3010 | 2012 | 3012 | 2013 | 3013 | 2020 | 3020 | 2041 | 3041 | 2058 | 3058 | 2300 | 3300);
        if pos < b.len() && b[pos] == b'(' {
            let hend = skip_group(b, pos)?;
            let htext = String::from_utf8_lossy(&b[pos + 1..hend - 1]).to_string();
            let words: Vec<String> = htext.split_whitespace().map(|s| s.trim_matches('"').to_string()).collect();
            let header: Vec<i64> = words.iter().map(|w| i64::from_str_radix(w, 16).unwrap_or(-1)).collect();
            pos = hend;
            while pos < b.len() && b[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let body = if pos < b.len() && b[pos] == b'(' {
                if binary {
                    // Binary data until ")End of Binary Section" (or the size the header gives).
                    let tail = b"End of Binary Section";
                    let rel = memchr::memmem::find(&b[pos..], tail).ok_or_else(|| anyhow!("Fluent: binary section {index} has no end"))?;
                    let body = b[pos + 1..pos + rel].to_vec();
                    pos += rel + tail.len();
                    body.strip_suffix(b")").map(|x| x.to_vec()).unwrap_or(body)
                } else {
                    let bend = skip_group(b, pos)?;
                    let body = b[pos + 1..bend - 1].to_vec();
                    pos = bend;
                    body
                }
            } else {
                Vec::new()
            };
            // The section's own close.
            pos = match b[pos..].iter().position(|&c| c == b')') {
                Some(p) => pos + p + 1,
                None => b.len(),
            };
            out.push(Section { index, header, header_words: words, body });
        } else {
            // A short section such as (2 3) or (0 "comment").
            let end = skip_group(b, start - 1)?;
            let text = String::from_utf8_lossy(&b[pos..end - 1]).to_string();
            out.push(Section { index, header: text.split_whitespace().map(|w| w.parse().unwrap_or(-1)).collect(), header_words: vec![text], body: Vec::new() });
            pos = end;
        }
    }
    Ok(out)
}

/// Position just after the group that opens at `open` (strings and nested groups skipped).
fn skip_group(b: &[u8], open: usize) -> Result<usize> {
    let mut depth = 0i32;
    let mut i = open;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Ok(i + 1);
            }
        }
        i += 1;
    }
    Ok(b.len())
}

fn hex_ints(body: &[u8]) -> Vec<i64> {
    String::from_utf8_lossy(body).split_whitespace().map(|w| i64::from_str_radix(w, 16).unwrap_or(0)).collect()
}

/// Read a Fluent mesh or case file: nodes, faces with their two cells, cells rebuilt from their
/// faces (triangles, quadrilaterals, tetrahedra, hexahedra, wedges, pyramids; other cells as
/// polyhedra), cell zones as blocks and boundary zones as side sets named after the zones.
pub fn read_fluent(path: &Path) -> Result<DataSet> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let secs = fluent_sections(&b)?;
    let mut dim = 3usize;
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut zone_names: HashMap<i64, (String, String)> = HashMap::new();
    let mut ncells = 0usize;
    let mut cell_types: Vec<i64> = Vec::new();
    let mut cell_zone: Vec<i64> = Vec::new();
    // Faces: (zone, nodes (0-based), c0, c1) with cells 1-based (0 = none).
    let mut faces: Vec<(i64, Vec<usize>, usize, usize)> = Vec::new();
    for s in &secs {
        match s.index {
            2 => dim = s.header.first().copied().unwrap_or(3).clamp(2, 3) as usize,
            10 | 2010 | 3010 => {
                let h = &s.header;
                if h.len() < 4 || h[0] == 0 {
                    if h.len() >= 3 && h[2] > 0 && points.is_empty() {
                        points.reserve(h[2] as usize);
                    }
                    continue;
                }
                let (first, last) = (h[1] as usize, h[2] as usize);
                let nd = h.get(4).copied().unwrap_or(dim as i64).max(2) as usize;
                if points.len() < last {
                    points.resize(last, [0.0; 3]);
                }
                let n = last + 1 - first;
                match s.index {
                    10 => {
                        let v: Vec<f64> = String::from_utf8_lossy(&s.body).split_whitespace().map(|w| w.parse().unwrap_or(0.0)).collect();
                        for k in 0..n.min(v.len() / nd) {
                            let mut p = [0.0; 3];
                            p[..nd.min(3)].copy_from_slice(&v[k * nd..k * nd + nd.min(3)]);
                            points[first - 1 + k] = p;
                        }
                    }
                    3010 => {
                        for k in 0..n.min(s.body.len() / (8 * nd)) {
                            for a in 0..nd.min(3) {
                                let o = (k * nd + a) * 8;
                                points[first - 1 + k][a] = f64::from_le_bytes(s.body[o..o + 8].try_into().unwrap());
                            }
                        }
                    }
                    _ => {
                        for k in 0..n.min(s.body.len() / (4 * nd)) {
                            for a in 0..nd.min(3) {
                                let o = (k * nd + a) * 4;
                                points[first - 1 + k][a] = f32::from_le_bytes(s.body[o..o + 4].try_into().unwrap()) as f64;
                            }
                        }
                    }
                }
            }
            12 | 2012 | 3012 => {
                let h = &s.header;
                if h.len() < 3 {
                    continue;
                }
                if h[0] == 0 {
                    ncells = ncells.max(h[2] as usize);
                    continue;
                }
                let (zone, first, last) = (h[0], h[1] as usize, h[2] as usize);
                let elem = h.get(4).copied().unwrap_or(0);
                if cell_types.len() < last {
                    cell_types.resize(last, 0);
                    cell_zone.resize(last, 0);
                }
                let n = last + 1 - first;
                let types: Vec<i64> = if elem == 0 {
                    if s.index == 12 {
                        hex_ints(&s.body)
                    } else {
                        s.body.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64).collect()
                    }
                } else {
                    vec![elem; n]
                };
                for k in 0..n {
                    cell_types[first - 1 + k] = types.get(k).copied().unwrap_or(elem);
                    cell_zone[first - 1 + k] = zone;
                }
                ncells = ncells.max(last);
            }
            13 | 2013 | 3013 => {
                let h = &s.header;
                if h.len() < 4 || h[0] == 0 {
                    continue;
                }
                let (zone, first, last) = (h[0], h[1] as usize, h[2] as usize);
                let ftype = h.get(4).copied().unwrap_or(0);
                let vals: Vec<i64> =
                    if s.index == 13 { hex_ints(&s.body) } else { s.body.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64).collect() };
                let mut q = 0;
                for _ in first..=last {
                    let nn = if ftype == 0 || ftype == 5 {
                        let x = vals.get(q).copied().unwrap_or(0) as usize;
                        q += 1;
                        x
                    } else {
                        ftype as usize
                    };
                    if q + nn + 2 > vals.len() {
                        bail!("Fluent: face section of zone {zone} ends early");
                    }
                    let nodes: Vec<usize> = vals[q..q + nn].iter().map(|&x| (x - 1).max(0) as usize).collect();
                    let (c0, c1) = (vals[q + nn] as usize, vals[q + nn + 1] as usize);
                    q += nn + 2;
                    faces.push((zone, nodes, c0, c1));
                }
            }
            39 | 45 => {
                // (45 (zone-id type name)()) — the words of the header.
                let w = &s.header_words;
                if w.len() >= 3 {
                    if let Ok(id) = w[0].parse::<i64>() {
                        zone_names.insert(id, (w[1].clone(), w[2].clone()));
                    }
                }
            }
            _ => {}
        }
    }
    if points.is_empty() {
        bail!("{}: no nodes", path.display());
    }
    let ncells = ncells.max(faces.iter().map(|f| f.2.max(f.3)).max().unwrap_or(0));
    if cell_types.len() < ncells {
        cell_types.resize(ncells, 0);
        cell_zone.resize(ncells, 0);
    }
    // Faces of every cell, oriented outward: Fluent's right-hand normal points toward c0 (the
    // face's nodes as listed run clockwise seen from c0), so c0 takes them reversed.
    let mut cell_faces: Vec<Vec<Vec<usize>>> = vec![Vec::new(); ncells];
    for (_, nodes, c0, c1) in &faces {
        if *c0 > 0 {
            let mut r = nodes.clone();
            r.reverse();
            cell_faces[c0 - 1].push(r);
        }
        if *c1 > 0 {
            cell_faces[c1 - 1].push(nodes.clone());
        }
    }
    let mut cells = Cells::new();
    let mut poly_faces: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut any_poly = false;
    for (c, fs) in cell_faces.iter().enumerate() {
        let (ty, ids) = cell_from_faces(fs, &points, dim, cell_types[c]);
        if ty == CellType::Polyhedron {
            any_poly = true;
        }
        poly_faces.push(if ty == CellType::Polyhedron { fs.iter().map(|f| f.iter().map(|&x| x as u32).collect()).collect() } else { Vec::new() });
        cells.push(ty, &ids.iter().map(|&x| x as u32).collect::<Vec<_>>());
    }
    if any_poly {
        cells.polyhedron_faces = Some(poly_faces);
    }
    let mut ug = UnstructuredGrid { points: Points::new_f64(points.iter().flatten().copied().collect()), cells, ..Default::default() };
    // Cell zones as blocks.
    let mut blocks: BTreeMap<i64, String> = BTreeMap::new();
    for z in &cell_zone {
        blocks.entry(*z).or_insert_with(|| zone_names.get(z).map(|n| n.1.clone()).unwrap_or_else(|| format!("zone {z}")));
    }
    ug.cell_data.insert_active(AttributeKind::Scalars, Array::new("ElementBlock", 1, ArrayData::I64(cell_zone.clone())));
    block_fields(&mut ug, &blocks);
    // Boundary zones (faces with one cell) as side sets: find each face among its cell's sides.
    let mut by_zone: BTreeMap<i64, Vec<(usize, usize)>> = BTreeMap::new();
    for (zone, nodes, c0, c1) in &faces {
        if *c1 != 0 && *c0 != 0 {
            continue;
        }
        let c = c0.max(c1) - 1;
        let t = ug.cells.cell_type(c);
        let ids = ug.cells.cell(c);
        let mut key: Vec<usize> = nodes.clone();
        key.sort_unstable();
        if let Some(side) = exodus_sides(t).iter().position(|sd| {
            let mut k: Vec<usize> = sd.iter().map(|&i| ids[i] as usize).collect();
            k.sort_unstable();
            k == key
        }) {
            by_zone.entry(*zone).or_default().push((c, side + 1));
        }
    }
    for (zone, sides) in by_zone {
        let name = zone_names.get(&zone).map(|n| n.1.clone()).unwrap_or_else(|| format!("zone {zone}"));
        ug.field_data.insert(Array::new(format!("SideSet {name}"), 2, ArrayData::I64(sides.iter().flat_map(|&(c, s)| [c as i64, s as i64]).collect())));
    }
    Ok(DataSet::UnstructuredGrid(ug))
}

fn signed_volume(p: &[[f64; 3]], a: usize, b: usize, c: usize, d: usize) -> f64 {
    let s = |i: usize, j: usize| [p[i][0] - p[j][0], p[i][1] - p[j][1], p[i][2] - p[j][2]];
    let (u, v, w) = (s(b, a), s(c, a), s(d, a));
    u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0])
}

/// A standard cell from its outward faces (or a polyhedron when it is not one).
fn cell_from_faces(fs: &[Vec<usize>], p: &[[f64; 3]], dim: usize, fluent_type: i64) -> (CellType, Vec<usize>) {
    let count = |k: usize| fs.iter().filter(|f| f.len() == k).count();
    if dim == 2 {
        // Edges into a loop.
        let mut loop_: Vec<usize> = Vec::new();
        if let Some(first) = fs.first() {
            loop_.extend_from_slice(first);
            let mut used = vec![false; fs.len()];
            used[0] = true;
            while loop_.len() < fs.len() {
                let last = *loop_.last().unwrap();
                let Some(k) = (0..fs.len()).find(|&k| !used[k] && fs[k].contains(&last)) else { break };
                used[k] = true;
                let nxt = if fs[k][0] == last { fs[k][1] } else { fs[k][0] };
                loop_.push(nxt);
            }
        }
        // Counter-clockwise in the xy plane.
        let area: f64 = (0..loop_.len())
            .map(|k| {
                let (a, b) = (p[loop_[k]], p[loop_[(k + 1) % loop_.len()]]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum();
        if area < 0.0 {
            loop_.reverse();
        }
        let ty = match loop_.len() {
            3 => CellType::Triangle,
            4 => CellType::Quad,
            _ => CellType::Polygon,
        };
        return (ty, loop_);
    }
    let _ = fluent_type;
    let nodes = |f: &[usize]| f.to_vec();
    match (fs.len(), count(3), count(4)) {
        (4, 4, 0) => {
            // Tetrahedron: a face and the other node, then orient.
            let f = &fs[0];
            let d = *fs[1].iter().find(|x| !f.contains(x)).unwrap_or(&f[0]);
            let mut ids = vec![f[0], f[1], f[2], d];
            if signed_volume(p, ids[0], ids[1], ids[2], ids[3]) < 0.0 {
                ids.swap(1, 2);
            }
            (CellType::Tetra, ids)
        }
        (6, 0, 6) => {
            // Hexahedron: a base face and the node across each of its edges.
            let base = nodes(&fs[0]);
            let top: Vec<usize> = base
                .iter()
                .enumerate()
                .map(|(k, &a)| {
                    let _ = k;
                    // A side face holds a, a base neighbour and two top nodes: the top node next to a.
                    fs[1..]
                        .iter()
                        .filter(|f| f.contains(&a))
                        .flat_map(|f| {
                            let i = f.iter().position(|&x| x == a).unwrap();
                            [f[(i + 1) % 4], f[(i + 3) % 4]]
                        })
                        .find(|x| !base.contains(x))
                        .unwrap_or(a)
                })
                .collect();
            let mut ids = [base.clone(), top].concat();
            // VTK: the base runs counter-clockwise seen from the top.
            if signed_volume(p, ids[0], ids[1], ids[3], ids[4]) < 0.0 {
                ids = vec![ids[0], ids[3], ids[2], ids[1], ids[4], ids[7], ids[6], ids[5]];
            }
            (CellType::Hexahedron, ids)
        }
        (5, 2, 3) => {
            // Wedge: the two triangles, matched by the quadrilaterals.
            let tris: Vec<&Vec<usize>> = fs.iter().filter(|f| f.len() == 3).collect();
            let (a, b) = (tris[0].clone(), tris[1]);
            let other: Vec<usize> = a
                .iter()
                .map(|&x| {
                    fs.iter()
                        .filter(|f| f.len() == 4 && f.contains(&x))
                        .flat_map(|f| {
                            let i = f.iter().position(|&y| y == x).unwrap();
                            [f[(i + 1) % 4], f[(i + 3) % 4]]
                        })
                        .find(|y| b.contains(y))
                        .unwrap_or(x)
                })
                .collect();
            let mut ids = [a, other].concat();
            // VTK: the first triangle's normal points away from the second.
            if signed_volume(p, ids[0], ids[1], ids[2], ids[3]) > 0.0 {
                ids = vec![ids[0], ids[2], ids[1], ids[3], ids[5], ids[4]];
            }
            (CellType::Wedge, ids)
        }
        (5, 4, 1) => {
            let base = fs.iter().find(|f| f.len() == 4).unwrap().clone();
            let apex = *fs.iter().find(|f| f.len() == 3).unwrap().iter().find(|x| !base.contains(x)).unwrap_or(&base[0]);
            let mut ids = [base, vec![apex]].concat();
            if signed_volume(p, ids[0], ids[1], ids[3], ids[4]) < 0.0 {
                ids = vec![ids[0], ids[3], ids[2], ids[1], ids[4]];
            }
            (CellType::Pyramid, ids)
        }
        _ => {
            let mut all: Vec<usize> = fs.iter().flatten().copied().collect();
            all.sort_unstable();
            all.dedup();
            (CellType::Polyhedron, all)
        }
    }
}

/// Write a Fluent mesh file (ASCII): nodes, cells by zone (one per block), the interior faces,
/// and one boundary zone per side set (other outer faces in a zone named `wall`), with zone names.
pub fn write_fluent(ds: &DataSet, path: &Path) -> Result<()> {
    let (ug, _) = mesh_of(ds)?;
    let m = Mesh::new(&ug);
    let dim = if ug.cells.iter().any(|(t, _)| t.dimension() == 3) { 3 } else { 2 };
    let fluent_type = |t: CellType| -> Result<i64> {
        Ok(match t {
            CellType::Triangle => 1,
            CellType::Tetra => 2,
            CellType::Quad => 3,
            CellType::Hexahedron | CellType::Voxel => 4,
            CellType::Pyramid => 5,
            CellType::Wedge => 6,
            other => bail!("{other:?} cells cannot be written to a Fluent mesh"),
        })
    };
    let cells: Vec<(CellType, Vec<usize>)> = ug
        .cells
        .iter()
        .map(|(t, ids)| {
            let ids: Vec<usize> = ids.iter().map(|&x| x as usize).collect();
            if t == CellType::Voxel {
                (CellType::Hexahedron, vec![ids[0], ids[1], ids[3], ids[2], ids[4], ids[5], ids[7], ids[6]])
            } else {
                (t, ids)
            }
        })
        .collect();
    if cells.iter().any(|(t, _)| t.dimension() != dim as u8) {
        bail!("a Fluent mesh holds cells of one dimension (extract the {}-D cells first)", dim);
    }
    // Every face once: key = sorted nodes; (nodes as seen from the first cell, outward), cells.
    let mut face_of: HashMap<Vec<usize>, usize> = HashMap::new();
    let mut faces: Vec<(Vec<usize>, usize, usize, usize)> = Vec::new(); // nodes, cell a, side a (1-based), cell b
    for (c, (t, ids)) in cells.iter().enumerate() {
        fluent_type(*t)?;
        for (s, side) in exodus_sides(*t).iter().enumerate() {
            let nodes: Vec<usize> = side.iter().map(|&i| ids[i]).collect();
            let mut key = nodes.clone();
            key.sort_unstable();
            match face_of.get(&key) {
                Some(&f) => faces[f].3 = c + 1,
                None => {
                    face_of.insert(key, faces.len());
                    faces.push((nodes, c + 1, s + 1, 0));
                }
            }
        }
    }
    // Boundary faces by side set (first set wins), the rest in `wall`.
    let mut set_of_face: HashMap<usize, usize> = HashMap::new();
    for (k, (_, sides)) in m.side_sets.iter().enumerate() {
        for &(c, s) in sides {
            if let Some((t, ids)) = cells.get(c) {
                if let Some(side) = exodus_sides(*t).get(s.wrapping_sub(1)) {
                    let mut key: Vec<usize> = side.iter().map(|&i| ids[i]).collect();
                    key.sort_unstable();
                    if let Some(&f) = face_of.get(&key) {
                        if faces[f].3 == 0 {
                            set_of_face.entry(f).or_insert(k);
                        }
                    }
                }
            }
        }
    }
    let mut out = String::new();
    writeln!(out, "(0 \"Written by auraFX {}\")", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "(2 {dim})")?;
    let np = ug.points.len();
    writeln!(out, "(10 (0 1 {np:x} 0 {dim}))")?;
    writeln!(out, "(10 (1 1 {np:x} 1 {dim})(")?;
    for i in 0..np {
        let p = ug.points.get(i);
        if dim == 3 {
            writeln!(out, "{:.16e} {:.16e} {:.16e}", p[0], p[1], p[2])?;
        } else {
            writeln!(out, "{:.16e} {:.16e}", p[0], p[1])?;
        }
    }
    writeln!(out, "))")?;
    // Cells: renumber so each zone (block) is a contiguous range.
    let mut zones: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for c in 0..cells.len() {
        zones.entry(m.block[c]).or_default().push(c);
    }
    let mut new_id = vec![0usize; cells.len()];
    let mut next = 1;
    for cs in zones.values() {
        for &c in cs {
            new_id[c] = next;
            next += 1;
        }
    }
    let nc = cells.len();
    writeln!(out, "(12 (0 1 {nc:x} 0))")?;
    let mut zone_ids: Vec<(i64, String, String)> = Vec::new();
    let mut zid = 2i64;
    let mut first = 1usize;
    for (b, cs) in &zones {
        let last = first + cs.len() - 1;
        writeln!(out, "(12 ({zid:x} {first:x} {last:x} 1 0)(")?;
        let types: Vec<String> = cs.iter().map(|&c| format!("{:x}", fluent_type(cells[c].0).unwrap())).collect();
        for chunk in types.chunks(20) {
            writeln!(out, "{}", chunk.join(" "))?;
        }
        writeln!(out, "))")?;
        zone_ids.push((zid, "fluid".into(), clean(m.names.get(b).map(|s| s.as_str()).unwrap_or("fluid"))));
        zid += 1;
        first = last + 1;
    }
    // Faces: interior first, then each boundary zone.
    let nf = faces.len();
    writeln!(out, "(13 (0 1 {nf:x} 0))")?;
    let mut groups: Vec<(String, String, Vec<usize>)> = vec![("interior".into(), "interior".into(), (0..nf).filter(|&f| faces[f].3 != 0).collect())];
    for (k, (name, _)) in m.side_sets.iter().enumerate() {
        let fs: Vec<usize> = (0..nf).filter(|f| set_of_face.get(f) == Some(&k)).collect();
        if !fs.is_empty() {
            groups.push(("wall".into(), clean(name), fs));
        }
    }
    let rest: Vec<usize> = (0..nf).filter(|f| faces[*f].3 == 0 && !set_of_face.contains_key(f)).collect();
    if !rest.is_empty() {
        groups.push(("wall".into(), "wall".into(), rest));
    }
    let mut ffirst = 1usize;
    for (bc, name, fs) in &groups {
        let flast = ffirst + fs.len() - 1;
        let bc_code = if bc == "interior" { 2 } else { 3 };
        writeln!(out, "(13 ({zid:x} {ffirst:x} {flast:x} {bc_code:x} 0)(")?;
        for &f in fs {
            let (nodes, ca, _, cb) = &faces[f];
            // The nodes run outward from cell a, so their normal points to b: Fluent's normal points
            // to c0, so c0 = b and c1 = a. A boundary face has no b: listed reversed, with c0 = a.
            if *cb == 0 {
                let mut r = nodes.clone();
                r.reverse();
                writeln!(out, "{:x} {} {:x} 0", r.len(), r.iter().map(|x| format!("{:x}", x + 1)).collect::<Vec<_>>().join(" "), new_id[ca - 1])?;
            } else {
                writeln!(
                    out,
                    "{:x} {} {:x} {:x}",
                    nodes.len(),
                    nodes.iter().map(|x| format!("{:x}", x + 1)).collect::<Vec<_>>().join(" "),
                    new_id[cb - 1],
                    new_id[ca - 1]
                )?;
            }
        }
        writeln!(out, "))")?;
        zone_ids.push((zid, bc.clone(), name.clone()));
        zid += 1;
        ffirst = flast + 1;
    }
    for (id, ty, name) in &zone_ids {
        writeln!(out, "(45 ({id} {ty} {name})())")?;
    }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two hexahedra side by side (x 0..2) with a side set on x- and a node set on x+.
    fn two_hexes() -> DataSet {
        let mut ug = UnstructuredGrid::default();
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..3 {
                    ug.points.push([x as f64, y as f64, z as f64]);
                }
            }
        }
        let id = |x: u32, y: u32, z: u32| x + 3 * (y + 2 * z);
        for x in 0..2 {
            ug.cells.push(
                CellType::Hexahedron,
                &[id(x, 0, 0), id(x + 1, 0, 0), id(x + 1, 1, 0), id(x, 1, 0), id(x, 0, 1), id(x + 1, 0, 1), id(x + 1, 1, 1), id(x, 1, 1)],
            );
        }
        ug.cell_data.insert(Array::new("ElementBlock", 1, ArrayData::I64(vec![1, 2])));
        ug.field_data.insert(Array::new("ElementBlockIds", 1, ArrayData::I64(vec![1, 2])));
        ug.field_data.insert(Array::new("ElementBlockNames", 1, ArrayData::String(vec!["left".into(), "right".into()])));
        // Exodus side 4 of cell 0 is x-.
        ug.field_data.insert(Array::new("SideSet inlet", 2, ArrayData::I64(vec![0, 4])));
        ug.field_data.insert(Array::new("NodeSet outlet", 1, ArrayData::I64(vec![2, 5, 8, 11])));
        DataSet::UnstructuredGrid(ug)
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("afx-solver-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    fn volume(ds: &DataSet) -> f64 {
        let DataSet::UnstructuredGrid(u) = ds else { panic!() };
        u.cells
            .iter()
            .map(|(t, ids)| {
                let p: Vec<[f64; 3]> = ids.iter().map(|&i| u.points.get(i as usize)).collect();
                let v = |a: usize, b: usize, c: usize, d: usize| signed_volume(&p, a, b, c, d) / 6.0;
                match t {
                    CellType::Hexahedron => v(0, 1, 3, 4) + v(1, 2, 3, 6) + v(1, 3, 4, 6) + v(1, 5, 6, 4) + v(3, 7, 4, 6),
                    CellType::Tetra => v(0, 1, 2, 3),
                    _ => 0.0,
                }
            })
            .sum()
    }

    #[test]
    fn nastran_fields_and_round_trip() {
        // Small field, large field, free field with continuations, an implicit exponent and a
        // cylindrical coordinate system.
        let text = "$ test\nBEGIN BULK\nCORD2C  1       0       0.0     0.0     0.0     0.0     0.0     1.0\n+       1.0     0.0     0.0\nGRID    1               0.0     0.0     0.0\nGRID*   2                               1.0             0.0\n*       0.0\nGRID,3,,0.,1.,0.\nGRID,4,1,1.0,90.0,5.-1\nCTETRA,7,3,1,2,3,4\nPSOLID,3,1\nENDDATA\n";
        let p = tmp("t.bdf");
        std::fs::write(&p, text).unwrap();
        let DataSet::UnstructuredGrid(u) = read_nastran(&p).unwrap() else { panic!() };
        assert_eq!((u.points.len(), u.cells.len()), (4, 1));
        let q = u.points.get(3);
        assert!(q[0].abs() < 1e-12 && (q[1] - 1.0).abs() < 1e-12 && (q[2] - 0.5).abs() < 1e-12, "{q:?}");
        assert_eq!(u.cell_data.get("ElementBlock").unwrap().get(0, 0), 3.0);
        // Write the two hexahedra and read them back.
        let ds = two_hexes();
        let p = tmp("h.bdf");
        write_nastran(&ds, &p).unwrap();
        let back = read_nastran(&p).unwrap();
        assert_eq!(back.n_cells(), 2);
        assert!((volume(&back) - 2.0).abs() < 1e-12);
        assert_eq!(back.cell_data().unwrap().get("ElementBlock").unwrap().get(1, 0), 2.0);
    }

    #[test]
    fn fluent_round_trip() {
        let ds = two_hexes();
        let p = tmp("h.msh");
        write_fluent(&ds, &p).unwrap();
        let back = read_fluent(&p).unwrap();
        assert_eq!((back.n_points(), back.n_cells()), (12, 2));
        assert!((volume(&back) - 2.0).abs() < 1e-12, "{}", volume(&back));
        let DataSet::UnstructuredGrid(u) = &back else { panic!() };
        // The side set comes back as a zone, the other 9 outer faces as `wall`.
        // (The rebuilt cells may start at another corner: check the face by its points.)
        let inlet = u.field_data.get("SideSet inlet").unwrap();
        assert_eq!(inlet.len(), 1);
        let (c, side) = (inlet.get(0, 0) as usize, inlet.get(0, 1) as usize);
        let corners = exodus_sides(u.cells.cell_type(c))[side - 1];
        assert!(corners.iter().all(|&k| u.points.get(u.cells.cell(c)[k] as usize)[0] == 0.0));
        assert_eq!(u.field_data.get("SideSet wall").unwrap().len(), 9);
        let names = match &u.field_data.get("ElementBlockNames").unwrap().data {
            ArrayData::String(v) => v.clone(),
            _ => panic!(),
        };
        assert_eq!(names, vec!["left".to_string(), "right".to_string()]);
    }
}
