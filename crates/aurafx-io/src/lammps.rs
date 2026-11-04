//! LAMMPS text dump reader (`dump atom`/`dump custom` style), optionally gzip-compressed.
//!
//! Multi-frame files are opened as a [`TimeSeries`] whose steps are loaded on demand by
//! byte offset (uncompressed files) or from an in-memory cache (gzipped files).

use crate::Loaded;
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

struct Index {
    /// Byte offset of each "ITEM: TIMESTEP" line.
    offsets: Vec<usize>,
    timesteps: Vec<f64>,
    /// Entire decompressed content (only for .gz files, which have no random access).
    content: Option<Arc<Vec<u8>>>,
}

static INDEX: LazyLock<Mutex<HashMap<PathBuf, Arc<Index>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn is_gz(path: &Path) -> bool {
    path.extension().map(|e| e.eq_ignore_ascii_case("gz")).unwrap_or(false)
}

fn load_content(path: &Path) -> Result<Vec<u8>> {
    let raw = crate::read_bytes(path)?;
    if is_gz(path) {
        let mut out = Vec::new();
        flate2::read::MultiGzDecoder::new(&raw[..]).read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(raw)
    }
}

fn build_index(path: &Path) -> Result<Arc<Index>> {
    if let Some(ix) = INDEX.lock().get(path) {
        return Ok(ix.clone());
    }
    let content = load_content(path)?;
    let mut offsets = Vec::new();
    let mut timesteps = Vec::new();
    let finder = memchr::memmem::Finder::new(b"ITEM: TIMESTEP");
    let mut pos = 0;
    while let Some(i) = finder.find(&content[pos..]) {
        let at = pos + i;
        if at == 0 || content[at - 1] == b'\n' {
            offsets.push(at);
            let line_start = at + memchr::memchr(b'\n', &content[at..]).map(|x| x + 1).unwrap_or(0);
            let line_end = line_start + memchr::memchr(b'\n', &content[line_start..]).unwrap_or(0);
            let ts: f64 = std::str::from_utf8(&content[line_start..line_end])?.trim().parse().unwrap_or(timesteps.len() as f64);
            timesteps.push(ts);
        }
        pos = at + 14;
    }
    if offsets.is_empty() {
        bail!("no 'ITEM: TIMESTEP' records found in {}", path.display());
    }
    let ix = Arc::new(Index { offsets, timesteps, content: if is_gz(path) { Some(Arc::new(content)) } else { None } });
    INDEX.lock().insert(path.to_path_buf(), ix.clone());
    Ok(ix)
}

/// Open a dump file. Single-frame files are returned as a static dataset.
pub fn open(path: &Path) -> Result<Loaded> {
    let ix = build_index(path)?;
    if ix.offsets.len() == 1 {
        return Ok(Loaded::Static(Arc::new(read_frame(path, 0)?)));
    }
    let mut series = TimeSeries::default();
    for (i, &t) in ix.timesteps.iter().enumerate() {
        series.times.push(t);
        series.steps.push(TimeStepRef::FileFrame { path: path.to_path_buf(), frame: i });
    }
    Ok(Loaded::Series(series))
}

/// Read one frame.
pub fn read_frame(path: &Path, frame: usize) -> Result<DataSet> {
    let ix = build_index(path)?;
    let start = *ix.offsets.get(frame).ok_or_else(|| anyhow!("frame {frame} out of range"))?;
    let end = ix.offsets.get(frame + 1).copied();
    let bytes: Vec<u8> = match &ix.content {
        Some(c) => c[start..end.unwrap_or(c.len())].to_vec(),
        None => {
            use std::io::{Seek, SeekFrom};
            let mut f = std::fs::File::open(path)?;
            f.seek(SeekFrom::Start(start as u64))?;
            match end {
                Some(e) => {
                    let mut v = vec![0u8; e - start];
                    f.read_exact(&mut v)?;
                    v
                }
                None => {
                    let mut v = Vec::new();
                    f.read_to_end(&mut v)?;
                    v
                }
            }
        }
    };
    parse_frame(&bytes).with_context(|| format!("parsing LAMMPS frame {frame} of {}", path.display()))
}

#[derive(Default)]
struct Box3 {
    lo: [f64; 3],
    hi: [f64; 3],
    tilt: [f64; 3], // xy xz yz
    triclinic: bool,
}

