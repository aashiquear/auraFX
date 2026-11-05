//! Tecplot ASCII (`.dat`, `.plt` text, `.tec`): ordered (I/J/K) and finite-element zones
//! (line segments, triangles, quadrilaterals, tetrahedra, bricks), POINT and BLOCK packing,
//! cell-centered variables (VARLOCATION), old-style `F=FEPOINT, ET=...` headers and `n*value`
//! repeats. One block per zone; X, Y, Z variables are the coordinates.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;

/// Split a header into KEY=VALUE pairs (values may be quoted or parenthesised).
fn pairs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let skip = |i: &mut usize| {
        while *i < b.len() && (b[*i].is_whitespace() || b[*i] == ',') {
            *i += 1;
        }
    };
    loop {
        skip(&mut i);
        if i >= b.len() {
            break;
        }
        let ks = i;
        while i < b.len() && b[i] != '=' && !b[i].is_whitespace() && b[i] != ',' {
            i += 1;
        }
        let key = b[ks..i].iter().collect::<String>().to_ascii_uppercase();
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != '=' {
            out.push((key, String::new()));
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        let vs = i;
        let value = if i < b.len() && b[i] == '"' {
            i += 1;
            let s0 = i;
            while i < b.len() && b[i] != '"' {
                i += 1;
            }
            let v: String = b[s0..i].iter().collect();
            i += 1;
            v
        } else if i < b.len() && b[i] == '(' {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == '(' {
                    depth += 1;
                } else if b[i] == ')' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            b[vs..i].iter().collect()
        } else {
            while i < b.len() && !b[i].is_whitespace() && b[i] != ',' {
                i += 1;
            }
            b[vs..i].iter().collect()
        };
        out.push((key, value));
    }
    out
}

/// Variable names of a VARIABLES record (quoted or bare, comma/space separated).
fn variable_names(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => {
                if quoted {
                    out.push(std::mem::take(&mut cur));
                }
                quoted = !quoted;
            }
            ',' | ' ' | '\t' if !quoted => {
                if !cur.trim().is_empty() {
                    out.push(std::mem::take(&mut cur).trim().to_string());
                }
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Cell-centered variables (1-based) of a VARLOCATION=([3,5-6]=CELLCENTERED) value.
fn cell_centered(v: &str, nvars: usize) -> Vec<bool> {
    let mut out = vec![false; nvars];
    let v = v.trim().trim_start_matches('(').trim_end_matches(')');
    let mut rest = v;
    while let Some(open) = rest.find('[') {
        let close = rest[open..].find(']').map(|c| open + c).unwrap_or(rest.len());
        let list = &rest[open + 1..close];
        let after = &rest[close + 1..];
        let loc = after.trim_start().trim_start_matches('=').trim_start();
        let cc = loc.to_ascii_uppercase().starts_with("CELLCENTERED");
        for part in list.split(',') {
            let (a, b) = match part.split_once('-') {
                Some((a, b)) => (a.trim().parse::<usize>().unwrap_or(0), b.trim().parse::<usize>().unwrap_or(0)),
                None => {
                    let x = part.trim().parse::<usize>().unwrap_or(0);
                    (x, x)
                }
            };
            for k in a..=b {
                if k >= 1 && k <= nvars {
                    out[k - 1] = cc;
                }
            }
        }
        rest = after;
    }
    out
}

const RECORDS: [&str; 8] = ["TITLE", "VARIABLES", "ZONE", "TEXT", "GEOMETRY", "AUXDATA", "DATASETAUXDATA", "VARAUXDATA"];

fn record_of(line: &str) -> Option<&'static str> {
    let u = line.trim_start().to_ascii_uppercase();
    RECORDS.iter().copied().find(|r| u.starts_with(r) && u[r.len()..].chars().next().is_none_or(|c| c == '=' || c.is_whitespace()))
}

/// Numbers from the data section, expanding `n*value`.
struct Numbers<'a> {
    lines: &'a [&'a str],
    line: usize,
    pending: std::collections::VecDeque<f64>,
}

