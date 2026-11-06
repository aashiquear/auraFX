//! Enzo AMR output: a parameter file (`DD0010/data0010`), its `.hierarchy` (one entry per
//! grid: cell index range, edges, the HDF5 file holding it) and HDF5 files with a
//! `GridNNNNNNNN` group per grid. Read as overlapping AMR data the way VTK's Enzo reader lays
//! it out: the hierarchy is parsed as VTK parses it (so levels and parents are the same), the
//! fields of the grid with the fewest cells name the cell arrays, and values keep their stored
//! type. "Convert to CGS" multiplies fields by the parameter file's `#DataCGSConversionFactor`.

use crate::{option_bool, ReaderOption, ReaderOptionKind};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::amr::{Amr, AmrBlock, AmrLevel};
use aurafx_data::*;
use hdf5_reader::Hdf5File;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn is_enzo(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("hierarchy" | "boundary"))
}

pub fn options(_path: &Path) -> Vec<ReaderOption> {
    vec![ReaderOption::new(
        "cgs",
        "Convert to CGS",
        ReaderOptionKind::Bool,
        false,
        "Multiply each field by its #DataCGSConversionFactor from the parameter file (ParaView's Convert To CGS; off by default, as in ParaView).",
    )]
}

#[derive(Debug, Clone, Default)]
struct Grid {
    index: usize,
    level: usize,
    cells: [usize; 3],
    lo: [f64; 3],
    hi: [f64; 3],
    file: PathBuf,
    particles: i64,
}

/// Whitespace-separated tokens, with access to the raw characters (for `Pointer:` lines).
struct Tokens<'a> {
    s: &'a [u8],
    at: usize,
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Option<String> {
        while self.at < self.s.len() && self.s[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
        if self.at >= self.s.len() {
            return None;
        }
        let start = self.at;
        while self.at < self.s.len() && !self.s[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.at]).into_owned())
    }
    fn until(&mut self, word: &str) -> bool {
        while let Some(t) = self.next() {
            if t == word {
                return true;
            }
        }
        false
    }
    fn num<T: std::str::FromStr>(&mut self) -> Option<T> {
        self.next().and_then(|t| t.parse().ok())
    }
    fn char(&mut self) -> Option<u8> {
        let c = *self.s.get(self.at)?;
        self.at += 1;
        Some(c)
    }
}

/// The parameter file: (rank, time, cycle, field index by label, conversion factor by index).
fn parameters(path: &Path) -> Result<(usize, f64, HashMap<String, usize>, HashMap<usize, f64>)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading the Enzo parameter file {}", path.display()))?;
    let mut rank = 3;
    let mut time = 0.0;
    let mut labels = HashMap::new();
    let mut factors = HashMap::new();
    let mut t = Tokens { s: text.as_bytes(), at: 0 };
    while let Some(w) = t.next() {
        match w.as_str() {
            "InitialTime" => {
                t.next();
                time = t.num().unwrap_or(time);
            }
            "TopGridRank" => {
                t.next();
                rank = t.num().unwrap_or(rank);
            }
            _ => {}
        }
    }
    // VTK takes the label index from the character before the closing bracket.
    let index_of = |w: &str| -> usize { w.as_bytes().get(w.len().saturating_sub(2)).map(|c| c.wrapping_sub(b'0') as usize).unwrap_or(0) };
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().filter(|w| !w.starts_with('=')).collect();
        if line.starts_with("DataLabel") && words.len() >= 2 {
            labels.insert(words[words.len() - 1].to_string(), index_of(words[0]));
        } else if line.starts_with("#DataCGSConversionFactor") && words.len() >= 2 {
            factors.insert(index_of(words[0]), words[words.len() - 1].parse().unwrap_or(1.0));
        }
    }
    Ok((rank, time, labels, factors))
}

