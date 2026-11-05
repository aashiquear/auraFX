//! Molecular / particle formats: XYZ and extended XYZ (multi-frame), LAMMPS data files.

use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;
use std::sync::Arc;

const ELEMENTS: [&str; 54] = [
    "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl", "Ar", "K", "Ca", "Sc", "Ti", "V", "Cr", "Mn", "Fe", "Co",
    "Ni", "Cu", "Zn", "Ga", "Ge", "As", "Se", "Br", "Kr", "Rb", "Sr", "Y", "Zr", "Nb", "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In", "Sn", "Sb", "Te", "I",
    "Xe",
];

fn atomic_number(sym: &str) -> f64 {
    ELEMENTS.iter().position(|e| e.eq_ignore_ascii_case(sym)).map(|i| (i + 1) as f64).unwrap_or(0.0)
}

/// Parse extended-XYZ "Properties=species:S:1:pos:R:3:..." into (name, type, ncols).
fn parse_properties(comment: &str) -> Option<Vec<(String, char, usize)>> {
    let start = comment.find("Properties=")? + "Properties=".len();
    let spec: String = comment[start..].split_whitespace().next()?.trim_matches('"').to_string();
    let parts: Vec<&str> = spec.split(':').collect();
    let mut out = Vec::new();
    for c in parts.chunks(3) {
        if c.len() == 3 {
            out.push((c[0].to_string(), c[1].chars().next().unwrap_or('R'), c[2].parse().unwrap_or(1)));
        }
    }
    Some(out)
}

fn parse_xyz_frame(lines: &mut std::str::Lines, n: usize) -> Result<DataSet> {
    let comment = lines.next().unwrap_or("").to_string();
    let props = parse_properties(&comment).unwrap_or_else(|| vec![("species".into(), 'S', 1), ("pos".into(), 'R', 3)]);
    let mut cols: Vec<Vec<String>> = vec![Vec::with_capacity(n); props.iter().map(|p| p.2).sum()];
    for _ in 0..n {
        let l = lines.next().ok_or_else(|| anyhow!("truncated XYZ frame"))?;
        for (k, tok) in l.split_whitespace().enumerate().take(cols.len()) {
            cols[k].push(tok.to_string());
        }
    }
    let mut pd_arrays = Vec::new();
    let mut points: Option<Vec<f64>> = None;
    let mut c0 = 0;
    for (name, ty, nc) in &props {
        let slice = &cols[c0..c0 + nc];
        c0 += nc;
        let lname = name.to_ascii_lowercase();
        if lname == "pos" && *nc == 3 {
            let mut v = Vec::with_capacity(3 * n);
            for i in 0..n {
                for c in slice {
                    v.push(c.get(i).and_then(|s| s.parse().ok()).unwrap_or(f64::NAN));
                }
            }
            points = Some(v);
            continue;
        }
        match ty {
            'S' => {
                let names: Vec<String> = slice[0].clone();
                if lname == "species" || lname == "element" {
                    pd_arrays.push(Array::scalars_f64("atomic_number", names.iter().map(|s| atomic_number(s)).collect()));
                }
                pd_arrays.push(Array::new(name.clone(), 1, ArrayData::String(names)));
            }
            'I' | 'L' => {
                let mut v = Vec::with_capacity(n * nc);
                for i in 0..n {
                    for c in slice {
                        v.push(
                            c.get(i)
                                .and_then(|s| {
                                    s.parse::<f64>().ok().or_else(|| {
                                        if s == "T" {
                                            Some(1.0)
                                        } else if s == "F" {
                                            Some(0.0)
                                        } else {
                                            None
                                        }
                                    })
                                })
                                .unwrap_or(0.0) as i64,
                        );
                    }
                }
                pd_arrays.push(Array::new(name.clone(), *nc, ArrayData::I64(v)));
            }
            _ => {
                let mut v = Vec::with_capacity(n * nc);
                for i in 0..n {
                    for c in slice {
                        v.push(c.get(i).and_then(|s| s.parse().ok()).unwrap_or(f64::NAN));
                    }
                }
                pd_arrays.push(Array::new(name.clone(), *nc, ArrayData::F64(v)));
            }
        }
    }
    let pts = points.ok_or_else(|| anyhow!("XYZ frame has no positions"))?;
    let mut pd = PolyData::from_points(Points::new_f64(pts));
    for a in pd_arrays {
        pd.point_data.insert(a);
    }
    if pd.point_data.get("atomic_number").is_some() {
        pd.point_data.set_active(AttributeKind::Scalars, "atomic_number");
    }
    // Lattice and simple key=value pairs from the comment line.
    if let Some(i) = comment.find("Lattice=\"") {
        let rest = &comment[i + 9..];
        if let Some(j) = rest.find('"') {
            let v: Vec<f64> = rest[..j].split_whitespace().filter_map(|s| s.parse().ok()).collect();
            if v.len() == 9 {
                pd.field_data.insert(Array::new("Lattice", 9, ArrayData::F64(v)));
            }
        }
    }
    for kv in comment.split_whitespace() {
        if let Some((k, v)) = kv.split_once('=') {
            if let Ok(x) = v.trim_matches('"').parse::<f64>() {
                pd.field_data.insert(Array::scalars_f64(k, vec![x]));
            }
        }
    }
    Ok(DataSet::PolyData(pd))
}

