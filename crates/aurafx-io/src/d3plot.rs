//! LS-DYNA results (`d3plot` and its family files `d3plot01`, `d3plot02`, ...): the 64 control
//! words, the geometry (nodes; solids, thick shells, beams and shells with their materials;
//! user numbering), and every state: time, global values, the current node positions (the
//! mesh moves; `Displacement` is the move), velocities, accelerations, temperatures, and the
//! element values (solid and shell stresses with effective plastic strain, beam resultants).
//! Single (4-byte) and double (8-byte) words, little or big endian.

use anyhow::{bail, Context, Result};
use aurafx_data::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The control words that matter here (named as in the database description).
#[derive(Debug, Clone, Default)]
struct Control {
    ndim: usize,
    numnp: usize,
    nglbv: usize,
    it: usize,
    iu: bool,
    iv: bool,
    ia: bool,
    nel8: usize,
    ten_node_tets: bool,
    nv3d: usize,
    nel2: usize,
    nv1d: usize,
    nel4: usize,
    nv2d: usize,
    maxint: usize,
    mdlopt: u8,
    neips: usize,
    narbs: usize,
    nelt: usize,
    nv3dt: usize,
    ioshl: [bool; 4],
    nmmat: usize,
    extra: usize,
    numrbe_section: bool,
}

struct Words<'a> {
    b: &'a [u8],
    ws: usize,
    little: bool,
}

impl Words<'_> {
    fn len(&self) -> usize {
        self.b.len() / self.ws
    }
    fn i(&self, w: usize) -> i64 {
        let s = &self.b[w * self.ws..w * self.ws + self.ws];
        match (self.ws, self.little) {
            (4, true) => i32::from_le_bytes(s.try_into().unwrap()) as i64,
            (4, false) => i32::from_be_bytes(s.try_into().unwrap()) as i64,
            (_, true) => i64::from_le_bytes(s.try_into().unwrap()),
            (_, false) => i64::from_be_bytes(s.try_into().unwrap()),
        }
    }
    fn f(&self, w: usize) -> f64 {
        let s = &self.b[w * self.ws..w * self.ws + self.ws];
        match (self.ws, self.little) {
            (4, true) => f32::from_le_bytes(s.try_into().unwrap()) as f64,
            (4, false) => f32::from_be_bytes(s.try_into().unwrap()) as f64,
            (_, true) => f64::from_le_bytes(s.try_into().unwrap()),
            (_, false) => f64::from_be_bytes(s.try_into().unwrap()),
        }
    }
}

/// Word size and byte order: the version (word 14) is a float such as 960 or 971; NDIM is 2..7.
fn detect(b: &[u8]) -> Result<(usize, bool)> {
    for ws in [4usize, 8] {
        for little in [true, false] {
            if b.len() < 64 * ws {
                continue;
            }
            let w = Words { b, ws, little };
            let ndim = w.i(15);
            let numnp = w.i(16);
            if (2..=7).contains(&ndim) && numnp >= 0 && numnp < (1 << 40) {
                return Ok((ws, little));
            }
        }
    }
    bail!("not an LS-DYNA d3plot file (no sensible control words)")
}

fn control(w: &Words) -> Result<Control> {
    let i = |k: usize| w.i(k);
    let mut c = Control::default();
    let ndim = i(15);
    c.numrbe_section = ndim == 5 || ndim == 7;
    c.ndim = if ndim >= 4 { 3 } else { ndim as usize };
    c.numnp = i(16).max(0) as usize;
    c.nglbv = i(18).max(0) as usize;
    let it = i(19);
    c.it = (it % 10).max(0) as usize;
    c.iu = i(20) != 0;
    c.iv = i(21) != 0;
    c.ia = i(22) != 0;
    let nel8 = i(23);
    c.ten_node_tets = nel8 < 0;
    c.nel8 = nel8.unsigned_abs() as usize;
    c.nv3d = i(27).max(0) as usize;
    c.nel2 = i(28).max(0) as usize;
    c.nv1d = i(30).max(0) as usize;
    c.nel4 = i(31).max(0) as usize;
    c.nv2d = i(33).max(0) as usize;
    c.neips = i(35).max(0) as usize;
    let maxint = i(36);
    c.mdlopt = if maxint >= 0 {
        0
    } else if maxint < -10000 {
        2
    } else {
        1
    };
    c.maxint = if maxint >= 0 {
        maxint as usize
    } else if maxint < -10000 {
        (-maxint - 10000) as usize
    } else {
        (-maxint) as usize
    };
    c.narbs = i(39).max(0) as usize;
    c.nelt = i(40).max(0) as usize;
    c.nv3dt = i(42).max(0) as usize;
    for k in 0..4 {
        c.ioshl[k] = i(43 + k) == 1000;
    }
    c.nmmat = i(51).max(0) as usize;
    c.extra = i(57).max(0) as usize;
    if i(52) != 0 {
        bail!("d3plot files with fluid materials are not supported");
    }
    if i(55) != 0 || i(50) != 0 {
        bail!("d3plot files with 8-node shells or adaptive meshes are not supported");
    }
    Ok(c)
}

