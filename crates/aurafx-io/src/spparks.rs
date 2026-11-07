//! SPPARKS site files (`read_sites` input and `dump sites` output) and lattice dumps.
//!
//! A site file: a comment line, header lines (`3 dimension`, `N sites`, `xlo xhi`, `ylo yhi`,
//! `zlo zhi`, optionally `N max neighbors`), then sections: `Sites` (id x y z), `Neighbors` and
//! `Values` (id followed by the site's integer and real values). Files without a `Sites` section
//! describe a simple-cubic lattice of unit spacing filling the box, numbered x fastest from 1:
//! they read as a volume whose cells are the sites. Text dumps of a lattice application (columns
//! `id i1 ... x y z`) also read as a volume (see [`lattice_volume`]).

use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::path::Path;

/// Read a site file.
pub fn read_sites(path: &Path) -> Result<DataSet> {
    let text = String::from_utf8_lossy(&crate::read_bytes(path)?).to_string();
    let mut lines = text.lines().skip(1).peekable();
    let (mut n_sites, mut bounds) = (0usize, [[0.0f64; 2]; 3]);
    let mut dim = 3usize;
    // Header.
    while let Some(l) = lines.peek() {
        let t = l.trim();
        if t.is_empty() {
            lines.next();
            continue;
        }
        let w: Vec<&str> = t.split_whitespace().collect();
        if w.len() == 1 && matches!(w[0], "Sites" | "Neighbors" | "Values") {
            break;
        }
        let num = |i: usize| w.get(i).and_then(|x| x.parse::<f64>().ok()).ok_or_else(|| anyhow!("bad header line: {t}"));
        if t.ends_with("dimension") {
            dim = num(0)? as usize;
        } else if t.ends_with(" sites") {
            n_sites = num(0)? as usize;
        } else if t.ends_with("xlo xhi") {
            bounds[0] = [num(0)?, num(1)?];
        } else if t.ends_with("ylo yhi") {
            bounds[1] = [num(0)?, num(1)?];
        } else if t.ends_with("zlo zhi") {
            bounds[2] = [num(0)?, num(1)?];
        }
        lines.next();
    }
    let mut coords: Vec<[f64; 3]> = Vec::new();
    let mut values: Vec<Vec<f64>> = Vec::new();
    let mut ids: Vec<usize> = Vec::new();
    let mut section = String::new();
    for l in lines {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if matches!(t, "Sites" | "Neighbors" | "Values") {
            section = t.to_string();
            continue;
        }
        let w: Vec<f64> = t.split_whitespace().map(|x| x.parse::<f64>()).collect::<Result<_, _>>().map_err(|_| anyhow!("bad line in {section}: {t}"))?;
        match section.as_str() {
            "Sites" if w.len() >= 4 => coords.push([w[1], w[2], w[3]]),
            "Values" if w.len() >= 2 => {
                ids.push(w[0] as usize);
                values.push(w[1..].to_vec());
            }
            _ => {}
        }
    }
    let n = if n_sites > 0 { n_sites } else { values.len().max(coords.len()) };
    let nv = values.first().map(|v| v.len()).unwrap_or(0);
    // Values in site order (ids from 1).
    let mut cols: Vec<Vec<f64>> = vec![vec![0.0; n]; nv];
    for (id, v) in ids.iter().zip(&values) {
        if *id == 0 || *id > n {
            bail!("site id {id} outside 1..{n}");
        }
        for (k, x) in v.iter().enumerate().take(nv) {
            cols[k][id - 1] = *x;
        }
    }
    let arrays: Vec<Array> = cols
        .into_iter()
        .enumerate()
        .map(|(k, c)| {
            let integer = c.iter().all(|x| x.fract() == 0.0 && x.abs() < 2e9);
            match (k, integer) {
                (0, true) => Array::new("FeatureIds", 1, ArrayData::I32(c.iter().map(|x| *x as i32).collect())),
                (_, true) => Array::new(format!("i{}", k + 1), 1, ArrayData::I32(c.iter().map(|x| *x as i32).collect())),
                (_, false) => Array::new(format!("d{}", k + 1), 1, ArrayData::F64(c)),
            }
        })
        .collect();
    if coords.is_empty() {
        // A lattice filling the box with unit spacing.
        let d: [usize; 3] = std::array::from_fn(|a| if a >= dim { 1 } else { (bounds[a][1] - bounds[a][0]).round().max(1.0) as usize });
        if d[0] * d[1] * d[2] != n {
            bail!("{n} sites do not fill the {} x {} x {} box: a Sites section is needed", d[0], d[1], d[2]);
        }
        let mut img = ImageData { extent: [0, d[0] as i64, 0, d[1] as i64, 0, d[2] as i64], origin: [bounds[0][0], bounds[1][0], if dim > 2 { bounds[2][0] } else { 0.0 }], spacing: [1.0; 3], ..Default::default() };
        for a in arrays {
            img.cell_data.insert(a);
        }
        return Ok(DataSet::ImageData(img));
    }
    let mut pd = PolyData { points: Points::new_f64(coords.iter().flatten().copied().collect()), ..Default::default() };
    for i in 0..coords.len() {
        pd.verts.push(CellType::Vertex, &[i as u32]);
    }
    for a in arrays {
        pd.point_data.insert(a);
    }
    lattice_volume(DataSet::PolyData(pd))
}

