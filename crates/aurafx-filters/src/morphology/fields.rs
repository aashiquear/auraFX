//! Random sets made through random functions: excursions of Gaussian fields, dead leaves
//! (opaque and transparent), and a medium annealed to a two-point probability function.

use super::*;
use crate::synthetic::{grain_orientations, Draws, Rng};
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::Laue;

/// White noise smoothed by a Gaussian of `sigma` cells along every axis that has more than
/// one cell: a Gaussian random field of covariance `exp(-(d / 2 sigma)^2)` at the distance
/// `d` cells. In a box that is not periodic the noise is drawn on a wider grid and cropped.
pub fn gaussian_field(g: &Grid, sigma: f64, r: &mut dyn Draws) -> Vec<f32> {
    let reach = (3.0 * sigma).ceil().max(1.0) as usize;
    let pad: [usize; 3] = std::array::from_fn(|k| if g.periodic || g.n[k] == 1 { 0 } else { reach });
    let n: [usize; 3] = std::array::from_fn(|k| g.n[k] + 2 * pad[k]);
    let mut field: Vec<f32> = (0..n[0] * n[1] * n[2]).map(|_| r.std_normal() as f32).collect();
    let kernel: Vec<f32> = {
        let k: Vec<f64> = (-(reach as i64)..=reach as i64).map(|i| (-0.5 * (i as f64 / sigma.max(1e-9)).powi(2)).exp()).collect();
        let s: f64 = k.iter().sum();
        k.iter().map(|x| (x / s) as f32).collect()
    };
    let stride = [1, n[0], n[0] * n[1]];
    for axis in 0..3 {
        if n[axis] == 1 {
            continue;
        }
        let src = field;
        field = (0..src.len())
            .into_par_iter()
            .map(|c| {
                let i = (c / stride[axis]) % n[axis];
                let base = c - i * stride[axis];
                kernel
                    .iter()
                    .enumerate()
                    .map(|(k, w)| {
                        let j = (i as i64 + k as i64 - reach as i64).rem_euclid(n[axis] as i64) as usize;
                        w * src[base + j * stride[axis]]
                    })
                    .sum()
            })
            .collect();
    }
    if pad == [0; 3] {
        return field;
    }
    (0..g.len())
        .map(|c| {
            let p = g.ijk(c);
            field[(p[0] + pad[0]) + n[0] * ((p[1] + pad[1]) + n[1] * (p[2] + pad[2]))]
        })
        .collect()
}

pub const EXCURSION_ABOVE: &str = "One side of a level";
pub const EXCURSION_BAND: &str = "Between two levels";

/// The excursion set of a Gaussian random field: the solid where the field passes a level
/// (or lies between two levels about its mean: sheets), the level set by the volume fraction.
/// `length` is the distance over which the covariance `exp(-(d / length)^2)` falls to 1/e.
pub fn gaussian_field_medium(g: &Grid, length: f64, fraction: f64, kind: &str, seed: u64) -> Result<DataSet> {
    if !(0.0..=1.0).contains(&fraction) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let mut r = Rng::new(seed);
    let field = gaussian_field(g, 0.5 * length / g.h, &mut r);
    let scale = (field.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / field.len() as f64).sqrt().max(1e-30) as f32;
    let field: Vec<f32> = field.iter().map(|x| x / scale).collect();
    let phases =
        if kind == EXCURSION_BAND { below_quantile(&field.iter().map(|x| x.abs()).collect::<Vec<_>>(), fraction) } else { below_quantile(&field, fraction) };
    let mut img = phase_image(g, phases, None);
    if kind != EXCURSION_BAND && fraction > 0.0 && fraction < 1.0 {
        // The level in standard deviations, and what the field then gives exactly: two points
        // a distance apart both under the level, and the surface of the level set.
        let level = quantile(&field, fraction) as f64;
        let reach = g.n.iter().filter(|n| **n > 1).min().copied().unwrap_or(1) / 2;
        let two: Vec<f64> = (0..=reach)
            .map(|k| {
                let rho = (-(k as f64 * g.h / length).powi(2)).exp();
                let top = rho.min(1.0).asin();
                let steps = 200;
                let sum: f64 = (0..steps).map(|i| (-level * level / (1.0 + ((i as f64 + 0.5) * top / steps as f64).sin())).exp()).sum();
                fraction * fraction + sum * top / steps as f64 / std::f64::consts::TAU
            })
            .collect();
        note(&mut img, "ExpectedFraction", vec![fraction]);
        note(&mut img, "ExpectedTwoPoint", two);
        if !g.planar() {
            note(&mut img, "ExpectedSpecificSurface", vec![2.0 / std::f64::consts::PI * 2f64.sqrt() / length * (-0.5 * level * level).exp()]);
        }
    }
    img.cell_data.insert(Array::new("Field", 1, ArrayData::F32(field)));
    Ok(DataSet::ImageData(img))
}

