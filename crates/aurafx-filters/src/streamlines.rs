//! Stream tracer: integrate a vector field from seed points (RK4 with adaptive-ish step).

use crate::probe::Locator;
use crate::simplex::parts;
use anyhow::{anyhow, Result};
use aurafx_data::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
    Both,
}

pub struct StreamOptions {
    pub vectors: String,
    pub direction: Direction,
    pub max_steps: usize,
    pub max_length: f64,
    /// Step as a fraction of the dataset diagonal.
    pub step_fraction: f64,
    pub terminal_speed: f64,
}

/// Merge multi-block inputs into one unstructured grid so the locator sees everything.
fn single(ds: &DataSet) -> Result<DataSet> {
    match ds {
        DataSet::MultiBlock(mb) => {
            let mut out = UnstructuredGrid::default();
            let mut first = true;
            for (_, leaf) in mb.leaves() {
                let Some(u) = leaf.to_unstructured() else { continue };
                if first {
                    out = u;
                    first = false;
                } else {
                    let off = out.points.len() as u32;
                    out.points.append(&u.points);
                    out.cells.append(&u.cells, off);
                    let names: Vec<String> = out.point_data.arrays.keys().cloned().collect();
                    for n in names {
                        match u.point_data.get(&n) {
                            Some(a) => {
                                let _ = out.point_data.get_mut(&n).unwrap().data.append(&a.data);
                            }
                            None => {
                                out.point_data.remove(&n);
                            }
                        }
                    }
                    out.cell_data = Attributes::new();
                }
            }
            Ok(DataSet::UnstructuredGrid(out))
        }
        d => Ok(d.clone()),
    }
}

