//! Particle and field files on HDF5: H5Part (`Step#n` groups of one-dimensional datasets: x, y,
//! z and the particles' properties) and openPMD (iterations of meshes and particle species,
//! in one file or one file per iteration), as time series.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use hdf5_reader::group::Group;
use hdf5_reader::Hdf5File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn attr_f64s(g: &Group, name: &str) -> Option<Vec<f64>> {
    g.attribute(name).ok().map(|a| crate::chombo::attr_nums(&a)).filter(|v| !v.is_empty())
}

fn attr_string(g: &Group, name: &str) -> Option<String> {
    g.attribute(name).ok().and_then(|a| a.read_string().ok()).map(|s| s.trim_end_matches('\0').to_string())
}

fn attr_strings(g: &Group, name: &str) -> Vec<String> {
    let Ok(a) = g.attribute(name) else { return Vec::new() };
    if let Ok(v) = a.read_strings() {
        return v.into_iter().map(|s| s.trim_end_matches('\0').to_string()).collect();
    }
    a.read_string().map(|s| s.split(['\0', ';']).filter(|x| !x.is_empty()).map(|x| x.to_string()).collect()).unwrap_or_default()
}

fn values(g: &Group, name: &str) -> Result<(Vec<f64>, Vec<usize>)> {
    let d = g.dataset(name)?;
    let (a, shape) = crate::vtkhdf::read(&d)?;
    Ok((a.to_f64_vec(), shape))
}

fn is_hdf5(path: &Path) -> bool {
    let mut head = [0u8; 8];
    std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && head == *b"\x89HDF\r\n\x1a\n"
}

// ------------------------------------------------------------------ H5Part

/// An HDF5 file with `Step#0`-style groups.
pub fn is_h5part(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("h5part")) {
        return true;
    }
    if !is_hdf5(path) {
        return false;
    }
    let Ok(f) = Hdf5File::open(path) else { return false };
    f.root_group().and_then(|r| r.member_names()).map(|(g, _)| g.iter().any(|n| n.starts_with("Step#"))).unwrap_or(false)
}

/// H5Part: each `Step#n` group is a time step; x, y, z (any case) are the positions and every
/// other one-dimensional dataset of the same length a point array. Times from a `TimeValue` (or
/// `time`) attribute of each step, else the step numbers.
pub fn read_h5part(path: &Path) -> Result<crate::Loaded> {
    let f = Hdf5File::open(path).with_context(|| format!("opening {} as HDF5", path.display()))?;
    let root = f.root_group()?;
    let (groups, _) = root.member_names()?;
    let mut steps: Vec<(i64, String)> =
        groups.iter().filter_map(|n| n.strip_prefix("Step#").and_then(|k| k.parse::<i64>().ok()).map(|k| (k, n.clone()))).collect();
    steps.sort();
    if steps.is_empty() {
        bail!("{}: no Step# groups", path.display());
    }
    let mut times = Vec::new();
    let mut out = Vec::new();
    for (k, name) in &steps {
        let g = root.group(name)?;
        let t = attr_f64s(&g, "TimeValue")
            .or_else(|| attr_f64s(&g, "time"))
            .or_else(|| attr_f64s(&g, "Time"))
            .and_then(|v| v.first().copied())
            .unwrap_or(*k as f64);
        let (_, dsets) = g.member_names()?;
        let pick = |want: &str| dsets.iter().find(|d| d.eq_ignore_ascii_case(want)).cloned();
        let x = pick("x").ok_or_else(|| anyhow!("{name}: no x dataset"))?;
        let (xs, _) = values(&g, &x)?;
        let n = xs.len();
        let ys = match pick("y") {
            Some(y) => values(&g, &y)?.0,
            None => vec![0.0; n],
        };
        let zs = match pick("z") {
            Some(z) => values(&g, &z)?.0,
            None => vec![0.0; n],
        };
        let pts: Vec<f64> = (0..n).flat_map(|i| [xs[i], ys.get(i).copied().unwrap_or(0.0), zs.get(i).copied().unwrap_or(0.0)]).collect();
        let mut pd = PolyData::from_points(Points::new_f64(pts));
        let mut names: Vec<&String> = dsets.iter().filter(|d| !["x", "y", "z"].iter().any(|c| d.eq_ignore_ascii_case(c))).collect();
        names.sort();
        for d in names {
            let ds = g.dataset(d)?;
            let (a, shape) = crate::vtkhdf::read(&ds)?;
            if shape.first() == Some(&n) {
                let nc = shape.get(1).copied().unwrap_or(1).max(1);
                pd.point_data.insert(Array::new(d.clone(), nc, a));
            }
        }
        times.push(t);
        out.push(TimeStepRef::Loaded(Arc::new(DataSet::PolyData(pd))));
    }
    if out.len() == 1 {
        let TimeStepRef::Loaded(d) = out.pop().unwrap() else { unreachable!() };
        return Ok(crate::Loaded::Static(d));
    }
    Ok(crate::Loaded::Series(TimeSeries { times, steps: out }))
}