pub struct LeavesParams {
    pub radius: f64,
    pub sigma: f64,
    /// Long axis over the two others (1: spheres; above 1 prolate, below 1 flat).
    pub aspect: f64,
    /// Volume fraction of each phase of leaves.
    pub phase_fractions: Vec<f64>,
    /// Radius of the leaves of each phase (empty: `radius` for all).
    pub phase_radii: Vec<f64>,
    /// How many leaves cover a point on average; 0: leaves fall until every cell is covered.
    pub coverage: f64,
    pub orientations: String,
    pub spread_deg: f64,
    pub structure: String,
    pub seed: u64,
}

/// One leaf: its centre, the half-length along its axis, the radius across and the axis.
struct Leaf {
    c: [f64; 3],
    along: f64,
    across: f64,
    axis: [f64; 3],
}

fn leaf(g: &Grid, radius: f64, sigma: f64, aspect: f64, r: &mut dyn Draws) -> Leaf {
    let across = lognormal(radius, sigma, r);
    let along = across * aspect.max(1e-3);
    let reach = along.max(across);
    let mut c = g.random_point_around(reach, r);
    let mut axis = if aspect == 1.0 { [0.0, 0.0, 1.0] } else { isotropic(r) };
    if g.planar() {
        c[2] = 0.5 * g.h;
        axis = unit([axis[0], axis[1], 0.0]);
    }
    Leaf { c, along, across, axis }
}

fn stamp_leaf(g: &Grid, l: &Leaf, mut f: impl FnMut(usize)) {
    let reach = l.along.max(l.across);
    g.stamp(l.c.map(|x| x - reach), l.c.map(|x| x + reach), |cell, p| {
        let q = sub(p, l.c);
        let a = dot(q, l.axis);
        let across2 = dot(q, q) - a * a;
        if a * a / (l.along * l.along) + across2 / (l.across * l.across) <= 1.0 {
            f(cell);
        }
    });
}

