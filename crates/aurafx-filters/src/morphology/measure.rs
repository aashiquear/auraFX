//! Measures of a microstructure read from its grid of cells: the volume fraction, surface,
//! connectivity and shape number of every phase, the probability that two points both lie in
//! a phase, chord lengths, the thickness at every point, the length of the paths through a
//! phase, its fabric, the clusters that cross the box, the volume that represents the
//! material, the sizes of spheres from the sizes of their sections, and touching particles
//! told apart.

use super::bio::{phases_of, refresh_fractions};
use super::fft::Fft3;
use super::*;
use anyhow::{anyhow, bail, Result};
use std::f64::consts::PI;

/// A microstructure as read from a dataset.
pub struct Cells {
    pub img: ImageData,
    pub n: [usize; 3],
    pub sp: [f64; 3],
    pub phases: Vec<i32>,
}

impl Cells {
    pub fn of(ds: &DataSet, array: &str) -> Result<Cells> {
        let (img, phases) = phases_of(ds, array)?;
        let v = crate::features::Vox::of(&img);
        Ok(Cells { n: v.n, sp: v.sp, img, phases })
    }
    pub fn mask(&self, phase: i32) -> Result<Vec<bool>> {
        let m: Vec<bool> = self.phases.iter().map(|p| *p == phase).collect();
        if !m.contains(&true) {
            bail!("no cell of phase {phase}");
        }
        Ok(m)
    }
    pub fn len(&self) -> usize {
        self.phases.len()
    }
    fn ijk(&self, c: usize) -> [usize; 3] {
        [c % self.n[0], (c / self.n[0]) % self.n[1], c / (self.n[0] * self.n[1])]
    }
    fn planar(&self) -> bool {
        self.n[2] == 1
    }
}

fn table(columns: Vec<(&str, Vec<f64>)>) -> Table {
    let mut t = Table::default();
    for (name, v) in columns {
        t.columns.insert(Array::scalars_f64(name, v));
    }
    t
}

/// The cell at an offset, through the faces when `periodic`, none beyond the box otherwise.
#[inline]
fn offset(n: [usize; 3], p: [usize; 3], o: [i64; 3], periodic: bool) -> Option<usize> {
    let mut q = [0usize; 3];
    for k in 0..3 {
        let v = p[k] as i64 + o[k];
        let m = n[k] as i64;
        q[k] = if (0..m).contains(&v) {
            v as usize
        } else if periodic {
            v.rem_euclid(m) as usize
        } else {
            return None;
        };
    }
    Some(q[0] + n[0] * (q[1] + n[1] * q[2]))
}

