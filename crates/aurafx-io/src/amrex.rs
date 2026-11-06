//! AMReX / BoxLib plotfiles: a directory with a `Header` and `Level_n/Cell_H` + `Cell_D_*`
//! files, read as AMR data (levels of image boxes with cell data, covered coarse cells marked)
//! the way ParaView's AMReX grid reader lays them out. Variables named `amrexvec_<name>_x/_y/_z`
//! become one vector `<name>`.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::amr::{Amr, AmrBlock, AmrLevel};
use aurafx_data::*;
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The plotfile directory for `path` (the directory itself, or its `Header` file).
pub fn plotfile_dir(path: &Path) -> Option<PathBuf> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else if path.file_name().is_some_and(|n| n == "Header") {
        path.parent()?.to_path_buf()
    } else {
        return None;
    };
    let mut head = String::new();
    std::fs::File::open(dir.join("Header")).ok()?.take(64).read_to_string(&mut head).ok()?;
    let first = head.lines().next().unwrap_or("").trim();
    (first.starts_with("HyperCLaw") || first.starts_with("NavierStokes") || first.starts_with("AMReX")).then_some(dir)
}

struct Tokens<'a> {
    it: std::iter::Peekable<std::str::SplitWhitespace<'a>>,
}

impl<'a> Tokens<'a> {
    fn new(s: &'a str) -> Self {
        Tokens { it: s.split_whitespace().peekable() }
    }
    fn word(&mut self) -> Result<&'a str> {
        self.it.next().ok_or_else(|| anyhow!("plotfile header ends early"))
    }
    fn num<T: std::str::FromStr>(&mut self) -> Result<T> {
        let w = self.word()?;
        w.trim_end_matches(',').parse().map_err(|_| anyhow!("plotfile header: expected a number, found {w:?}"))
    }
}

/// Integers of a box "((lo,lo,lo) (hi,hi,hi) (t,t,t))" (possibly split over tokens).
fn ints(s: &str) -> Vec<i64> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect()
}

struct Fab {
    lo: Vec<i64>,
    hi: Vec<i64>,
    /// Per variable, the values of the valid box (x fastest).
    values: Vec<Vec<f64>>,
    single: bool,
}

/// Read one FAB (header line, then the values variable after variable) at `offset`.
fn read_fab(file: &Path, offset: u64, dim: usize, valid: (&[i64], &[i64])) -> Result<Fab> {
    let mut f = std::io::BufReader::new(std::fs::File::open(file).with_context(|| format!("opening {}", file.display()))?);
    f.seek(SeekFrom::Start(offset))?;
    let mut line = Vec::new();
    f.read_until(b'\n', &mut line)?;
    let head = String::from_utf8_lossy(&line).into_owned();
    let head = head.trim();
    if !head.starts_with("FAB") {
        bail!("{}: no FAB header at offset {offset} (plotfiles without FAB headers are not supported)", file.display());
    }
    // FAB ((8, (64 11 52 0 1 12 0 1023)),(8, (8 7 6 5 4 3 2 1)))((lo) (hi) (type)) ncomp
    let split = head.find(")))").ok_or_else(|| anyhow!("bad FAB header {head:?}"))? + 3;
    let desc = ints(&head[3..split]);
    // desc: [nfmt, fmt..., nord, ord...]
    let nfmt = *desc.first().ok_or_else(|| anyhow!("bad FAB header {head:?}"))? as usize;
    let bits = *desc.get(1).ok_or_else(|| anyhow!("bad FAB header"))?;
    let ord_at = 1 + nfmt;
    let order: Vec<i64> = desc.get(ord_at + 1..).unwrap_or(&[]).to_vec();
    let little = order.first().is_some_and(|&o| o as usize == order.len());
    let rest = &head[split..];
    let nums = ints(rest);
    if nums.len() < 3 * dim + 1 {
        bail!("bad FAB box in {head:?}");
    }
    let (lo, hi) = (nums[..dim].to_vec(), nums[dim..2 * dim].to_vec());
    let ncomp = *nums.last().unwrap() as usize;
    let size: Vec<usize> = (0..dim).map(|k| (hi[k] - lo[k] + 1).max(0) as usize).collect();
    let npts: usize = size.iter().product();
    let bytes = match bits {
        64 => 8,
        32 => 4,
        b => bail!("{} bit reals are not supported", b),
    };
    let mut raw = vec![0u8; npts * ncomp * bytes];
    f.read_exact(&mut raw).with_context(|| format!("{}: FAB data ends early", file.display()))?;
    let val = |i: usize| -> f64 {
        let b = &raw[i * bytes..(i + 1) * bytes];
        match (bytes, little) {
            (8, true) => f64::from_le_bytes(b.try_into().unwrap()),
            (8, false) => f64::from_be_bytes(b.try_into().unwrap()),
            (4, true) => f32::from_le_bytes(b.try_into().unwrap()) as f64,
            _ => f32::from_be_bytes(b.try_into().unwrap()) as f64,
        }
    };
    // Keep the valid box (FABs may carry ghost cells around it).
    let (vlo, vhi) = valid;
    let vsize: Vec<usize> = (0..dim).map(|k| (vhi[k] - vlo[k] + 1).max(0) as usize).collect();
    let nv: usize = vsize.iter().product();
    let mut values = Vec::with_capacity(ncomp);
    for c in 0..ncomp {
        let mut v = Vec::with_capacity(nv);
        let base = c * npts;
        let mut idx = [0usize; 3];
        for n in 0..nv {
            let mut r = n;
            for k in 0..dim {
                idx[k] = r % vsize[k];
                r /= vsize[k];
            }
            let mut lin = 0usize;
            let mut stride = 1usize;
            for k in 0..dim {
                lin += ((vlo[k] - lo[k]) as usize + idx[k]) * stride;
                stride *= size[k];
            }
            v.push(val(base + lin));
        }
        values.push(v);
    }
    Ok(Fab { lo: vlo.to_vec(), hi: vhi.to_vec(), values, single: bytes == 4 })
}