/// Dead leaves: grains (spheres or spheroids) fall one after another and hide what lies
/// under them; what shows at the end is a mosaic of grains that are not convex. Made from
/// the top down: a cell belongs to the first leaf that reaches it, so the leaves stop when
/// every cell is taken. Every leaf is a feature with a phase and an orientation; with a
/// coverage, the cells no leaf reached are a background, the last feature, of the phase
/// after those of the leaves.
pub fn dead_leaves(g: &Grid, p: &LeavesParams) -> Result<DataSet> {
    let laue = Laue::from_label(&p.structure).ok_or_else(|| anyhow!("unknown crystal structure {}", p.structure))?;
    let fractions: Vec<f64> = if p.phase_fractions.is_empty() { vec![1.0] } else { p.phase_fractions.iter().map(|x| x.max(0.0)).collect() };
    let total: f64 = fractions.iter().sum();
    if !(total > 0.0) {
        bail!("the phase fractions must not all be 0");
    }
    let radius_of = |k: usize| p.phase_radii.get(k).copied().filter(|x| *x > 0.0).unwrap_or(p.radius);
    // A phase shows in proportion to the number of its leaves times their volume.
    let dim = if g.planar() { 2 } else { 3 };
    let weights: Vec<f64> = fractions.iter().enumerate().map(|(k, f)| f / radius_of(k).powi(dim)).collect();
    let wsum: f64 = weights.iter().sum();
    let mut r = Rng::new(p.seed);
    let mut ids = vec![0i32; g.len()];
    let mut left = g.len();
    let mut leaf_phase: Vec<i32> = Vec::new();
    let limit = if p.coverage > 0.0 {
        // The mean volume of a leaf over the phases, for the number that covers `coverage` times.
        let unit_volume = if g.planar() { std::f64::consts::PI * g.h } else { 4.0 / 3.0 * std::f64::consts::PI };
        let mean: f64 =
            (0..fractions.len()).map(|k| weights[k] / wsum * unit_volume * p.aspect.max(1e-3) * lognormal_moment(radius_of(k), p.sigma, dim as f64)).sum();
        let reach = p.radius * (3.0 * p.sigma).exp() * p.aspect.max(1.0);
        poisson(p.coverage * g.volume_around(reach) / mean, &mut r)
    } else {
        usize::MAX
    };
    let mut fallen = 0usize;
    while left > 0 && fallen < limit {
        fallen += 1;
        if fallen > 200_000_000 {
            bail!("the leaves do not cover the box: raise their radius");
        }
        let mut pick = r.random() * wsum;
        let mut phase = 0;
        for (k, w) in weights.iter().enumerate() {
            phase = k;
            if pick < *w {
                break;
            }
            pick -= w;
        }
        let l = leaf(g, radius_of(phase), p.sigma, p.aspect, &mut r);
        let id = leaf_phase.len() as i32 + 1;
        let mut took = false;
        stamp_leaf(g, &l, |cell| {
            if ids[cell] == 0 {
                ids[cell] = id;
                left -= 1;
                took = true;
            }
        });
        if took {
            leaf_phase.push(phase as i32 + 1);
        }
    }
    let background = left > 0;
    if background {
        let id = leaf_phase.len() as i32 + 1;
        ids.iter_mut().filter(|i| **i == 0).for_each(|i| *i = id);
        leaf_phase.push(fractions.len() as i32 + 1);
    }
    let orients = grain_orientations(leaf_phase.len(), &p.orientations, p.spread_deg.to_radians(), &mut r);
    let phases: Vec<i32> = ids.iter().map(|i| leaf_phase[(*i - 1) as usize]).collect();
    let mut img = crate::synthetic::microstructure(g.n, [g.h; 3], ids, &orients, laue);
    let top = fractions.len() + background as usize;
    let mut count = vec![0usize; top + 1];
    phases.iter().for_each(|x| count[*x as usize] += 1);
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
    let mut classes = vec![Laue::UNKNOWN_CODE];
    classes.extend(std::iter::repeat_n(laue.code(), top));
    img.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(classes)));
    note(&mut img, "PhaseFractions", count.iter().map(|c| *c as f64 / g.len() as f64).collect());
    note(&mut img, "Leaves", vec![fallen as f64]);
    Ok(DataSet::ImageData(img))
}

