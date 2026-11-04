//! OpenFOAM case reader (reconstructed cases; ASCII or binary; optional .gz files).
//!
//! Open the case through an (empty) `<case>.foam` file, like ParaView. The internal mesh is
//! built from `constant/polyMesh`; hexahedra, prisms and tetrahedra are recognised, other
//! cells become polyhedra. Fields from each time directory become cell data, averaged to
//! points as well.

use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

struct CaseInfo {
    times: Vec<(f64, String)>,
    mesh: Arc<UnstructuredGrid>,
}

static CASES: LazyLock<Mutex<HashMap<PathBuf, Arc<CaseInfo>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn read_maybe_gz(path: &Path) -> Result<Vec<u8>> {
    if path.exists() {
        return std::fs::read(path).with_context(|| format!("reading {}", path.display()));
    }
    let gz = PathBuf::from(format!("{}.gz", path.display()));
    if gz.exists() {
        let raw = std::fs::read(&gz)?;
        let mut out = Vec::new();
        flate2::read::MultiGzDecoder::new(&raw[..]).read_to_end(&mut out)?;
        return Ok(out);
    }
    bail!("missing {}", path.display())
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
    binary: bool,
    label64: bool,
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self {
        let head = String::from_utf8_lossy(&b[..b.len().min(2000)]).to_string();
        let binary = head.contains("format") && head.split("format").nth(1).map(|r| r.trim_start().starts_with("binary")).unwrap_or(false);
        let label64 = head.contains("label=64");
        let mut c = Cur { b, p: 0, binary, label64 };
        // Skip the FoamFile header dictionary.
        if let Some(i) = memchr::memmem::find(b, b"FoamFile") {
            if let Some(j) = memchr::memchr(b'}', &b[i..]) {
                c.p = i + j + 1;
            }
        }
        c
    }
    fn skip(&mut self) {
        loop {
            while self.p < self.b.len() && self.b[self.p].is_ascii_whitespace() {
                self.p += 1;
            }
            if self.b[self.p..].starts_with(b"//") {
                self.p += memchr::memchr(b'\n', &self.b[self.p..]).unwrap_or(self.b.len() - self.p);
            } else if self.b[self.p..].starts_with(b"/*") {
                self.p += memchr::memmem::find(&self.b[self.p..], b"*/").map(|i| i + 2).unwrap_or(self.b.len() - self.p);
            } else {
                break;
            }
        }
    }
    fn peek(&mut self) -> u8 {
        self.skip();
        *self.b.get(self.p).unwrap_or(&0)
    }
    fn expect(&mut self, ch: u8) -> Result<()> {
        if self.peek() != ch {
            bail!("expected '{}' at byte {}", ch as char, self.p);
        }
        self.p += 1;
        Ok(())
    }
    fn word(&mut self) -> String {
        self.skip();
        let s = self.p;
        while self.p < self.b.len() && !self.b[self.p].is_ascii_whitespace() && !b"(){};".contains(&self.b[self.p]) {
            self.p += 1;
        }
        String::from_utf8_lossy(&self.b[s..self.p]).to_string()
    }
    fn number(&mut self) -> Result<f64> {
        let w = self.word();
        w.parse::<f64>().map_err(|_| anyhow!("expected a number, found {w:?} at byte {}", self.p))
    }
    /// List of labels, scalars or n-vectors (ncomp > 1), flattened.
    fn list(&mut self, ncomp: usize, is_label: bool) -> Result<Vec<f64>> {
        let n = self.number()? as usize;
        if self.binary {
            self.expect(b'(')?;
            let sz = if is_label {
                if self.label64 {
                    8
                } else {
                    4
                }
            } else {
                8
            };
            let nb = n * ncomp * sz;
            let raw = self.b.get(self.p..self.p + nb).ok_or_else(|| anyhow!("truncated binary list"))?;
            let v = raw
                .chunks_exact(sz)
                .map(|c| match (is_label, sz) {
                    (true, 4) => i32::from_le_bytes(c.try_into().unwrap()) as f64,
                    (true, _) => i64::from_le_bytes(c.try_into().unwrap()) as f64,
                    _ => f64::from_le_bytes(c.try_into().unwrap()),
                })
                .collect();
            self.p += nb;
            self.expect(b')')?;
            return Ok(v);
        }
        // ASCII may also be "N{value}" (uniform list).
        if self.peek() == b'{' {
            self.p += 1;
            let v = self.number()?;
            self.expect(b'}')?;
            return Ok(vec![v; n * ncomp]);
        }
        self.expect(b'(')?;
        let mut out = Vec::with_capacity(n * ncomp);
        for _ in 0..n {
            if ncomp > 1 {
                self.expect(b'(')?;
                for _ in 0..ncomp {
                    out.push(self.number()?);
                }
                self.expect(b')')?;
            } else {
                out.push(self.number()?);
            }
        }
        self.expect(b')')?;
        Ok(out)
    }
    /// faceList (ASCII "N( 4(a b c d) ...)") or faceCompactList (two lists, used in binary).
    fn faces(&mut self) -> Result<Vec<Vec<u32>>> {
        let save = self.p;
        let n = self.number()? as usize;
        if self.binary {
            self.p = save;
            let offsets = self.list(1, true)?;
            let conn = self.list(1, true)?;
            return Ok(offsets.windows(2).map(|w| conn[w[0] as usize..w[1] as usize].iter().map(|&x| x as u32).collect()).collect());
        }
        self.expect(b'(')?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.number()? as usize;
            self.expect(b'(')?;
            let mut f = Vec::with_capacity(k);
            for _ in 0..k {
                f.push(self.number()? as u32);
            }
            self.expect(b')')?;
            out.push(f);
        }
        self.expect(b')')?;
        Ok(out)
    }
}

