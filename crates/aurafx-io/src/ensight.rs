//! EnSight Gold (`.case`): ASCII and C Binary geometry and variables, all standard element
//! types (linear, quadratic, nsided, nfaced), structured blocks, per-node and per-element
//! scalars/vectors/tensors, time sets and wildcard file names. One block per part.

use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---- the case file ----

#[derive(Debug, Clone)]
struct Variable {
    kind: String, // "scalar", "vector", "tensor symm", "tensor asym"
    per_node: bool,
    name: String,
    file: String,
    time_set: Option<usize>,
}

#[derive(Debug, Clone, Default)]
struct TimeSet {
    times: Vec<f64>,
    numbers: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
struct Case {
    dir: PathBuf,
    geometry: String,
    geometry_ts: Option<usize>,
    variables: Vec<Variable>,
    time_sets: std::collections::HashMap<usize, TimeSet>,
}

fn parse_case(path: &Path) -> Result<Case> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut c = Case { dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(), ..Default::default() };
    let mut section = String::new();
    let mut ts_id = 1usize;
    let mut pending: Option<&'static str> = None; // multi-line lists: "times" or "numbers"
    let mut nsteps = 0usize;
    let (mut start, mut incr) = (0i64, 1i64);
    let flush = |c: &mut Case, ts_id: usize, nsteps: usize, start: i64, incr: i64| {
        let t = c.time_sets.entry(ts_id).or_default();
        if t.numbers.is_empty() && nsteps > 0 {
            t.numbers = (0..nsteps as i64).map(|i| start + i * incr).collect();
        }
    };
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "FORMAT" | "GEOMETRY" | "VARIABLE" | "TIME" | "FILE" | "MATERIAL" | "BLOCK_CONTINUATION" | "SCRIPTS") {
            section = line.to_string();
            pending = None;
            continue;
        }
        let (key, rest) = match line.split_once(':') {
            Some((k, r)) => (k.trim().to_ascii_lowercase(), r.trim()),
            None => (String::new(), line),
        };
        if key.is_empty() {
            // Continuation of a list of numbers.
            if let (Some(p), Some(t)) = (pending, c.time_sets.get_mut(&ts_id)) {
                for v in rest.split_whitespace().filter_map(|v| v.parse::<f64>().ok()) {
                    if p == "times" {
                        t.times.push(v);
                    } else {
                        t.numbers.push(v as i64);
                    }
                }
            }
            continue;
        }
        pending = None;
        match section.as_str() {
            "FORMAT" => {
                if key == "type" && !rest.to_ascii_lowercase().contains("gold") {
                    bail!("{}: only EnSight Gold is supported (found {rest:?})", path.display());
                }
            }
            "GEOMETRY" if key == "model" => {
                let toks: Vec<&str> = rest.split_whitespace().collect();
                let nums: Vec<usize> = toks.iter().take_while(|t| t.parse::<usize>().is_ok()).map(|t| t.parse().unwrap()).collect();
                c.geometry_ts = nums.first().copied();
                c.geometry = toks.get(nums.len()).ok_or_else(|| anyhow!("model: without a file"))?.to_string();
            }
            "VARIABLE" => {
                let k = key.to_string();
                let Some((kind, per)) = k.split_once(" per ") else { continue };
                if kind.starts_with("complex") || per.starts_with("measured") || kind.starts_with("constant") {
                    continue;
                }
                let toks: Vec<&str> = rest.split_whitespace().collect();
                let nums: Vec<usize> = toks.iter().take(2).take_while(|t| t.parse::<usize>().is_ok()).map(|t| t.parse().unwrap()).collect();
                let rest_toks = &toks[nums.len()..];
                if rest_toks.len() < 2 {
                    continue;
                }
                c.variables.push(Variable {
                    kind: kind.to_string(),
                    per_node: per.starts_with("node"),
                    name: rest_toks[..rest_toks.len() - 1].join(" "),
                    file: rest_toks[rest_toks.len() - 1].to_string(),
                    time_set: nums.first().copied(),
                });
            }
            "TIME" => match key.as_str() {
                "time set" => {
                    flush(&mut c, ts_id, nsteps, start, incr);
                    ts_id = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(1);
                    (nsteps, start, incr) = (0, 0, 1);
                    c.time_sets.entry(ts_id).or_default();
                }
                "number of steps" => nsteps = rest.parse().unwrap_or(0),
                "filename start number" => start = rest.parse().unwrap_or(0),
                "filename increment" => incr = rest.parse().unwrap_or(1),
                "time values" | "filename numbers" => {
                    let t = c.time_sets.entry(ts_id).or_default();
                    let is_times = key == "time values";
                    for v in rest.split_whitespace().filter_map(|v| v.parse::<f64>().ok()) {
                        if is_times {
                            t.times.push(v);
                        } else {
                            t.numbers.push(v as i64);
                        }
                    }
                    pending = Some(if is_times { "times" } else { "numbers" });
                }
                _ => {}
            },
            "FILE" => bail!("{}: single-file EnSight time steps (FILE section) are not supported yet", path.display()),
            _ => {}
        }
    }
    flush(&mut c, ts_id, nsteps, start, incr);
    if c.geometry.is_empty() {
        bail!("{}: no geometry file", path.display());
    }
    Ok(c)
}