/// Transparent leaves: every leaf that falls carries a random value and lets the fraction
/// `transparency` of what lies under it show through, so the value of a point is a weighted
/// mean of the leaves over it. The solid is where this random function is below a level, set
/// by the volume fraction.
pub fn transparent_leaves_medium(g: &Grid, radius: f64, sigma: f64, transparency: f64, fraction: f64, seed: u64) -> Result<DataSet> {
    if !(0.0..1.0).contains(&transparency) {
        bail!("the transparency must be at least 0 and below 1");
    }
    if !(0.0..=1.0).contains(&fraction) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let mut r = Rng::new(seed);
    // Enough leaves that the first values are hidden: each layer leaves `transparency` of them.
    let layers = if transparency > 0.0 { (0.001f64.ln() / transparency.ln()).max(1.0) } else { 1.0 } + 12.0;
    let dim = if g.planar() { 2.0 } else { 3.0 };
    let unit_volume = if g.planar() { std::f64::consts::PI * g.h } else { 4.0 / 3.0 * std::f64::consts::PI };
    let reach = radius * (3.0 * sigma).exp();
    let count = poisson(layers * g.volume_around(reach) / (unit_volume * lognormal_moment(radius, sigma, dim)), &mut r);
    let mut field = vec![0.5f32; g.len()];
    let t = transparency as f32;
    for _ in 0..count {
        let l = leaf(g, radius, sigma, 1.0, &mut r);
        let value = r.random() as f32;
        stamp_leaf(g, &l, |cell| field[cell] = t * field[cell] + (1.0 - t) * value);
    }
    let phases = below_quantile(&field, fraction);
    let mut img = phase_image(g, phases, None);
    img.cell_data.insert(Array::new("Field", 1, ArrayData::F32(field)));
    Ok(DataSet::ImageData(img))
}

/// The two-point counts of a 0/1 image along its axes, for distances 1 to `reach` cells: the
/// number of pairs of cells that distance apart (through the periodic faces) both 1.
fn pair_counts(state: &[u8], n: [usize; 3], axes: &[usize], reach: usize) -> Vec<Vec<f64>> {
    let stride = [1, n[0], n[0] * n[1]];
    axes.iter()
        .map(|&a| {
            (1..=reach)
                .into_par_iter()
                .map(|d| {
                    (0..state.len())
                        .filter(|&c| {
                            if state[c] == 0 {
                                return false;
                            }
                            let i = (c / stride[a]) % n[a];
                            state[c - i * stride[a] + ((i + d) % n[a]) * stride[a]] == 1
                        })
                        .count() as f64
                })
                .collect()
        })
        .collect()
}

/// For one line of cells (through its periodic end), the number of places where a segment
/// of `d` cells more than one lies wholly in the phase, for `d` from 1 to `reach`: added to
/// `out` with the weight `sign`.
fn line_segments(state: &[u8], base: usize, stride: usize, n: usize, sign: f64, out: &mut [f64]) {
    let at = |i: usize| state[base + (i % n) * stride];
    let Some(start) = (0..n).find(|i| at(*i) == 0) else {
        out.iter_mut().for_each(|v| *v += sign * n as f64);
        return;
    };
    let mut run = 0usize;
    for k in 1..=n {
        if at(start + k) == 1 {
            run += 1;
        } else {
            for (d, v) in out.iter_mut().enumerate().take(run.saturating_sub(1)) {
                *v += sign * (run - d - 1) as f64;
            }
            run = 0;
        }
    }
}

/// What an annealed medium is brought to: for every axis, the number of pairs of cells of
/// the phase at each distance and, when the lineal path is asked too, the number of
/// segments of each length that lie wholly in the phase.
pub struct Target {
    pub axes: Vec<usize>,
    pub reach: usize,
    pub pairs: Vec<Vec<f64>>,
    pub segments: Option<Vec<Vec<f64>>>,
}