/// Write the volume's feature ids (or `grain_id`, or the first integer cell array) as a site
/// file for a simple-cubic lattice of unit spacing (`Values` section only, x fastest).
pub fn write_sites(ds: &DataSet, path: &Path) -> Result<()> {
    let DataSet::ImageData(img) = ds else { bail!("a SPPARKS site file is written from a volume (image data)") };
    let d: [usize; 3] = std::array::from_fn(|a| (img.extent[2 * a + 1] - img.extent[2 * a]).max(0) as usize);
    let a = ["FeatureIds", "grain_id", "Spins", "i1"]
        .iter()
        .find_map(|n| img.cell_data.get(n))
        .or_else(|| img.cell_data.iter().find(|a| a.ncomp == 1 && matches!(a.data, ArrayData::I32(_) | ArrayData::I64(_) | ArrayData::U32(_) | ArrayData::I16(_) | ArrayData::U16(_) | ArrayData::U8(_))))
        .ok_or_else(|| anyhow!("the volume has no FeatureIds (or other integer cell array) to write"))?;
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let n = d[0] * d[1] * d[2];
    writeln!(f, "Site values ({}) written by auraFX", a.name)?;
    writeln!(f, "3 dimension")?;
    writeln!(f, "{n} sites")?;
    writeln!(f, "0 {} xlo xhi", d[0])?;
    writeln!(f, "0 {} ylo yhi", d[1])?;
    writeln!(f, "0 {} zlo zhi", d[2])?;
    writeln!(f, "\nValues\n")?;
    for i in 0..n {
        writeln!(f, "{} {}", i + 1, a.get(i, 0) as i64)?;
    }
    Ok(())
}

/// Points on a complete regular lattice that carry site values (`i1`, `FeatureIds`) become a
/// volume whose cells are the sites; anything else is returned unchanged.
pub fn lattice_volume(ds: DataSet) -> Result<DataSet> {
    let DataSet::PolyData(pd) = &ds else { return Ok(ds) };
    if pd.point_data.get("i1").is_none() && pd.point_data.get("FeatureIds").is_none() {
        return Ok(ds);
    }
    let n = pd.points.len();
    if n == 0 {
        return Ok(ds);
    }
    let mut axes: Vec<Vec<f64>> = vec![Vec::new(); 3];
    for (a, ax) in axes.iter_mut().enumerate() {
        let mut v: Vec<f64> = (0..n).map(|i| pd.points.get(i)[a]).collect();
        v.sort_by(f64::total_cmp);
        v.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
        *ax = v;
    }
    let d: [usize; 3] = std::array::from_fn(|a| axes[a].len());
    if d[0] * d[1] * d[2] != n {
        return Ok(ds);
    }
    let step: [f64; 3] = std::array::from_fn(|a| if d[a] > 1 { (axes[a][d[a] - 1] - axes[a][0]) / (d[a] - 1) as f64 } else { 1.0 });
    if (0..3).any(|a| axes[a].windows(2).any(|w| ((w[1] - w[0]) - step[a]).abs() > 1e-6 * step[a].abs().max(1.0))) {
        return Ok(ds);
    }
    let mut order = vec![usize::MAX; n];
    for i in 0..n {
        let p = pd.points.get(i);
        let ix: [usize; 3] = std::array::from_fn(|a| ((p[a] - axes[a][0]) / step[a]).round() as usize);
        let k = ix[0] + d[0] * (ix[1] + d[1] * ix[2]);
        if order[k] != usize::MAX {
            return Ok(ds);
        }
        order[k] = i;
    }
    let mut img = ImageData {
        extent: [0, d[0] as i64, 0, d[1] as i64, 0, d[2] as i64],
        origin: std::array::from_fn(|a| axes[a][0] - 0.5 * step[a]),
        spacing: step,
        ..Default::default()
    };
    for a in pd.point_data.iter() {
        img.cell_data.insert(a.gather(&order));
    }
    if img.cell_data.get("FeatureIds").is_none() {
        if let Some(s) = img.cell_data.get("i1") {
            let v: Vec<i32> = (0..s.len()).map(|i| s.get(i, 0) as i32).collect();
            img.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(v)));
        }
    }
    img.field_data = pd.field_data.clone();
    Ok(DataSet::ImageData(img))
}
