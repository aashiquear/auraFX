//! Plots along lines: line segments joined and ordered into curves (Plot On Sorted Lines), the
//! curves where a plane cuts a surface (Plot On Intersection Curves), and a line sampled
//! uniformly, at the cell boundaries it crosses or at the middle of each cell (Resample To Line).

use crate::probe::{probe_points, Locator};
use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::collections::HashMap;

/// Join the line segments of `ds` (lines and polylines) into curves, each ordered from one end
/// to the other (or around, for closed curves), with `arc_length`. One block per curve, longest
/// first.
pub fn sort_lines(ds: &DataSet) -> Result<DataSet> {
    let leaves: Vec<&DataSet> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|l| l.1).collect(),
        d => vec![d],
    };
    let mut curves: Vec<PolyData> = Vec::new();
    for leaf in leaves {
        let DataSet::PolyData(pd) = leaf else { continue };
        curves.extend(sort_polydata_lines(pd));
    }
    if curves.is_empty() {
        return Err(anyhow!("no lines to sort (use the output of Slice, Contour on a surface, Feature Edges or Plot Over Line)"));
    }
    curves.sort_by(|a, b| arc_end(b).total_cmp(&arc_end(a)));
    let mut mb = MultiBlock::default();
    for (k, c) in curves.into_iter().enumerate() {
        mb.blocks.push((format!("line {}", k + 1), Some(DataSet::PolyData(c))));
    }
    Ok(DataSet::MultiBlock(mb))
}

fn arc_end(p: &PolyData) -> f64 {
    p.point_data.get("arc_length").map(|a| a.get(a.len().saturating_sub(1), 0)).unwrap_or(0.0)
}