/// A file name with its `*` run replaced by the step's number (zero-padded).
fn step_file(case: &Case, name: &str, ts: Option<usize>, step: usize) -> PathBuf {
    let name = if name.contains('*') {
        let n = ts.and_then(|t| case.time_sets.get(&t)).and_then(|t| t.numbers.get(step).copied()).unwrap_or(step as i64);
        let stars = name.chars().filter(|c| *c == '*').count();
        let first = name.find('*').unwrap();
        format!("{}{:0width$}{}", &name[..first], n, &name[first + stars..], width = stars)
    } else {
        name.to_string()
    };
    case.dir.join(name)
}

// ---- token streams over ASCII and C Binary files ----

enum Stream {
    Text { lines: Vec<String>, line: usize, tokens: std::collections::VecDeque<String> },
    Binary { data: Vec<u8>, pos: usize, big: bool },
}

impl Stream {
    fn open(path: &Path) -> Result<Stream> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if data.len() >= 8 && data[..8].eq_ignore_ascii_case(b"C Binary") {
            return Ok(Stream::Binary { data, pos: 80, big: false });
        }
        if data.len() >= 14 && data[..14].eq_ignore_ascii_case(b"Fortran Binary") {
            bail!("{}: Fortran binary EnSight files are not supported (use C Binary or ASCII)", path.display());
        }
        Ok(Stream::Text { lines: String::from_utf8_lossy(&data).lines().map(|s| s.to_string()).collect(), line: 0, tokens: Default::default() })
    }

    /// A variable file: binary when not readable as text (variable files have no "C Binary").
    fn open_var(path: &Path, binary: bool, big: bool) -> Result<Stream> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if binary {
            Ok(Stream::Binary { data, pos: 0, big })
        } else {
            Ok(Stream::Text { lines: String::from_utf8_lossy(&data).lines().map(|s| s.to_string()).collect(), line: 0, tokens: Default::default() })
        }
    }

    fn at_end(&mut self) -> bool {
        match self {
            Stream::Text { lines, line, tokens } => tokens.is_empty() && lines[*line..].iter().all(|l| l.trim().is_empty()),
            Stream::Binary { data, pos, .. } => *pos + 80 > data.len(),
        }
    }

    /// The next description/keyword line.
    fn string(&mut self) -> Result<String> {
        match self {
            Stream::Text { lines, line, tokens } => {
                tokens.clear();
                while *line < lines.len() && lines[*line].trim().is_empty() {
                    *line += 1;
                }
                let s = lines.get(*line).ok_or_else(|| anyhow!("unexpected end of file"))?.clone();
                *line += 1;
                Ok(s.trim().to_string())
            }
            Stream::Binary { data, pos, .. } => {
                let s = data.get(*pos..*pos + 80).ok_or_else(|| anyhow!("unexpected end of file"))?;
                *pos += 80;
                Ok(String::from_utf8_lossy(s.split(|b| *b == 0).next().unwrap_or(&[])).trim().to_string())
            }
        }
    }

    /// Peek at the next line/string without consuming it.
    fn peek_string(&mut self) -> Option<String> {
        match self {
            Stream::Text { lines, line, tokens } => {
                if !tokens.is_empty() {
                    return None;
                }
                lines[*line..].iter().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string())
            }
            Stream::Binary { data, pos, .. } => {
                data.get(*pos..*pos + 80).map(|s| String::from_utf8_lossy(s.split(|b| *b == 0).next().unwrap_or(&[])).trim().to_string())
            }
        }
    }

    fn token(&mut self) -> Result<String> {
        let Stream::Text { lines, line, tokens } = self else { unreachable!() };
        while tokens.is_empty() {
            let l = lines.get(*line).ok_or_else(|| anyhow!("unexpected end of file"))?;
            *line += 1;
            tokens.extend(split_numbers(l));
        }
        Ok(tokens.pop_front().unwrap())
    }

    fn ints(&mut self, n: usize) -> Result<Vec<i64>> {
        match self {
            Stream::Binary { data, pos, big } => {
                let b = data.get(*pos..*pos + 4 * n).ok_or_else(|| anyhow!("unexpected end of file"))?;
                *pos += 4 * n;
                Ok(b.chunks_exact(4)
                    .map(|c| {
                        let a = [c[0], c[1], c[2], c[3]];
                        (if *big { i32::from_be_bytes(a) } else { i32::from_le_bytes(a) }) as i64
                    })
                    .collect())
            }
            _ => (0..n).map(|_| self.token().and_then(|t| t.parse::<i64>().map_err(|e| anyhow!("{t:?}: {e}")))).collect(),
        }
    }

    fn int(&mut self) -> Result<i64> {
        Ok(self.ints(1)?[0])
    }

    fn floats(&mut self, n: usize) -> Result<Vec<f32>> {
        match self {
            Stream::Binary { data, pos, big } => {
                let b = data.get(*pos..*pos + 4 * n).ok_or_else(|| anyhow!("unexpected end of file"))?;
                *pos += 4 * n;
                Ok(b.chunks_exact(4)
                    .map(|c| {
                        let a = [c[0], c[1], c[2], c[3]];
                        if *big {
                            f32::from_be_bytes(a)
                        } else {
                            f32::from_le_bytes(a)
                        }
                    })
                    .collect())
            }
            _ => (0..n).map(|_| self.token().and_then(|t| t.parse::<f32>().map_err(|e| anyhow!("{t:?}: {e}")))).collect(),
        }
    }
}