impl Numbers<'_> {
    fn next(&mut self) -> Result<f64> {
        while self.pending.is_empty() {
            let l = self.lines.get(self.line).ok_or_else(|| anyhow!("the file ends inside a zone"))?;
            if record_of(l).is_some() {
                bail!("a zone has fewer values than its header says");
            }
            self.line += 1;
            for t in l.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()) {
                if let Some((n, v)) = t.split_once('*') {
                    let n: usize = n.parse().with_context(|| format!("bad repeat {t:?}"))?;
                    let v: f64 = parse_num(v)?;
                    self.pending.extend(std::iter::repeat_n(v, n));
                } else {
                    self.pending.push_back(parse_num(t)?);
                }
            }
        }
        Ok(self.pending.pop_front().unwrap())
    }

    fn take(&mut self, n: usize) -> Result<Vec<f64>> {
        (0..n).map(|_| self.next()).collect()
    }
}

fn parse_num(t: &str) -> Result<f64> {
    // Fortran-style exponents (1.0D+03) too.
    t.replace(['d', 'D'], "e").parse::<f64>().map_err(|_| anyhow!("not a number: {t:?}"))
}

pub fn read(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
    let mut vars: Vec<String> = Vec::new();
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    let mut i = 0;
    while i < lines.len() {
        let Some(rec) = record_of(lines[i]) else {
            if lines[i].trim().is_empty() {
                i += 1;
                continue;
            }
            bail!("{}: line {}: unexpected {:?}", path.display(), i + 1, lines[i].trim());
        };
        // A record runs on over lines that don't start with a number or another record.
        let mut header = lines[i].trim_start()[rec.len()..].to_string();
        i += 1;
        while i < lines.len() && record_of(lines[i]).is_none() {
            let t = lines[i].trim_start();
            if t.is_empty() || t.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') {
                break;
            }
            header.push(' ');
            header.push_str(t);
            i += 1;
        }
        match rec {
            "VARIABLES" => vars = variable_names(header.trim_start().trim_start_matches('=')),
            "ZONE" => {
                if vars.is_empty() {
                    bail!("{}: a ZONE before VARIABLES", path.display());
                }
                let p: std::collections::HashMap<String, String> = pairs(&header).into_iter().collect();
                let get = |k: &str| p.get(k).map(|s| s.as_str());
                let num = |k: &str| get(k).and_then(|v| v.parse::<usize>().ok());
                let name = get("T").map(|s| s.to_string()).unwrap_or_else(|| format!("Zone {}", mb.blocks.len() + 1));
                let packing = get("DATAPACKING").or(get("F")).unwrap_or("POINT").to_ascii_uppercase();
                let block = packing.contains("BLOCK");
                let zonetype = get("ZONETYPE").or(get("ET")).unwrap_or("ORDERED").to_ascii_uppercase();
                let fe = packing.starts_with("FE") || get("N").or(get("NODES")).is_some() || zonetype != "ORDERED";
                let nv = vars.len();
                let cc = get("VARLOCATION").map(|v| cell_centered(v, nv)).unwrap_or(vec![false; nv]);
                let mut nums = Numbers { lines: &lines, line: i, pending: Default::default() };
                let (np, ncells, dims) = if fe {
                    let np = num("N").or(num("NODES")).ok_or_else(|| anyhow!("FE zone without N"))?;
                    let ne = num("E").or(num("ELEMENTS")).ok_or_else(|| anyhow!("FE zone without E"))?;
                    (np, ne, [np, 1, 1])
                } else {
                    let d = [num("I").unwrap_or(1), num("J").unwrap_or(1), num("K").unwrap_or(1)];
                    let nc = d.iter().map(|&x| x.saturating_sub(1).max(1)).product::<usize>();
                    (d[0] * d[1] * d[2], nc, d)
                };
                // Values: per variable (BLOCK) or per point (POINT).
                let mut values: Vec<Vec<f64>> = vec![Vec::new(); nv];
                if block {
                    for (v, vals) in values.iter_mut().enumerate() {
                        *vals = nums.take(if cc[v] { ncells } else { np })?;
                    }
                } else {
                    if cc.iter().any(|&c| c) {
                        bail!("cell-centered variables need BLOCK packing");
                    }
                    for _ in 0..np {
                        for vals in values.iter_mut() {
                            vals.push(nums.next()?);
                        }
                    }
                }
                let coord = |n: &str| {
                    vars.iter().position(|v| v.eq_ignore_ascii_case(n) || v.to_ascii_uppercase().starts_with(&format!("{} ", n.to_ascii_uppercase())))
                };
                let (cx, cy, cz) = (coord("X"), coord("Y"), coord("Z"));
                let xs = |c: Option<usize>| c.filter(|&k| !cc[k]).map(|k| values[k].clone()).unwrap_or(vec![0.0; np]);
                let (x, y, z) = (xs(cx), xs(cy), xs(cz));
                let points = Points::new_f64((0..np).flat_map(|k| [x[k], y[k], z[k]]).collect());
                let mut pd = Attributes::new();
                let mut cd = Attributes::new();
                for (k, name) in vars.iter().enumerate() {
                    if Some(k) == cx || Some(k) == cy || Some(k) == cz {
                        continue;
                    }
                    let arr = Array::new(name.clone(), 1, ArrayData::F64(values[k].clone()));
                    if cc[k] {
                        cd.insert(arr);
                    } else {
                        pd.insert(arr);
                    }
                }
                let ds = if fe {
                    let (npe, ty) = match zonetype.trim_start_matches("FE") {
                        "LINESEG" => (2, CellType::Line),
                        "TRIANGLE" => (3, CellType::Triangle),
                        "QUADRILATERAL" => (4, CellType::Quad),
                        "TETRAHEDRON" => (4, CellType::Tetra),
                        "BRICK" => (8, CellType::Hexahedron),
                        other => bail!("{}: zone {name:?}: element type {other} is not supported", path.display()),
                    };
                    let conn = nums.take(ncells * npe)?;
                    let mut cells = Cells::new();
                    for e in conn.chunks_exact(npe) {
                        let ids: Vec<u32> = e.iter().map(|&v| (v as i64 - 1).max(0) as u32).collect();
                        if ids.iter().any(|&k| k as usize >= np) {
                            bail!("{}: zone {name:?}: node number out of range", path.display());
                        }
                        cells.push(ty, &ids);
                    }
                    DataSet::UnstructuredGrid(UnstructuredGrid { points, cells, point_data: pd, cell_data: cd, field_data: Attributes::new() })
                } else {
                    DataSet::StructuredGrid(StructuredGrid {
                        extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1],
                        points,
                        point_data: pd,
                        cell_data: cd,
                        field_data: Attributes::new(),
                    })
                };
                mb.blocks.push((name, Some(ds)));
                i = nums.line;
            }
            _ => {}
        }
    }
    if mb.blocks.is_empty() {
        bail!("{}: no zones", path.display());
    }
    Ok(DataSet::MultiBlock(mb))
}

