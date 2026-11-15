//! Nek5000 results: a `.nek5000` file names the data files (`filetemplate`, `firsttimestep`,
//! `numtimesteps`); each data file holds, element by element, the points of every spectral
//! element (nx x ny x nz Gauss-Lobatto points) and the fields its header lists (X coordinates,
//! U velocity, P pressure, T temperature, S passive scalars). Each element becomes
//! (nx-1)(ny-1)(nz-1) hexahedra (quadrilaterals in 2-D); the coordinates come from the first
//! file that has them.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Header {
    wdsize: usize,
    n: [usize; 3],
    nelt: usize,
    time: f64,
    fields: Vec<String>,
    little: bool,
    nscalars: usize,
}

fn header(b: &[u8], path: &Path) -> Result<Header> {
    if b.len() < 136 {
        bail!("{} is too short for a Nek5000 data file", path.display());
    }
    let h = String::from_utf8_lossy(&b[..132]).to_string();
    let t: Vec<&str> = h.split_whitespace().collect();
    if t.first() != Some(&"#std") || t.len() < 11 {
        bail!("{}: not a Nek5000 data file (header {:?})", path.display(), h.trim());
    }
    let num = |k: usize| t.get(k).and_then(|v| v.parse::<f64>().ok()).ok_or_else(|| anyhow!("{}: bad header field {k}", path.display()));
    let wdsize = num(1)? as usize;
    let n = [num(2)? as usize, num(3)? as usize, num(4)? as usize];
    let nelt = num(5)? as usize;
    let time = num(7)?;
    // The field codes: the rest after the 11th token, letters possibly glued (XUPT) or spaced.
    let rd: String = t[11..].join("");
    let mut fields = Vec::new();
    let mut nscalars = 0;
    let mut chars = rd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'X' | 'U' | 'P' | 'T' => fields.push(c.to_string()),
            'S' => {
                let mut d = String::new();
                while let Some(&x) = chars.peek() {
                    if x.is_ascii_digit() {
                        d.push(x);
                        chars.next();
                    } else {
                        break;
                    }
                }
                nscalars = d.parse().unwrap_or(1);
                fields.push("S".into());
            }
            _ => {}
        }
    }
    let tag = &b[132..136];
    let little = (f32::from_le_bytes(tag.try_into().unwrap()) - 6.54321).abs() < 1e-4;
    if !little && (f32::from_be_bytes(tag.try_into().unwrap()) - 6.54321).abs() > 1e-4 {
        bail!("{}: no byte-order mark", path.display());
    }
    Ok(Header { wdsize, n, nelt, time, fields, little, nscalars })
}

fn floats(b: &[u8], wd: usize, little: bool) -> Vec<f64> {
    if wd == 8 {
        b.chunks_exact(8).map(|c| if little { f64::from_le_bytes(c.try_into().unwrap()) } else { f64::from_be_bytes(c.try_into().unwrap()) }).collect()
    } else {
        b.chunks_exact(4).map(|c| (if little { f32::from_le_bytes(c.try_into().unwrap()) } else { f32::from_be_bytes(c.try_into().unwrap()) }) as f64).collect()
    }
}

/// One data file: header, coordinates (if present) and fields, all per element in file order.
struct Step {
    time: f64,
    n: [usize; 3],
    nelt: usize,
    coords: Option<Vec<[f64; 3]>>,
    arrays: Vec<(String, usize, Vec<f64>)>,
}

fn read_step(path: &Path) -> Result<Step> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let h = header(&b, path)?;
    let dim = if h.n[2] > 1 { 3 } else { 2 };
    let npe = h.n[0] * h.n[1] * h.n[2];
    let mut pos = 136 + 4 * h.nelt;
    let wd = h.wdsize;
    let mut take = |count: usize| -> Result<Vec<f64>> {
        let bytes = count * wd;
        let s = b.get(pos..pos + bytes).ok_or_else(|| anyhow!("{}: the data ends early", path.display()))?;
        pos += bytes;
        Ok(floats(s, wd, h.little))
    };
    let mut coords = None;
    let mut arrays = Vec::new();
    for f in &h.fields {
        match f.as_str() {
            "X" | "U" => {
                let mut v = vec![[0.0; 3]; h.nelt * npe];
                for e in 0..h.nelt {
                    for c in 0..dim {
                        let block = take(npe)?;
                        for (k, x) in block.into_iter().enumerate() {
                            v[e * npe + k][c] = x;
                        }
                    }
                }
                if f == "X" {
                    coords = Some(v);
                } else {
                    arrays.push(("Velocity".to_string(), 3, v.into_iter().flatten().collect()));
                }
            }
            "P" | "T" => {
                let v = take(h.nelt * npe)?;
                arrays.push((if f == "P" { "Pressure" } else { "Temperature" }.to_string(), 1, v));
            }
            "S" => {
                for s in 0..h.nscalars {
                    let v = take(h.nelt * npe)?;
                    arrays.push((format!("S{:02}", s + 1), 1, v));
                }
            }
            _ => {}
        }
    }
    Ok(Step { time: h.time, n: h.n, nelt: h.nelt, coords, arrays })
}