/// Cells of the phase swapped with cells outside it, a swap kept when it brings the counts
/// nearer to the target and, less and less often as the annealing cools, when it takes them
/// away. Returns the state, the distance left and the pair counts reached.
fn anneal(n: [usize; 3], ones: usize, target: &Target, sweeps: f64, r: &mut dyn Draws) -> (Vec<u8>, f64, Vec<Vec<f64>>) {
    let total = n[0] * n[1] * n[2];
    let (axes, reach) = (&target.axes, target.reach);
    let mut state = vec![0u8; total];
    let mut placed = 0;
    while placed < ones {
        let c = (r.random() * total as f64) as usize % total;
        if state[c] == 0 {
            state[c] = 1;
            placed += 1;
        }
    }
    let stride = [1, n[0], n[0] * n[1]];
    let mut counts = pair_counts(&state, n, axes, reach);
    let lineal = target.segments.is_some();
    let mut paths: Vec<Vec<f64>> = vec![vec![0.0; reach]; axes.len()];
    if lineal {
        for (k, &a) in axes.iter().enumerate() {
            for c in (0..total).filter(|c| (c / stride[a]) % n[a] == 0) {
                line_segments(&state, c, stride[a], n[a], 1.0, &mut paths[k]);
            }
        }
    }
    let norm2 = (total as f64).powi(2);
    let distance = |counts: &[Vec<f64>], extra: &[Vec<f64>], goal: &[Vec<f64>]| -> f64 {
        counts.iter().zip(extra).zip(goal).map(|((c, d), t)| c.iter().zip(d).zip(t).map(|((c, d), t)| (c + d - t).powi(2)).sum::<f64>()).sum::<f64>() / norm2
    };
    // The change of the pair counts when the cell `c` changes (the state not yet changed).
    let change = |state: &[u8], c: usize, sign: f64, d: &mut [Vec<f64>]| {
        for (k, &a) in axes.iter().enumerate() {
            let i = (c / stride[a]) % n[a];
            let base = c - i * stride[a];
            for dist in 1..=reach {
                let up = state[base + ((i + dist) % n[a]) * stride[a]];
                let down = state[base + ((i + n[a] - dist) % n[a]) * stride[a]];
                d[k][dist - 1] += sign * (up + down) as f64;
            }
        }
    };
    let lines = |state: &[u8], c: usize, sign: f64, d: &mut [Vec<f64>]| {
        for (k, &a) in axes.iter().enumerate() {
            let i = (c / stride[a]) % n[a];
            line_segments(state, c - i * stride[a], stride[a], n[a], sign, &mut d[k]);
        }
    };
    let zero = vec![vec![0.0; reach]; axes.len()];
    let energy = |counts: &[Vec<f64>], d: &[Vec<f64>], paths: &[Vec<f64>], dp: &[Vec<f64>]| -> f64 {
        distance(counts, d, &target.pairs) + target.segments.as_ref().map(|t| distance(paths, dp, t)).unwrap_or(0.0)
    };
    let mut e = energy(&counts, &zero, &paths, &zero);
    let trials = (sweeps.max(0.0) * total as f64) as usize;
    let mut temperature = 0.0;
    let mut warm = Vec::new();
    let mut delta = zero.clone();
    let mut dpath = zero.clone();
    for k in 0..trials {
        let (a, b) = loop {
            let a = (r.random() * total as f64) as usize % total;
            let b = (r.random() * total as f64) as usize % total;
            if state[a] == 1 && state[b] == 0 {
                break (a, b);
            }
        };
        delta.iter_mut().for_each(|d| d.fill(0.0));
        if lineal {
            dpath.iter_mut().for_each(|d| d.fill(0.0));
            lines(&state, a, -1.0, &mut dpath);
            lines(&state, b, -1.0, &mut dpath);
        }
        change(&state, a, -1.0, &mut delta);
        state[a] = 0;
        change(&state, b, 1.0, &mut delta);
        state[b] = 1;
        if lineal {
            lines(&state, a, 1.0, &mut dpath);
            lines(&state, b, 1.0, &mut dpath);
            // Both cells on one line: that line was taken off and put back twice.
            for (k, &ax) in axes.iter().enumerate() {
                let (ia, ib) = ((a / stride[ax]) % n[ax], (b / stride[ax]) % n[ax]);
                if a - ia * stride[ax] == b - ib * stride[ax] {
                    dpath[k].iter_mut().for_each(|v| *v *= 0.5);
                }
            }
        }
        let e_new = energy(&counts, &delta, &paths, &dpath);
        let de = e_new - e;
        if k < 200 {
            // The first swaps set the starting temperature: about half the bad ones pass.
            warm.push(de.abs());
            temperature = warm.iter().sum::<f64>() / warm.len() as f64;
        }
        let t = temperature * 1e-4f64.powf(k as f64 / trials as f64);
        if de <= 0.0 || (t > 0.0 && r.random() < (-de / t).exp()) {
            counts.iter_mut().zip(&delta).for_each(|(c, d)| c.iter_mut().zip(d).for_each(|(c, d)| *c += d));
            if lineal {
                paths.iter_mut().zip(&dpath).for_each(|(c, d)| c.iter_mut().zip(d).for_each(|(c, d)| *c += d));
            }
            e = e_new;
        } else {
            state[a] = 1;
            state[b] = 0;
        }
    }
    (state, e, counts)
}

