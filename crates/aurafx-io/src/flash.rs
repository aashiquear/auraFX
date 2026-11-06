//! FLASH AMR files (HDF5 plot and checkpoint files, `name_hdf5_plt_cnt_NNNN`): PARAMESH blocks
//! of nxb x nyb x nzb cells on refinement levels, read as overlapping AMR data the way VTK's
//! FLASH reader lays them out (every block, coarse ones included; level = refine level - 1;
//! cell data as doubles; cells under finer blocks hidden).

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::amr::{Amr, AmrBlock, AmrLevel};
use aurafx_data::*;
use hdf5_reader::{Datatype, Hdf5File};
use std::collections::HashMap;
use std::path::Path;

/// A value of a compound record.
#[derive(Debug, Clone)]
pub(crate) enum Rec {
    Int(i64),
    Real(f64),
    Text(String),
}

impl Rec {
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Rec::Int(i) => Some(*i as f64),
            Rec::Real(r) => Some(*r),
            Rec::Text(_) => None,
        }
    }
}

/// The records of a compound (little-endian) HDF5 dataset, field by field.
pub(crate) fn records(f: &Hdf5File, name: &str) -> Result<Vec<HashMap<String, Rec>>> {
    let ds = f.dataset(name).with_context(|| format!("missing {name}"))?;
    let Datatype::Compound { size, fields } = ds.dtype().clone() else { bail!("{name} is not a compound dataset") };
    let raw = ds.read_raw_bytes()?;
    let size = size as usize;
    let mut out = Vec::new();
    for rec in raw.chunks_exact(size.max(1)) {
        let mut m = HashMap::new();
        for fl in &fields {
            let o = fl.byte_offset as usize;
            let v = match &fl.datatype {
                Datatype::FixedPoint { size, signed, .. } => {
                    let b = rec.get(o..o + *size as usize).ok_or_else(|| anyhow!("{name}: field out of the record"))?;
                    let mut x = [0u8; 8];
                    x[..b.len()].copy_from_slice(b);
                    let mut v = i64::from_le_bytes(x);
                    if *signed && b.len() < 8 && b[b.len() - 1] & 0x80 != 0 {
                        v -= 1i64 << (8 * b.len());
                    }
                    Rec::Int(v)
                }
                Datatype::FloatingPoint { size: 4, .. } => Rec::Real(f32::from_le_bytes(rec[o..o + 4].try_into()?) as f64),
                Datatype::FloatingPoint { size: 8, .. } => Rec::Real(f64::from_le_bytes(rec[o..o + 8].try_into()?)),
                Datatype::String { size: hdf5_reader::StringSize::Fixed(n), .. } => {
                    let b = rec.get(o..o + *n as usize).unwrap_or(&[]);
                    Rec::Text(String::from_utf8_lossy(b).trim_end_matches(['\0', ' ']).to_string())
                }
                _ => continue,
            };
            m.insert(fl.name.clone(), v);
        }
        out.push(m);
    }
    Ok(out)
}

/// A FLASH name/value table ("integer scalars", "real scalars") as a map.
fn scalars(f: &Hdf5File, name: &str) -> HashMap<String, f64> {
    records(f, name)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| match (r.get("name"), r.get("value").and_then(|v| v.as_f64())) {
            (Some(Rec::Text(n)), Some(v)) => Some((n.trim().to_string(), v)),
            _ => None,
        })
        .collect()
}

fn values(f: &Hdf5File, name: &str) -> Result<(Vec<f64>, Vec<usize>)> {
    let ds = f.dataset(name).with_context(|| format!("missing {name}"))?;
    let (d, shape) = crate::vtkhdf::read(&ds)?;
    Ok((d.to_f64_vec(), shape))
}

fn is_hdf5(path: &Path) -> bool {
    let mut head = [0u8; 8];
    std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && head == *b"\x89HDF\r\n\x1a\n"
}

/// An HDF5 file with FLASH's block datasets.
pub fn is_flash(path: &Path) -> bool {
    if !path.is_file() || !is_hdf5(path) {
        return false;
    }
    let Ok(f) = Hdf5File::open(path) else { return false };
    ["gid", "unknown names", "bounding box", "refine level"].iter().all(|n| f.dataset(n).is_ok())
}

/// The particle dataset of a FLASH file, if it has particles.
fn particle_dataset(f: &Hdf5File) -> Option<hdf5_reader::Dataset> {
    ["particle tracers", "tracer particles"].iter().find_map(|n| f.dataset(n).ok()).filter(|d| d.shape().first().copied().unwrap_or(0) > 0)
}