fn sort_polydata_lines(pd: &PolyData) -> Vec<PolyData> {
    let n = pd.points.len();
    if n == 0 || pd.lines.is_empty() {
        return Vec::new();
    }
    // Merge points that coincide (cut lines from neighbouring cells share their ends).
    let b = pd.points.bounds();
    let tol = (b.diagonal() * 1e-9).max(1e-300);
    let mut key_of: HashMap<[i64; 3], usize> = HashMap::new();
    let mut rep = vec![0usize; n];
    for (i, r) in rep.iter_mut().enumerate() {
        let p = pd.points.get(i);
        let k = [(p[0] / tol).round() as i64, (p[1] / tol).round() as i64, (p[2] / tol).round() as i64];
        *r = *key_of.entry(k).or_insert(i);
    }
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut seen_edge: std::collections::HashSet<(usize, usize)> = Default::default();
    for (_, cell) in pd.lines.iter() {
        for w in cell.windows(2) {
            let (a, b) = (rep[w[0] as usize], rep[w[1] as usize]);
            if a == b || !seen_edge.insert((a.min(b), a.max(b))) {
                continue;
            }
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    }
    let mut used: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut starts: Vec<usize> = adj.keys().copied().collect();
    // Open curves start at their ends (one neighbour), then closed ones anywhere; in a fixed order.
    starts.sort_by_key(|v| (adj[v].len() != 1, *v));
    let mut out = Vec::new();
    for s in starts {
        let Some(&first) = adj[&s].iter().find(|&&w| !used.contains(&(s.min(w), s.max(w)))) else { continue };
        let mut chain = vec![s];
        let (mut prev, mut cur) = (s, first);
        used.insert((s.min(first), s.max(first)));
        loop {
            chain.push(cur);
            if cur == s {
                break;
            }
            // Continue along an unused edge, preferring the straightest one at a junction.
            let next = adj[&cur].iter().copied().filter(|&w| !used.contains(&(cur.min(w), cur.max(w)))).max_by(|&a, &b| {
                let d0 = sub(pd.points.get(cur), pd.points.get(prev));
                let ca = cos(d0, sub(pd.points.get(a), pd.points.get(cur)));
                let cb = cos(d0, sub(pd.points.get(b), pd.points.get(cur)));
                ca.total_cmp(&cb)
            });
            let Some(w) = next else { break };
            used.insert((cur.min(w), cur.max(w)));
            prev = cur;
            cur = w;
        }
        out.push(curve(pd, &chain));
    }
    out
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cos(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let n = ((a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) * (b[0] * b[0] + b[1] * b[1] + b[2] * b[2])).sqrt();
    if n > 0.0 {
        d / n
    } else {
        -1.0
    }
}

fn curve(pd: &PolyData, chain: &[usize]) -> PolyData {
    let mut arc = Vec::with_capacity(chain.len());
    let mut s = 0.0;
    for (k, &i) in chain.iter().enumerate() {
        if k > 0 {
            let d = sub(pd.points.get(i), pd.points.get(chain[k - 1]));
            s += (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        }
        arc.push(s);
    }
    let mut attrs = pd.point_data.gather(chain);
    attrs.insert(Array::scalars_f64("arc_length", arc));
    let mut lines = Cells::new();
    lines.push(CellType::PolyLine, &(0..chain.len() as u32).collect::<Vec<_>>());
    PolyData { points: pd.points.gather(chain), lines, point_data: attrs, ..Default::default() }
}

/// The curves where the plane (origin, normal) cuts `ds`, each a block ordered along its length.
pub fn intersection_curves(ds: &DataSet, origin: [f64; 3], normal: [f64; 3]) -> Result<DataSet> {
    let cut = crate::slice::slice_plane(ds, origin, normal)?;
    if cut.lines.is_empty() {
        return Err(anyhow!("the plane does not cut a surface of this data (for a volume, use Plot Over Line or a slice)"));
    }
    sort_lines(&DataSet::PolyData(cut))
}

/// Where Resample To Line takes its samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampling {
    Uniform,
    /// Where the line enters and leaves each cell.
    CellBoundaries,
    /// Half way between the boundaries, one sample per cell crossed.
    CellCenters,
}

/// Sample `ds` along the segment p1-p2: `arc_length`, the probed arrays and vtkValidPointMask.
pub fn resample_to_line(ds: &DataSet, p1: [f64; 3], p2: [f64; 3], resolution: usize, sampling: Sampling) -> Result<PolyData> {
    if sampling == Sampling::Uniform {
        return crate::probe::plot_over_line(ds, p1, p2, resolution);
    }
    let at = |t: f64| [p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1]), p1[2] + t * (p2[2] - p1[2])];
    let len = {
        let d = sub(p2, p1);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    // Find the cells along the line on a fine uniform sampling, then each change of cell by bisection.
    let (loc, eds) = Locator::new(ds)?;
    let _ = &eds;
    let m = resolution.max(2000);
    let cell_at = |t: f64| loc.find(at(t)).map(|h| h.cell);
    let mut ts: Vec<f64> = Vec::new();
    let mut prev = cell_at(0.0);
    if prev.is_some() {
        ts.push(0.0);
    }
    let mut t_prev = 0.0;
    for k in 1..=m {
        let t = k as f64 / m as f64;
        let c = cell_at(t);
        if c != prev {
            let (mut lo, mut hi) = (t_prev, t);
            for _ in 0..48 {
                let mid = 0.5 * (lo + hi);
                if cell_at(mid) == prev {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            ts.push(0.5 * (lo + hi));
        }
        prev = c;
        t_prev = t;
    }
    if prev.is_some() {
        ts.push(1.0);
    }
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let ts: Vec<f64> = match sampling {
        Sampling::CellBoundaries => ts,
        // The middle of each stretch that lies inside a cell.
        _ => ts.windows(2).map(|w| 0.5 * (w[0] + w[1])).filter(|&t| cell_at(t).is_some()).collect(),
    };
    if ts.is_empty() {
        return Err(anyhow!("the line does not cross the data"));
    }
    let pts: Vec<[f64; 3]> = ts.iter().map(|&t| at(t)).collect();
    let mut attrs = probe_points(ds, &pts)?;
    attrs.insert(Array::scalars_f64("arc_length", ts.iter().map(|t| t * len).collect()));
    let mut lines = Cells::new();
    lines.push(CellType::PolyLine, &(0..pts.len() as u32).collect::<Vec<_>>());
    Ok(PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), lines, point_data: attrs, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit square of 4 x 4 quads in z = 0 with the point array x.
    fn grid() -> DataSet {
        let n = 5;
        let mut pd = PolyData::default();
        let mut xs = Vec::new();
        for j in 0..n {
            for i in 0..n {
                pd.points.push([i as f64 / 4.0, j as f64 / 4.0, 0.0]);
                xs.push(i as f64 / 4.0);
            }
        }
        for j in 0..4u32 {
            for i in 0..4u32 {
                let a = j * 5 + i;
                pd.polys.push(CellType::Quad, &[a, a + 1, a + 6, a + 5]);
            }
        }
        pd.point_data.insert(Array::scalars_f64("x", xs));
        DataSet::PolyData(pd)
    }

    #[test]
    fn sorted_lines_and_curves() {
        // Shuffled, reversed segments of an open polyline and a closed triangle.
        let mut pd = PolyData::default();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [10.0, 0.0, 0.0], [11.0, 0.0, 0.0], [10.0, 1.0, 0.0]] {
            pd.points.push(p);
        }
        pd.point_data.insert(Array::scalars_f64("id", (0..7).map(|i| i as f64).collect()));
        for (a, b) in [(2u32, 1u32), (3, 2), (0, 1), (4, 5), (6, 4), (5, 6)] {
            pd.lines.push(CellType::Line, &[a, b]);
        }
        let DataSet::MultiBlock(mb) = sort_lines(&DataSet::PolyData(pd)).unwrap() else { panic!() };
        assert_eq!(mb.blocks.len(), 2);
        // Longest first: the triangle (2 + sqrt 2), then the open line (3).
        let Some(DataSet::PolyData(open)) = &mb.blocks[1].1 else { panic!() };
        let ids: Vec<f64> = (0..4).map(|i| open.point_data.get("id").unwrap().get(i, 0)).collect();
        assert!(ids == [0.0, 1.0, 2.0, 3.0] || ids == [3.0, 2.0, 1.0, 0.0], "{ids:?}");
        assert!((arc_end(open) - 3.0).abs() < 1e-12);
        let Some(DataSet::PolyData(tri)) = &mb.blocks[0].1 else { panic!() };
        assert_eq!(tri.points.len(), 4, "a closed curve comes back to its start");
        assert!((arc_end(tri) - (2.0 + 2f64.sqrt())).abs() < 1e-12);
        // A plane x = 0.6 cuts the square in one segment chain of length 1 along y.
        let DataSet::MultiBlock(c) = intersection_curves(&grid(), [0.6, 0.0, 0.0], [1.0, 0.0, 0.0]).unwrap() else { panic!() };
        assert_eq!(c.blocks.len(), 1);
        let Some(DataSet::PolyData(l)) = &c.blocks[0].1 else { panic!() };
        assert!((arc_end(l) - 1.0).abs() < 1e-9);
        // Slices write single-precision points.
        assert!((l.point_data.get("x").unwrap().get(0, 0) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn line_at_cell_boundaries_and_centres() {
        let ds = grid();
        let b = resample_to_line(&ds, [0.0, 0.3, 0.0], [1.0, 0.3, 0.0], 100, Sampling::CellBoundaries).unwrap();
        let arc: Vec<f64> = (0..b.points.len()).map(|i| b.point_data.get("arc_length").unwrap().get(i, 0)).collect();
        assert_eq!(arc.len(), 5, "{arc:?}");
        for (k, a) in arc.iter().enumerate() {
            assert!((a - k as f64 * 0.25).abs() < 1e-9, "{arc:?}");
        }
        let c = resample_to_line(&ds, [0.0, 0.3, 0.0], [1.0, 0.3, 0.0], 100, Sampling::CellCenters).unwrap();
        assert_eq!(c.points.len(), 4);
        assert!((c.point_data.get("x").unwrap().get(0, 0) - 0.125).abs() < 1e-9);
        assert_eq!(resample_to_line(&ds, [0.0; 3], [1.0, 1.0, 0.0], 50, Sampling::Uniform).unwrap().points.len(), 51);
    }
}