/// Numbers on a line; fixed-width fields may run together ("1.0e+00-2.0e+00").
fn split_numbers(l: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in l.split_whitespace() {
        let b = w.as_bytes();
        let mut start = 0;
        for i in 1..b.len() {
            if (b[i] == b'-' || b[i] == b'+') && !matches!(b[i - 1], b'e' | b'E') {
                out.push(w[start..i].to_string());
                start = i;
            }
        }
        out.push(w[start..].to_string());
    }
    out
}

/// Element types: (EnSight name, nodes, VTK type, reordering from EnSight to VTK).
fn element(name: &str) -> Option<(usize, CellType, &'static [usize])> {
    let n = name.strip_prefix("g_").unwrap_or(name);
    Some(match n {
        "point" => (1, CellType::Vertex, &[]),
        "bar2" => (2, CellType::Line, &[]),
        "bar3" => (3, CellType::QuadraticEdge, &[]),
        "tria3" => (3, CellType::Triangle, &[]),
        "tria6" => (6, CellType::QuadraticTriangle, &[]),
        "quad4" => (4, CellType::Quad, &[]),
        "quad8" => (8, CellType::QuadraticQuad, &[]),
        "tetra4" => (4, CellType::Tetra, &[]),
        "tetra10" => (10, CellType::QuadraticTetra, &[]),
        "pyramid5" => (5, CellType::Pyramid, &[]),
        "pyramid13" => (13, CellType::QuadraticPyramid, &[]),
        // EnSight wedges run the other way round (as VTK's reader converts them).
        "penta6" => (6, CellType::Wedge, &[0, 2, 1, 3, 5, 4]),
        "penta15" => (15, CellType::QuadraticWedge, &[0, 2, 1, 3, 5, 4, 8, 7, 6, 11, 10, 9, 12, 14, 13]),
        "hexa8" => (8, CellType::Hexahedron, &[]),
        "hexa20" => (20, CellType::QuadraticHexahedron, &[]),
        "nsided" | "nfaced" => (0, CellType::Polygon, &[]),
        _ => return None,
    })
}