fn parse_frame(bytes: &[u8]) -> Result<DataSet> {
    let text = std::str::from_utf8(bytes)?;
    let mut lines = text.lines();
    let mut timestep = 0.0;
    let mut natoms = 0usize;
    let mut bx = Box3::default();
    let mut columns: Vec<String> = Vec::new();
    let mut data: Vec<Vec<f64>> = Vec::new();
    let mut extra_items: Vec<(String, Vec<f64>)> = Vec::new();

    while let Some(line) = lines.next() {
        let Some(item) = line.strip_prefix("ITEM:") else { continue };
        let item = item.trim();
        if item.starts_with("TIMESTEP") {
            timestep = lines.next().unwrap_or("0").trim().parse().unwrap_or(0.0);
        } else if item.starts_with("NUMBER OF ATOMS") {
            natoms = lines.next().unwrap_or("0").trim().parse()?;
        } else if item.starts_with("BOX BOUNDS") {
            bx.triclinic = item.contains("xy");
            for d in 0..3 {
                let l = lines.next().ok_or_else(|| anyhow!("truncated box"))?;
                let v: Vec<f64> = l.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() < 2 {
                    bail!("bad box line {l:?}");
                }
                bx.lo[d] = v[0];
                bx.hi[d] = v[1];
                if bx.triclinic && v.len() >= 3 {
                    bx.tilt[d] = v[2];
                }
            }
            if bx.triclinic {
                // LAMMPS writes bounding box of the tilted cell; recover the actual lo/hi.
                let [xy, xz, yz] = bx.tilt;
                bx.lo[0] -= [0.0, xy, xz, xy + xz].iter().cloned().fold(f64::INFINITY, f64::min);
                bx.hi[0] -= [0.0, xy, xz, xy + xz].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                bx.lo[1] -= yz.min(0.0);
                bx.hi[1] -= yz.max(0.0);
            }
        } else if item.starts_with("ATOMS") {
            columns = item["ATOMS".len()..].split_whitespace().map(|s| s.to_string()).collect();
            data = vec![Vec::with_capacity(natoms); columns.len()];
            for _ in 0..natoms {
                let l = lines.next().ok_or_else(|| anyhow!("truncated atoms section"))?;
                for (i, tok) in l.split_whitespace().enumerate().take(columns.len()) {
                    data[i].push(tok.parse().unwrap_or(f64::NAN));
                }
            }
        } else if item.starts_with("TIME") {
            let v = lines.next().unwrap_or("0").trim().parse().unwrap_or(0.0);
            extra_items.push(("Time".into(), vec![v]));
        } else if item.starts_with("UNITS") {
            lines.next();
        } else {
            // Unknown item: skip until next ITEM.
        }
    }
    if columns.is_empty() {
        bail!("no ATOMS section");
    }

    let col = |name: &str| columns.iter().position(|c| c == name);
    let n = data.first().map(|c| c.len()).unwrap_or(0);
    let len = [bx.hi[0] - bx.lo[0], bx.hi[1] - bx.lo[1], bx.hi[2] - bx.lo[2]];

    // Position columns, in preference order: unwrapped, wrapped, scaled.
    let mut pts = Vec::with_capacity(3 * n);
    let get3 = |a: &str, b: &str, c: &str| -> Option<[usize; 3]> { Some([col(a)?, col(b)?, col(c)?]) };
    if let Some([ix, iy, iz]) = get3("xu", "yu", "zu").or_else(|| get3("x", "y", "z")) {
        for i in 0..n {
            pts.extend_from_slice(&[data[ix][i], data[iy][i], data[iz][i]]);
        }
    } else if let Some([ix, iy, iz]) = get3("xs", "ys", "zs").or_else(|| get3("xsu", "ysu", "zsu")) {
        let [xy, xz, yz] = bx.tilt;
        for i in 0..n {
            let (sx, sy, sz) = (data[ix][i], data[iy][i], data[iz][i]);
            pts.extend_from_slice(&[bx.lo[0] + sx * len[0] + sy * xy + sz * xz, bx.lo[1] + sy * len[1] + sz * yz, bx.lo[2] + sz * len[2]]);
        }
    } else {
        bail!("dump has no position columns (x y z / xs ys zs / xu yu zu)");
    }

    let mut pd = PolyData::from_points(Points::new_f64(pts));
    // Every remaining column becomes a point array; group v*, f*, mu*, omega*, tq*, c_/f_ vectors.
    let mut consumed = vec![false; columns.len()];
    for ci in 0..columns.len() {
        if ["x", "y", "z", "xu", "yu", "zu", "xs", "ys", "zs", "xsu", "ysu", "zsu"].contains(&columns[ci].as_str()) {
            consumed[ci] = true;
        }
    }
    for (prefix, out_name) in
        [("v", "velocity"), ("f", "force"), ("mu", "dipole"), ("omega", "angular_velocity"), ("tq", "torque"), ("angmom", "angular_momentum")]
    {
        if let Some([a, b, c]) = get3(&format!("{prefix}x"), &format!("{prefix}y"), &format!("{prefix}z")) {
            let mut v = Vec::with_capacity(3 * n);
            for i in 0..n {
                v.extend_from_slice(&[data[a][i], data[b][i], data[c][i]]);
            }
            let kind = if prefix == "v" { Some(AttributeKind::Vectors) } else { None };
            let arr = Array::new(out_name, 3, ArrayData::F64(v));
            match kind {
                Some(k) => pd.point_data.insert_active(k, arr),
                None => {
                    pd.point_data.insert(arr);
                }
            }
            consumed[a] = true;
            consumed[b] = true;
            consumed[c] = true;
        }
    }
    // Bracketed compute/fix vectors like c_stress[1] c_stress[2] ...
    let mut groups: indexmap::IndexMap<String, Vec<(usize, usize)>> = indexmap::IndexMap::new();
    for (ci, c) in columns.iter().enumerate() {
        if consumed[ci] {
            continue;
        }
        if let Some((base, rest)) = c.split_once('[') {
            if let Some(idx) = rest.strip_suffix(']').and_then(|s| s.parse::<usize>().ok()) {
                groups.entry(base.to_string()).or_default().push((idx, ci));
            }
        }
    }
    for (base, mut comps) in groups {
        comps.sort();
        let nc = comps.len();
        let mut v = Vec::with_capacity(nc * n);
        for i in 0..n {
            for &(_, ci) in &comps {
                v.push(data[ci][i]);
            }
        }
        for &(_, ci) in &comps {
            consumed[ci] = true;
        }
        pd.point_data.insert(Array::new(base, nc, ArrayData::F64(v)));
    }
    for (ci, c) in columns.iter().enumerate() {
        if consumed[ci] {
            continue;
        }
        let v = std::mem::take(&mut data[ci]);
        let is_int =
            ["id", "type", "mol", "proc", "procp1", "ix", "iy", "iz", "element"].contains(&c.as_str()) || v.iter().all(|x| x.fract() == 0.0 && x.abs() < 1e15);
        let arr =
            if is_int { Array::new(c.clone(), 1, ArrayData::I64(v.iter().map(|&x| x as i64).collect())) } else { Array::new(c.clone(), 1, ArrayData::F64(v)) };
        if c == "type" {
            pd.point_data.insert_active(AttributeKind::Scalars, arr);
        } else {
            pd.point_data.insert(arr);
        }
    }
    if pd.point_data.active(AttributeKind::Scalars).is_none() {
        let first = pd.point_data.iter().find(|a| a.ncomp == 1).map(|a| a.name.clone());
        if let Some(first) = first {
            pd.point_data.set_active(AttributeKind::Scalars, &first);
        }
    }
    pd.field_data.insert(Array::scalars_f64("TimeStep", vec![timestep]));
    pd.field_data.insert(Array::new("BoxLo", 3, ArrayData::F64(bx.lo.to_vec())));
    pd.field_data.insert(Array::new("BoxHi", 3, ArrayData::F64(bx.hi.to_vec())));
    if bx.triclinic {
        pd.field_data.insert(Array::new("BoxTilt", 3, ArrayData::F64(bx.tilt.to_vec())));
    }
    for (name, v) in extra_items {
        pd.field_data.insert(Array::scalars_f64(name, v));
    }
    Ok(DataSet::PolyData(pd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let s = b"ITEM: TIMESTEP\n100\nITEM: NUMBER OF ATOMS\n2\nITEM: BOX BOUNDS pp pp pp\n0 10\n0 10\n0 10\nITEM: ATOMS id type x y z vx vy vz c_pe\n1 1 1.0 2.0 3.0 0.1 0.2 0.3 -3.5\n2 2 4.0 5.0 6.0 0.0 0.0 0.0 -3.6\n";
        let ds = parse_frame(s).unwrap();
        assert_eq!(ds.n_points(), 2);
        let pd = ds.point_data().unwrap();
        assert!(pd.get("velocity").is_some());
        assert_eq!(pd.get("type").unwrap().get(1, 0), 2.0);
        assert_eq!(pd.get("c_pe").unwrap().get(0, 0), -3.5);
    }
}
