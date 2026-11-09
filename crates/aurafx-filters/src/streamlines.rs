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
    // Every seed is traced independently (in parallel); lines are joined in seed order.
    let trace = |seed: [f64; 3]| -> Vec<([f64; 3], crate::probe::Hit, f64)> {
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
        seq
    };
    use rayon::prelude::*;
    let traced: Vec<Vec<([f64; 3], crate::probe::Hit, f64)>> = seeds.par_iter().map(|&s| trace(s)).collect();
    for (si, seq) in traced.into_iter().enumerate() {
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

/// Evenly spaced streamlines in the XY plane: lines of a 2-D vector field, each started at
/// `separation` from the lines already there and stopped where it comes within
/// `test_ratio * separation` of another (or of itself, closing a loop), until no gap is left.
/// The field's z component is ignored; the lines lie at the start point's z.
pub fn evenly_spaced_2d(ds: &DataSet, vectors: &str, start: [f64; 3], separation: f64, test_ratio: f64, step_ratio: f64, max_steps: usize) -> Result<PolyData> {
    let ds = single(ds)?;
    let b = ds.bounds();
    if b.is_empty() {
        return Err(anyhow!("evenly spaced streamlines: empty data"));
    }
    let (loc, ex) = Locator::new(&ds)?;
    let (_, pd, _, _) = parts(&ex).ok_or_else(|| anyhow!("evenly spaced streamlines: unsupported dataset"))?;
    let vec = pd.get(vectors).filter(|a| a.ncomp >= 2).ok_or_else(|| anyhow!("no vector point array {vectors:?}"))?;
    let size = [b.max[0] - b.min[0], b.max[1] - b.min[1]];
    let dsep = if separation > 0.0 { separation } else { size[0].max(size[1]) / 30.0 };
    let dtest = dsep * test_ratio.clamp(0.01, 1.0);
    let h = dsep * step_ratio.clamp(0.01, 1.0);
    let z = if start[2].is_finite() { start[2] } else { 0.5 * (b.min[2] + b.max[2]) };
    let dir = |p: [f64; 2]| -> Option<[f64; 2]> {
        let hit = loc.find([p[0], p[1], z])?;
        let mut v = [0.0; 2];
        for k in 0..hit.n {
            for (c, vc) in v.iter_mut().enumerate() {
                *vc += hit.w[k] * vec.get(hit.ids[k] as usize, c);
            }
        }
        let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
        (l > 1e-300).then(|| [v[0] / l, v[1] / l])
    };
    // Accepted points in a grid of cells as large as the separation: (point, line).
    let cell = |p: [f64; 2]| (((p[0] - b.min[0]) / dsep).floor() as i64, ((p[1] - b.min[1]) / dsep).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<([f64; 2], usize)>> = Default::default();
    let near = |grid: &std::collections::HashMap<(i64, i64), Vec<([f64; 2], usize)>>, p: [f64; 2], r: f64| -> bool {
        let (ci, cj) = cell(p);
        let reach = (r / dsep).ceil() as i64;
        for i in ci - reach..=ci + reach {
            for j in cj - reach..=cj + reach {
                if let Some(v) = grid.get(&(i, j)) {
                    if v.iter().any(|(q, _)| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) < r * r) {
                        return true;
                    }
                }
            }
        }
        false
    };
    let inside = |p: [f64; 2]| p[0] >= b.min[0] - 1e-12 && p[0] <= b.max[0] + 1e-12 && p[1] >= b.min[1] - 1e-12 && p[1] <= b.max[1] + 1e-12;
    let trace = |grid: &std::collections::HashMap<(i64, i64), Vec<([f64; 2], usize)>>, seed: [f64; 2]| -> Vec<[f64; 2]> {
        let mut halves = Vec::new();
        for sgn in [-1.0, 1.0] {
            let mut path = vec![seed];
            let mut p = seed;
            for step in 0..max_steps {
                // Midpoint rule on the unit direction.
                let Some(k1) = dir(p) else { break };
                let mid = [p[0] + 0.5 * h * sgn * k1[0], p[1] + 0.5 * h * sgn * k1[1]];
                let Some(k2) = dir(mid) else { break };
                let np = [p[0] + h * sgn * k2[0], p[1] + h * sgn * k2[1]];
                if !inside(np) || dir(np).is_none() || near(grid, np, dtest) {
                    break;
                }
                // Closing on itself: near a point of this line laid down well before.
                let lag = (2.0 * dtest / h).ceil() as usize + 2;
                if step > lag && path[..path.len() - lag].iter().any(|q| (q[0] - np[0]).powi(2) + (q[1] - np[1]).powi(2) < dtest * dtest) {
                    path.push(np);
                    break;
                }
                path.push(np);
                p = np;
            }
            halves.push(path);
        }
        let mut back = halves.remove(0);
        back.reverse();
        back.extend(halves.remove(0).into_iter().skip(1));
        back
    };
    let mut lines: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let first = if start[0].is_finite() && start[1].is_finite() { [start[0], start[1]] } else { [0.5 * (b.min[0] + b.max[0]), 0.5 * (b.min[1] + b.max[1])] };
    let mut seeds = vec![first];
    loop {
        for s in seeds.drain(..) {
            if !inside(s) || dir(s).is_none() || near(&grid, s, dsep * 0.999) {
                continue;
            }
            let l = trace(&grid, s);
            if l.len() < 2 {
                continue;
            }
            let id = lines.len();
            for p in &l {
                grid.entry(cell(*p)).or_default().push((*p, id));
            }
            queue.push_back(id);
            lines.push(l);
        }
        // New seeds at the separation on both sides of the next line in the queue.
        let Some(id) = queue.pop_front() else { break };
        let l = &lines[id];
        for k in 0..l.len() {
            let a = l[k.saturating_sub(1)];
            let c = l[(k + 1).min(l.len() - 1)];
            let (tx, ty) = (c[0] - a[0], c[1] - a[1]);
            let n = (tx * tx + ty * ty).sqrt();
            if n <= 0.0 {
                continue;
            }
            for sgn in [-1.0, 1.0] {
                seeds.push([l[k][0] - sgn * ty / n * dsep, l[k][1] + sgn * tx / n * dsep]);
            }
        }
        if lines.len() > 100_000 {
            break;
        }
    }
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut cells = Cells::new();
    let mut line_ids = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        let off = pts.len() as u32;
        pts.extend(l.iter().map(|p| [p[0], p[1], z]));
        cells.push(CellType::PolyLine, &(off..off + l.len() as u32).collect::<Vec<_>>());
        line_ids.push(li as f64);
    }
    let mut point_data = crate::probe::probe_points(&ds, &pts)?;
    point_data.remove("vtkValidPointMask");
    point_data.set_active(AttributeKind::Vectors, vectors);
    let mut cell_data = Attributes::new();
    cell_data.insert(Array::scalars_f64("LineId", line_ids));
    Ok(PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), lines: cells, point_data, cell_data, ..Default::default() })
}