const ELEMENT_NAMES: [&str; 17] = [
    "point",
    "bar2",
    "bar3",
    "tria3",
    "tria6",
    "quad4",
    "quad8",
    "tetra4",
    "tetra10",
    "pyramid5",
    "pyramid13",
    "penta6",
    "penta15",
    "hexa8",
    "hexa20",
    "nsided",
    "nfaced",
];

fn is_element(s: &str) -> bool {
    let n = s.strip_prefix("g_").unwrap_or(s);
    ELEMENT_NAMES.contains(&n)
}

/// One part's geometry and, per element group, (type name, cell count) for element variables.
struct Part {
    number: i64,
    name: String,
    data: DataSet,
    groups: Vec<(String, usize)>,
    nodes: usize,
}

fn read_geometry(path: &Path) -> Result<(Vec<Part>, bool, bool)> {
    let mut s = Stream::open(path)?;
    let binary = matches!(s, Stream::Binary { .. });
    if binary {
        // Byte order: the part number after "part" must be small.
        if let Stream::Binary { data, big, .. } = &mut s {
            let pos = data.windows(4).position(|w| w == b"part").unwrap_or(0);
            let p = pos / 80 * 80 + 80;
            if let Some(b) = data.get(p..p + 4) {
                let le = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                *big = !(0..1_000_000).contains(&le);
            }
        }
    }
    let big = matches!(s, Stream::Binary { big: true, .. });
    s.string()?; // description 1
    s.string()?; // description 2
    let node_ids = s.string()?.to_ascii_lowercase();
    let elem_ids = s.string()?.to_ascii_lowercase();
    let node_ids_present = node_ids.contains("given") || node_ids.contains("ignore");
    let elem_ids_present = elem_ids.contains("given") || elem_ids.contains("ignore");
    if s.peek_string().is_some_and(|l| l.starts_with("extents")) {
        s.string()?;
        s.floats(6)?;
    }
    let mut parts = Vec::new();
    while !s.at_end() {
        let kw = s.string()?;
        if !kw.starts_with("part") {
            bail!("{}: expected \"part\", found {kw:?}", path.display());
        }
        let number = s.int()?;
        let name = s.string()?;
        let kind = s.string()?.to_ascii_lowercase();
        if kind.starts_with("block") {
            let dims = s.ints(3)?;
            let (ni, nj, nk) = (dims[0].max(1) as usize, dims[1].max(1) as usize, dims[2].max(1) as usize);
            let n = ni * nj * nk;
            let pts: Vec<f64> = if kind.contains("uniform") {
                let o = s.floats(3)?;
                let d = s.floats(3)?;
                (0..n)
                    .flat_map(|p| {
                        let (i, j, k) = (p % ni, (p / ni) % nj, p / (ni * nj));
                        [o[0] as f64 + i as f64 * d[0] as f64, o[1] as f64 + j as f64 * d[1] as f64, o[2] as f64 + k as f64 * d[2] as f64]
                    })
                    .collect()
            } else if kind.contains("rectilinear") {
                let (x, y, z) = (s.floats(ni)?, s.floats(nj)?, s.floats(nk)?);
                (0..n).flat_map(|p| [x[p % ni] as f64, y[(p / ni) % nj] as f64, z[p / (ni * nj)] as f64]).collect()
            } else {
                let (x, y, z) = (s.floats(n)?, s.floats(n)?, s.floats(n)?);
                (0..n).flat_map(|p| [x[p] as f64, y[p] as f64, z[p] as f64]).collect()
            };
            if kind.contains("iblanked") {
                s.ints(n)?;
            }
            let nc = ni.saturating_sub(1).max(1) * nj.saturating_sub(1).max(1) * nk.saturating_sub(1).max(1);
            if kind.contains("with_ghost") {
                s.ints(nc)?;
            }
            let sg = StructuredGrid {
                extent: [0, ni as i64 - 1, 0, nj as i64 - 1, 0, nk as i64 - 1],
                points: Points::new_f64(pts),
                point_data: Attributes::new(),
                cell_data: Attributes::new(),
                field_data: Attributes::new(),
            };
            parts.push(Part { number, name, data: DataSet::StructuredGrid(sg), groups: vec![("block".into(), nc)], nodes: n });
            continue;
        }
        if !kind.starts_with("coordinates") {
            bail!("{}: part {number}: expected coordinates or block, found {kind:?}", path.display());
        }
        let nn = s.int()? as usize;
        if node_ids_present {
            s.ints(nn)?;
        }
        let (x, y, z) = (s.floats(nn)?, s.floats(nn)?, s.floats(nn)?);
        let points = Points::new_f32((0..nn).flat_map(|i| [x[i], y[i], z[i]]).collect());
        let mut cells = Cells::new();
        let mut faces: Vec<Vec<Vec<u32>>> = Vec::new();
        let mut groups = Vec::new();
        while let Some(next) = s.peek_string() {
            if !is_element(&next.to_ascii_lowercase()) {
                break;
            }
            let ty = s.string()?.to_ascii_lowercase();
            let ghost = ty.starts_with("g_");
            let (npe, vtk, order) = element(&ty).unwrap();
            let ne = s.int()? as usize;
            if elem_ids_present {
                s.ints(ne)?;
            }
            let base = ty.trim_start_matches("g_");
            let push = |cells: &mut Cells, ty: CellType, ids: &[i64]| -> Result<()> {
                let ids: Vec<u32> = ids.iter().map(|&i| (i - 1) as u32).collect();
                if ids.iter().any(|&i| i as usize >= nn) {
                    bail!("node number out of range in {base}");
                }
                cells.push(ty, &ids);
                Ok(())
            };
            match base {
                "nsided" => {
                    let counts = s.ints(ne)?;
                    for &c in &counts {
                        let ids = s.ints(c as usize)?;
                        if !ghost {
                            push(&mut cells, CellType::Polygon, &ids)?;
                            faces.push(Vec::new());
                        }
                    }
                }
                "nfaced" => {
                    let nfaces = s.ints(ne)?;
                    let total: i64 = nfaces.iter().sum();
                    let npf = s.ints(total as usize)?;
                    let mut f = 0;
                    for &nf in &nfaces {
                        let mut cell_faces = Vec::new();
                        let mut all: Vec<i64> = Vec::new();
                        for _ in 0..nf {
                            let ids = s.ints(npf[f] as usize)?;
                            f += 1;
                            all.extend(&ids);
                            cell_faces.push(ids.iter().map(|&i| (i - 1) as u32).collect());
                        }
                        all.sort_unstable();
                        all.dedup();
                        if !ghost {
                            push(&mut cells, CellType::Polyhedron, &all)?;
                            faces.push(cell_faces);
                        }
                    }
                }
                _ => {
                    let conn = s.ints(ne * npe)?;
                    if !ghost {
                        for e in conn.chunks_exact(npe) {
                            let ids: Vec<i64> = if order.is_empty() { e.to_vec() } else { order.iter().map(|&k| e[k]).collect() };
                            push(&mut cells, vtk, &ids)?;
                            faces.push(Vec::new());
                        }
                    }
                }
            }
            groups.push((ty.clone(), if ghost { 0 } else { ne }));
        }
        if faces.iter().any(|f| !f.is_empty()) {
            cells.polyhedron_faces = Some(faces);
        }
        let ug = UnstructuredGrid { points, cells, point_data: Attributes::new(), cell_data: Attributes::new(), field_data: Attributes::new() };
        parts.push(Part { number, name, data: DataSet::UnstructuredGrid(ug), groups, nodes: nn });
    }
    Ok((parts, binary, big))
}