/// The family files of `path` (d3plot, d3plot01, d3plot02, ...).
fn family(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("d3plot").to_string();
    for k in 1..10000 {
        let p = path.with_file_name(format!("{name}{k:02}"));
        if !p.exists() {
            break;
        }
        out.push(p);
    }
    out
}

/// Turn a degenerate hexahedron (LS-DYNA writes tetrahedra, pyramids and wedges as hexahedra
/// with repeated nodes) into the cell it is.
fn solid_cell(n: &[u32]) -> (CellType, Vec<u32>) {
    let mut uniq: Vec<u32> = Vec::new();
    for &x in n {
        if !uniq.contains(&x) {
            uniq.push(x);
        }
    }
    match uniq.len() {
        4 => (CellType::Tetra, vec![n[0], n[1], n[2], n[4]]),
        5 => (CellType::Pyramid, vec![n[0], n[1], n[2], n[3], n[4]]),
        6 => (CellType::Wedge, vec![n[0], n[1], n[4], n[3], n[2], n[6]]),
        _ => (CellType::Hexahedron, n.to_vec()),
    }
}

/// Read a d3plot family as a time series.
pub fn read(path: &Path) -> Result<crate::Loaded> {
    let files = family(path);
    let first = std::fs::read(&files[0]).with_context(|| format!("reading {}", files[0].display()))?;
    let (ws, little) = detect(&first).with_context(|| files[0].display().to_string())?;
    let w = Words { b: &first, ws, little };
    let c = control(&w)?;
    let mut pos = 64 + c.extra;
    if c.numrbe_section {
        // Material types: NUMRBE, NUMMAT, then NUMMAT words.
        let nummat = w.i(pos + 1).max(0) as usize;
        pos += 2 + nummat;
    }
    let need = |p: usize, n: usize| -> Result<()> {
        if p + n > w.len() {
            bail!("{}: the geometry runs past the end of the file", files[0].display());
        }
        Ok(())
    };
    // Nodes.
    need(pos, c.ndim * c.numnp)?;
    let mut x0 = Vec::with_capacity(c.numnp * 3);
    for k in 0..c.numnp {
        for a in 0..3 {
            x0.push(if a < c.ndim { w.f(pos + k * c.ndim + a) } else { 0.0 });
        }
    }
    pos += c.ndim * c.numnp;
    let node = |v: i64| -> Result<u32> {
        if v < 1 || v as usize > c.numnp {
            bail!("an element refers to node {v} of {}", c.numnp);
        }
        Ok((v - 1) as u32)
    };
    let mut cells = Cells::new();
    let mut mat = Vec::new();
    let mut kind = Vec::new();
    need(pos, 9 * c.nel8)?;
    for e in 0..c.nel8 {
        let n: Vec<u32> = (0..8).map(|k| node(w.i(pos + 9 * e + k))).collect::<Result<_>>()?;
        let (t, ids) = solid_cell(&n);
        cells.push(t, &ids);
        mat.push(w.i(pos + 9 * e + 8));
        kind.push(1i64);
    }
    pos += 9 * c.nel8;
    if c.ten_node_tets {
        // Two extra nodes per element (only the corners are drawn).
        pos += 2 * c.nel8;
    }
    need(pos, 9 * c.nelt)?;
    for e in 0..c.nelt {
        let n: Vec<u32> = (0..8).map(|k| node(w.i(pos + 9 * e + k))).collect::<Result<_>>()?;
        cells.push(CellType::Hexahedron, &n);
        mat.push(w.i(pos + 9 * e + 8));
        kind.push(2);
    }
    pos += 9 * c.nelt;
    need(pos, 6 * c.nel2)?;
    for e in 0..c.nel2 {
        cells.push(CellType::Line, &[node(w.i(pos + 6 * e))?, node(w.i(pos + 6 * e + 1))?]);
        mat.push(w.i(pos + 6 * e + 5));
        kind.push(3);
    }
    pos += 6 * c.nel2;
    need(pos, 5 * c.nel4)?;
    for e in 0..c.nel4 {
        let n: Vec<u32> = (0..4).map(|k| node(w.i(pos + 5 * e + k))).collect::<Result<_>>()?;
        if n[2] == n[3] {
            cells.push(CellType::Triangle, &n[..3]);
        } else {
            cells.push(CellType::Quad, &n);
        }
        mat.push(w.i(pos + 5 * e + 4));
        kind.push(4);
    }
    pos += 5 * c.nel4;
    // User numbering.
    let (mut node_ids, mut elem_ids): (Vec<i64>, Vec<i64>) = ((1..=c.numnp as i64).collect(), (1..=cells.len() as i64).collect());
    if c.narbs > 0 {
        need(pos, c.narbs)?;
        let nsort = w.i(pos);
        let head = if nsort < 0 { 16 } else { 10 };
        let mut q = pos + head;
        node_ids = (0..c.numnp).map(|k| w.i(q + k)).collect();
        q += c.numnp;
        let solids: Vec<i64> = (0..c.nel8).map(|k| w.i(q + k)).collect();
        q += c.nel8;
        let beams: Vec<i64> = (0..c.nel2).map(|k| w.i(q + k)).collect();
        q += c.nel2;
        let shells: Vec<i64> = (0..c.nel4).map(|k| w.i(q + k)).collect();
        q += c.nel4;
        let thick: Vec<i64> = (0..c.nelt).map(|k| w.i(q + k)).collect();
        elem_ids = [solids, thick, beams, shells].concat();
        pos += c.narbs;
    }
    // The geometry ends here (or after an end marker in older files).
    if pos < w.len() && w.f(pos) == -999999.0 {
        pos += 1;
    }
    // Words per state.
    let nodal = c.it * c.numnp + c.ndim * c.numnp * (c.iu as usize + c.iv as usize + c.ia as usize);
    let elem = c.nel8 * c.nv3d + c.nelt * c.nv3dt + c.nel2 * c.nv1d + c.nel4 * c.nv2d;
    let deletion = match c.mdlopt {
        1 => c.numnp,
        2 => c.nel8 + c.nelt + c.nel4 + c.nel2,
        _ => 0,
    };
    let state_words = 1 + c.nglbv + nodal + elem + deletion;
    // Shell values layout: integration points of (6 stresses, plastic strain, extras), then
    // resultants (8), then thickness and energies (4), then strains (12).
    let shell_point = 6 * c.ioshl[0] as usize + c.ioshl[1] as usize + c.neips;
    let mut times = Vec::new();
    let mut steps = Vec::new();
    let base_cells = cells;
    let ncell = base_cells.len();
    for (fi, f) in files.iter().enumerate() {
        let bytes = if fi == 0 { first.clone() } else { std::fs::read(f).with_context(|| format!("reading {}", f.display()))? };
        let fw = Words { b: &bytes, ws, little };
        let mut p = if fi == 0 { pos } else { 0 };
        while p + state_words <= fw.len() {
            let t = fw.f(p);
            if t == -999999.0 {
                break;
            }
            let mut q = p + 1;
            let globals: Vec<f64> = (0..c.nglbv).map(|k| fw.f(q + k)).collect();
            q += c.nglbv;
            let temps: Option<Vec<f64>> = (c.it > 0).then(|| (0..c.numnp).map(|k| fw.f(q + c.it * k)).collect());
            q += c.it * c.numnp;
            let mut vec3 = |on: bool| -> Option<Vec<f64>> {
                if !on {
                    return None;
                }
                let v = (0..c.numnp).flat_map(|k| (0..3).map(move |a| (k, a))).map(|(k, a)| if a < c.ndim { fw.f(q + k * c.ndim + a) } else { 0.0 }).collect();
                q += c.ndim * c.numnp;
                Some(v)
            };
            let coords = vec3(c.iu);
            let vel = vec3(c.iv);
            let acc = vec3(c.ia);
            let pts = coords.clone().unwrap_or_else(|| x0.clone());
            let mut ug = UnstructuredGrid { points: Points::new_f64(pts.clone()), cells: base_cells.clone(), ..Default::default() };
            let pdat = &mut ug.point_data;
            pdat.insert(Array::new("NodeId", 1, ArrayData::I64(node_ids.clone())));
            if coords.is_some() {
                pdat.insert_active(AttributeKind::Vectors, Array::new("Displacement", 3, ArrayData::F64(pts.iter().zip(&x0).map(|(a, b)| a - b).collect())));
            }
            if let Some(v) = vel {
                pdat.insert(Array::new("Velocity", 3, ArrayData::F64(v)));
            }
            if let Some(a) = acc {
                pdat.insert(Array::new("Acceleration", 3, ArrayData::F64(a)));
            }
            if let Some(tt) = temps {
                pdat.insert(Array::new("Temperature", 1, ArrayData::F64(tt)));
            }
            // Element values: stress (6), effective plastic strain, beam resultants (6).
            let mut stress = vec![f64::NAN; ncell * 6];
            let mut eps = vec![f64::NAN; ncell];
            let mut beam = vec![f64::NAN; ncell * 6];
            let mut cell = 0;
            for e in 0..c.nel8 {
                let b0 = q + e * c.nv3d;
                if c.nv3d >= 6 {
                    for k in 0..6 {
                        stress[cell * 6 + k] = fw.f(b0 + k);
                    }
                }
                if c.nv3d >= 7 {
                    eps[cell] = fw.f(b0 + 6);
                }
                cell += 1;
            }
            q += c.nel8 * c.nv3d;
            for e in 0..c.nelt {
                let b0 = q + e * c.nv3dt;
                if c.nv3dt >= 7 {
                    for k in 0..6 {
                        stress[cell * 6 + k] = fw.f(b0 + k);
                    }
                    eps[cell] = fw.f(b0 + 6);
                }
                cell += 1;
            }
            q += c.nelt * c.nv3dt;
            for e in 0..c.nel2 {
                let b0 = q + e * c.nv1d;
                for k in 0..c.nv1d.min(6) {
                    beam[cell * 6 + k] = fw.f(b0 + k);
                }
                cell += 1;
            }
            q += c.nel2 * c.nv1d;
            for e in 0..c.nel4 {
                let b0 = q + e * c.nv2d;
                // The middle integration point's stress and strain.
                if c.ioshl[0] && c.maxint > 0 && shell_point > 0 {
                    let ip = b0 + (c.maxint / 2) * shell_point;
                    for k in 0..6 {
                        stress[cell * 6 + k] = fw.f(ip + k);
                    }
                    if c.ioshl[1] {
                        eps[cell] = fw.f(ip + 6);
                    }
                }
                cell += 1;
            }
            q += c.nel4 * c.nv2d;
            let cd = &mut ug.cell_data;
            cd.insert(Array::new("ElementId", 1, ArrayData::I64(elem_ids.clone())));
            cd.insert_active(AttributeKind::Scalars, Array::new("Material", 1, ArrayData::I64(mat.clone())));
            cd.insert(Array::new("ElementKind", 1, ArrayData::I64(kind.clone())));
            if c.nv3d >= 6 || c.ioshl[0] {
                let mut a = Array::new("Stress", 6, ArrayData::F64(stress));
                a.component_names = Some(["XX", "YY", "ZZ", "XY", "YZ", "ZX"].iter().map(|s| s.to_string()).collect());
                cd.insert(a);
                cd.insert(Array::new("EffectivePlasticStrain", 1, ArrayData::F64(eps)));
            }
            if c.nel2 > 0 && c.nv1d > 0 {
                let mut a = Array::new("BeamResultants", 6, ArrayData::F64(beam));
                a.component_names = Some(["AxialForce", "ShearS", "ShearT", "MomentS", "MomentT", "Torsion"].iter().map(|s| s.to_string()).collect());
                cd.insert(a);
            }
            if deletion > 0 {
                // 0 = deleted.
                let del: Vec<f64> = (0..deletion).map(|k| fw.f(q + k)).collect();
                if c.mdlopt == 2 {
                    cd.insert(Array::new("Alive", 1, ArrayData::F64(del)));
                } else {
                    ug.point_data.insert(Array::new("NodeAlive", 1, ArrayData::F64(del)));
                }
            }
            for (k, g) in globals.iter().enumerate().take(6) {
                let name = ["KineticEnergy", "InternalEnergy", "TotalEnergy", "VelocityX", "VelocityY", "VelocityZ"][k];
                ug.field_data.insert(Array::scalars_f64(name, vec![*g]));
            }
            times.push(t);
            steps.push(TimeStepRef::Loaded(Arc::new(DataSet::UnstructuredGrid(ug))));
            p += state_words;
        }
    }
    if steps.is_empty() {
        // Geometry only.
        let mut ug = UnstructuredGrid { points: Points::new_f64(x0), cells: base_cells, ..Default::default() };
        ug.point_data.insert(Array::new("NodeId", 1, ArrayData::I64(node_ids)));
        ug.cell_data.insert(Array::new("ElementId", 1, ArrayData::I64(elem_ids)));
        ug.cell_data.insert_active(AttributeKind::Scalars, Array::new("Material", 1, ArrayData::I64(mat)));
        return Ok(crate::Loaded::Static(Arc::new(DataSet::UnstructuredGrid(ug))));
    }
    if steps.len() == 1 {
        let TimeStepRef::Loaded(d) = steps.pop().unwrap() else { unreachable!() };
        return Ok(crate::Loaded::Static(d));
    }
    Ok(crate::Loaded::Series(TimeSeries { times, steps }))
}

/// `d3plot` (or a file of its family).
pub fn is_d3plot(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    (name == "d3plot" || name.ends_with(".d3plot")) && path.is_file()
}