// ------------------------------------------------------------------ openPMD

/// An HDF5 file with the openPMD root attributes.
pub fn is_openpmd(path: &Path) -> bool {
    if !is_hdf5(path) {
        return false;
    }
    let Ok(f) = Hdf5File::open(path) else { return false };
    f.root_group().map(|r| r.attribute("openPMD").is_ok()).unwrap_or(false)
}

/// The iterations of an openPMD series: (iteration number, file, group path of the iteration).
fn openpmd_iterations(path: &Path) -> Result<Vec<(u64, PathBuf, String)>> {
    let f = Hdf5File::open(path)?;
    let root = f.root_group()?;
    let base = attr_string(&root, "basePath").unwrap_or_else(|| "/data/%T/".into());
    let encoding = attr_string(&root, "iterationEncoding").unwrap_or_else(|| "groupBased".into());
    let data_group = base.split("%T").next().unwrap_or("/data/").trim_matches('/').to_string();
    let mut out = Vec::new();
    let in_file = |file: &Path| -> Result<Vec<(u64, PathBuf, String)>> {
        let f = Hdf5File::open(file)?;
        let g = f.group(&data_group)?;
        let (names, _) = g.member_names()?;
        Ok(names.iter().filter_map(|n| n.parse::<u64>().ok().map(|k| (k, file.to_path_buf(), format!("{data_group}/{n}")))).collect())
    };
    if encoding == "fileBased" {
        // Sibling files that differ only in the iteration number.
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let digits_at = name.find(|c: char| c.is_ascii_digit()).unwrap_or(name.len());
        let digits_end = name[digits_at..].find(|c: char| !c.is_ascii_digit()).map(|e| digits_at + e).unwrap_or(name.len());
        let (pre, post) = (&name[..digits_at], &name[digits_end..]);
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.starts_with(pre)
                        && n.ends_with(post)
                        && n.len() > pre.len() + post.len()
                        && n[pre.len()..n.len() - post.len()].chars().all(|c| c.is_ascii_digit())
                })
            })
            .collect();
        files.sort();
        for p in files {
            out.extend(in_file(&p)?);
        }
    } else {
        out = in_file(path)?;
    }
    out.sort_by_key(|x| x.0);
    if out.is_empty() {
        bail!("{}: no openPMD iterations", path.display());
    }
    Ok(out)
}