fn ncomp(kind: &str) -> usize {
    match kind {
        "scalar" => 1,
        "vector" => 3,
        "tensor symm" => 6,
        "tensor asym" => 9,
        _ => 1,
    }
}

/// Read one variable file into the parts.
fn read_variable(path: &Path, v: &Variable, parts: &mut [Part], binary: bool, big: bool) -> Result<()> {
    let mut s = Stream::open_var(path, binary, big)?;
    let nc = ncomp(&v.kind);
    s.string()?; // description
                 // Per part: values component by component, then interleaved as VTK wants.
    let mut values: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    while !s.at_end() {
        let kw = s.string()?;
        if !kw.starts_with("part") {
            break;
        }
        let number = s.int()?;
        let Some(part) = parts.iter().find(|p| p.number == number) else { bail!("part {number} is not in the geometry") };
        let mut out = Vec::new();
        if v.per_node {
            let what = s.string()?.to_ascii_lowercase();
            if what.contains("undef") || what.contains("partial") {
                bail!("undefined/partial values are not supported yet");
            }
            let n = part.nodes;
            let comps: Vec<Vec<f32>> = (0..nc).map(|_| s.floats(n)).collect::<Result<_>>()?;
            out = (0..n).flat_map(|i| comps.iter().map(move |c| c[i])).collect::<Vec<f32>>();
        } else {
            for (gname, ne) in &part.groups {
                let ty = s.string()?.to_ascii_lowercase();
                if &ty != gname && !(gname == "block" && ty.starts_with("block")) {
                    bail!("element type {ty:?} where {gname:?} was expected");
                }
                let n = if gname.starts_with("g_") {
                    // Ghost elements have values too; skip them.
                    let ne_g = part.groups.iter().find(|g| &g.0 == gname).map(|g| g.1).unwrap_or(0);
                    s.floats(ne_g * nc)?;
                    continue;
                } else {
                    *ne
                };
                let comps: Vec<Vec<f32>> = (0..nc).map(|_| s.floats(n)).collect::<Result<_>>()?;
                out.extend((0..n).flat_map(|i| comps.iter().map(move |c| c[i])));
            }
        }
        values.insert(number, out);
    }
    for p in parts.iter_mut() {
        let Some(vals) = values.remove(&p.number) else { continue };
        // Symmetric tensors: EnSight 11 22 33 12 13 23; VTK's 6-component order is XX YY ZZ XY YZ XZ.
        let vals = if nc == 6 { vals.chunks_exact(6).flat_map(|t| [t[0], t[1], t[2], t[3], t[5], t[4]]).collect() } else { vals };
        let arr = Array::new(v.name.clone(), nc, ArrayData::F32(vals));
        let attrs = if v.per_node { p.data.point_data_mut() } else { p.data.cell_data_mut() };
        if let Some(a) = attrs {
            a.insert(arr);
        }
    }
    Ok(())
}

