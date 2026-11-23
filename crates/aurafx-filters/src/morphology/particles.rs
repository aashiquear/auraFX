//! Spheres: overlapping at random (the Boolean model), penetrable with a hard core, hard
//! (random sequential addition and dense packings), and clustered in the aggregates of
//! another set of spheres (a Cox process).

use super::*;
use crate::synthetic::{Draws, Rng};
use anyhow::{bail, Result};

const SPHERE: f64 = 4.0 / 3.0 * std::f64::consts::PI;

/// Phases and FeatureIds of spheres: a cell belongs to the first sphere that holds it.
pub fn sphere_cells(g: &Grid, centres: &[[f64; 3]], radii: &[f64]) -> (Vec<i32>, Vec<i32>) {
    let mut phases = vec![PHASE_MATRIX; g.len()];
    let mut ids = vec![0i32; g.len()];
    for (k, (c, rad)) in centres.iter().zip(radii).enumerate() {
        g.stamp_sphere(*c, *rad, |cell, _| {
            if ids[cell] == 0 {
                ids[cell] = k as i32 + 1;
                phases[cell] = PHASE_SOLID;
            }
        });
    }
    (phases, ids)
}

fn keep_spheres(img: &mut ImageData, centres: &[[f64; 3]], radii: &[f64]) {
    let mut c = vec![0.0; 3];
    c.extend(centres.iter().flatten());
    let mut rr = vec![0.0];
    rr.extend(radii);
    img.field_data.insert(Array::new("Centers", 3, ArrayData::F64(c)));
    img.field_data.insert(Array::scalars_f64("Radii", rr));
}

/// Overlapping spheres: centres of a Poisson point process, the union of the spheres as the
/// solid. Its volume fraction is `1 - exp(-theta V)` for `theta` centres per unit volume and
/// spheres of mean volume `V`, which gives the number of centres for a fraction.
pub fn overlapping_spheres(g: &Grid, radius: f64, sigma: f64, fraction: f64, seed: u64) -> Result<DataSet> {
    if !(0.0..1.0).contains(&fraction) {
        bail!("the volume fraction must be at least 0 and below 1");
    }
    let mut r = Rng::new(seed);
    let mean_volume =
        if g.planar() { g.h * std::f64::consts::PI * lognormal_moment(radius, sigma, 2.0) } else { SPHERE * lognormal_moment(radius, sigma, 3.0) };
    let theta = -(1.0 - fraction).ln() / mean_volume;
    let margin = radius * (3.0 * sigma).exp();
    let count = poisson(theta * g.volume_around(margin), &mut r);
    let radii: Vec<f64> = (0..count).map(|_| lognormal(radius, sigma, &mut r)).collect();
    let centres: Vec<[f64; 3]> = (0..count).map(|_| planar_point(g, g.random_point_around(margin, &mut r))).collect();
    let (phases, ids) = sphere_cells(g, &centres, &radii);
    let mut img = phase_image(g, phases, Some(ids));
    note(&mut img, "ExpectedFraction", vec![fraction]);
    keep_spheres(&mut img, &centres, &radii);
    Ok(DataSet::ImageData(img))
}

/// In one layer of cells the centres lie in its middle plane: discs.
fn planar_point(g: &Grid, mut p: [f64; 3]) -> [f64; 3] {
    if g.planar() {
        p[2] = 0.5 * g.h;
    }
    p
}

/// Random sequential addition: the spheres are placed one after another (give the radii
/// largest first) at points drawn at random, a point being drawn again when the sphere would
/// come nearer than `gap` to one already placed. Returns the centres placed, fewer than the
/// radii when a sphere found no place after `attempts` draws (jamming).
pub fn sequential_addition(g: &Grid, radii: &[f64], gap: f64, attempts: usize, r: &mut dyn Draws) -> Vec<[f64; 3]> {
    let rmax = radii.iter().cloned().fold(0.0, f64::max);
    let mut buckets = Buckets::new(g, 2.0 * rmax + gap);
    let mut centres: Vec<[f64; 3]> = Vec::with_capacity(radii.len());
    for (k, rad) in radii.iter().enumerate() {
        let mut placed = false;
        for _ in 0..attempts {
            let p = planar_point(g, g.random_point(r));
            let mut free = true;
            buckets.near(p, |j| {
                let reach = rad + radii[j] + gap;
                if free && g.dist2(p, centres[j]) < reach * reach {
                    free = false;
                }
            });
            if free {
                buckets.insert(p, k);
                centres.push(p);
                placed = true;
                break;
            }
        }
        if !placed {
            break;
        }
    }
    centres
}