/// A record (mesh or particle property): its components' values, or a constant (`value` with
/// `shape`), times `unitSI`.
fn record(g: &Group, name: &str, n_expect: Option<usize>) -> Result<Option<(Vec<Vec<f64>>, Vec<String>, Vec<usize>)>> {
    // A scalar record stored as a dataset.
    if let Ok(d) = g.dataset(name) {
        let (a, shape) = crate::vtkhdf::read(&d)?;
        let unit = d.attribute("unitSI").ok().map(|x| crate::chombo::attr_nums(&x)).and_then(|v| v.first().copied()).unwrap_or(1.0);
        return Ok(Some((vec![a.to_f64_vec().into_iter().map(|x| x * unit).collect()], vec![name.to_string()], shape)));
    }
    let Ok(rg) = g.group(name) else { return Ok(None) };
    // A constant scalar record.
    if let Some(v) = attr_f64s(&rg, "value") {
        let shape: Vec<usize> = attr_f64s(&rg, "shape").map(|s| s.iter().map(|&x| x as usize).collect()).unwrap_or_else(|| vec![n_expect.unwrap_or(1)]);
        let unit = attr_f64s(&rg, "unitSI").and_then(|u| u.first().copied()).unwrap_or(1.0);
        let n: usize = shape.iter().product();
        return Ok(Some((vec![vec![v[0] * unit; n]], vec![name.to_string()], shape)));
    }
    let (sub_groups, sub_dsets) = rg.member_names()?;
    let mut comps: Vec<String> = sub_dsets.into_iter().chain(sub_groups).collect();
    let order = |c: &str| ["x", "y", "z", "r", "t"].iter().position(|o| *o == c).unwrap_or(9);
    comps.sort_by(|a, b| order(a).cmp(&order(b)).then(a.cmp(b)));
    let mut vals = Vec::new();
    let mut shape = Vec::new();
    for c in &comps {
        if let Ok(d) = rg.dataset(c) {
            let (a, s) = crate::vtkhdf::read(&d)?;
            let unit = d.attribute("unitSI").ok().map(|x| crate::chombo::attr_nums(&x)).and_then(|v| v.first().copied()).unwrap_or(1.0);
            shape = s;
            vals.push(a.to_f64_vec().into_iter().map(|x| x * unit).collect());
        } else if let Ok(cg) = rg.group(c) {
            let v = attr_f64s(&cg, "value").and_then(|v| v.first().copied()).unwrap_or(0.0);
            let s: Vec<usize> = attr_f64s(&cg, "shape").map(|s| s.iter().map(|&x| x as usize).collect()).unwrap_or_else(|| vec![n_expect.unwrap_or(1)]);
            let unit = attr_f64s(&cg, "unitSI").and_then(|u| u.first().copied()).unwrap_or(1.0);
            shape = s.clone();
            vals.push(vec![v * unit; s.iter().product()]);
        }
    }
    if vals.is_empty() {
        return Ok(None);
    }
    Ok(Some((vals, comps, shape)))
}