/// XYZ / extended XYZ, possibly with many frames (becomes a time series indexed by frame).
pub fn read_xyz(path: &Path) -> Result<Loaded> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = text.lines();
    let mut frames = Vec::new();
    while let Some(l) = lines.next() {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        let n: usize = l.parse().map_err(|_| anyhow!("expected an atom count, found {l:?}"))?;
        frames.push(Arc::new(parse_xyz_frame(&mut lines, n)?));
    }
    match frames.len() {
        0 => bail!("no frames in {}", path.display()),
        1 => Ok(Loaded::Static(frames.pop().unwrap())),
        n => {
            let times: Vec<f64> = frames
                .iter()
                .enumerate()
                .map(|(i, f)| f.field_data().get("time").or_else(|| f.field_data().get("Time")).map(|a| a.get(0, 0)).unwrap_or(i as f64))
                .collect();
            let times = if times.windows(2).all(|w| w[1] > w[0]) { times } else { (0..n).map(|i| i as f64).collect() };
            Ok(Loaded::Series(TimeSeries { times, steps: frames.into_iter().map(TimeStepRef::Loaded).collect() }))
        }
    }
}

/// LAMMPS data file (atoms, optional velocities and bonds).
pub fn read_lammps_data(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).peekable();
    lines.next(); // title
    let mut natoms = 0usize;
    let mut nbonds = 0usize;
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    let mut ids: Vec<i64> = Vec::new();
    let mut cols: Vec<Vec<f64>> = Vec::new();
    let mut style = String::from("full");
    let mut vel: std::collections::HashMap<i64, [f64; 3]> = std::collections::HashMap::new();
    let mut bonds: Vec<(i64, i64)> = Vec::new();
    let mut masses: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    while let Some(l) = lines.next() {
        if l.is_empty() {
            continue;
        }
        let t: Vec<&str> = l.split_whitespace().collect();
        if l.ends_with("atoms") && t.len() == 2 {
            natoms = t[0].parse()?;
        } else if l.ends_with("bonds") && t.len() == 2 {
            nbonds = t[0].parse()?;
        } else if t.len() >= 4 && t[2] == "xlo" {
            lo[0] = t[0].parse()?;
            hi[0] = t[1].parse()?;
        } else if t.len() >= 4 && t[2] == "ylo" {
            lo[1] = t[0].parse()?;
            hi[1] = t[1].parse()?;
        } else if t.len() >= 4 && t[2] == "zlo" {
            lo[2] = t[0].parse()?;
            hi[2] = t[1].parse()?;
        } else if t[0] == "Masses" {
            while lines.peek().map(|x| x.is_empty()).unwrap_or(false) {
                lines.next();
            }
            while let Some(x) = lines.peek() {
                let f: Vec<&str> = x.split_whitespace().collect();
                if f.len() < 2 || f[0].parse::<i64>().is_err() {
                    break;
                }
                masses.insert(f[0].parse()?, f[1].parse()?);
                lines.next();
            }
        } else if t[0] == "Atoms" {
            if let Some(s) = l.split('#').nth(1) {
                style = s.trim().to_string();
            }
            let raw_line = text.lines().find(|x| x.trim_start().starts_with("Atoms")).unwrap_or("");
            if let Some(s) = raw_line.split('#').nth(1) {
                style = s.trim().to_string();
            }
            while lines.peek().map(|x| x.is_empty()).unwrap_or(false) {
                lines.next();
            }
            for _ in 0..natoms {
                let x = lines.next().ok_or_else(|| anyhow!("truncated Atoms section"))?;
                let f: Vec<f64> = x.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if f.is_empty() {
                    bail!("bad atom line {x:?}");
                }
                ids.push(f[0] as i64);
                cols.push(f[1..].to_vec());
            }
        } else if t[0] == "Velocities" {
            while lines.peek().map(|x| x.is_empty()).unwrap_or(false) {
                lines.next();
            }
            for _ in 0..natoms {
                let x = lines.next().unwrap_or("");
                let f: Vec<f64> = x.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if f.len() >= 4 {
                    vel.insert(f[0] as i64, [f[1], f[2], f[3]]);
                }
            }
        } else if t[0] == "Bonds" {
            while lines.peek().map(|x| x.is_empty()).unwrap_or(false) {
                lines.next();
            }
            for _ in 0..nbonds {
                let x = lines.next().unwrap_or("");
                let f: Vec<i64> = x.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if f.len() >= 4 {
                    bonds.push((f[2], f[3]));
                }
            }
        }
    }
    if ids.is_empty() {
        bail!("no Atoms section in {}", path.display());
    }
    // Column layout by atom style (after the id): type, [mol], [q], x y z.
    let (mol_col, type_col, q_col, x_col) = match style.as_str() {
        "atomic" => (None, 0, None, 1),
        "charge" => (None, 0, Some(1), 2),
        "bond" | "angle" | "molecular" => (Some(0), 1, None, 2),
        _ => (Some(0), 1, Some(2), 3), // full
    };
    let n = ids.len();
    let mut pts = Vec::with_capacity(3 * n);
    for c in &cols {
        pts.extend_from_slice(&[c.get(x_col).copied().unwrap_or(0.0), c.get(x_col + 1).copied().unwrap_or(0.0), c.get(x_col + 2).copied().unwrap_or(0.0)]);
    }
    let mut pd = PolyData::from_points(Points::new_f64(pts));
    pd.point_data.insert(Array::new("id", 1, ArrayData::I64(ids.clone())));
    let types: Vec<i64> = cols.iter().map(|c| c.get(type_col).copied().unwrap_or(0.0) as i64).collect();
    if !masses.is_empty() {
        pd.point_data.insert(Array::scalars_f64("mass", types.iter().map(|t| masses.get(t).copied().unwrap_or(0.0)).collect()));
    }
    pd.point_data.insert_active(AttributeKind::Scalars, Array::new("type", 1, ArrayData::I64(types)));
    if let Some(m) = mol_col {
        pd.point_data.insert(Array::new("mol", 1, ArrayData::I64(cols.iter().map(|c| c.get(m).copied().unwrap_or(0.0) as i64).collect())));
    }
    if let Some(q) = q_col {
        pd.point_data.insert(Array::scalars_f64("q", cols.iter().map(|c| c.get(q).copied().unwrap_or(0.0)).collect()));
    }
    if !vel.is_empty() {
        let v: Vec<f64> = ids.iter().flat_map(|i| vel.get(i).copied().unwrap_or([0.0; 3])).collect();
        pd.point_data.insert_active(AttributeKind::Vectors, Array::new("velocity", 3, ArrayData::F64(v)));
    }
    if !bonds.is_empty() {
        let index: std::collections::HashMap<i64, u32> = ids.iter().enumerate().map(|(k, &i)| (i, k as u32)).collect();
        let mut lines_c = Cells::new();
        for (a, b) in bonds {
            if let (Some(&ia), Some(&ib)) = (index.get(&a), index.get(&b)) {
                lines_c.push(CellType::Line, &[ia, ib]);
            }
        }
        pd.lines = lines_c;
    }
    pd.field_data.insert(Array::new("BoxLo", 3, ArrayData::F64(lo.to_vec())));
    pd.field_data.insert(Array::new("BoxHi", 3, ArrayData::F64(hi.to_vec())));
    Ok(DataSet::PolyData(pd))
}