/// A dense packing: the spheres are placed small, without overlap, then grown step by step
/// to their sizes; after each step the pairs that overlap are pushed apart until none does.
/// The growth slows down where the pushing does not end, and stops with an error when the
/// spheres cannot be packed to their sizes.
pub fn dense_packing(g: &Grid, radii: &[f64], gap: f64, rate: f64, r: &mut dyn Draws) -> Result<Vec<[f64; 3]>> {
    let n = radii.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    let volume = |x: f64| if g.planar() { std::f64::consts::PI * x * x * g.h } else { SPHERE * x * x * x };
    let goal: f64 = radii.iter().map(|x| volume(*x)).sum::<f64>() / g.volume();
    let dim = if g.planar() { 2.0 } else { 3.0 };
    // Start where random sequential addition places every sphere easily.
    let mut s = (0.12 / goal).powf(1.0 / dim).min(1.0);
    let start: Vec<f64> = radii.iter().map(|x| x * s).collect();
    let mut centres = sequential_addition(g, &start, gap, 20_000, r);
    if centres.len() < n {
        bail!("the spheres do not fit in the box, even small");
    }
    let rmax = radii.iter().cloned().fold(0.0, f64::max);
    let rmin = radii.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut rate = rate.clamp(1e-4, 0.2);
    while s < 1.0 {
        let next = (s * (1.0 + rate)).min(1.0);
        let saved = centres.clone();
        if push_apart(g, &mut centres, radii, next, gap, rmax, 1e-5 * rmin, 600, r) {
            s = next;
        } else {
            centres = saved;
            rate *= 0.5;
            if rate < 2e-5 {
                bail!("the spheres pack to a volume fraction of {:.3} at most ({:.3} asked): lower the fraction", goal * s.powf(dim), goal);
            }
        }
    }
    Ok(centres)
}

/// Push overlapping pairs apart, each by half its overlap, until no pair overlaps by more
/// than `tolerance`. False when `sweeps` were not enough.
#[allow(clippy::too_many_arguments)]
fn push_apart(g: &Grid, centres: &mut [[f64; 3]], radii: &[f64], s: f64, gap: f64, rmax: f64, tolerance: f64, sweeps: usize, r: &mut dyn Draws) -> bool {
    let mut buckets = Buckets::new(g, 2.0 * rmax * s + gap);
    for _ in 0..sweeps {
        buckets.clear();
        for (k, c) in centres.iter().enumerate() {
            buckets.insert(*c, k);
        }
        let mut worst: f64 = 0.0;
        for i in 0..centres.len() {
            let mut near = Vec::new();
            buckets.near(centres[i], |j| {
                if j != i {
                    near.push(j);
                }
            });
            for j in near {
                let d = g.delta(centres[i], centres[j]);
                let dist = norm(d);
                let overlap = (radii[i] + radii[j]) * s + gap - dist;
                if overlap > 0.0 {
                    worst = worst.max(overlap);
                    let dir = if dist > 1e-12 { scale(d, 1.0 / dist) } else { planar_dir(g, r) };
                    let step = 0.5 * overlap * 1.001;
                    centres[i] = add(centres[i], scale(dir, step));
                    centres[j] = sub(centres[j], scale(dir, step));
                    for c in [i, j] {
                        for k in 0..3 {
                            centres[c][k] = if g.periodic { centres[c][k].rem_euclid(g.size[k]) } else { centres[c][k].clamp(0.0, g.size[k]) };
                        }
                    }
                }
            }
        }
        if worst <= tolerance {
            return true;
        }
    }
    false
}