#[cfg(test)]
mod evenly_tests {
    use super::*;

    fn field(f: impl Fn(f64, f64) -> [f64; 2], n: usize) -> DataSet {
        let sp = 2.0 / (n - 1) as f64;
        let mut v = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (-1.0 + i as f64 * sp, -1.0 + j as f64 * sp);
                let w = f(x, y);
                v.extend_from_slice(&[w[0], w[1], 0.0]);
            }
        }
        let mut im = ImageData { extent: [0, n as i64 - 1, 0, n as i64 - 1, 0, 0], origin: [-1.0, -1.0, 0.0], spacing: [sp, sp, 1.0], ..Default::default() };
        im.point_data.insert(Array::new("V", 3, ArrayData::F64(v)));
        DataSet::ImageData(im)
    }

    fn min_gap_between_lines(p: &PolyData) -> f64 {
        let ids: Vec<usize> = (0..p.lines.len()).flat_map(|c| p.lines.cell(c).iter().map(move |_| c)).collect();
        let mut best = f64::INFINITY;
        for i in 0..p.points.len() {
            for j in 0..p.points.len() {
                if ids[i] != ids[j] {
                    let (a, b) = (p.points.get(i), p.points.get(j));
                    best = best.min(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt());
                }
            }
        }
        best
    }

    #[test]
    fn uniform_flow_gives_parallel_lines() {
        let ds = field(|_, _| [1.0, 0.0], 21);
        let out = evenly_spaced_2d(&ds, "V", [0.0, 0.0, 0.0], 0.2, 0.5, 0.1, 10_000).unwrap();
        // Lines at y = 0, ±0.2, ..., ±1: eleven.
        assert_eq!(out.lines.len(), 11);
        for c in 0..out.lines.len() {
            let ids = out.lines.cell(c);
            let y0 = out.points.get(ids[0] as usize)[1];
            assert!(ids.iter().all(|&i| (out.points.get(i as usize)[1] - y0).abs() < 1e-9));
            assert!((y0 / 0.2 - (y0 / 0.2).round()).abs() < 1e-6, "{y0}");
        }
    }

    #[test]
    fn rotation_gives_rings_kept_apart() {
        let ds = field(|x, y| [-y, x], 41);
        let out = evenly_spaced_2d(&ds, "V", [0.3, 0.0, 0.0], 0.15, 0.5, 0.05, 20_000).unwrap();
        assert!(out.lines.len() >= 5, "{}", out.lines.len());
        // No two lines come closer than the test distance (less a step).
        assert!(min_gap_between_lines(&out) > 0.5 * 0.15 - 0.05 * 0.15 - 1e-9, "{}", min_gap_between_lines(&out));
        assert!(out.point_data.get("V").is_some() && out.cell_data.get("LineId").is_some());
    }
}
