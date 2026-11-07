//! Molecules (polygonal data: atoms as points with an atomic number array, bonds as lines).

use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::collections::{HashMap, HashSet};

/// The atomic number array of a molecule: the named one, else `atomic_number` or VTK's
/// `Atomic Numbers`.
pub fn atomic_numbers<'a>(pd: &'a Attributes, name: Option<&str>) -> Option<&'a Array> {
    match name.filter(|n| !n.is_empty()) {
        Some(n) => pd.get(n),
        None => pd.get("atomic_number").or_else(|| pd.get("Atomic Numbers")),
    }
}

/// Compute Molecule Bonds (VTK's simple bond perceiver): atoms closer than the sum of their
/// covalent radii, each plus half the tolerance (absolute) or times the tolerance (relative),
/// are bonded. Bonds already present are kept; new ones are added once each (ParaView adds
/// them again next to the existing ones).
pub fn compute_bonds(ds: &DataSet, array: Option<&str>, tolerance: f64, absolute: bool) -> Result<DataSet> {
    let mut pd = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => {
            let pts = other.points().ok_or_else(|| anyhow!("molecule bonds need atoms (points)"))?;
            let mut p = PolyData::from_points(pts);
            p.point_data = other.point_data().cloned().unwrap_or_default();
            p
        }
    };
    let z: Vec<i64> = {
        let a = atomic_numbers(&pd.point_data, array).ok_or_else(|| anyhow!("no atomic number array (atomic_number); choose one"))?;
        (0..pd.points.len()).map(|i| a.get(i, 0).round() as i64).collect()
    };
    let radius = |z: i64| {
        let c = aurafx_data::elements::element(z).covalent_radius;
        if absolute {
            c + tolerance / 2.0
        } else {
            c * tolerance
        }
    };
    let valid = |z: i64| (1..=118).contains(&z);
    let n = pd.points.len();
    let rmax = (1..=118).map(radius).fold(0.0f64, f64::max).max(1e-6);
    let cell = 2.0 * rmax;
    let key = |p: [f64; 3]| [(p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for i in 0..n {
        if valid(z[i]) {
            grid.entry(key(pd.points.get(i))).or_default().push(i);
        }
    }
    let existing: HashSet<(u32, u32)> = (0..pd.lines.len())
        .filter(|&c| pd.lines.cell(c).len() == 2)
        .map(|c| {
            let l = pd.lines.cell(c);
            (l[0].min(l[1]), l[0].max(l[1]))
        })
        .collect();
    let mut new = Vec::new();
    for i in 0..n {
        if !valid(z[i]) {
            continue;
        }
        let p = pd.points.get(i);
        let k = key(p);
        let ri = radius(z[i]);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &j in grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if j <= i {
                            continue;
                        }
                        let q = pd.points.get(j);
                        let d2 = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
                        let s = ri + radius(z[j]);
                        if d2 <= s * s && !existing.contains(&(i as u32, j as u32)) {
                            new.push((i as u32, j as u32));
                        }
                    }
                }
            }
        }
    }
    new.sort_unstable();
    // New bond cells: arrays get 1 for bond orders, 0 (or empty text) otherwise.
    let base = pd.verts.len() + pd.lines.len();
    let after = pd.polys.len() + pd.strips.len();
    if after > 0 {
        // Lines come before polygons in cell order: rebuild with the new lines in place.
        let idx: Vec<usize> = (0..base).chain(std::iter::repeat_n(usize::MAX, new.len())).chain(base..base + after).collect();
        let mut cd = Attributes::new();
        for a in pd.cell_data.iter() {
            cd.insert(fill_array(a, &idx));
        }
        pd.cell_data = cd;
    } else {
        let idx: Vec<usize> = (0..base).chain(std::iter::repeat_n(usize::MAX, new.len())).collect();
        let mut cd = Attributes::new();
        for a in pd.cell_data.iter() {
            cd.insert(fill_array(a, &idx));
        }
        pd.cell_data = cd;
    }
    for (a, b) in new {
        pd.lines.push(CellType::Line, &[a, b]);
    }
    Ok(DataSet::PolyData(pd))
}

/// `a` gathered by `idx`, with usize::MAX meaning a new bond (order 1, else 0).
fn fill_array(a: &Array, idx: &[usize]) -> Array {
    let fill = if a.name.to_ascii_lowercase().contains("order") { 1.0 } else { 0.0 };
    let data = match &a.data {
        ArrayData::String(v) => ArrayData::String(
            idx.iter().flat_map(|&i| (0..a.ncomp).map(move |c| if i == usize::MAX { String::new() } else { v[i * a.ncomp + c].clone() })).collect(),
        ),
        d => {
            let vals: Vec<f64> = idx.iter().flat_map(|&i| (0..a.ncomp).map(move |c| if i == usize::MAX { fill } else { d.get_f64(i * a.ncomp + c) })).collect();
            ArrayData::from_f64(d.scalar_type(), &vals)
        }
    };
    let mut out = Array::new(a.name.clone(), a.ncomp, data);
    out.component_names = a.component_names.clone();
    out
}