/// Order 8 points of a hexahedral cell (6 quads) the VTK way, if it is one.
fn hex_from_faces(faces: &[Vec<u32>]) -> Option<Vec<u32>> {
    if faces.len() != 6 || faces.iter().any(|f| f.len() != 4) {
        return None;
    }
    let bottom: Vec<u32> = faces[0].iter().rev().copied().collect(); // outward -> inward winding
    let top = faces[1..].iter().find(|f| f.iter().all(|p| !bottom.contains(p)))?;
    let mut ids = bottom.clone();
    for &b in &bottom {
        // The top point that shares an edge with b in some side face.
        let t = faces.iter().filter(|f| f.contains(&b) && !std::ptr::eq(*f, top)).flat_map(|f| {
            let i = f.iter().position(|&x| x == b).unwrap();
            [f[(i + 1) % 4], f[(i + 3) % 4]]
        });
        let tp = t.into_iter().find(|p| top.contains(p))?;
        ids.push(tp);
    }
    Some(ids)
}

fn wedge_from_faces(faces: &[Vec<u32>]) -> Option<Vec<u32>> {
    if faces.len() != 5 {
        return None;
    }
    let tris: Vec<&Vec<u32>> = faces.iter().filter(|f| f.len() == 3).collect();
    if tris.len() != 2 || faces.iter().filter(|f| f.len() == 4).count() != 3 {
        return None;
    }
    let bottom: Vec<u32> = tris[0].iter().rev().copied().collect();
    let top = tris[1];
    let mut ids = bottom.clone();
    for &b in &bottom {
        let tp = faces.iter().filter(|f| f.len() == 4 && f.contains(&b)).flat_map(|f| {
            let i = f.iter().position(|&x| x == b).unwrap();
            [f[(i + 1) % 4], f[(i + 3) % 4]]
        });
        let tp = tp.into_iter().find(|p| top.contains(p))?;
        ids.push(tp);
    }
    Some(ids)
}