pub fn stream_tracer(ds: &DataSet, seeds: &[[f64; 3]], opt: &StreamOptions) -> Result<PolyData> {
    let ds = single(ds)?;
    let (loc, ex) = Locator::new(&ds)?;
    let (_, pd, _, _) = parts(&ex).ok_or_else(|| anyhow!("stream tracer: unsupported dataset"))?;
    let vec = pd.get(&opt.vectors).filter(|a| a.ncomp >= 3).ok_or_else(|| anyhow!("no 3-component point array {:?}", opt.vectors))?;
    let h = ds.bounds().diagonal().max(1e-30) * opt.step_fraction.max(1e-6);
    let sample = |p: [f64; 3]| -> Option<([f64; 3], crate::probe::Hit)> {
        let hit = loc.find(p)?;
        let mut v = [0.0; 3];
        for k in 0..hit.n {
            for c in 0..3 {
                v[c] += hit.w[k] * vec.get(hit.ids[k] as usize, c);
            }
        }
        Some((v, hit))
    };
    let unit = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.0 {
            [v[0] / l, v[1] / l, v[2] / l]
        } else {
            [0.0; 3]
        }
    };
    let arrays: Vec<&Array> = pd.iter().filter(|a| a.scalar_type() != ScalarType::String).collect();
    let mut out_pts: Vec<f64> = Vec::new();
    let mut out_vals: Vec<Vec<f64>> = vec![Vec::new(); arrays.len()];
    let mut out_time: Vec<f64> = Vec::new();
    let mut out_seed: Vec<i64> = Vec::new();
    let mut lines = Cells::new();
    let mut push_point = |p: [f64; 3], hit: &crate::probe::Hit, t: f64, seed: usize, out_pts: &mut Vec<f64>| -> u32 {
        out_pts.extend_from_slice(&p);
        for (ai, a) in arrays.iter().enumerate() {
            for c in 0..a.ncomp {
                let mut s = 0.0;
                for k in 0..hit.n {
                    s += hit.w[k] * a.get(hit.ids[k] as usize, c);
                }
                out_vals[ai].push(s);
            }
        }
        out_time.push(t);
        out_seed.push(seed as i64);
        (out_pts.len() / 3 - 1) as u32
    };
    let dirs: Vec<f64> = match opt.direction {
        Direction::Forward => vec![1.0],
        Direction::Backward => vec![-1.0],
        Direction::Both => vec![-1.0, 1.0],
    };
    for (si, &seed) in seeds.iter().enumerate() {
        let mut halves: Vec<Vec<([f64; 3], crate::probe::Hit, f64)>> = Vec::new();
        for &sgn in &dirs {
            let mut path = Vec::new();
            let Some((_, hit0)) = sample(seed) else { break };
            path.push((seed, hit0, 0.0));
            let mut p = seed;
            let mut len = 0.0;
            let mut time = 0.0;
            for _ in 0..opt.max_steps {
                // RK4 on the normalized field (fixed spatial step), tracking time with the true speed.
                let Some((v1, _)) = sample(p) else { break };
                let speed = (v1[0] * v1[0] + v1[1] * v1[1] + v1[2] * v1[2]).sqrt();
                if speed <= opt.terminal_speed {
                    break;
                }
                let k1 = unit(v1).map(|x| x * sgn);
                let Some((v2, _)) = sample([p[0] + 0.5 * h * k1[0], p[1] + 0.5 * h * k1[1], p[2] + 0.5 * h * k1[2]]) else { break };
                let k2 = unit(v2).map(|x| x * sgn);
                let Some((v3, _)) = sample([p[0] + 0.5 * h * k2[0], p[1] + 0.5 * h * k2[1], p[2] + 0.5 * h * k2[2]]) else { break };
                let k3 = unit(v3).map(|x| x * sgn);
                let Some((v4, _)) = sample([p[0] + h * k3[0], p[1] + h * k3[1], p[2] + h * k3[2]]) else { break };
                let k4 = unit(v4).map(|x| x * sgn);
                let np = [
                    p[0] + h / 6.0 * (k1[0] + 2.0 * k2[0] + 2.0 * k3[0] + k4[0]),
                    p[1] + h / 6.0 * (k1[1] + 2.0 * k2[1] + 2.0 * k3[1] + k4[1]),
                    p[2] + h / 6.0 * (k1[2] + 2.0 * k2[2] + 2.0 * k3[2] + k4[2]),
                ];
                let Some((_, hit)) = sample(np) else { break };
                len += h;
                time += sgn * h / speed;
                path.push((np, hit, time));
                p = np;
                if len >= opt.max_length {
                    break;
                }
            }
            halves.push(path);
        }
        // Join backward (reversed) + forward into one polyline.
        let mut seq: Vec<([f64; 3], crate::probe::Hit, f64)> = Vec::new();
        if halves.len() == 2 {
            let mut back = halves.remove(0);
            back.reverse();
            let fwd = halves.remove(0);
            seq.extend(back);
            seq.extend(fwd.into_iter().skip(1));
        } else if let Some(h) = halves.pop() {
            seq = h;
        }
        if seq.len() < 2 {
            continue;
        }
        let ids: Vec<u32> = seq.iter().map(|(p, hit, t)| push_point(*p, hit, *t, si, &mut out_pts)).collect();
        lines.push(CellType::PolyLine, &ids);
    }
    let mut point_data = Attributes::new();
    for (ai, a) in arrays.iter().enumerate() {
        point_data.insert(Array::new(a.name.clone(), a.ncomp, ArrayData::F64(std::mem::take(&mut out_vals[ai]))));
    }
    point_data.insert(Array::scalars_f64("IntegrationTime", out_time));
    point_data.insert(Array::new("SeedIds", 1, ArrayData::I64(out_seed)));
    if let Some(s) = pd.active(AttributeKind::Scalars) {
        point_data.set_active(AttributeKind::Scalars, &s.name.clone());
    }
    point_data.set_active(AttributeKind::Vectors, &opt.vectors);
    Ok(PolyData { points: Points::new_f64(out_pts), lines, point_data, ..Default::default() })
}

/// Seeds along a line.
pub fn line_seeds(p1: [f64; 3], p2: [f64; 3], n: usize) -> Vec<[f64; 3]> {
    let n = n.max(1);
    (0..n)
        .map(|i| {
            let t = if n == 1 { 0.5 } else { i as f64 / (n - 1) as f64 };
            [p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1]), p1[2] + t * (p2[2] - p1[2])]
        })
        .collect()
}

/// Seeds randomly inside a sphere (deterministic).
pub fn sphere_seeds(c: [f64; 3], r: f64, n: usize) -> Vec<[f64; 3]> {
    let pd = crate::sources::point_source(c, r, n, 7);
    (0..pd.points.len()).map(|i| pd.points.get(i)).collect()
}