pub const GRID: &str = "Grid";
pub const PARTICLES: &str = "Particles";

pub fn options(path: &Path) -> Vec<crate::ReaderOption> {
    let has = Hdf5File::open(path).ok().is_some_and(|f| particle_dataset(&f).is_some());
    if !has {
        return Vec::new();
    }
    vec![crate::ReaderOption::new(
        "data",
        "Data",
        crate::ReaderOptionKind::Choice(vec![GRID.into(), PARTICLES.into()]),
        GRID,
        "The AMR grid, or the file's tracer particles as points with their properties.",
    )]
}

/// The tracer particles: a point each, with every property as a double (as VTK reads them).
/// FLASH 3 and later keep them in a table whose columns "particle names" lists; FLASH 2 in a
/// compound dataset with a field per property.
pub fn read_particles(path: &Path) -> Result<PolyData> {
    let f = Hdf5File::open(path).with_context(|| format!("opening {} as HDF5", path.display()))?;
    let ds = particle_dataset(&f).ok_or_else(|| anyhow!("{}: no particles", path.display()))?;
    let n = ds.shape()[0] as usize;
    let mut cols: Vec<(String, Vec<f64>)> = Vec::new();
    let (px, py, pz);
    if let Datatype::Compound { size, fields } = ds.dtype().clone() {
        let raw = ds.read_raw_bytes()?;
        for fl in &fields {
            let o = fl.byte_offset as usize;
            let get = |rec: &[u8]| -> f64 {
                match &fl.datatype {
                    Datatype::FloatingPoint { size: 8, .. } => f64::from_le_bytes(rec[o..o + 8].try_into().unwrap()),
                    Datatype::FloatingPoint { size: 4, .. } => f32::from_le_bytes(rec[o..o + 4].try_into().unwrap()) as f64,
                    Datatype::FixedPoint { size: 4, signed, .. } => {
                        let b: [u8; 4] = rec[o..o + 4].try_into().unwrap();
                        if *signed { i32::from_le_bytes(b) as f64 } else { u32::from_le_bytes(b) as f64 }
                    }
                    Datatype::FixedPoint { size: 8, .. } => i64::from_le_bytes(rec[o..o + 8].try_into().unwrap()) as f64,
                    _ => f64::NAN,
                }
            };
            cols.push((fl.name.clone(), raw.chunks_exact(size as usize).take(n).map(get).collect()));
        }
        let take = |name: &str| cols.iter().find(|c| c.0 == name).map(|c| c.1.clone()).unwrap_or_else(|| vec![0.0; n]);
        px = take("particle_x");
        py = take("particle_y");
        pz = take("particle_z");
        cols.retain(|c| !matches!(c.0.as_str(), "particle_x" | "particle_y" | "particle_z"));
    } else {
        let names: Vec<String> = f
            .dataset("particle names")
            .context("FLASH particles without particle names")?
            .read_strings()?
            .into_iter()
            .map(|s| s.trim_end_matches(['\0', ' ']).split(' ').next().unwrap_or("").to_string())
            .collect();
        let (v, shape) = values(&f, ds.name().trim_start_matches('/'))?;
        let np = shape.get(1).copied().unwrap_or(1);
        for (k, name) in names.iter().enumerate() {
            if name == "particle_x" || name == "particle_y" || name == "particle_z" {
                continue;
            }
            cols.push((name.clone(), (0..n).map(|i| v.get(i * np + k).copied().unwrap_or(f64::NAN)).collect()));
        }
        let take = |name: &str| cols.iter().find(|c| c.0 == name).map(|c| c.1.clone()).unwrap_or_else(|| vec![0.0; n]);
        px = take("posx");
        py = take("posy");
        pz = take("posz");
    }
    let pts: Vec<f64> = (0..n).flat_map(|i| [px[i], py[i], pz[i]]).collect();
    let mut pd = PolyData::from_points(Points::new_f64(pts));
    for (name, v) in cols {
        pd.point_data.insert(Array::scalars_f64(name, v));
    }
    Ok(pd)
}