/// Covalent radius in Å (Cordero et al. 2008) by atomic number; 1.5 for others.
fn covalent_radius(z: usize) -> f64 {
    const R: [f64; 37] = [
        0.0, 0.31, 0.28, 1.28, 0.96, 0.84, 0.76, 0.71, 0.66, 0.57, 0.58, 1.66, 1.41, 1.21, 1.11, 1.07, 1.05, 1.02, 1.06, 2.03, 1.76, 1.70, 1.60, 1.53, 1.39,
        1.39, 1.32, 1.26, 1.24, 1.32, 1.22, 1.22, 1.20, 1.19, 1.20, 1.20, 1.16,
    ];
    R.get(z).copied().unwrap_or(1.5)
}

/// Bonds between atoms closer than the sum of their covalent radii plus 0.45 Å (as VMD and
/// OVITO do), found with a uniform grid.
fn distance_bonds(pos: &[[f64; 3]], z: &[usize]) -> Vec<(u32, u32)> {
    let cell = 2.0 * 2.03 + 0.45;
    let mut grid: std::collections::HashMap<(i64, i64, i64), Vec<usize>> = std::collections::HashMap::new();
    let key = |p: &[f64; 3]| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64);
    for (i, p) in pos.iter().enumerate() {
        grid.entry(key(p)).or_default().push(i);
    }
    let mut out = Vec::new();
    for (i, p) in pos.iter().enumerate() {
        let (kx, ky, kz) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &j in grid.get(&(kx + dx, ky + dy, kz + dz)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if j <= i {
                            continue;
                        }
                        let q = pos[j];
                        let d2 = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
                        let max = covalent_radius(z[i]) + covalent_radius(z[j]) + 0.45;
                        if d2 > 0.16 && d2 < max * max {
                            out.push((i as u32, j as u32));
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out
}

fn molecule(pos: Vec<[f64; 3]>, z: Vec<usize>, extra_bonds: &[(u32, u32)], arrays: Vec<Array>) -> DataSet {
    let mut bonds = distance_bonds(&pos, &z);
    for &(a, b) in extra_bonds {
        let e = (a.min(b), a.max(b));
        if !bonds.contains(&e) {
            bonds.push(e);
        }
    }
    let mut pd = PolyData::from_points(Points::new_f64(pos.iter().flatten().copied().collect()));
    for (a, b) in bonds {
        pd.lines.push(CellType::Line, &[a, b]);
    }
    pd.point_data.insert(Array::scalars_f64("atomic_number", z.iter().map(|&v| v as f64).collect()));
    pd.point_data.insert(Array::scalars_f64("radius", z.iter().map(|&v| covalent_radius(v)).collect()));
    for a in arrays {
        pd.point_data.insert(a);
    }
    pd.point_data.set_active(AttributeKind::Scalars, "atomic_number");
    DataSet::PolyData(pd)
}

/// Protein Data Bank files: ATOM/HETATM records (every atom, hydrogens included), CONECT bonds
/// plus bonds by distance, and MODELs as time steps.
pub fn read_pdb(path: &Path) -> Result<Loaded> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let col = |l: &str, a: usize, b: usize| l.get(a.min(l.len())..b.min(l.len())).unwrap_or("").trim().to_string();
    #[derive(Default)]
    struct Model {
        pos: Vec<[f64; 3]>,
        z: Vec<usize>,
        serial: Vec<i64>,
        element: Vec<String>,
        name: Vec<String>,
        residue: Vec<String>,
        residue_id: Vec<i64>,
        chain: Vec<String>,
        occupancy: Vec<f64>,
        b_factor: Vec<f64>,
        hetatm: Vec<i64>,
    }
    let mut models: Vec<Model> = vec![Model::default()];
    let mut conect: Vec<(i64, i64)> = Vec::new();
    for l in text.lines() {
        let rec = col(l, 0, 6);
        match rec.as_str() {
            "MODEL" => {
                if !models.last().unwrap().pos.is_empty() {
                    models.push(Model::default());
                }
            }
            "ATOM" | "HETATM" => {
                let m = models.last_mut().unwrap();
                let num = |a, b| col(l, a, b).parse::<f64>().map_err(|_| anyhow!("bad number in {l:?}"));
                m.pos.push([num(30, 38)?, num(38, 46)?, num(46, 54)?]);
                let name = col(l, 12, 16);
                // Element: columns 77-78, else the letters of the atom name.
                let mut el = col(l, 76, 78);
                if el.is_empty() {
                    el = name.chars().filter(|c| c.is_ascii_alphabetic()).take(1).collect();
                }
                let z = atomic_number(&el) as usize;
                m.z.push(z);
                m.serial.push(col(l, 6, 11).parse().unwrap_or(0));
                m.element.push(el);
                m.name.push(name);
                m.residue.push(col(l, 17, 20));
                m.residue_id.push(col(l, 22, 26).parse().unwrap_or(0));
                m.chain.push(col(l, 21, 22));
                m.occupancy.push(col(l, 54, 60).parse().unwrap_or(1.0));
                m.b_factor.push(col(l, 60, 66).parse().unwrap_or(0.0));
                m.hetatm.push((rec == "HETATM") as i64);
            }
            "CONECT" => {
                let a: i64 = col(l, 6, 11).parse().unwrap_or(0);
                for (s, e) in [(11, 16), (16, 21), (21, 26), (26, 31)] {
                    if let Ok(b) = col(l, s, e).parse::<i64>() {
                        conect.push((a, b));
                    }
                }
            }
            _ => {}
        }
    }
    models.retain(|m| !m.pos.is_empty());
    if models.is_empty() {
        bail!("{}: no ATOM or HETATM records", path.display());
    }
    let frames: Vec<Arc<DataSet>> = models
        .into_iter()
        .map(|m| {
            let index: std::collections::HashMap<i64, u32> = m.serial.iter().enumerate().map(|(i, &s)| (s, i as u32)).collect();
            let bonds: Vec<(u32, u32)> = conect.iter().filter_map(|(a, b)| Some((*index.get(a)?, *index.get(b)?))).collect();
            let arrays = vec![
                Array::new("element", 1, ArrayData::String(m.element)),
                Array::new("atom_name", 1, ArrayData::String(m.name)),
                Array::new("residue", 1, ArrayData::String(m.residue)),
                Array::new("residue_id", 1, ArrayData::I64(m.residue_id)),
                Array::new("chain", 1, ArrayData::String(m.chain)),
                Array::scalars_f64("occupancy", m.occupancy),
                Array::scalars_f64("b_factor", m.b_factor),
                Array::new("ishetatm", 1, ArrayData::I64(m.hetatm)),
            ];
            Arc::new(molecule(m.pos, m.z, &bonds, arrays))
        })
        .collect();
    Ok(match frames.len() {
        1 => Loaded::Static(frames.into_iter().next().unwrap()),
        n => Loaded::Series(TimeSeries { times: (0..n).map(|i| i as f64).collect(), steps: frames.into_iter().map(TimeStepRef::Loaded).collect() }),
    })
}

const BOHR: f64 = 0.529177210903;

/// Gaussian cube files: the volume as image data ("density", Å, oriented by the cube's axes)
/// and the atoms with bonds, as two blocks.
pub fn read_cube(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = text.lines();
    lines.next();
    lines.next();
    let nums = |l: Option<&str>| -> Result<Vec<f64>> {
        l.ok_or_else(|| anyhow!("truncated cube header"))?.split_whitespace().map(|t| t.parse::<f64>().map_err(|_| anyhow!("bad number {t:?}"))).collect()
    };
    let h = nums(lines.next())?;
    let natoms = h[0] as i64;
    let mut origin = [h[1], h[2], h[3]];
    let nval = h.get(4).copied().unwrap_or(1.0).max(1.0) as usize;
    let mut n = [0usize; 3];
    let mut axes = [[0.0f64; 3]; 3];
    let mut angstrom = false;
    for k in 0..3 {
        let a = nums(lines.next())?;
        n[k] = a[0].abs() as usize;
        angstrom |= a[0] < 0.0;
        axes[k] = [a[1], a[2], a[3]];
    }
    let scale = if angstrom { 1.0 } else { BOHR };
    origin = origin.map(|v| v * scale);
    let mut pos = Vec::new();
    let mut z = Vec::new();
    for _ in 0..natoms.unsigned_abs() {
        let a = nums(lines.next())?;
        z.push(a[0] as usize);
        pos.push([a[2] * scale, a[3] * scale, a[4] * scale]);
    }
    if natoms < 0 {
        lines.next(); // orbital numbers
    }
    let vals: Vec<f64> = lines.flat_map(|l| l.split_whitespace()).map(|t| t.parse::<f64>().map_err(|_| anyhow!("bad value {t:?}"))).collect::<Result<_>>()?;
    let total = n[0] * n[1] * n[2];
    if vals.len() < total * nval {
        bail!("{}: {} values for a {}x{}x{} grid", path.display(), vals.len(), n[0], n[1], n[2]);
    }
    // The file runs z fastest; VTK images run x fastest.
    let mut density = vec![0.0f64; total];
    for i in 0..n[0] {
        for j in 0..n[1] {
            for k in 0..n[2] {
                density[i + n[0] * (j + n[1] * k)] = vals[((i * n[1] + j) * n[2] + k) * nval];
            }
        }
    }
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let spacing = [len(axes[0]) * scale, len(axes[1]) * scale, len(axes[2]) * scale];
    let unit = |v: [f64; 3]| {
        let l = len(v).max(1e-300);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let (ux, uy, uz) = (unit(axes[0]), unit(axes[1]), unit(axes[2]));
    let mut img = ImageData {
        extent: [0, n[0] as i64 - 1, 0, n[1] as i64 - 1, 0, n[2] as i64 - 1],
        origin,
        spacing,
        // Columns are the grid axes.
        direction: [ux[0], uy[0], uz[0], ux[1], uy[1], uz[1], ux[2], uy[2], uz[2]],
        point_data: Attributes::new(),
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    };
    img.point_data.insert_active(AttributeKind::Scalars, Array::scalars_f64("density", density));
    let atoms = molecule(pos, z, &[], Vec::new());
    Ok(DataSet::MultiBlock(MultiBlock {
        blocks: vec![("volume".into(), Some(DataSet::ImageData(img))), ("atoms".into(), Some(atoms))],
        field_data: Attributes::new(),
    }))
}
