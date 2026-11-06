//! Chombo AMR files (HDF5, `.hdf5`/`.h5`, cell-centered): levels `level_N` with boxes, every
//! box's components stored one after the other in `data:datatype=0` (with `outputGhost` ghost
//! cells around each box), dx (or `vec_dx`), `prob_lo` and the component names at the root.
//! Read as overlapping AMR data like VisIt's Chombo reader (ParaView's VisItChomboReader),
//! with the ghost cells removed on every level and the covered cells hidden. Node-centered
//! files (data_centering 7) give point data; the particles in `/particles` are read with the
//! reader option data = Particles.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::amr::{Amr, AmrBlock, AmrLevel};
use aurafx_data::*;
use hdf5_reader::attribute_api::Attribute;
use hdf5_reader::group::Group;
use hdf5_reader::{Datatype, Hdf5File};
use std::path::Path;

/// The numbers in an attribute: a scalar, an array, or the fields of compound values in order.
fn attr_nums(a: &Attribute) -> Vec<f64> {
    fn decode(dt: &Datatype, b: &[u8]) -> Vec<f64> {
        match dt {
            Datatype::FixedPoint { size, signed, .. } => b
                .chunks_exact(*size as usize)
                .map(|c| {
                    let mut x = [0u8; 8];
                    x[..c.len()].copy_from_slice(c);
                    let mut v = i64::from_le_bytes(x);
                    if *signed && c.len() < 8 && c[c.len() - 1] & 0x80 != 0 {
                        v -= 1i64 << (8 * c.len());
                    }
                    v as f64
                })
                .collect(),
            Datatype::FloatingPoint { size: 8, .. } => b.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect(),
            Datatype::FloatingPoint { size: 4, .. } => b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64).collect(),
            Datatype::Compound { size, fields } => b
                .chunks_exact((*size as usize).max(1))
                .flat_map(|rec| {
                    fields
                        .iter()
                        .flat_map(|f| {
                            let n = match &f.datatype {
                                Datatype::FixedPoint { size, .. } | Datatype::FloatingPoint { size, .. } => *size as usize,
                                _ => 0,
                            };
                            let o = f.byte_offset as usize;
                            rec.get(o..o + n).map(|s| decode(&f.datatype, s)).unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    decode(&a.datatype, &a.raw_data)
}

fn nums(g: &Group, name: &str) -> Option<Vec<f64>> {
    g.attribute(name).ok().map(|a| attr_nums(&a)).filter(|v| !v.is_empty())
}

fn is_hdf5(path: &Path) -> bool {
    let mut head = [0u8; 8];
    std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && head == *b"\x89HDF\r\n\x1a\n"
}

/// An HDF5 file with Chombo's global group and a first level.
pub fn is_chombo(path: &Path) -> bool {
    if !path.is_file() || !is_hdf5(path) {
        return false;
    }
    let Ok(f) = Hdf5File::open(path) else { return false };
    f.group("Chombo_global").is_ok() && f.group("level_0").is_ok()
}

/// Whether the file has particles (positions in /particles).
fn has_particles(f: &Hdf5File, dim: usize) -> bool {
    let Ok(g) = f.group("particles") else { return false };
    ["position_x", "position_y", "position_z"].iter().take(dim.max(2)).all(|n| g.dataset(n).is_ok())
}

pub fn options(path: &Path) -> Vec<crate::ReaderOption> {
    let Ok(f) = Hdf5File::open(path) else { return Vec::new() };
    let dim = f.group("Chombo_global").ok().and_then(|g| nums(&g, "SpaceDim")).and_then(|v| v.first().copied()).unwrap_or(3.0) as usize;
    if !has_particles(&f, dim) {
        return Vec::new();
    }
    vec![crate::ReaderOption::new(
        "data",
        "Data",
        crate::ReaderOptionKind::Choice(vec!["Grid".into(), "Particles".into()]),
        "Grid",
        "The AMR grid, or the particles as points with their properties.",
    )]
}

/// The particles: a point each (position_x/y/z), every other dataset of /particles as a
/// double array, in name order (as VisIt lists them).
fn particles(f: &Hdf5File, dim: usize) -> Result<DataSet> {
    let g = f.group("particles")?;
    let col = |n: &str| -> Result<Vec<f64>> { Ok(crate::vtkhdf::read(&g.dataset(n)?)?.0.to_f64_vec()) };
    let x = col("position_x")?;
    let y = col("position_y")?;
    let z = if dim == 3 { col("position_z")? } else { vec![0.0; x.len()] };
    let pts: Vec<f64> = (0..x.len()).flat_map(|i| [x[i], y.get(i).copied().unwrap_or(0.0), z.get(i).copied().unwrap_or(0.0)]).collect();
    let mut pd = PolyData::from_points(Points::new_f64(pts));
    let mut names: Vec<String> = g.datasets()?.iter().map(|d| d.name().rsplit('/').next().unwrap_or("").to_string()).filter(|n| !n.starts_with("position_")).collect();
    names.sort();
    for n in names {
        let v = col(&n)?;
        if v.len() == x.len() {
            pd.point_data.insert(Array::scalars_f64(n, v));
        }
    }
    Ok(DataSet::PolyData(pd))
}

pub fn read(path: &Path) -> Result<DataSet> {
    let f = Hdf5File::open(path).with_context(|| format!("opening {} as HDF5", path.display()))?;
    let root = f.root_group()?;
    let global = f.group("Chombo_global").context("no Chombo_global group")?;
    let dim = nums(&global, "SpaceDim").and_then(|v| v.first().copied()).ok_or_else(|| anyhow!("no SpaceDim"))? as usize;
    if !(1..=3).contains(&dim) {
        bail!("{}: {dim}-dimensional Chombo data is not supported (1 to 3 dimensions are)", path.display());
    }
    if crate::option("data").as_deref() == Some("Particles") {
        return particles(&f, dim);
    }
    // Node-centered data: a value per node (one more per axis than cells).
    let node = nums(&root, "data_centering").and_then(|v| v.first().copied()) == Some(7.0);
    let extra = if node { 1 } else { 0 };
    let n_levels = nums(&root, "num_levels").and_then(|v| v.first().copied()).unwrap_or(1.0) as usize;
    let ncomp = nums(&root, "num_components").and_then(|v| v.first().copied()).unwrap_or(0.0) as usize;
    let names: Vec<String> = (0..ncomp)
        .map(|i| root.attribute(&format!("component_{i}")).ok().and_then(|a| a.read_string().ok()).map(|s| s.trim_end_matches('\0').to_string()).unwrap_or_else(|| format!("component_{i}")))
        .collect();
    let time = nums(&root, "time").and_then(|v| v.first().copied());
    let mut prob_lo = [0.0; 3];
    if let Some(v) = nums(&root, "prob_lo") {
        prob_lo[..dim.min(v.len())].copy_from_slice(&v[..dim.min(v.len())]);
    }
    let mut aspect = [1.0; 3];
    if let Some(v) = nums(&root, "aspect_ratio") {
        aspect[..dim.min(v.len())].copy_from_slice(&v[..dim.min(v.len())]);
    }
    let mut amr = Amr { origin: prob_lo, levels: Vec::new() };
    for l in 0..n_levels {
        let g = f.group(&format!("level_{l}")).with_context(|| format!("no level_{l} group"))?;
        let dx = nums(&g, "vec_dx").or_else(|| nums(&g, "dx")).ok_or_else(|| anyhow!("level {l}: no dx"))?;
        let spacing: [f64; 3] = std::array::from_fn(|k| if k < dim { dx.get(k).or(dx.first()).copied().unwrap_or(1.0) * aspect[k] } else { 1.0 });
        let boxes_ds = g.dataset("boxes").with_context(|| format!("level {l}: no boxes"))?;
        let raw = boxes_ds.read_raw_bytes()?;
        let Datatype::Compound { size, fields } = boxes_ds.dtype().clone() else { bail!("level {l}: boxes are not compound") };
        let field = |rec: &[u8], name: &str| -> i64 {
            fields
                .iter()
                .find(|x| x.name == name)
                .and_then(|x| rec.get(x.byte_offset as usize..x.byte_offset as usize + 4))
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()) as i64)
                .unwrap_or(0)
        };
        let axes = ["i", "j", "k"];
        let boxes: Vec<([i64; 3], [i64; 3])> = raw
            .chunks_exact(size as usize)
            .map(|rec| {
                let lo = std::array::from_fn(|k| if k < dim { field(rec, &format!("lo_{}", axes[k])) } else { 0 });
                let hi = std::array::from_fn(|k| if k < dim { field(rec, &format!("hi_{}", axes[k])) } else { 0 });
                (lo, hi)
            })
            .collect();
        let ghost: [i64; 3] = {
            let v = g.group("data_attributes").ok().and_then(|d| nums(&d, "outputGhost")).unwrap_or_default();
            std::array::from_fn(|k| if k < dim { v.get(k).copied().unwrap_or(0.0) as i64 } else { 0 })
        };
        let data = crate::vtkhdf::read(&g.dataset("data:datatype=0").with_context(|| format!("level {l}: no data"))?)?.0.to_f64_vec();
        let mut lev = AmrLevel { spacing, blocks: Vec::with_capacity(boxes.len()) };
        let mut at = 0usize;
        for (b, (lo, hi)) in boxes.iter().enumerate() {
            let real: [usize; 3] = std::array::from_fn(|k| (hi[k] - lo[k] + 1).max(1) as usize);
            let full: [usize; 3] = std::array::from_fn(|k| if k < dim { real[k] + extra + 2 * ghost[k] as usize } else { 1 });
            let vals: [usize; 3] = std::array::from_fn(|k| if k < dim { real[k] + extra } else { 1 });
            let amt = full[0] * full[1] * full[2];
            if at + ncomp * amt > data.len() {
                bail!("{}: level {l} box {b} runs past the data", path.display());
            }
            let origin: [f64; 3] = std::array::from_fn(|k| if k < dim { prob_lo[k] + lo[k] as f64 * spacing[k] } else { 0.0 });
            let dims: [usize; 3] = std::array::from_fn(|k| if k < dim { real[k] + 1 } else { 1 });
            let mut img = ImageData { origin, spacing, extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1], ..Default::default() };
            for (c, name) in names.iter().enumerate() {
                let src = &data[at + c * amt..at + (c + 1) * amt];
                let mut v = Vec::with_capacity(vals[0] * vals[1] * vals[2]);
                for z in 0..vals[2] {
                    for y in 0..vals[1] {
                        let row = (z + ghost[2] as usize) * full[1] * full[0] + (y + ghost[1] as usize) * full[0] + ghost[0] as usize;
                        v.extend_from_slice(&src[row..row + vals[0]]);
                    }
                }
                let a = Array::scalars_f64(name.clone(), v);
                if node {
                    img.point_data.insert(a);
                } else {
                    img.cell_data.insert(a);
                }
            }
            at += ncomp * amt;
            lev.blocks.push(AmrBlock { lo: *lo, hi: *hi, data: img });
        }
        amr.levels.push(lev);
    }
    amr.blank();
    let mut ds = amr.into_dataset();
    if let (DataSet::MultiBlock(mb), Some(t)) = (&mut ds, time) {
        mb.field_data.insert(Array::scalars_f64("TimeValue", vec![t]));
    }
    Ok(ds)
}