/// One iteration: a block of meshes (image data on the first mesh's grid) and a block per
/// particle species (positions = position + positionOffset, in metres).
fn openpmd_iteration(file: &Path, group: &str) -> Result<DataSet> {
    let f = Hdf5File::open(file)?;
    let root = f.root_group()?;
    let it = f.group(group)?;
    let meshes_path = attr_string(&root, "meshesPath").unwrap_or_else(|| "meshes/".into());
    let particles_path = attr_string(&root, "particlesPath").unwrap_or_else(|| "particles/".into());
    let mut mb = MultiBlock::default();
    if let Ok(mg) = it.group(meshes_path.trim_matches('/')) {
        let (gnames, dnames) = mg.member_names()?;
        let mut names: Vec<String> = gnames.into_iter().chain(dnames).collect();
        names.sort();
        let mut img: Option<ImageData> = None;
        for name in names {
            let Some((vals, _, shape)) = record(&mg, &name, None)? else { continue };
            // Grid description: on the record group (or dataset).
            let holder_attrs = |key: &str| -> Option<Vec<f64>> {
                mg.group(&name)
                    .ok()
                    .and_then(|g| attr_f64s(&g, key))
                    .or_else(|| mg.dataset(&name).ok().and_then(|d| d.attribute(key).ok()).map(|a| crate::chombo::attr_nums(&a)))
            };
            let labels: Vec<String> = mg.group(&name).map(|g| attr_strings(&g, "axisLabels")).unwrap_or_default();
            let order = mg.group(&name).ok().and_then(|g| attr_string(&g, "dataOrder")).unwrap_or_else(|| "C".into());
            let nd = shape.len().clamp(1, 3);
            // HDF5 stores the values in C order (the last axis fastest); dataOrder says only how
            // the grid attributes list the axes ("F": fastest first), so line them up with the shape.
            let fast_first: Vec<usize> = (0..nd).rev().collect();
            let mut spacing_l = holder_attrs("gridSpacing").unwrap_or_else(|| vec![1.0; nd]);
            let mut offset_l = holder_attrs("gridGlobalOffset").unwrap_or_else(|| vec![0.0; nd]);
            let mut labels = labels;
            if order == "F" {
                spacing_l.reverse();
                offset_l.reverse();
                labels.reverse();
            }
            let unit = holder_attrs("gridUnitSI").and_then(|v| v.first().copied()).unwrap_or(1.0);
            let mut dims = [1usize; 3];
            let mut spacing = [1.0; 3];
            let mut origin = [0.0; 3];
            let mut direction = [0.0; 9];
            let mut used = [false; 3];
            for (k, &a) in fast_first.iter().enumerate() {
                dims[k] = shape[a];
                spacing[k] = spacing_l.get(a).copied().unwrap_or(1.0) * unit;
                origin[k] = offset_l.get(a).copied().unwrap_or(0.0) * unit;
                let axis = match labels.get(a).map(|s| s.as_str()) {
                    Some("x") => 0,
                    Some("y") => 1,
                    Some("z") => 2,
                    _ => k,
                };
                let axis = if used[axis] { (0..3).find(|i| !used[*i]).unwrap_or(k) } else { axis };
                used[axis] = true;
                direction[axis * 3 + k] = 1.0;
            }
            for k in nd..3 {
                let axis = (0..3).find(|i| !used[*i]).unwrap_or(k);
                used[axis] = true;
                direction[axis * 3 + k] = 1.0;
            }
            // Origin in space: each image axis' offset along its direction.
            let mut o = [0.0; 3];
            for k in 0..3 {
                for r in 0..3 {
                    o[r] += direction[r * 3 + k] * origin[k];
                }
            }
            let im = img.get_or_insert_with(|| ImageData {
                extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1],
                origin: o,
                spacing,
                direction,
                ..Default::default()
            });
            if im.dims() != dims {
                continue;
            }
            let n: usize = dims.iter().product();
            let nc = vals.len();
            let mut data = vec![0.0; n * nc];
            for (c, v) in vals.iter().enumerate() {
                if v.len() != n {
                    continue;
                }
                // C order: the last axis of the shape (the image's x) varies fastest.
                for (i, x) in v.iter().enumerate() {
                    data[i * nc + c] = *x;
                }
            }
            im.point_data.insert(Array::new(name.clone(), nc, ArrayData::F64(data)));
        }
        if let Some(im) = img {
            mb.blocks.push(("meshes".into(), Some(DataSet::ImageData(im))));
        }
    }
    if let Ok(pg) = it.group(particles_path.trim_matches('/')) {
        let (species, _) = pg.member_names()?;
        for sp in species {
            let g = pg.group(&sp)?;
            let Some((pos, _, shape)) = record(&g, "position", None)? else { continue };
            let n = shape.first().copied().unwrap_or(0);
            let off = record(&g, "positionOffset", Some(n))?.map(|r| r.0).unwrap_or_default();
            let at = |c: usize, i: usize| pos.get(c).and_then(|v| v.get(i)).copied().unwrap_or(0.0) + off.get(c).and_then(|v| v.get(i)).copied().unwrap_or(0.0);
            let pts: Vec<f64> = (0..n).flat_map(|i| [at(0, i), at(1, i), at(2, i)]).collect();
            let mut pd = PolyData::from_points(Points::new_f64(pts));
            let (gn, dn) = g.member_names()?;
            let mut recs: Vec<String> = gn.into_iter().chain(dn).filter(|r| r != "position" && r != "positionOffset").collect();
            recs.sort();
            for r in recs {
                if let Some((vals, _, _)) = record(&g, &r, Some(n))? {
                    if vals.iter().all(|v| v.len() == n) {
                        let nc = vals.len();
                        let data: Vec<f64> = (0..n).flat_map(|i| vals.iter().map(move |v| v[i])).collect();
                        pd.point_data.insert(Array::new(r, nc, ArrayData::F64(data)));
                    }
                }
            }
            mb.blocks.push((sp, Some(DataSet::PolyData(pd))));
        }
    }
    if mb.blocks.is_empty() {
        bail!("{}: iteration {group} has no meshes or particles", file.display());
    }
    Ok(DataSet::MultiBlock(mb))
}

/// openPMD on HDF5: every iteration (one group each, or one file each) as a time step, with the
/// iteration's time times its time unit as the time.
pub fn read_openpmd(path: &Path) -> Result<crate::Loaded> {
    let its = openpmd_iterations(path)?;
    let mut times = Vec::new();
    let mut steps = Vec::new();
    for (k, file, group) in &its {
        let f = Hdf5File::open(file)?;
        let g = f.group(group)?;
        let t = attr_f64s(&g, "time").and_then(|v| v.first().copied()).unwrap_or(*k as f64);
        let unit = attr_f64s(&g, "timeUnitSI").and_then(|v| v.first().copied()).unwrap_or(1.0);
        times.push(t * unit);
        steps.push(TimeStepRef::Loaded(Arc::new(openpmd_iteration(file, group)?)));
    }
    if steps.len() == 1 {
        let TimeStepRef::Loaded(d) = steps.pop().unwrap() else { unreachable!() };
        return Ok(crate::Loaded::Static(d));
    }
    Ok(crate::Loaded::Series(TimeSeries { times, steps }))
}