/// The grids of a hierarchy file, as VTK's Enzo reader parses it.
fn hierarchy(path: &Path, rank: usize, dir: &Path) -> Result<(Vec<Grid>, Option<f64>)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut t = Tokens { s: text.as_bytes(), at: 0 };
    // Index 0 is VTK's root pseudo-block (level -1, here 0 with `root` true).
    let mut levels: Vec<i64> = vec![-1];
    let mut grids: Vec<Grid> = Vec::new();
    let (mut level, mut time) = (0i64, None);
    let mut word = String::new();
    loop {
        while word != "Grid" && word != "Time" && word != "Pointer:" {
            match t.next() {
                Some(w) => word = w,
                None => return Ok((grids, time)),
            }
        }
        match word.as_str() {
            "Grid" => {
                t.next();
                let index: usize = t.num().ok_or_else(|| anyhow!("bad Grid line"))?;
                let read3 = |t: &mut Tokens, key: &str| -> Result<[f64; 3]> {
                    if !t.until(key) {
                        bail!("grid {index}: no {key}");
                    }
                    t.next();
                    let mut v = [0.0; 3];
                    for x in v.iter_mut().take(rank) {
                        *x = t.num().ok_or_else(|| anyhow!("grid {index}: bad {key}"))?;
                    }
                    Ok(v)
                };
                let start = read3(&mut t, "GridStartIndex")?;
                let end = read3(&mut t, "GridEndIndex")?;
                let mut cells = [1usize; 3];
                for k in 0..rank.min(3) {
                    cells[k] = (end[k] - start[k] + 1.0).max(1.0) as usize;
                }
                let lo = read3(&mut t, "GridLeftEdge")?;
                let hi = read3(&mut t, "GridRightEdge")?;
                if !t.until("BaryonFileName") {
                    bail!("grid {index}: no BaryonFileName");
                }
                t.next();
                let name = t.next().unwrap_or_default();
                let base = Path::new(&name).file_name().map(|f| f.to_os_string()).unwrap_or_default();
                if !t.until("NumberOfParticles") {
                    bail!("grid {index}: no NumberOfParticles");
                }
                t.next();
                let particles: i64 = t.num().unwrap_or(0);
                if particles > 0 && t.until("ParticleFileName") {
                    t.next();
                    t.next();
                }
                if index != levels.len() {
                    bail!("{}: grids must be listed in order (grid {index} after {})", path.display(), levels.len() - 1);
                }
                levels.push(level);
                grids.push(Grid { index, level: level.max(0) as usize, cells, lo, hi, file: dir.join(base), particles });
            }
            "Pointer:" => {
                while t.char().is_some_and(|c| c != b'[') {}
                let mut s = String::new();
                while let Some(c) = t.char() {
                    if c == b']' {
                        break;
                    }
                    s.push(c as char);
                }
                let blk: usize = s.trim().parse().unwrap_or(0);
                t.char();
                t.char();
                let which = t.next().unwrap_or_default();
                t.next();
                let target: i64 = t.num().unwrap_or(0);
                if which == "NextGridNextLevel" && target != 0 {
                    level = levels.get(blk).copied().unwrap_or(-1) + 1;
                }
            }
            _ => {
                t.next();
                time = t.num();
            }
        }
        word = t.next().unwrap_or_default();
        if word.is_empty() && t.at >= t.s.len() {
            return Ok((grids, time));
        }
    }
}

/// The group of grid `index` in its HDF5 file (VTK takes the first group named Grid<index> or
/// Grid<index + 1>).
fn grid_group(f: &Hdf5File, index: usize) -> Result<hdf5_reader::group::Group> {
    let root = f.root_group()?;
    for g in root.groups()? {
        let n = g.name().trim_start_matches('/').to_string();
        if let Some(i) = n.strip_prefix("Grid").and_then(|i| i.parse::<usize>().ok()) {
            if i == index || i == index + 1 {
                return Ok(g);
            }
        }
    }
    bail!("no group for grid {index}")
}