/// The neighbours of a cell that come after it: across faces (6 in all), edges (18) or corners (26).
fn forward_offsets(n: [usize; 3], connectivity: usize) -> Vec<[i64; 3]> {
    let mut out = Vec::new();
    for z in if n[2] > 1 { -1..=1i64 } else { 0..=0 } {
        for y in -1..=1i64 {
            for x in -1..=1i64 {
                let m = x.abs() + y.abs() + z.abs();
                let wanted = m > 0 && (connectivity >= 26 || (connectivity >= 18 && m <= 2) || m == 1);
                if wanted && (z, y, x) > (0, 0, 0) {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}

/// The connected parts of the cells of `mask`: a number from 1 for every cell (0 outside) and
/// the number of parts.
pub fn label(mask: &[bool], n: [usize; 3], connectivity: usize, periodic: bool) -> (Vec<u32>, usize) {
    let mut parent: Vec<u32> = (0..mask.len() as u32).collect();
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    let offs = forward_offsets(n, connectivity);
    for c in 0..mask.len() {
        if !mask[c] {
            continue;
        }
        let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
        for o in &offs {
            if let Some(q) = offset(n, p, *o, periodic) {
                if mask[q] {
                    let (a, b) = (find(&mut parent, c as u32), find(&mut parent, q as u32));
                    if a != b {
                        parent[a.max(b) as usize] = a.min(b);
                    }
                }
            }
        }
    }
    let mut ids = vec![0u32; mask.len()];
    let mut count = 0;
    for c in 0..mask.len() {
        if mask[c] {
            let root = find(&mut parent, c as u32) as usize;
            if ids[root] == 0 {
                count += 1;
                ids[root] = count;
            }
            ids[c] = ids[root];
        }
    }
    (ids, count as usize)
}

/// The directions of the lines along which the boundaries are counted, with their weights.
fn directions(planar: bool) -> Vec<([i64; 3], f64)> {
    if planar {
        return vec![([1, 0, 0], 0.25), ([0, 1, 0], 0.25), ([1, 1, 0], 0.25), ([1, -1, 0], 0.25)];
    }
    let (a, b, c) = (0.091556, 0.073961, 0.070391);
    vec![
        ([1, 0, 0], a),
        ([0, 1, 0], a),
        ([0, 0, 1], a),
        ([1, 1, 0], b),
        ([1, -1, 0], b),
        ([1, 0, 1], b),
        ([1, 0, -1], b),
        ([0, 1, 1], b),
        ([0, 1, -1], b),
        ([1, 1, 1], c),
        ([1, 1, -1], c),
        ([1, -1, 1], c),
        ([1, -1, -1], c),
    ]
}

/// Boundaries of the mask met per unit length of the lines along the offset `o`.
fn crossings(mask: &[bool], n: [usize; 3], sp: [f64; 3], o: [i64; 3], periodic: bool) -> f64 {
    let (pairs, cuts) = (0..mask.len())
        .into_par_iter()
        .map(|c| {
            let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
            match offset(n, p, o, periodic) {
                Some(q) => (1usize, (mask[c] != mask[q]) as usize),
                None => (0, 0),
            }
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let step = ((o[0] as f64 * sp[0]).powi(2) + (o[1] as f64 * sp[1]).powi(2) + (o[2] as f64 * sp[2]).powi(2)).sqrt();
    cuts as f64 / (pairs.max(1) as f64 * step)
}

/// The surface of the mask per unit volume, from the boundaries met along lines in 13
/// directions (4 in a plane, where it is the length of the outline per unit area).
pub fn specific_surface(mask: &[bool], n: [usize; 3], sp: [f64; 3], periodic: bool) -> f64 {
    let planar = n[2] == 1;
    let mean: f64 = directions(planar).iter().map(|(o, w)| w * crossings(mask, n, sp, *o, periodic)).sum();
    if planar {
        0.5 * PI * mean
    } else {
        2.0 * mean
    }
}

/// The Euler number of the mask, its cells taken as closed cubes: parts less tunnels plus
/// cavities (in a plane: parts less holes).
pub fn euler_number(mask: &[bool], n: [usize; 3], periodic: bool) -> i64 {
    let at = |i: i64, j: i64, k: i64| -> bool {
        let v = [i, j, k];
        let mut q = [0usize; 3];
        for a in 0..3 {
            let m = n[a] as i64;
            q[a] = if (0..m).contains(&v[a]) {
                v[a] as usize
            } else if periodic && n[a] > 1 {
                v[a].rem_euclid(m) as usize
            } else {
                return false;
            };
        }
        mask[q[0] + n[0] * (q[1] + n[1] * q[2])]
    };
    let planar = n[2] == 1;
    // The corners, edges and faces along an axis: one more than the cells, unless they wrap.
    let top = |a: usize| -> i64 { n[a] as i64 + if periodic || (planar && a == 2) { 0 } else { 1 } };
    let cells = n.map(|x| x as i64);
    let any = |i: i64, j: i64, k: i64, back: [i64; 3]| -> bool {
        for c in 0..=back[2] {
            for b in 0..=back[1] {
                for a in 0..=back[0] {
                    if at(i - a, j - b, k - c) {
                        return true;
                    }
                }
            }
        }
        false
    };
    let count = |range: [i64; 3], back: [i64; 3]| -> i64 {
        (0..range[2]).into_par_iter().map(|k| (0..range[1]).map(|j| (0..range[0]).filter(|i| any(*i, j, k, back)).count() as i64).sum::<i64>()).sum()
    };
    let z = if planar { 0 } else { 1 };
    let solid = mask.iter().filter(|m| **m).count() as i64;
    let corners = count([top(0), top(1), top(2)], [1, 1, z]);
    let edges = count([cells[0], top(1), top(2)], [0, 1, z])
        + count([top(0), cells[1], top(2)], [1, 0, z])
        + if planar { 0 } else { count([top(0), top(1), cells[2]], [1, 1, 0]) };
    if planar {
        return corners - edges + solid;
    }
    let faces =
        count([top(0), cells[1], cells[2]], [1, 0, 0]) + count([cells[0], top(1), cells[2]], [0, 1, 0]) + count([cells[0], cells[1], top(2)], [0, 0, 1]);
    corners - edges + faces - solid
}

fn field_value(img: &ImageData, name: &str, k: usize) -> f64 {
    img.field_data.get(name).filter(|a| a.len() > k).map(|a| a.get(k, 0)).unwrap_or(f64::NAN)
}

/// Phase Measures: a row for every phase with its volume fraction, its surface per unit
/// volume, the mean length of the chords through it, its Euler number and the number of its
/// connected parts, beside the values the model gives where it kept them.
pub fn phase_measures(ds: &DataSet, array: &str, periodic: bool) -> Result<DataSet> {
    let m = Cells::of(ds, array)?;
    let mut present: Vec<i32> = m.phases.clone();
    present.sort_unstable();
    present.dedup();
    if present.len() > 64 {
        bail!("more than 64 phases: is {} a phase array?", if array.is_empty() { "Phases" } else { array });
    }
    let volume: f64 = m.len() as f64 * m.sp[0] * m.sp[1] * if m.planar() { 1.0 } else { m.sp[2] };
    let mut cols: Vec<Vec<f64>> = vec![Vec::new(); 9];
    for &p in &present {
        let mask: Vec<bool> = m.phases.iter().map(|x| *x == p).collect();
        let f = mask.iter().filter(|x| **x).count() as f64 / m.len() as f64;
        let s = specific_surface(&mask, m.n, m.sp, periodic);
        let chi = euler_number(&mask, m.n, periodic);
        let expected_f = if p == PHASE_SOLID && m.img.field_data.get("ExpectedFraction").is_some() {
            field_value(&m.img, "ExpectedFraction", 0)
        } else {
            field_value(&m.img, "ExpectedFractions", p.max(0) as usize)
        };
        let expected_s = if present.len() == 2 || p == PHASE_SOLID { field_value(&m.img, "ExpectedSpecificSurface", 0) } else { f64::NAN };
        let row = [
            p as f64,
            f,
            s,
            if s > 0.0 { 4.0 * f / s } else { f64::NAN },
            chi as f64,
            chi as f64 / volume,
            label(&mask, m.n, 6, periodic).1 as f64,
            expected_f,
            expected_s,
        ];
        cols.iter_mut().zip(row).for_each(|(c, v)| c.push(v));
    }
    let names = [
        "Phase",
        "VolumeFraction",
        "SpecificSurface",
        "MeanChord",
        "EulerNumber",
        "EulerNumberPerVolume",
        "ConnectedParts",
        "ExpectedFraction",
        "ExpectedSpecificSurface",
    ];
    let keep = |k: usize| k < 7 || cols[k].iter().any(|v| v.is_finite());
    let t = table((0..9).filter(|k| keep(*k)).map(|k| (names[k], cols[k].clone())).collect());
    Ok(DataSet::Table(t))
}

/// The number of pairs of cells of the mask at every offset up to `reach` cells along each
/// axis (index `(dx + reach) + w ((dy + reach) + w (dz + reach))`, `w = 2 reach + 1`; one
/// layer along a flat axis), and the number of pairs of cells of the box at that offset.
fn pair_counts(mask: &[bool], n: [usize; 3], reach: usize, periodic: bool) -> (Vec<f64>, Vec<f64>, [usize; 3]) {
    let pad: [usize; 3] = std::array::from_fn(|k| if n[k] == 1 || periodic { n[k] } else { n[k] + reach });
    let fft = Fft3::new(pad);
    let mut field = vec![0.0; fft.len()];
    for (c, m) in mask.iter().enumerate() {
        if *m {
            let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
            field[p[0] + pad[0] * (p[1] + pad[1] * p[2])] = 1.0;
        }
    }
    let mut hat = fft.forward(&field);
    hat.par_iter_mut().for_each(|v| *v = [v[0] * v[0] + v[1] * v[1], 0.0]);
    fft.run(&mut hat, true);
    let w: [usize; 3] = std::array::from_fn(|k| if n[k] == 1 { 1 } else { 2 * reach + 1 });
    let r: [i64; 3] = std::array::from_fn(|k| if n[k] == 1 { 0 } else { reach as i64 });
    let mut both = vec![0.0; w[0] * w[1] * w[2]];
    let mut pairs = vec![0.0; both.len()];
    for dz in -r[2]..=r[2] {
        for dy in -r[1]..=r[1] {
            for dx in -r[0]..=r[0] {
                let d = [dx, dy, dz];
                let q: [usize; 3] = std::array::from_fn(|k| d[k].rem_euclid(pad[k] as i64) as usize);
                let i = (dx + r[0]) as usize + w[0] * ((dy + r[1]) as usize + w[1] * (dz + r[2]) as usize);
                both[i] = hat[q[0] + pad[0] * (q[1] + pad[1] * q[2])][0];
                pairs[i] = (0..3).map(|k| if periodic { n[k] as f64 } else { (n[k] as f64 - d[k].abs() as f64).max(0.0) }).product();
            }
        }
    }
    (both, pairs, w)
}

/// The runs of cells of the mask along the lines of an axis: `f(length, cut)` for each, `cut`
/// when the run touches an end of its line.
fn runs(mask: &[bool], n: [usize; 3], axis: usize, mut f: impl FnMut(usize, bool)) {
    let stride = [1, n[0], n[0] * n[1]][axis];
    for start in (0..mask.len()).filter(|c| (c / stride) % n[axis] == 0) {
        let mut run = 0;
        let mut first = true;
        for i in 0..n[axis] {
            if mask[start + i * stride] {
                run += 1;
            } else {
                if run > 0 {
                    f(run, first);
                }
                run = 0;
                first = false;
            }
        }
        if run > 0 {
            f(run, true);
        }
    }
}

/// Two-Point Statistics of a phase against the distance: the probability that two points
/// that far apart both lie in the phase (along each axis, and averaged over the directions),
/// that the whole segment between them does (the lineal path), and that they lie in the same
/// connected part (the cluster function).
pub fn two_point_statistics(ds: &DataSet, array: &str, phase: i32, reach: usize, periodic: bool) -> Result<DataSet> {
    let m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let axes: Vec<usize> = (0..3).filter(|k| m.n[*k] > 1).collect();
    let least = axes.iter().map(|k| m.n[*k]).min().ok_or_else(|| anyhow!("the grid has a single cell"))?;
    let reach = if reach == 0 { least / 2 } else { reach }.clamp(1, (least - 1).max(1));
    let (both, pairs, w) = pair_counts(&mask, m.n, reach, periodic);
    let f = mask.iter().filter(|x| **x).count() as f64 / m.len() as f64;
    let r: [i64; 3] = std::array::from_fn(|k| if m.n[k] == 1 { 0 } else { reach as i64 });
    let at = |d: [i64; 3]| -> f64 {
        let i = (d[0] + r[0]) as usize + w[0] * ((d[1] + r[1]) as usize + w[1] * (d[2] + r[2]) as usize);
        both[i] / pairs[i].max(1.0)
    };
    let h = m.sp[axes[0]];
    let mut cols: Vec<(String, Vec<f64>)> = vec![("Distance".into(), (0..=reach).map(|d| d as f64 * h).collect())];
    for &a in &axes {
        cols.push((
            format!("S2 {}", ["X", "Y", "Z"][a]),
            (0..=reach as i64)
                .map(|d| {
                    let mut o = [0i64; 3];
                    o[a] = d;
                    at(o)
                })
                .collect(),
        ));
    }
    // Over the directions: every offset goes to the nearest whole distance.
    let (mut sum, mut count) = (vec![0.0; reach + 1], vec![0.0; reach + 1]);
    for dz in -r[2]..=r[2] {
        for dy in -r[1]..=r[1] {
            for dx in -r[0]..=r[0] {
                let d = ((dx * dx + dy * dy + dz * dz) as f64).sqrt().round() as usize;
                if d <= reach {
                    sum[d] += at([dx, dy, dz]);
                    count[d] += 1.0;
                }
            }
        }
    }
    // The offsets of a class are not at its whole distance on average: the means of the
    // classes are carried to the whole distances along straight lines between them.
    let mut far = vec![0.0; reach + 1];
    for dz in -r[2]..=r[2] {
        for dy in -r[1]..=r[1] {
            for dx in -r[0]..=r[0] {
                let len = ((dx * dx + dy * dy + dz * dz) as f64).sqrt();
                if len.round() as usize <= reach {
                    far[len.round() as usize] += len;
                }
            }
        }
    }
    let class: Vec<(f64, f64)> = (0..=reach).map(|d| (far[d] / count[d], sum[d] / count[d])).collect();
    let radial: Vec<f64> = (0..=reach)
        .map(|d| {
            let x = d as f64;
            let k = if class[d].0 >= x { d.max(1) - 1 } else { d.min(reach - 1) };
            let ((x0, y0), (x1, y1)) = (class[k], class[(k + 1).min(reach)]);
            if x1 > x0 {
                y0 + (y1 - y0) * (x - x0) / (x1 - x0)
            } else {
                class[d].1
            }
        })
        .collect();
    cols.push(("S2".into(), radial.clone()));
    let (ids, _) = label(&mask, m.n, 6, periodic);
    for &a in &axes {
        // Periodic: a segment or a pair that leaves through a face comes back through the
        // opposite one, and every cell starts one. Otherwise only those that stay in the box count.
        let positions = |d: usize| if periodic { m.len() as f64 } else { (m.len() / m.n[a] * (m.n[a] - d)) as f64 };
        let stride = [1, m.n[0], m.n[0] * m.n[1]][a];
        let mut inside = vec![0.0; reach + 1];
        if periodic {
            // For every cell, how many cells of the phase follow one another from it along the
            // axis, around the line when it comes to its end.
            let n = m.n[a];
            for start in (0..m.len()).filter(|c| (c / stride) % n == 0) {
                let line: Vec<bool> = (0..n).map(|k| mask[start + k * stride]).collect();
                let Some(gap) = line.iter().rposition(|x| !*x) else {
                    inside.iter_mut().for_each(|v| *v += n as f64);
                    continue;
                };
                // Going backwards from the last cell outside the phase, around the line once.
                let mut run = 0usize;
                for k in 0..n {
                    let x = (gap + n - k) % n;
                    run = if line[x] { run + 1 } else { 0 };
                    for v in inside.iter_mut().take(run.min(reach + 1)) {
                        *v += 1.0;
                    }
                }
            }
        } else {
            runs(&mask, m.n, a, |len, _| {
                for (d, v) in inside.iter_mut().enumerate().take(len.min(reach + 1)) {
                    *v += (len - d) as f64;
                }
            });
        }
        cols.push((format!("LinealPath {}", ["X", "Y", "Z"][a]), inside.iter().enumerate().map(|(d, v)| v / positions(d)).collect()));
        let same: Vec<f64> = (0..=reach)
            .into_par_iter()
            .map(|d| {
                (0..m.len())
                    .filter(|c| {
                        let x = (c / stride) % m.n[a];
                        ids[*c] != 0
                            && if periodic {
                                ids[c - x * stride + (x + d) % m.n[a] * stride] == ids[*c]
                            } else {
                                x + d < m.n[a] && ids[c + d * stride] == ids[*c]
                            }
                    })
                    .count() as f64
                    / positions(d)
            })
            .collect();
        cols.push((format!("Cluster {}", ["X", "Y", "Z"][a]), same));
    }
    if let Some(e) = m.img.field_data.get("ExpectedTwoPoint").filter(|_| phase == PHASE_SOLID) {
        cols.push(("Expected".into(), (0..=reach).map(|d| if d < e.len() { e.get(d, 0) } else { f64::NAN }).collect()));
    }
    let mut t = Table::default();
    for (name, v) in cols {
        t.columns.insert(Array::scalars_f64(name, v));
    }
    // The integral range: the volume (area in a plane) over which the phase is correlated.
    let variance = f * (1.0 - f);
    let mut range = 0.0;
    if variance > 0.0 {
        for d in 0..=reach {
            let rho = (radial[d] - f * f) / variance;
            if rho <= 0.0 {
                break;
            }
            let shell = if d == 0 {
                if m.planar() {
                    PI * 0.25
                } else {
                    4.0 / 3.0 * PI * 0.125
                }
            } else if m.planar() {
                2.0 * PI * d as f64
            } else {
                4.0 * PI * (d * d) as f64 + PI / 3.0
            };
            range += rho * shell * h.powi(if m.planar() { 2 } else { 3 });
        }
    }
    t.field_data.insert(Array::scalars_f64("VolumeFraction", vec![f]));
    t.field_data.insert(Array::scalars_f64("IntegralRange", vec![range]));
    Ok(DataSet::Table(t))
}

/// Phase Chord Lengths: the lengths of the stretches of a phase along the lines of each axis
/// (those cut by the box left out), as shares of their number per class of length.
pub fn chord_lengths(ds: &DataSet, array: &str, phase: i32, bins: usize) -> Result<DataSet> {
    let m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let axes: Vec<usize> = (0..3).filter(|k| m.n[*k] > 1).collect();
    let mut found: Vec<Vec<usize>> = Vec::new();
    for &a in &axes {
        let mut v = Vec::new();
        runs(&mask, m.n, a, |len, cut| {
            if !cut {
                v.push(len);
            }
        });
        found.push(v);
    }
    let longest = found.iter().flatten().copied().max().ok_or_else(|| anyhow!("every chord of phase {phase} is cut by the box"))?;
    let width = longest.div_ceil(bins.max(1)).max(1);
    let classes = longest.div_ceil(width);
    let mut cols: Vec<(String, Vec<f64>)> =
        vec![("Length".into(), (0..classes).map(|k| (k as f64 + 0.5) * width as f64 * m.sp[axes[0]] + 0.5 * m.sp[axes[0]]).collect())];
    let mut means = vec![f64::NAN; 3];
    for (v, &a) in found.iter().zip(&axes) {
        let mut hist = vec![0.0; classes];
        v.iter().for_each(|len| hist[(len - 1) / width] += 1.0 / v.len() as f64);
        cols.push((["X", "Y", "Z"][a].to_string(), hist));
        if !v.is_empty() {
            means[a] = v.iter().sum::<usize>() as f64 / v.len() as f64 * m.sp[a];
        }
    }
    let mut t = Table::default();
    for (name, v) in cols {
        t.columns.insert(Array::scalars_f64(name, v));
    }
    t.field_data.insert(Array::scalars_f64("MeanChord", means));
    Ok(DataSet::Table(t))
}

/// For every cell of the mask, the radius of the largest ball about it that stays in the
/// mask (0 outside), from the distance to the nearest cell outside.
fn inscribed_radius(m: &Cells, mask: &[bool]) -> Result<Vec<f64>> {
    if !mask.contains(&false) {
        bail!("the phase fills the whole box");
    }
    let v = crate::features::Vox::of(&m.img);
    let outside: Vec<bool> = mask.iter().map(|x| !*x).collect();
    let half = 0.5 * (0..3).filter(|k| m.n[*k] > 1).map(|k| m.sp[k]).fold(f64::INFINITY, f64::min);
    Ok(crate::features::distance_transform(&v, &outside).iter().map(|d2| (d2.sqrt() - half).max(0.0)).collect())
}

/// The diameter of the largest ball inside the mask that holds each cell.
pub fn thickness_map(m: &Cells, mask: &[bool]) -> Result<Vec<f32>> {
    let rad = inscribed_radius(m, mask)?;
    let g = Grid { n: m.n, h: m.sp[0], size: std::array::from_fn(|k| m.n[k] as f64 * m.sp[0]), periodic: false };
    let offs: Vec<([i64; 3], f64)> = {
        let mut o = forward_offsets(m.n, 26);
        o.extend(o.clone().iter().map(|v| v.map(|x| -x)));
        o.iter().map(|v| (*v, (0..3).map(|k| (v[k] as f64 * m.sp[k]).powi(2)).sum::<f64>().sqrt())).collect()
    };
    // The balls that no neighbouring ball holds: only those need drawing.
    let mut centres: Vec<usize> = (0..m.len())
        .into_par_iter()
        .filter(|&c| {
            mask[c] && {
                let p = m.ijk(c);
                !offs.iter().any(|(o, d)| offset(m.n, p, *o, false).is_some_and(|q| rad[q] >= rad[c] + d - 1e-9))
            }
        })
        .collect();
    centres.sort_by(|a, b| rad[*a].total_cmp(&rad[*b]));
    let mut out = vec![0f32; m.len()];
    let scale: [f64; 3] = std::array::from_fn(|k| m.sp[0] / m.sp[k]);
    for c in centres {
        // The grid of cubes of the first spacing stands for the cells; exact when they are cubes.
        let centre: [f64; 3] = std::array::from_fn(|k| (m.ijk(c)[k] as f64 + 0.5) * m.sp[0]);
        let r = (rad[c] * scale.iter().cloned().fold(f64::INFINITY, f64::min)).max(0.5 * m.sp[0]);
        let d = (2.0 * rad[c].max(0.5 * m.sp[0])) as f32;
        g.stamp_sphere(centre, r + 0.5 * m.sp[0], |cell, _| {
            if mask[cell] {
                out[cell] = d;
            }
        });
    }
    Ok(out)
}

/// Local Thickness: at every cell of a phase, the diameter of the largest ball inside the
/// phase that holds the cell (for the pores: the pore size). As a table: the share of the
/// phase by class of thickness, and the share thinner than each size (what openings by
/// balls of growing size would remove).
pub fn local_thickness(ds: &DataSet, array: &str, phase: i32, as_table: bool, bins: usize) -> Result<DataSet> {
    let mut m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let thick = thickness_map(&m, &mask)?;
    let inside: Vec<f64> = thick.iter().zip(&mask).filter(|(_, m)| **m).map(|(t, _)| *t as f64).collect();
    let mean = inside.iter().sum::<f64>() / inside.len() as f64;
    let top = inside.iter().cloned().fold(0.0, f64::max);
    if as_table {
        let bins = bins.max(1);
        let width = top / bins as f64;
        let mut hist = vec![0.0; bins];
        inside.iter().for_each(|t| hist[((t / width.max(1e-300)) as usize).min(bins - 1)] += 1.0 / inside.len() as f64);
        let mut run = 0.0;
        let cumulative: Vec<f64> = hist
            .iter()
            .map(|h| {
                run += h;
                run
            })
            .collect();
        let mut t = table(vec![("Size", (0..bins).map(|k| (k as f64 + 1.0) * width).collect()), ("Share", hist), ("ShareBelow", cumulative)]);
        t.field_data.insert(Array::scalars_f64("MeanThickness", vec![mean]));
        t.field_data.insert(Array::scalars_f64("LargestThickness", vec![top]));
        return Ok(DataSet::Table(t));
    }
    m.img.cell_data.insert(Array::scalars_f32("LocalThickness", thick));
    note(&mut m.img, "MeanThickness", vec![mean]);
    note(&mut m.img, "LargestThickness", vec![top]);
    Ok(DataSet::ImageData(m.img))
}

#[derive(PartialEq)]
struct Reached(f32, u32);
impl Eq for Reached {}
impl PartialOrd for Reached {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Reached {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// Tortuosity: the length of the shortest path through a phase from the face of the box at
/// the start of an axis to every cell, and how much longer than straight the paths that
/// reach the opposite face are.
pub fn tortuosity(ds: &DataSet, array: &str, phase: i32, axis: usize) -> Result<DataSet> {
    let mut m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let a = axis.min(2);
    if m.n[a] < 2 {
        bail!("one layer of cells along that axis");
    }
    let stride = [1, m.n[0], m.n[0] * m.n[1]][a];
    let layer = |c: usize| (c / stride) % m.n[a];
    let offs: Vec<([i64; 3], f32)> = {
        let mut o = forward_offsets(m.n, 26);
        o.extend(o.clone().iter().map(|v| v.map(|x| -x)));
        o.iter().map(|v| (*v, (0..3).map(|k| (v[k] as f64 * m.sp[k]).powi(2)).sum::<f64>().sqrt() as f32)).collect()
    };
    let mut dist = vec![f32::INFINITY; m.len()];
    let mut heap = std::collections::BinaryHeap::new();
    for c in (0..m.len()).filter(|c| mask[*c] && layer(*c) == 0) {
        dist[c] = 0.0;
        heap.push(Reached(0.0, c as u32));
    }
    while let Some(Reached(d, c)) = heap.pop() {
        let c = c as usize;
        if d > dist[c] {
            continue;
        }
        let p = m.ijk(c);
        for (o, step) in &offs {
            if let Some(q) = offset(m.n, p, *o, false) {
                if mask[q] && d + step < dist[q] {
                    dist[q] = d + step;
                    heap.push(Reached(d + step, q as u32));
                }
            }
        }
    }
    let far: Vec<usize> = (0..m.len()).filter(|c| mask[*c] && layer(*c) == m.n[a] - 1).collect();
    let reached: Vec<f64> = far.iter().filter(|c| dist[**c].is_finite()).map(|c| dist[*c] as f64).collect();
    let straight = (m.n[a] - 1) as f64 * m.sp[a];
    let value = if reached.is_empty() { f64::NAN } else { reached.iter().sum::<f64>() / reached.len() as f64 / straight };
    m.img.cell_data.insert(Array::scalars_f32("PathLength", dist.iter().map(|d| if d.is_finite() { *d } else { -1.0 }).collect()));
    note(&mut m.img, "Tortuosity", vec![value]);
    note(&mut m.img, "ShortestPath", vec![reached.iter().cloned().fold(f64::INFINITY, f64::min) / straight]);
    note(&mut m.img, "ReachedShare", vec![if far.is_empty() { 0.0 } else { reached.len() as f64 / far.len() as f64 }]);
    Ok(DataSet::ImageData(m.img))
}

/// Solve the least squares problem `a x = b` (rows of `a` of length `k`) by the normal equations.
fn least_squares(a: &[Vec<f64>], b: &[f64], k: usize) -> Option<Vec<f64>> {
    let mut m = vec![vec![0.0; k + 1]; k];
    for (row, y) in a.iter().zip(b) {
        for i in 0..k {
            for j in 0..k {
                m[i][j] += row[i] * row[j];
            }
            m[i][k] += row[i] * y;
        }
    }
    for c in 0..k {
        let p = (c..k).max_by(|x, y| m[*x][c].abs().total_cmp(&m[*y][c].abs()))?;
        if m[p][c].abs() < 1e-300 {
            return None;
        }
        m.swap(c, p);
        for r in 0..k {
            if r != c {
                let f = m[r][c] / m[c][c];
                for j in c..=k {
                    m[r][j] -= f * m[c][j];
                }
            }
        }
    }
    Some((0..k).map(|i| m[i][k] / m[i][i]).collect())
}

/// Fabric Tensor: the mean length of the chords through a phase along 13 directions (4 in a
/// plane) and the ellipsoid that fits them: its axes and lengths, and how far from round it is.
pub fn fabric_tensor(ds: &DataSet, array: &str, phase: i32, periodic: bool) -> Result<DataSet> {
    let m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let f = mask.iter().filter(|x| **x).count() as f64 / m.len() as f64;
    let planar = m.planar();
    let (mut rows, mut rhs) = (Vec::new(), Vec::new());
    for (o, _) in directions(planar) {
        let cuts = crossings(&mask, m.n, m.sp, o, periodic);
        if cuts <= 0.0 {
            bail!("no boundary of phase {phase} along a direction: the fabric needs a phase with boundaries all around");
        }
        let chord = 2.0 * f / cuts;
        let d = unit(std::array::from_fn(|k| o[k] as f64 * m.sp[k]));
        rows.push(if planar {
            vec![d[0] * d[0], d[1] * d[1], 2.0 * d[0] * d[1]]
        } else {
            vec![d[0] * d[0], d[1] * d[1], d[2] * d[2], 2.0 * d[1] * d[2], 2.0 * d[0] * d[2], 2.0 * d[0] * d[1]]
        });
        rhs.push(1.0 / (chord * chord));
    }
    let x = least_squares(&rows, &rhs, if planar { 3 } else { 6 }).ok_or_else(|| anyhow!("the chords do not fit an ellipsoid"))?;
    let tensor = if planar { [[x[0], x[2], 0.0], [x[2], x[1], 0.0], [0.0, 0.0, 1.0]] } else { [[x[0], x[5], x[4]], [x[5], x[1], x[3]], [x[4], x[3], x[2]]] };
    let (values, vectors) = crate::features::sym_eigen(tensor);
    // The chord along an axis of the ellipsoid: one over the root of its eigenvalue.
    let column = |k: usize| [vectors[0][k], vectors[1][k], vectors[2][k]];
    let mut axes: Vec<(f64, [f64; 3])> =
        (0..3).filter(|k| !planar || column(*k)[2].abs() < 0.5).map(|k| (1.0 / values[k].max(1e-300).sqrt(), column(k))).collect();
    axes.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = axes.iter().map(|a| a.0).sum();
    let mut t = table(vec![
        ("MeanChord", axes.iter().map(|a| a.0).collect()),
        ("Share", axes.iter().map(|a| a.0 / total).collect()),
        ("DirectionX", axes.iter().map(|a| a.1[0]).collect()),
        ("DirectionY", axes.iter().map(|a| a.1[1]).collect()),
        ("DirectionZ", axes.iter().map(|a| a.1[2]).collect()),
    ]);
    t.field_data.insert(Array::scalars_f64("DegreeOfAnisotropy", vec![axes[0].0 / axes[axes.len() - 1].0]));
    t.field_data.insert(Array::new("FabricTensor", 9, ArrayData::F64(tensor.iter().flatten().copied().collect())));
    Ok(DataSet::Table(t))
}

/// Whether a connected part of the mask joins the two faces of the box across each axis, and
/// the share of the mask in such parts.
pub fn spanning(mask: &[bool], n: [usize; 3], connectivity: usize) -> ([bool; 3], f64) {
    let (ids, count) = label(mask, n, connectivity, false);
    let mut size = vec![0usize; count + 1];
    ids.iter().for_each(|i| size[*i as usize] += 1);
    let mut joins = [false; 3];
    let mut spans = vec![false; count + 1];
    for a in 0..3 {
        if n[a] < 2 {
            continue;
        }
        let stride = [1, n[0], n[0] * n[1]][a];
        let (mut low, mut high) = (vec![false; count + 1], vec![false; count + 1]);
        for c in 0..mask.len() {
            let i = (c / stride) % n[a];
            if i == 0 {
                low[ids[c] as usize] = true;
            } else if i == n[a] - 1 {
                high[ids[c] as usize] = true;
            }
        }
        for k in 1..=count {
            if low[k] && high[k] {
                joins[a] = true;
                spans[k] = true;
            }
        }
    }
    let inside: usize = size[1..].iter().sum();
    let share = (1..=count).filter(|k| spans[*k]).map(|k| size[k]).sum::<usize>() as f64 / inside.max(1) as f64;
    (joins, share)
}

/// Percolation: the connected parts of a phase, numbered from the largest, and whether one
/// of them joins opposite faces of the box.
pub fn percolation(ds: &DataSet, array: &str, phase: i32, connectivity: usize, periodic: bool) -> Result<DataSet> {
    let mut m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let (ids, count) = label(&mask, m.n, connectivity, periodic);
    let mut size = vec![0usize; count + 1];
    ids.iter().for_each(|i| size[*i as usize] += 1);
    let mut order: Vec<usize> = (1..=count).collect();
    order.sort_by(|a, b| size[*b].cmp(&size[*a]).then(a.cmp(b)));
    let mut rank = vec![0i32; count + 1];
    order.iter().enumerate().for_each(|(k, id)| rank[*id] = k as i32 + 1);
    let (joins, share) = spanning(&mask, m.n, connectivity);
    let inside: usize = size[1..].iter().sum();
    m.img.cell_data.insert(Array::new("ClusterIds", 1, ArrayData::I32(ids.iter().map(|i| rank[*i as usize]).collect())));
    note(&mut m.img, "Percolates", joins.iter().map(|j| *j as u8 as f64).collect());
    note(&mut m.img, "SpanningShare", vec![share]);
    note(&mut m.img, "Clusters", vec![count as f64]);
    note(&mut m.img, "LargestClusterShare", vec![size[order[0]] as f64 / inside as f64]);
    Ok(DataSet::ImageData(m.img))
}

/// Representative Volume: the box is cut into blocks of growing size, and the variance of
/// the mean of a property over the blocks is followed against their volume. It falls as
/// `variance of a point x (range / volume)^exponent`; the fit gives the volume that brings
/// the mean of the property to a relative precision with a number of samples.
pub fn representative_volume(ds: &DataSet, array: &str, phase: i32, property: &str, precision: f64, samples: usize) -> Result<DataSet> {
    let m = Cells::of(ds, array)?;
    let values: Vec<f64> = if property.is_empty() {
        m.mask(phase)?.iter().map(|x| *x as u8 as f64).collect()
    } else {
        let a = m.img.cell_data.get(property).ok_or_else(|| anyhow!("the input has no cell array {property}"))?;
        (0..a.len()).map(|c| a.get(c, 0)).collect()
    };
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let point = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    if !(point > 0.0) || mean == 0.0 {
        bail!("the property is the same everywhere (or of zero mean)");
    }
    let axes: Vec<usize> = (0..3).filter(|k| m.n[*k] > 1).collect();
    let least = axes.iter().map(|k| m.n[*k]).min().ok_or_else(|| anyhow!("the grid has a single cell"))?;
    let cell: f64 = axes.iter().map(|k| m.sp[*k]).product();
    let mut rows: Vec<[f64; 4]> = Vec::new();
    let mut edge = 1usize;
    while edge * 2 <= least {
        let blocks: [usize; 3] = std::array::from_fn(|k| if m.n[k] > 1 { m.n[k] / edge } else { 1 });
        let count = blocks[0] * blocks[1] * blocks[2];
        if count < 8 {
            break;
        }
        let mut sums = vec![0.0; count];
        for (c, v) in values.iter().enumerate() {
            let p = m.ijk(c);
            let b: [usize; 3] = std::array::from_fn(|k| if m.n[k] > 1 { p[k] / edge } else { 0 });
            if (0..3).all(|k| b[k] < blocks[k]) {
                sums[b[0] + blocks[0] * (b[1] + blocks[1] * b[2])] += v;
            }
        }
        let per = (edge as f64).powi(axes.len() as i32);
        let bm: Vec<f64> = sums.iter().map(|s| s / per).collect();
        let mu = bm.iter().sum::<f64>() / count as f64;
        let var = bm.iter().map(|v| (v - mu).powi(2)).sum::<f64>() / (count - 1) as f64;
        rows.push([edge as f64 * m.sp[axes[0]], per * cell, count as f64, var]);
        edge *= 2;
    }
    // The fit, over the blocks larger than a cell and of variance above 0.
    let fit: Vec<&[f64; 4]> = rows.iter().skip(1).filter(|r| r[3] > 0.0).collect();
    if fit.len() < 2 {
        bail!("the grid is too small to follow the variance against the volume (at least 16 cells along every axis)");
    }
    let lines: Vec<Vec<f64>> = fit.iter().map(|r| vec![1.0, -r[1].ln()]).collect();
    let rhs: Vec<f64> = fit.iter().map(|r| (r[3] / point).ln()).collect();
    let x = least_squares(&lines, &rhs, 2).ok_or_else(|| anyhow!("the variance does not fall with the volume"))?;
    let exponent = x[1];
    if !(exponent > 0.0) {
        bail!("the variance does not fall with the volume: the box is smaller than the structure");
    }
    let range = (x[0] / exponent).exp();
    let fitted = |volume: f64| point * (range / volume).powf(exponent);
    let needed = range * (4.0 * point / (precision.max(1e-12) * mean).powi(2) / samples.max(1) as f64).powf(1.0 / exponent);
    let whole = values.len() as f64 * cell;
    let mut t = table(vec![
        ("Edge", rows.iter().map(|r| r[0]).collect()),
        ("Volume", rows.iter().map(|r| r[1]).collect()),
        ("Blocks", rows.iter().map(|r| r[2]).collect()),
        ("Variance", rows.iter().map(|r| r[3]).collect()),
        ("Fit", rows.iter().map(|r| fitted(r[1])).collect()),
    ]);
    let mut put = |name: &str, v: f64| {
        t.field_data.insert(Array::scalars_f64(name, vec![v]));
    };
    put("Mean", mean);
    put("PointVariance", point);
    put("IntegralRange", range);
    put("Exponent", exponent);
    put("RepresentativeVolume", needed);
    put("RepresentativeEdge", needed.powf(1.0 / axes.len() as f64));
    put("PrecisionOfThisVolume", 2.0 * fitted(whole).sqrt() / mean.abs());
    Ok(DataSet::Table(t))
}

/// The values of an array wherever the dataset keeps it: a column, or field, cell or point data.
pub fn values_named(ds: &DataSet, name: &str) -> Result<Vec<f64>> {
    let found = match ds {
        DataSet::Table(t) => t.columns.get(name),
        _ => None,
    }
    .or_else(|| ds.field_data().get(name))
    .or_else(|| ds.cell_data().and_then(|a| a.get(name)))
    .or_else(|| ds.point_data().and_then(|a| a.get(name)))
    .ok_or_else(|| anyhow!("the input has no array {name}"))?;
    Ok((0..found.len()).map(|k| found.get(k, 0)).collect())
}

/// Unfold Section Sizes: the diameters of spheres in space from the diameters of their
/// sections by a plane. A sphere shows a section of any size up to its own, so the largest
/// sections count the largest spheres, and what those spheres add to the smaller classes is
/// taken off class after class. `area`: the area of the section the diameters were measured on.
pub fn unfold_sections(ds: &DataSet, name: &str, classes: usize, area: f64) -> Result<DataSet> {
    let mut d: Vec<f64> = values_named(ds, name)?.into_iter().filter(|v| v.is_finite() && *v > 0.0).collect();
    if d.len() < 2 {
        bail!("at least two section diameters above 0");
    }
    let area = if area > 0.0 {
        area
    } else if let DataSet::ImageData(img) = ds {
        let v = crate::features::Vox::of(img);
        let mut sides: Vec<f64> = (0..3).map(|k| v.n[k] as f64 * v.sp[k]).collect();
        sides.sort_by(|a, b| b.total_cmp(a));
        sides[0] * sides[1]
    } else {
        bail!("give the area of the section");
    };
    d.sort_by(f64::total_cmp);
    let classes = classes.max(1);
    let width = d[d.len() - 1] * (1.0 + 1e-9) / classes as f64;
    let mut per_area = vec![0.0; classes];
    d.iter().for_each(|v| per_area[((v / width) as usize).min(classes - 1)] += 1.0 / area);
    // A sphere of class j (diameter j x width) shows a section of class i when its centre lies
    // in a slab of this thickness.
    let slab = |i: usize, j: usize| width * ((((j * j) as f64) - ((i - 1) * (i - 1)) as f64).sqrt() - (((j * j) as f64) - (i * i) as f64).max(0.0).sqrt());
    let mut per_volume = vec![0.0; classes];
    for j in (1..=classes).rev() {
        let mut left = per_area[j - 1];
        for k in j + 1..=classes {
            left -= per_volume[k - 1] * slab(j, k);
        }
        per_volume[j - 1] = (left / slab(j, j)).max(0.0);
    }
    let total: f64 = per_volume.iter().sum();
    let diameter = |j: usize| (j as f64 + 0.5) * width;
    let mut t = table(vec![
        ("Diameter", (0..classes).map(diameter).collect()),
        ("SectionsPerArea", per_area),
        ("SpheresPerVolume", per_volume.clone()),
        ("Share", per_volume.iter().map(|v| if total > 0.0 { v / total } else { 0.0 }).collect()),
    ]);
    t.field_data.insert(Array::scalars_f64("SpheresPerVolume", vec![total]));
    t.field_data.insert(Array::scalars_f64(
        "MeanDiameter",
        vec![if total > 0.0 { (0..classes).map(|j| per_volume[j] * diameter(j)).sum::<f64>() / total } else { f64::NAN }],
    ));
    t.field_data.insert(Array::scalars_f64("VolumeFraction", vec![(0..classes).map(|j| per_volume[j] * PI / 6.0 * ((j + 1) as f64 * width).powi(3)).sum()]));
    Ok(DataSet::Table(t))
}

/// Split Touching Particles: the cells of a phase are given to the particle whose core they
/// drain to, the cores being the places farthest from the boundary. Two cores are one
/// particle when the neck between them is less than `depth` narrower than the smaller core.
pub fn split_particles(ds: &DataSet, array: &str, phase: i32, depth: f64) -> Result<DataSet> {
    let mut m = Cells::of(ds, array)?;
    let mask = m.mask(phase)?;
    let rad = inscribed_radius(&m, &mask)?;
    let mut order: Vec<u32> = (0..m.len() as u32).filter(|c| mask[*c as usize]).collect();
    order.sort_by(|a, b| rad[*b as usize].total_cmp(&rad[*a as usize]).then(a.cmp(b)));
    let offs: Vec<[i64; 3]> = {
        let mut o = forward_offsets(m.n, 26);
        o.extend(o.clone().iter().map(|v| v.map(|x| -x)));
        o
    };
    // Basins: each with the radius at its core; a cell joins the deepest basin it touches.
    let mut basin = vec![u32::MAX; m.len()];
    let mut parent: Vec<u32> = Vec::new();
    let mut peak: Vec<f64> = Vec::new();
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    for &c in &order {
        let c = c as usize;
        let p = m.ijk(c);
        let mut near: Vec<u32> = Vec::new();
        // Uphill: the basin of the highest neighbour already placed.
        let mut up: Option<(f64, usize)> = None;
        for o in &offs {
            if let Some(q) = offset(m.n, p, *o, false) {
                if basin[q] != u32::MAX {
                    let root = find(&mut parent, basin[q]);
                    if !near.contains(&root) {
                        near.push(root);
                    }
                    if up.is_none_or(|(r, k)| rad[q] > r || (rad[q] == r && q < k)) {
                        up = Some((rad[q], q));
                    }
                }
            }
        }
        let Some((_, uphill)) = up else {
            basin[c] = parent.len() as u32;
            parent.push(parent.len() as u32);
            peak.push(rad[c]);
            continue;
        };
        let mut mine = find(&mut parent, basin[uphill]);
        for other in near {
            let other = find(&mut parent, other);
            if other != mine && peak[other as usize].min(peak[mine as usize]) - rad[c] < depth {
                // One particle: the two basins become one, under the deeper core.
                let (keep, gone) = if peak[other as usize] > peak[mine as usize] { (other, mine) } else { (mine, other) };
                parent[gone as usize] = keep;
                mine = keep;
            }
        }
        basin[c] = mine;
    }
    let mut number = vec![0i32; parent.len()];
    let mut count = 0;
    let ids: Vec<i32> = (0..m.len())
        .map(|c| {
            if basin[c] == u32::MAX {
                return 0;
            }
            let root = find(&mut parent, basin[c]) as usize;
            if number[root] == 0 {
                count += 1;
                number[root] = count;
            }
            number[root]
        })
        .collect();
    m.img.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
    note(&mut m.img, "Particles", vec![count as f64]);
    Ok(DataSet::ImageData(m.img))
}

/// Stretch Microstructure: the structure stretched by a factor along each of three axes and
/// turned (Bunge angles, degrees), on the same grid. What the stretch brings in from beyond
/// the box is the structure repeated (`wrap`, for periodic structures) or mirrored.
pub fn stretch(ds: &DataSet, factors: [f64; 3], rotation: [f64; 3], wrap: bool) -> Result<DataSet> {
    let DataSet::ImageData(img) = ds else { bail!("the input must be an image (a grid of cells)") };
    if factors.iter().any(|s| !(*s > 0.0)) {
        bail!("the stretches must be above 0");
    }
    let v = crate::features::Vox::of(img);
    let [a, b, c] = rotation.map(f64::to_radians);
    let (s1, c1, s, cc, s2, c2) = (a.sin(), a.cos(), b.sin(), b.cos(), c.sin(), c.cos());
    let turn = [[c1 * c2 - s1 * s2 * cc, -c1 * s2 - s1 * c2 * cc, s1 * s], [s1 * c2 + c1 * s2 * cc, -s1 * s2 + c1 * c2 * cc, -c1 * s], [s2 * s, c2 * s, cc]];
    let size: [f64; 3] = std::array::from_fn(|k| v.n[k] as f64 * v.sp[k]);
    let source: Vec<usize> = (0..v.len())
        .into_par_iter()
        .map(|cell| {
            let p = v.ijk(cell);
            let x: [f64; 3] = std::array::from_fn(|k| (p[k] as f64 + 0.5) * v.sp[k] - 0.5 * size[k]);
            // Back through the turn, then the stretch.
            let y: [f64; 3] = std::array::from_fn(|k| (turn[0][k] * x[0] + turn[1][k] * x[1] + turn[2][k] * x[2]) / factors[k]);
            let z: [f64; 3] = std::array::from_fn(|k| turn[k][0] * y[0] + turn[k][1] * y[1] + turn[k][2] * y[2] + 0.5 * size[k]);
            let q: [usize; 3] = std::array::from_fn(|k| {
                if v.n[k] == 1 {
                    return 0;
                }
                let i = (z[k] / v.sp[k]).floor() as i64;
                let n = v.n[k] as i64;
                if wrap {
                    i.rem_euclid(n) as usize
                } else {
                    let j = i.rem_euclid(2 * n);
                    (if j < n { j } else { 2 * n - 1 - j }) as usize
                }
            });
            q[0] + v.n[0] * (q[1] + v.n[1] * q[2])
        })
        .collect();
    let mut out = img.clone();
    out.cell_data = img.cell_data.gather(&source);
    // What described the structure before no longer does.
    for name in ["Shapes", "Centers", "Radii", "ExpectedFraction", "ExpectedSpecificSurface", "ExpectedTwoPoint"] {
        out.field_data.remove(name);
    }
    if let Some(p) = out.cell_data.get("Phases") {
        let phases: Vec<i32> = (0..p.len()).map(|c| p.get(c, 0) as i32).collect();
        refresh_fractions(&mut out, phases);
    }
    Ok(DataSet::ImageData(out))
}