fn build_mesh(case: &Path, mesh_dir: &Path) -> Result<UnstructuredGrid> {
    let pts_b = read_maybe_gz(&mesh_dir.join("points"))?;
    let pts = Cur::new(&pts_b).list(3, false).context("polyMesh/points")?;
    let faces_b = read_maybe_gz(&mesh_dir.join("faces"))?;
    let faces = Cur::new(&faces_b).faces().context("polyMesh/faces")?;
    let own_b = read_maybe_gz(&mesh_dir.join("owner"))?;
    let owner = Cur::new(&own_b).list(1, true).context("polyMesh/owner")?;
    let nei_b = read_maybe_gz(&mesh_dir.join("neighbour"))?;
    let neighbour = Cur::new(&nei_b).list(1, true).context("polyMesh/neighbour")?;
    let ncells = owner.iter().chain(&neighbour).fold(0usize, |m, &c| m.max(c as usize + 1));
    let mut cell_faces: Vec<Vec<Vec<u32>>> = vec![Vec::new(); ncells];
    for (fi, f) in faces.iter().enumerate() {
        if let Some(&o) = owner.get(fi) {
            cell_faces[o as usize].push(f.clone());
        }
        if let Some(&n) = neighbour.get(fi) {
            // Neighbour sees the face reversed (outward for it).
            cell_faces[n as usize].push(f.iter().rev().copied().collect());
        }
    }
    let mut cells = Cells::with_capacity(ncells, ncells * 8);
    let mut poly_faces: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut any_poly = false;
    for cf in &cell_faces {
        if let Some(h) = hex_from_faces(cf) {
            cells.push(CellType::Hexahedron, &h);
            poly_faces.push(Vec::new());
        } else if let Some(w) = wedge_from_faces(cf) {
            cells.push(CellType::Wedge, &w);
            poly_faces.push(Vec::new());
        } else if cf.len() == 4 && cf.iter().all(|f| f.len() == 3) {
            let base: Vec<u32> = cf[0].iter().rev().copied().collect();
            let apex = cf[1].iter().copied().find(|p| !base.contains(p)).unwrap_or(base[0]);
            cells.push(CellType::Tetra, &[base[0], base[1], base[2], apex]);
            poly_faces.push(Vec::new());
        } else {
            let mut ids: Vec<u32> = cf.iter().flatten().copied().collect();
            ids.sort_unstable();
            ids.dedup();
            cells.push(CellType::Polyhedron, &ids);
            poly_faces.push(cf.clone());
            any_poly = true;
        }
    }
    if any_poly {
        cells.polyhedron_faces = Some(poly_faces);
    }
    let mut fd = Attributes::new();
    fd.insert(Array::new("CasePath", 1, ArrayData::String(vec![case.display().to_string()])));
    Ok(UnstructuredGrid { points: Points::new_f64(pts), cells, field_data: fd, ..Default::default() })
}

fn case_dir(foam: &Path) -> PathBuf {
    if foam.is_dir() {
        foam.to_path_buf()
    } else {
        foam.parent().unwrap_or(Path::new(".")).to_path_buf()
    }
}

fn time_dirs(case: &Path) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = std::fs::read_dir(case)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.parse::<f64>().ok().map(|t| (t, n))
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    // A "0" directory without fields (only initial conditions dir names) is still a valid step.
    out
}

fn info(path: &Path) -> Result<Arc<CaseInfo>> {
    if let Some(c) = CASES.lock().get(path) {
        return Ok(c.clone());
    }
    let case = case_dir(path);
    let mesh_dir = case.join("constant/polyMesh");
    if !mesh_dir.exists() {
        if case.join("processor0").exists() {
            bail!("{} is a decomposed case; run reconstructPar first (decomposed cases are not supported yet)", case.display());
        }
        bail!("{} has no constant/polyMesh", case.display());
    }
    let mesh = Arc::new(build_mesh(&case, &mesh_dir)?);
    let c = Arc::new(CaseInfo { times: time_dirs(&case), mesh });
    CASES.lock().insert(path.to_path_buf(), c.clone());
    Ok(c)
}

pub fn open(path: &Path) -> Result<Loaded> {
    let c = info(path)?;
    if c.times.len() <= 1 {
        return Ok(Loaded::Static(Arc::new(read_time(path, 0)?)));
    }
    let mut s = TimeSeries::default();
    for (i, (t, _)) in c.times.iter().enumerate() {
        s.times.push(*t);
        s.steps.push(TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i });
    }
    Ok(Loaded::Series(s))
}