/// A medium rebuilt from a two-point probability function. The cells start at random at the
/// volume fraction, and two cells of different phases are swapped again and again; a swap
/// that brings the two-point probability along the axes nearer to the target is kept, one
/// that takes it away is kept less and less often (simulated annealing). The target is that
/// of a medium with no characteristic shape, `S2(d) = f^2 + f (1 - f) exp(-d / length)`.
pub fn reconstructed_medium(g: &Grid, fraction: f64, length: f64, sweeps: f64, seed: u64) -> Result<DataSet> {
    if !(fraction > 0.0 && fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let n = g.n;
    let total = g.len();
    if total > 8_000_000 {
        bail!("the reconstruction is limited to 8 million cells");
    }
    let axes: Vec<usize> = (0..3).filter(|a| n[*a] > 3).collect();
    if axes.is_empty() {
        bail!("the grid is too small");
    }
    let least = axes.iter().map(|a| n[*a]).min().unwrap();
    let reach = (((4.0 * length / g.h).ceil() as usize).max(4)).min((least - 1) / 2);
    let mut r = Rng::new(seed);
    let ones = ((fraction * total as f64).round() as usize).clamp(1, total - 1);
    let f = ones as f64 / total as f64;
    let pairs: Vec<f64> = (1..=reach).map(|d| total as f64 * (f * f + f * (1.0 - f) * (-(d as f64) * g.h / length).exp())).collect();
    let target = Target { pairs: vec![pairs.clone(); axes.len()], axes, reach, segments: None };
    let (state, e, counts) = anneal(n, ones, &target, sweeps, &mut r);
    let phases: Vec<i32> = state.iter().map(|s| if *s == 1 { PHASE_SOLID } else { PHASE_MATRIX }).collect();
    let mut img = phase_image(g, phases, None);
    note(&mut img, "Energy", vec![e]);
    note(&mut img, "TwoPointTarget", pairs.iter().map(|t| t / total as f64).collect());
    note(&mut img, "TwoPointResult", (0..reach).map(|d| counts.iter().map(|c| c[d]).sum::<f64>() / (counts.len() * total) as f64).collect());
    Ok(DataSet::ImageData(img))
}

/// Reconstruct Microstructure: a new medium of any size with the statistics of a phase of
/// an image: its volume fraction, the probability that two points a distance apart along
/// each axis both lie in it and, when asked, that the whole segment between them does. From
/// a single layer of cells (a section) a volume is made, the third axis taking the mean of
/// the two measured ones.
#[allow(clippy::too_many_arguments)]
pub fn reconstruct(ds: &DataSet, array: &str, phase: i32, cells: &[f64], lineal: bool, reach: usize, sweeps: f64, seed: u64) -> Result<DataSet> {
    let m = super::measure::Cells::of(ds, array)?;
    let mask: Vec<u8> = m.mask(phase)?.iter().map(|x| *x as u8).collect();
    let g = Grid::new(if cells.is_empty() { &[40.0, 40.0, 40.0] } else { cells }, m.sp[0], true)?;
    let n = g.n;
    let total = g.len();
    if total > 8_000_000 {
        bail!("the reconstruction is limited to 8 million cells");
    }
    let axes: Vec<usize> = (0..3).filter(|a| n[*a] > 3).collect();
    let measured: Vec<usize> = (0..3).filter(|a| m.n[*a] > 3).collect();
    if axes.is_empty() || measured.is_empty() {
        bail!("the grid is too small");
    }
    let least = axes.iter().map(|a| n[*a]).chain(measured.iter().map(|a| m.n[*a])).min().unwrap();
    let reach = if reach == 0 { least / 3 } else { reach }.clamp(2, (least - 1) / 2);
    let fraction = mask.iter().filter(|x| **x == 1).count() as f64 / mask.len() as f64;
    let ones = ((fraction * total as f64).round() as usize).clamp(1, total - 1);
    // The probabilities measured in the image along each of its axes, without its faces wrapped.
    let stride = [1, m.n[0], m.n[0] * m.n[1]];
    let places = |a: usize, d: usize| (mask.len() / m.n[a] * (m.n[a] - d)) as f64;
    let two = |a: usize| -> Vec<f64> {
        (1..=reach)
            .map(|d| {
                (0..mask.len()).filter(|c| mask[*c] == 1 && (c / stride[a]) % m.n[a] + d < m.n[a] && mask[c + d * stride[a]] == 1).count() as f64 / places(a, d)
            })
            .collect()
    };
    let path = |a: usize| -> Vec<f64> {
        let mut out = vec![0.0; reach];
        // Runs along the lines: a run of `len` cells holds `len - d - 1` segments of `d + 2` cells.
        let mut run = |len: usize| {
            for (d, v) in out.iter_mut().enumerate().take(len.saturating_sub(1)) {
                *v += (len - d - 1) as f64;
            }
        };
        for start in (0..mask.len()).filter(|c| (c / stride[a]) % m.n[a] == 0) {
            let mut len = 0;
            for i in 0..m.n[a] {
                if mask[start + i * stride[a]] == 1 {
                    len += 1;
                } else {
                    run(len);
                    len = 0;
                }
            }
            run(len);
        }
        out.iter().enumerate().map(|(d, v)| v / places(a, d + 1)).collect()
    };
    let mean = |f: &dyn Fn(usize) -> Vec<f64>| -> Vec<f64> {
        let all: Vec<Vec<f64>> = measured.iter().map(|a| f(*a)).collect();
        (0..reach).map(|d| all.iter().map(|v| v[d]).sum::<f64>() / all.len() as f64).collect()
    };
    let per_axis = |f: &dyn Fn(usize) -> Vec<f64>| -> Vec<Vec<f64>> {
        let fallback = mean(f);
        axes.iter().map(|a| if measured.contains(a) { f(*a) } else { fallback.clone() }.iter().map(|p| p * total as f64).collect()).collect()
    };
    let target = Target { pairs: per_axis(&two), segments: if lineal { Some(per_axis(&path)) } else { None }, axes: axes.clone(), reach };
    let mut r = Rng::new(seed);
    let (state, e, counts) = anneal(n, ones, &target, sweeps, &mut r);
    let phases: Vec<i32> = state.iter().map(|s| if *s == 1 { PHASE_SOLID } else { PHASE_MATRIX }).collect();
    let mut img = phase_image(&g, phases, None);
    note(&mut img, "Energy", vec![e]);
    note(&mut img, "TwoPointTarget", (0..reach).map(|d| target.pairs.iter().map(|c| c[d]).sum::<f64>() / (axes.len() * total) as f64).collect());
    note(&mut img, "TwoPointResult", (0..reach).map(|d| counts.iter().map(|c| c[d]).sum::<f64>() / (axes.len() * total) as f64).collect());
    Ok(DataSet::ImageData(img))
}