pub fn read(path: &Path) -> Result<DataSet> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let major = path.with_extension("");
    let hier = major.with_extension(if ext == "boundary" { "hierarchy" } else { ext });
    let hier = if hier.exists() { hier } else { PathBuf::from(format!("{}.hierarchy", major.display())) };
    let dir = major.parent().map(Path::to_path_buf).unwrap_or_default();
    let (rank, ptime, labels, factors) = parameters(&major)?;
    let (grids, htime) = hierarchy(&hier, rank, &dir)?;
    if grids.is_empty() {
        bail!("{}: no grids in the hierarchy", hier.display());
    }
    let cgs = option_bool("cgs", false);
    let mut files: HashMap<PathBuf, Hdf5File> = HashMap::new();
    fn open(files: &mut HashMap<PathBuf, Hdf5File>, p: &Path) -> Result<()> {
        if !files.contains_key(p) {
            files.insert(p.to_path_buf(), Hdf5File::open(p).with_context(|| format!("opening the Enzo grid file {}", p.display()))?);
        }
        Ok(())
    }
    // Field names from the grid with the fewest cells (one with particles if any has them).
    let mut reference: Option<&Grid> = None;
    for g in &grids {
        let n = g.cells.iter().product::<usize>();
        let better = match reference {
            None => true,
            Some(r) => (g.particles > 0 && r.particles <= 0) || ((g.particles > 0) == (r.particles > 0) && n < r.cells.iter().product::<usize>()),
        };
        if better {
            reference = Some(g);
        }
    }
    let r = reference.unwrap();
    open(&mut files, &r.file)?;
    let names: Vec<String> = grid_group(&files[&r.file], r.index)?
        .datasets()?
        .iter()
        .map(|d| d.name().rsplit('/').next().unwrap_or("").to_string())
        .filter(|n| !n.starts_with("particle") && !n.starts_with("tracer_particles"))
        .collect();
    let origin: [f64; 3] = std::array::from_fn(|k| grids.iter().map(|g| g.lo[k]).fold(f64::INFINITY, f64::min));
    let n_levels = grids.iter().map(|g| g.level).max().unwrap_or(0) + 1;
    let mut amr = Amr { origin, levels: vec![AmrLevel::default(); n_levels] };
    for g in &grids {
        open(&mut files, &g.file)?;
        let group = grid_group(&files[&g.file], g.index).with_context(|| format!("grid {}", g.index))?;
        let dims: [usize; 3] = std::array::from_fn(|k| if k < rank { g.cells[k] + 1 } else { 1 });
        let spacing: [f64; 3] = std::array::from_fn(|k| if dims[k] > 1 { (g.hi[k] - g.lo[k]) / (dims[k] as f64 - 1.0) } else { 1.0 });
        let mut img = ImageData { origin: g.lo, spacing, extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1], ..Default::default() };
        let n = g.cells.iter().product::<usize>();
        for name in &names {
            let Ok(ds) = group.dataset(name) else { continue };
            let Ok((mut data, _)) = crate::vtkhdf::read(&ds) else { continue };
            if data.len() != n {
                continue;
            }
            if cgs {
                let fct = labels.get(name).and_then(|i| factors.get(i)).copied().unwrap_or(1.0);
                if fct != 1.0 {
                    let scaled: Vec<f64> = data.to_f64_vec().iter().map(|v| v * fct).collect();
                    data = ArrayData::from_f64(data.scalar_type(), &scaled);
                }
            }
            img.cell_data.insert(Array::new(name.clone(), 1, data));
        }
        amr.levels[g.level].spacing = spacing;
        amr.levels[g.level].blocks.push(AmrBlock::locate(origin, spacing, img));
    }
    amr.levels.retain(|l| !l.blocks.is_empty());
    amr.blank();
    let mut ds = amr.into_dataset();
    if let DataSet::MultiBlock(mb) = &mut ds {
        mb.field_data.insert(Array::scalars_f64("TimeValue", vec![htime.unwrap_or(ptime)]));
    }
    Ok(ds)
}