/// Parse a volScalarField / volVectorField internalField.
fn read_field(bytes: &[u8], ncells: usize) -> Result<Option<(usize, Vec<f64>)>> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2000)]).to_string();
    let ncomp = if head.contains("volVectorField") {
        3
    } else if head.contains("volScalarField") {
        1
    } else if head.contains("volSymmTensorField") {
        6
    } else if head.contains("volTensorField") {
        9
    } else {
        return Ok(None);
    };
    let mut c = Cur::new(bytes);
    let at = memchr::memmem::find(&bytes[c.p..], b"internalField").ok_or_else(|| anyhow!("no internalField"))?;
    c.p += at + "internalField".len();
    let kind = c.word();
    let vals = match kind.as_str() {
        "uniform" => {
            let v = if ncomp == 1 {
                vec![c.number()?]
            } else {
                c.expect(b'(')?;
                let v: Vec<f64> = (0..ncomp).map(|_| c.number()).collect::<Result<_>>()?;
                c.expect(b')')?;
                v
            };
            (0..ncells).flat_map(|_| v.clone()).collect()
        }
        "nonuniform" => {
            let _ty = c.word(); // List<scalar>
            c.list(ncomp, false)?
        }
        k => bail!("unsupported internalField kind {k:?}"),
    };
    if vals.len() != ncells * ncomp {
        bail!("field has {} values, mesh has {ncells} cells", vals.len() / ncomp.max(1));
    }
    Ok(Some((ncomp, vals)))
}

/// Mesh plus the fields of time directory number `index`.
pub fn read_time(path: &Path, index: usize) -> Result<DataSet> {
    let c = info(path)?;
    let mut ug = (*c.mesh).clone();
    let ncells = ug.cells.len();
    if let Some((t, dir)) = c.times.get(index) {
        let tdir = case_dir(path).join(dir);
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&tdir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        entries.sort();
        for f in entries {
            let name = f.file_name().unwrap().to_string_lossy().trim_end_matches(".gz").to_string();
            if name.starts_with('.') || name.ends_with(".orig") {
                continue;
            }
            let bytes = match read_maybe_gz(&f) {
                Ok(b) => b,
                Err(_) => continue,
            };
            match read_field(&bytes, ncells) {
                Ok(Some((nc, v))) => {
                    ug.cell_data.insert(Array::new(name, nc, ArrayData::F64(v)));
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("OpenFOAM field {}: {e:#}", f.display()),
            }
        }
        ug.field_data.insert(Array::scalars_f64("TimeValue", vec![*t]));
    }
    // ParaView-like default: also provide point data by averaging cells.
    let ds = DataSet::UnstructuredGrid(ug);
    let names: Vec<String> = ds.cell_data().map(|a| a.names().map(|s| s.to_string()).collect()).unwrap_or_default();
    if names.is_empty() {
        return Ok(ds);
    }
    Ok(aurafx_filters_free_cell_to_point(&ds))
}

/// Cell -> point averaging (local copy to avoid a dependency on the filters crate).
fn aurafx_filters_free_cell_to_point(ds: &DataSet) -> DataSet {
    let DataSet::UnstructuredGrid(u) = ds else { return ds.clone() };
    let n = u.points.len();
    let mut counts = vec![0u32; n];
    for i in 0..u.cells.len() {
        for &p in u.cells.cell(i) {
            counts[p as usize] += 1;
        }
    }
    let mut out = u.clone();
    for a in u.cell_data.iter() {
        let nc = a.ncomp;
        let mut sum = vec![0.0; n * nc];
        for i in 0..u.cells.len() {
            for &p in u.cells.cell(i) {
                for k in 0..nc {
                    sum[p as usize * nc + k] += a.get(i, k);
                }
            }
        }
        for i in 0..n {
            let c = counts[i].max(1) as f64;
            for k in 0..nc {
                sum[i * nc + k] /= c;
            }
        }
        out.point_data.insert(Array::new(a.name.clone(), nc, ArrayData::F64(sum)));
    }
    let first = out.point_data.iter().find(|a| a.ncomp == 1).map(|a| a.name.clone());
    if let Some(first) = first {
        out.point_data.set_active(AttributeKind::Scalars, &first);
    }
    DataSet::UnstructuredGrid(out)
}