pub fn read(path: &Path) -> Result<DataSet> {
    if crate::option("data").as_deref() == Some(PARTICLES) {
        return Ok(DataSet::PolyData(read_particles(path)?));
    }
    let f = Hdf5File::open(path).with_context(|| format!("opening {} as HDF5", path.display()))?;
    // File format version: 7 and older are FLASH 2, 8 and 9 FLASH 3 (as VTK decides it).
    let ffv: i64 = if let Ok((v, _)) = values(&f, "file format version") {
        v.first().copied().unwrap_or(7.0) as i64
    } else if let Some(v) = records(&f, "sim info").ok().and_then(|r| r.first().and_then(|m| m.get("file format version").and_then(|v| v.as_f64()))) {
        v as i64
    } else if f.dataset("particle names").is_ok() {
        8
    } else {
        7
    };
    let gid = f.dataset("gid").context("no gid dataset (block connectivity)")?;
    let nb = gid.shape().first().copied().unwrap_or(0) as usize;
    let dim = match gid.shape().get(1) {
        Some(5) => 1,
        Some(9) => 2,
        Some(15) => 3,
        s => bail!("{}: unexpected gid shape {s:?}", path.display()),
    };
    let (bbox, bshape) = values(&f, "bounding box")?;
    let bd = bshape.get(1).copied().unwrap_or(dim);
    let (levels, _) = values(&f, "refine level")?;
    if levels.len() != nb || bbox.len() != nb * bd * 2 {
        bail!("{}: inconsistent number of blocks", path.display());
    }
    let (n, time) = if ffv < 8 {
        let p = records(&f, "simulation parameters").context("FLASH 2 file without simulation parameters")?;
        let p = p.first().ok_or_else(|| anyhow!("empty simulation parameters"))?;
        let g = |k: &str| p.get(k).and_then(|v| v.as_f64()).unwrap_or(1.0);
        ([g("nxb"), g("nyb"), g("nzb")], p.get("time").and_then(|v| v.as_f64()))
    } else {
        let i = scalars(&f, "integer scalars");
        let r = scalars(&f, "real scalars");
        let g = |k: &str| i.get(k).copied().unwrap_or(1.0);
        ([g("nxb"), g("nyb"), g("nzb")], r.get("time").copied())
    };
    let cells: [usize; 3] = n.map(|v| (v as usize).max(1));
    let dims: [usize; 3] = cells.map(|c| if c == 1 { 1 } else { c + 1 });
    // Block bounds (axes past the file's dimension at 0).
    let bounds: Vec<([f64; 3], [f64; 3])> = (0..nb)
        .map(|b| {
            let mut lo = [0.0; 3];
            let mut hi = [0.0; 3];
            for d in 0..3.min(bd) {
                if bd == 3 || d < dim {
                    lo[d] = bbox[(b * bd + d) * 2];
                    hi[d] = bbox[(b * bd + d) * 2 + 1];
                }
            }
            (lo, hi)
        })
        .collect();
    let origin: [f64; 3] = std::array::from_fn(|k| bounds.iter().map(|b| b.0[k]).fold(f64::INFINITY, f64::min));
    let names: Vec<String> = f
        .dataset("unknown names")
        .context("no unknown names")?
        .read_strings()?
        .into_iter()
        .map(|s| s.trim_end_matches(['\0', ' ']).to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let per_block = cells[0] * cells[1] * cells[2];
    let mut fields: Vec<(String, Vec<f64>)> = Vec::new();
    for name in &names {
        let Ok((v, shape)) = values(&f, name) else { continue };
        if shape.len() != 4 || v.len() != nb * per_block {
            continue;
        }
        fields.push((name.clone(), v));
    }
    let n_levels = levels.iter().map(|&l| l as usize).max().unwrap_or(1).max(1);
    let mut amr = Amr { origin, levels: vec![AmrLevel::default(); n_levels] };
    for b in 0..nb {
        let l = (levels[b] as usize).saturating_sub(1);
        let (lo, hi) = bounds[b];
        let spacing: [f64; 3] = std::array::from_fn(|k| if dims[k] > 1 { (hi[k] - lo[k]) / (dims[k] as f64 - 1.0) } else { 1.0 });
        let mut img = ImageData {
            origin: lo,
            spacing,
            extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1],
            ..Default::default()
        };
        for (name, v) in &fields {
            img.cell_data.insert(Array::scalars_f64(name.clone(), v[b * per_block..(b + 1) * per_block].to_vec()));
        }
        amr.levels[l].spacing = spacing;
        amr.levels[l].blocks.push(AmrBlock::locate(origin, spacing, img));
    }
    amr.levels.retain(|l| !l.blocks.is_empty());
    amr.blank();
    let mut ds = amr.into_dataset();
    if let (DataSet::MultiBlock(mb), Some(t)) = (&mut ds, time) {
        mb.field_data.insert(Array::scalars_f64("TimeValue", vec![t]));
    }
    Ok(ds)
}
