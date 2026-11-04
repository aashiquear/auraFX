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