/// Whether a `.dat`/`.plt` file looks like Tecplot ASCII (a TITLE, VARIABLES or ZONE record).
pub fn sniff(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = f.take(4096).read(&mut head).unwrap_or(0);
    String::from_utf8_lossy(&head[..n]).lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).take(3).any(|l| record_of(l).is_some())
}

// ---- binary (.plt, "#!TDV1xx") ----

struct Bin<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Bin<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.b.get(self.pos..self.pos + n).ok_or_else(|| anyhow!("the binary Tecplot file is truncated"))?;
        self.pos += n;
        Ok(s)
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String> {
        let mut s = String::new();
        loop {
            let c = self.i32()?;
            if c == 0 {
                return Ok(s);
            }
            s.push(char::from_u32(c as u32).unwrap_or('?'));
        }
    }
}

struct BinZone {
    name: String,
    ztype: i32,
    cell_centered: Vec<bool>,
    dims: [usize; 3],
    nodes: usize,
    elements: usize,
}

/// Binary Tecplot (format 1xx, little-endian): ordered and classic FE zones, node- and
/// cell-centered variables of any storage type, shared variables and connectivity.
pub fn read_binary(path: &Path) -> Result<DataSet> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if !data.starts_with(b"#!TDV") {
        bail!("{}: not a binary Tecplot file", path.display());
    }
    let version: u32 = std::str::from_utf8(&data[5..8]).ok().and_then(|v| v.parse().ok()).unwrap_or(112);
    let mut b = Bin { b: &data, pos: 8 };
    if b.i32()? != 1 {
        bail!("{}: big-endian binary Tecplot files are not supported", path.display());
    }
    if version >= 109 {
        b.i32()?; // file type
    }
    b.string()?; // title
    let nv = b.i32()? as usize;
    let vars: Vec<String> = (0..nv).map(|_| b.string()).collect::<Result<_>>()?;
    let mut zones = Vec::new();
    loop {
        let marker = b.f32()?;
        if marker == 357.0 {
            break;
        }
        match marker as i32 {
            299 => {
                let name = b.string()?;
                b.i32()?; // parent zone
                if version >= 107 {
                    b.i32()?; // strand id
                    b.f64()?; // solution time
                }
                b.i32()?; // zone color
                let ztype = b.i32()?;
                if version < 112 {
                    b.i32()?; // data packing (always block for 112)
                }
                let mut cell_centered = vec![false; nv];
                if b.i32()? != 0 {
                    for c in cell_centered.iter_mut() {
                        *c = b.i32()? == 1;
                    }
                }
                if b.i32()? != 0 {
                    bail!("zone {name:?}: face neighbors are not supported");
                }
                let misc = b.i32()?;
                if misc != 0 {
                    bail!("zone {name:?}: user-defined face connections are not supported");
                }
                let (dims, nodes, elements) = if ztype == 0 {
                    let mut dims = [1usize; 3];
                    for d in dims.iter_mut() {
                        *d = b.i32()? as usize;
                    }
                    (dims, dims.iter().product(), dims.iter().map(|&d| d.saturating_sub(1).max(1)).product())
                } else {
                    if ztype > 5 {
                        bail!("zone {name:?}: polygonal/polyhedral zones are not supported yet");
                    }
                    let nodes = b.i32()? as usize;
                    let elements = b.i32()? as usize;
                    for _ in 0..3 {
                        b.i32()?;
                    }
                    ([1usize; 3], nodes, elements)
                };
                // Auxiliary data: name, format, value, until 0.
                while b.i32()? != 0 {
                    b.string()?;
                    b.i32()?;
                    b.string()?;
                }
                zones.push(BinZone { name, ztype, cell_centered, dims, nodes, elements });
            }
            799 => {
                b.string()?;
                b.i32()?;
                b.string()?;
            }
            899 => {
                b.i32()?;
                b.string()?;
                b.i32()?;
                b.string()?;
            }
            m => bail!("{}: header record {m} (geometry/text) is not supported", path.display()),
        }
    }
    let coord = |n: &str| vars.iter().position(|v| v.eq_ignore_ascii_case(n));
    let (cx, cy, cz) = (coord("X"), coord("Y"), coord("Z"));
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    let mut zone_values: Vec<Vec<Vec<f64>>> = Vec::new();
    let mut zone_conn: Vec<Vec<u32>> = Vec::new();
    for z in &zones {
        if b.f32()? != 299.0 {
            bail!("zone {}: data marker missing", z.name);
        }
        let formats: Vec<i32> = (0..nv).map(|_| b.i32()).collect::<Result<_>>()?;
        let passive: Vec<bool> = if b.i32()? != 0 { (0..nv).map(|_| b.i32().map(|v| v != 0)).collect::<Result<_>>()? } else { vec![false; nv] };
        let shared: Vec<i32> = if b.i32()? != 0 { (0..nv).map(|_| b.i32()).collect::<Result<_>>()? } else { vec![-1; nv] };
        let share_conn = b.i32()?;
        for v in 0..nv {
            if !passive[v] && shared[v] == -1 {
                b.f64()?;
                b.f64()?;
            }
        }
        let mut values = Vec::with_capacity(nv);
        for v in 0..nv {
            let n = if z.cell_centered[v] { z.elements } else { z.nodes };
            if passive[v] {
                values.push(vec![0.0; n]);
                continue;
            }
            if shared[v] >= 0 {
                values.push(
                    zone_values
                        .get(shared[v] as usize)
                        .and_then(|zv| zv.get(v))
                        .cloned()
                        .ok_or_else(|| anyhow!("zone {}: shares a variable with a later zone", z.name))?,
                );
                continue;
            }
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(match formats[v] {
                    1 => b.f32()? as f64,
                    2 => b.f64()?,
                    3 => b.i32()? as f64,
                    4 => i16::from_le_bytes(b.take(2)?.try_into().unwrap()) as f64,
                    5 => b.take(1)?[0] as f64,
                    f => bail!("zone {}: value format {f} is not supported", z.name),
                });
            }
            values.push(out);
        }
        let conn: Vec<u32> = if z.ztype == 0 {
            Vec::new()
        } else if share_conn >= 0 {
            zone_conn.get(share_conn as usize).cloned().ok_or_else(|| anyhow!("zone {}: shared connectivity missing", z.name))?
        } else {
            let npe = [0, 2, 3, 4, 4, 8][z.ztype as usize];
            (0..z.elements * npe).map(|_| b.i32().map(|v| v as u32)).collect::<Result<_>>()?
        };
        let pts: Vec<f64> = (0..z.nodes).flat_map(|i| [cx, cy, cz].map(|c| c.filter(|&k| !z.cell_centered[k]).map(|k| values[k][i]).unwrap_or(0.0))).collect();
        let (mut pd, mut cd) = (Attributes::new(), Attributes::new());
        for (k, name) in vars.iter().enumerate() {
            if Some(k) == cx || Some(k) == cy || Some(k) == cz {
                continue;
            }
            let a = Array::new(name.clone(), 1, ArrayData::F64(values[k].clone()));
            if z.cell_centered[k] {
                cd.insert(a);
            } else {
                pd.insert(a);
            }
        }
        let ds = if z.ztype == 0 {
            DataSet::StructuredGrid(StructuredGrid {
                extent: [0, z.dims[0] as i64 - 1, 0, z.dims[1] as i64 - 1, 0, z.dims[2] as i64 - 1],
                points: Points::new_f64(pts),
                point_data: pd,
                cell_data: cd,
                field_data: Attributes::new(),
            })
        } else {
            let (npe, ty) = match z.ztype {
                1 => (2, CellType::Line),
                2 => (3, CellType::Triangle),
                3 => (4, CellType::Quad),
                4 => (4, CellType::Tetra),
                _ => (8, CellType::Hexahedron),
            };
            let mut cells = Cells::new();
            for e in conn.chunks_exact(npe) {
                if e.iter().any(|&i| i as usize >= z.nodes) {
                    bail!("zone {}: node number out of range", z.name);
                }
                cells.push(ty, e);
            }
            DataSet::UnstructuredGrid(UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: pd, cell_data: cd, field_data: Attributes::new() })
        };
        zone_values.push(values);
        zone_conn.push(conn);
        mb.blocks.push((z.name.clone(), Some(ds)));
    }
    Ok(DataSet::MultiBlock(mb))
}

/// Whether a file is binary Tecplot.
pub fn is_binary(path: &Path) -> bool {
    let mut head = [0u8; 5];
    std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && &head == b"#!TDV"
}