/// The data files a `.nek5000` file lists.
fn files(path: &Path) -> Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let get = |key: &str| text.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(key)).map(|(_, v)| v.trim().to_string()));
    let template = get("filetemplate").ok_or_else(|| anyhow!("{}: no filetemplate", path.display()))?;
    let first: usize = get("firsttimestep").and_then(|v| v.parse().ok()).unwrap_or(1);
    let count: usize = get("numtimesteps").and_then(|v| v.parse().ok()).unwrap_or(1);
    // The last %0Nd is the step number; an earlier one (a folder or file index) is 0.
    let fill = |t: &str, k: usize| -> String {
        let mut out = String::new();
        let marks: Vec<usize> = t.match_indices('%').map(|(i, _)| i).collect();
        let mut last = 0;
        for (m, &i) in marks.iter().enumerate() {
            let end = t[i..].find('d').map(|e| i + e + 1).unwrap_or(t.len());
            let width: usize = t[i + 1..end - 1].trim_start_matches('0').parse().unwrap_or(1);
            out += &t[last..i];
            let v = if m + 1 == marks.len() { k } else { 0 };
            out += &format!("{v:0width$}");
            last = end;
        }
        out + &t[last..]
    };
    Ok((first..first + count).map(|k| crate::sibling(path, &fill(&template, k))).collect())
}

/// Read a Nek5000 result (`.nek5000`, or one data file `name0.f00001`) as a time series.
pub fn read(path: &Path) -> Result<crate::Loaded> {
    let list =
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("nek5000")) { files(path)? } else { vec![path.to_path_buf()] };
    let mut coords: Option<Vec<[f64; 3]>> = None;
    let mut times = Vec::new();
    let mut steps = Vec::new();
    for f in &list {
        let s = read_step(f)?;
        if let Some(c) = s.coords {
            coords = Some(c);
        }
        let pts = coords.clone().ok_or_else(|| anyhow!("{}: no coordinates in this or an earlier file", f.display()))?;
        let [nx, ny, nz] = s.n;
        let npe = nx * ny * nz;
        if pts.len() != s.nelt * npe {
            bail!("{}: {} elements here, coordinates for {}", f.display(), s.nelt, pts.len() / npe.max(1));
        }
        let mut cells = Cells::new();
        let id = |e: usize, i: usize, j: usize, k: usize| (e * npe + i + nx * (j + ny * k)) as u32;
        for e in 0..s.nelt {
            if nz > 1 {
                for k in 0..nz - 1 {
                    for j in 0..ny - 1 {
                        for i in 0..nx - 1 {
                            cells.push(
                                CellType::Hexahedron,
                                &[
                                    id(e, i, j, k),
                                    id(e, i + 1, j, k),
                                    id(e, i + 1, j + 1, k),
                                    id(e, i, j + 1, k),
                                    id(e, i, j, k + 1),
                                    id(e, i + 1, j, k + 1),
                                    id(e, i + 1, j + 1, k + 1),
                                    id(e, i, j + 1, k + 1),
                                ],
                            );
                        }
                    }
                }
            } else {
                for j in 0..ny - 1 {
                    for i in 0..nx - 1 {
                        cells.push(CellType::Quad, &[id(e, i, j, 0), id(e, i + 1, j, 0), id(e, i + 1, j + 1, 0), id(e, i, j + 1, 0)]);
                    }
                }
            }
        }
        let mut ug = UnstructuredGrid { points: Points::new_f64(pts.iter().flatten().copied().collect()), cells, ..Default::default() };
        for (name, nc, v) in s.arrays {
            ug.point_data.insert(Array::new(name, nc, ArrayData::F64(v)));
        }
        let elem: Vec<i64> =
            (0..s.nelt).flat_map(|e| std::iter::repeat_n(e as i64, if nz > 1 { (nx - 1) * (ny - 1) * (nz - 1) } else { (nx - 1) * (ny - 1) })).collect();
        ug.cell_data.insert(Array::new("Element", 1, ArrayData::I64(elem)));
        times.push(s.time);
        steps.push(TimeStepRef::Loaded(Arc::new(DataSet::UnstructuredGrid(ug))));
    }
    if steps.len() == 1 {
        let TimeStepRef::Loaded(d) = steps.pop().unwrap() else { unreachable!() };
        return Ok(crate::Loaded::Static(d));
    }
    Ok(crate::Loaded::Series(TimeSeries { times, steps }))
}

/// A Nek5000 data file name: `name0.f00001` (a dot, an f, digits).
pub fn is_nek_data(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.len() >= 2 && e.starts_with('f') && e[1..].chars().all(|c| c.is_ascii_digit()))
}