fn read_case_step(path: &Path, case: &Case, step: usize) -> Result<DataSet> {
    let geo = step_file(case, &case.geometry, case.geometry_ts, step);
    let (mut parts, binary, big) = read_geometry(&geo).with_context(|| format!("geometry {}", geo.display()))?;
    for v in &case.variables {
        let f = step_file(case, &v.file, v.time_set, step);
        if let Err(e) = read_variable(&f, v, &mut parts, binary, big) {
            tracing::warn!("EnSight variable {} ({}): {e:#}", v.name, f.display());
        }
    }
    let _ = path;
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    for p in parts {
        mb.blocks.push((p.name, Some(p.data)));
    }
    Ok(DataSet::MultiBlock(mb))
}

pub fn open(path: &Path) -> Result<Loaded> {
    let case = parse_case(path)?;
    // The time set of the geometry, else of the first variable with one.
    let ts = case.geometry_ts.or_else(|| case.variables.iter().find_map(|v| v.time_set)).and_then(|t| case.time_sets.get(&t));
    match ts {
        Some(t) if t.times.len() > 1 => {
            let mut s = TimeSeries { times: t.times.clone(), steps: Vec::new() };
            for i in 0..t.times.len() {
                s.steps.push(TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i });
            }
            Ok(Loaded::Series(s))
        }
        _ => Ok(Loaded::Static(Arc::new(read_case_step(path, &case, 0)?))),
    }
}

pub fn read_step(path: &Path, step: usize) -> Result<DataSet> {
    let case = parse_case(path)?;
    read_case_step(path, &case, step)
}