fn planar_dir(g: &Grid, r: &mut dyn Draws) -> [f64; 3] {
    if g.planar() {
        let a = r.uniform(0.0, std::f64::consts::TAU);
        [a.cos(), a.sin(), 0.0]
    } else {
        isotropic(r)
    }
}

/// Radii, largest first, whose spheres take the fraction `fraction` of the box.
pub fn radii_for_fraction(g: &Grid, radius: f64, sigma: f64, fraction: f64, r: &mut dyn Draws) -> Vec<f64> {
    let volume = |x: f64| if g.planar() { std::f64::consts::PI * x * x * g.h } else { SPHERE * x * x * x };
    let goal = fraction * g.volume();
    let mut radii = Vec::new();
    let mut total = 0.0;
    while total + 0.5 * volume(radius) < goal && radii.len() < 5_000_000 {
        let x = lognormal(radius, sigma, r);
        total += volume(x);
        radii.push(x);
    }
    radii.sort_by(|a, b| b.total_cmp(a));
    radii
}

pub const PACK_RSA: &str = "Random sequential addition";
pub const PACK_DENSE: &str = "Dense packing";

/// Hard spheres: no two nearer than `gap`, by random sequential addition (up to a fraction
/// near 0.38 for equal spheres) or as a dense packing (up to about 0.64).
pub fn hard_spheres(g: &Grid, radius: f64, sigma: f64, fraction: f64, gap: f64, method: &str, rate: f64, seed: u64) -> Result<DataSet> {
    if !(fraction > 0.0 && fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let mut r = Rng::new(seed);
    let radii = radii_for_fraction(g, radius, sigma, fraction, &mut r);
    let centres = if method == PACK_DENSE {
        dense_packing(g, &radii, gap, rate, &mut r)?
    } else {
        let c = sequential_addition(g, &radii, gap, 20_000, &mut r);
        if c.len() < radii.len() {
            bail!(
                "random sequential addition placed {} of {} spheres (it jams near a fraction of 0.38 for equal spheres): lower the fraction or the gap, or choose {PACK_DENSE}",
                c.len(),
                radii.len()
            );
        }
        c
    };
    let (phases, ids) = sphere_cells(g, &centres, &radii);
    let mut img = phase_image(g, phases, Some(ids));
    keep_spheres(&mut img, &centres, &radii);
    Ok(DataSet::ImageData(img))
}

/// Penetrable spheres with a hard core (the cherry-pit model): spheres of radius `radius`
/// whose cores of radius `impenetrability x radius` do not overlap. Impenetrability 0 gives
/// overlapping spheres, 1 hard spheres. `density` is the number of spheres per unit volume
/// times the volume of one (the fraction they would take without overlap).
pub fn penetrable_spheres(g: &Grid, radius: f64, impenetrability: f64, density: f64, seed: u64) -> Result<DataSet> {
    let mut r = Rng::new(seed);
    let one = if g.planar() { std::f64::consts::PI * radius * radius * g.h } else { SPHERE * radius.powi(3) };
    let count = (density * g.volume() / one).round() as usize;
    let core = radius * impenetrability.clamp(0.0, 1.0);
    let centres = sequential_addition(g, &vec![core; count], 0.0, 20_000, &mut r);
    if centres.len() < count {
        bail!("the hard cores jam after {} of {} spheres: lower the density or the impenetrability", centres.len(), count);
    }
    let radii = vec![radius; count];
    let (phases, ids) = sphere_cells(g, &centres, &radii);
    let mut img = phase_image(g, phases, Some(ids));
    keep_spheres(&mut img, &centres, &radii);
    Ok(DataSet::ImageData(img))
}

pub struct ClusterParams {
    pub aggregate_radius: f64,
    pub aggregate_fraction: f64,
    pub radius: f64,
    /// Fraction of the aggregates the particles take.
    pub local_fraction: f64,
    /// Fraction they take outside the aggregates.
    pub outside_fraction: f64,
    pub hard: bool,
    /// The aggregates as a phase of their own (3) around the particles.
    pub aggregate_phase: bool,
    pub seed: u64,
}

/// Particles clustered in aggregates: the aggregates are overlapping spheres, and the centres
/// of the particles a Poisson process that is denser inside them than outside (a Cox
/// process), or hard particles placed one after another to the same fractions.
pub fn clustered_spheres(g: &Grid, p: &ClusterParams) -> Result<DataSet> {
    if !(0.0..1.0).contains(&p.aggregate_fraction) || !(0.0..1.0).contains(&p.local_fraction) || !(0.0..1.0).contains(&p.outside_fraction) {
        bail!("the fractions must be at least 0 and below 1");
    }
    let mut r = Rng::new(p.seed);
    let theta = -(1.0 - p.aggregate_fraction).ln() / (SPHERE * p.aggregate_radius.powi(3));
    let count = poisson(theta * g.volume_around(p.aggregate_radius), &mut r);
    let agg: Vec<[f64; 3]> = (0..count).map(|_| g.random_point_around(p.aggregate_radius, &mut r)).collect();
    let mut inside = vec![false; g.len()];
    for c in &agg {
        g.stamp_sphere(*c, p.aggregate_radius, |cell, _| inside[cell] = true);
    }
    let in_aggregate = |x: [f64; 3]| -> bool {
        let c: [usize; 3] = std::array::from_fn(|k| {
            let v = if g.periodic { x[k].rem_euclid(g.size[k]) } else { x[k].clamp(0.0, g.size[k]) };
            ((v / g.h) as usize).min(g.n[k] - 1)
        });
        inside[g.index(c)]
    };
    let one = SPHERE * p.radius.powi(3);
    let mut centres: Vec<[f64; 3]> = Vec::new();
    if p.hard {
        let share = inside.iter().filter(|x| **x).count() as f64 / g.len() as f64;
        let goal = [(p.local_fraction * share * g.volume() / one).round() as usize, (p.outside_fraction * (1.0 - share) * g.volume() / one).round() as usize];
        let mut buckets = Buckets::new(g, 2.0 * p.radius);
        for (region, want) in goal.iter().enumerate() {
            let mut placed = 0;
            let mut tries = 0usize;
            while placed < *want {
                tries += 1;
                if tries > 4000 * want.max(&50) {
                    bail!("the particles jam after {placed} of {want}: lower their fraction");
                }
                let x = g.random_point(&mut r);
                if in_aggregate(x) != (region == 0) {
                    continue;
                }
                let mut free = true;
                buckets.near(x, |j| {
                    if free && g.dist2(x, centres[j]) < 4.0 * p.radius * p.radius {
                        free = false;
                    }
                });
                if free {
                    buckets.insert(x, centres.len());
                    centres.push(x);
                    placed += 1;
                }
            }
        }
    } else {
        // A Poisson process of the larger intensity, thinned where the other one applies.
        let rate = |f: f64| -(1.0 - f).ln() / one;
        let (t1, t0) = (rate(p.local_fraction), rate(p.outside_fraction));
        let top = t1.max(t0);
        for _ in 0..poisson(top * g.volume_around(p.radius), &mut r) {
            let x = g.random_point_around(p.radius, &mut r);
            let keep = if in_aggregate(x) { t1 / top } else { t0 / top };
            if r.random() < keep {
                centres.push(x);
            }
        }
    }
    let radii = vec![p.radius; centres.len()];
    let (mut phases, ids) = sphere_cells(g, &centres, &radii);
    if p.aggregate_phase {
        for (c, ph) in phases.iter_mut().enumerate() {
            if *ph == PHASE_MATRIX && inside[c] {
                *ph = 3;
            }
        }
    }
    let mut img = phase_image(g, phases, Some(ids));
    note(&mut img, "Aggregates", vec![agg.len() as f64]);
    keep_spheres(&mut img, &centres, &radii);
    Ok(DataSet::ImageData(img))
}