/// Variables as arrays: (array name, component variable indices; 3 for vectors).
fn group_variables(names: &[String]) -> Vec<(String, Vec<Option<usize>>)> {
    let mut out: Vec<(String, Vec<Option<usize>>)> = Vec::new();
    for (i, n) in names.iter().enumerate() {
        if let Some(rest) = n.strip_prefix("amrexvec_") {
            if let Some((base, comp)) = rest.rsplit_once('_') {
                let k = match comp {
                    "x" => Some(0),
                    "y" => Some(1),
                    "z" => Some(2),
                    _ => None,
                };
                if let Some(k) = k {
                    match out.iter_mut().find(|(b, c)| b == base && c.len() == 3) {
                        Some((_, c)) => c[k] = Some(i),
                        None => {
                            let mut c = vec![None; 3];
                            c[k] = Some(i);
                            out.push((base.to_string(), c));
                        }
                    }
                    continue;
                }
            }
        }
        out.push((n.clone(), vec![Some(i)]));
    }
    out
}

pub fn read(path: &Path) -> Result<DataSet> {
    let dir = plotfile_dir(path).ok_or_else(|| anyhow!("{} is not an AMReX plotfile (a directory with a Header)", path.display()))?;
    let text = std::fs::read_to_string(dir.join("Header"))?;
    let mut t = Tokens::new(&text);
    let _version = t.word()?;
    let nvars: usize = t.num()?;
    let names: Vec<String> = (0..nvars).map(|_| t.word().map(str::to_string)).collect::<Result<_>>()?;
    let dim: usize = t.num()?;
    if !(1..=3).contains(&dim) {
        bail!("plotfile dimension {dim}");
    }
    let time: f64 = t.num()?;
    let finest: usize = t.num()?;
    let prob_lo: Vec<f64> = (0..dim).map(|_| t.num()).collect::<Result<_>>()?;
    let _prob_hi: Vec<f64> = (0..dim).map(|_| t.num()).collect::<Result<_>>()?;
    let _ratios: Vec<i64> = (0..finest).map(|_| t.num()).collect::<Result<_>>()?;
    // Level domains: "((0,0,0) (15,15,15) (0,0,0))" per level, maybe split over tokens.
    let mut depth = 0i32;
    let mut domains_left = finest + 1;
    while domains_left > 0 {
        let w = t.word()?;
        for c in w.chars() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        domains_left -= 1;
                    }
                }
                _ => {}
            }
        }
    }
    for _ in 0..=finest {
        let _step: i64 = t.num()?;
    }
    let mut dx: Vec<[f64; 3]> = Vec::new();
    for _ in 0..=finest {
        let mut d = [0.0; 3];
        for v in d.iter_mut().take(dim) {
            *v = t.num()?;
        }
        // Missing axes: ParaView uses the y spacing for z in 2-D.
        for k in dim..3 {
            d[k] = d[dim - 1];
        }
        dx.push(d);
    }
    let _coord: i64 = t.num()?;
    let _zero: i64 = t.num()?;
    let mut level_paths = Vec::new();
    for _ in 0..=finest {
        let _lev: i64 = t.num()?;
        let ngrids: usize = t.num()?;
        let _time: f64 = t.num()?;
        let _step: i64 = t.num()?;
        for _ in 0..ngrids * dim * 2 {
            let _: f64 = t.num()?;
        }
        level_paths.push(t.word()?.to_string());
    }
    let mut origin = [0.0; 3];
    origin[..dim].copy_from_slice(&prob_lo);
    let groups = group_variables(&names);
    let mut amr = Amr { origin, levels: Vec::new() };
    for (l, lp) in level_paths.iter().enumerate() {
        let cell_h = dir.join(format!("{lp}_H"));
        let h = std::fs::read_to_string(&cell_h).with_context(|| format!("reading {}", cell_h.display()))?;
        let mut ht = Tokens::new(&h);
        let version: i64 = ht.num()?;
        if version != 1 {
            bail!("{}: VisMF version {version} (without FAB headers) is not supported", cell_h.display());
        }
        let _how: i64 = ht.num()?;
        let ncomp: usize = ht.num()?;
        let _nghost: i64 = ht.num()?;
        let first = ht.word()?; // "(n"
        let nboxes: usize = first.trim_start_matches('(').parse().map_err(|_| anyhow!("{}: bad box array", cell_h.display()))?;
        let _zero = ht.word()?;
        let mut boxes = Vec::with_capacity(nboxes);
        for _ in 0..nboxes {
            // ((lo) (hi) (type)) as one or more tokens.
            let mut s = String::new();
            let mut d = 0i32;
            loop {
                let w = ht.word()?;
                for c in w.chars() {
                    match c {
                        '(' => d += 1,
                        ')' => d -= 1,
                        _ => {}
                    }
                }
                s.push_str(w);
                s.push(' ');
                if d <= 0 {
                    break;
                }
            }
            let v = ints(&s);
            if v.len() < 2 * dim {
                bail!("{}: bad box {s:?}", cell_h.display());
            }
            boxes.push((v[..dim].to_vec(), v[dim..2 * dim].to_vec()));
        }
        let _close = ht.word()?;
        let nfabs: usize = ht.num()?;
        let mut fabs = Vec::with_capacity(nfabs);
        for _ in 0..nfabs {
            let _prefix = ht.word()?;
            let file = ht.word()?.to_string();
            let off: u64 = ht.num()?;
            fabs.push((file, off));
        }
        if ncomp != nvars {
            bail!("{}: {ncomp} components but the header names {nvars} variables", cell_h.display());
        }
        let level_dir = cell_h.parent().unwrap().to_path_buf();
        let spacing = dx[l];
        let mut lev = AmrLevel { spacing, blocks: Vec::with_capacity(nboxes) };
        for (b, ((lo, hi), (file, off))) in boxes.iter().zip(&fabs).enumerate() {
            let fab = read_fab(&level_dir.join(file), *off, dim, (lo, hi)).with_context(|| format!("level {l}, box {b}"))?;
            let mut img = ImageData { spacing, ..Default::default() };
            let mut lo3 = [0i64; 3];
            let mut hi3 = [0i64; 3];
            for k in 0..dim {
                lo3[k] = fab.lo[k];
                hi3[k] = fab.hi[k];
                img.origin[k] = origin[k] + fab.lo[k] as f64 * spacing[k];
                img.extent[2 * k] = 0;
                img.extent[2 * k + 1] = fab.hi[k] - fab.lo[k] + 1;
            }
            for k in dim..3 {
                img.extent[2 * k] = 0;
                img.extent[2 * k + 1] = 0;
            }
            let n = fab.values.first().map(|v| v.len()).unwrap_or(0);
            for (name, comps) in &groups {
                let data: Vec<f64> = if comps.len() == 1 {
                    fab.values[comps[0].unwrap()].clone()
                } else {
                    let vals = &fab.values;
                    let mut d = Vec::with_capacity(3 * n);
                    for i in 0..n {
                        d.extend(comps.iter().map(|c| c.map(|c| vals[c][i]).unwrap_or(0.0)));
                    }
                    d
                };
                let arr = if fab.single { ArrayData::F32(data.iter().map(|&v| v as f32).collect()) } else { ArrayData::F64(data) };
                img.cell_data.insert(Array::new(name.clone(), comps.len(), arr));
            }
            lev.blocks.push(AmrBlock { lo: lo3, hi: hi3, data: img });
        }
        amr.levels.push(lev);
    }
    amr.blank();
    let mut ds = amr.into_dataset();
    if let DataSet::MultiBlock(mb) = &mut ds {
        mb.field_data.insert(Array::scalars_f64("TimeValue", vec![time]));
    }
    Ok(ds)
}
