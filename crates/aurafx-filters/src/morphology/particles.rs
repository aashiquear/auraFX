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

fn keep_spheres(g: &Grid, img: &mut ImageData, centres: &[[f64; 3]], radii: &[f64]) {
    let all: Vec<shapes::Shape> = centres.iter().zip(radii).map(|(c, r)| shapes::sphere(*c, *r)).collect();
    shapes::keep(img, &all, g.periodic);
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
    if !g.planar() {
        // The surface of the union: that of the spheres, where no other sphere covers it.
        let area = 4.0 * std::f64::consts::PI * lognormal_moment(radius, sigma, 2.0);
        note(&mut img, "ExpectedSpecificSurface", vec![theta * area * (1.0 - fraction)]);
        if sigma <= 0.0 {
            // Two points a distance d apart both outside every sphere: q^(2 - r(d)), with r the
            // share of a sphere that its copy moved by d still covers.
            let q = 1.0 - fraction;
            let reach = g.n.iter().filter(|n| **n > 1).min().copied().unwrap_or(1) / 2;
            let two: Vec<f64> = (0..=reach)
                .map(|k| {
                    let u = k as f64 * g.h / (2.0 * radius);
                    let overlap = if u < 1.0 { 1.0 - 1.5 * u + 0.5 * u * u * u } else { 0.0 };
                    1.0 - 2.0 * q + q.powf(2.0 - overlap)
                })
                .collect();
            note(&mut img, "ExpectedTwoPoint", two);
        }
    }
    keep_spheres(g, &mut img, &centres, &radii);
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
pub const PACK_EVENTS: &str = "Dense packing by collisions";

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
    } else if method == PACK_EVENTS {
        collision_packing(g, &radii, gap, rate, &mut r)?
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
    keep_spheres(g, &mut img, &centres, &radii);
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
    keep_spheres(g, &mut img, &centres, &radii);
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
    keep_spheres(g, &mut img, &centres, &radii);
    Ok(DataSet::ImageData(img))
}

/// A dense packing by collisions: the spheres fly as a gas of hard spheres while they grow
/// at a steady rate, and bounce apart when they touch, each bounce pushing them apart a
/// little faster than they grow. Slow growth lets the spheres rearrange and packs them
/// tightly (up to about 0.64 for equal spheres); it stops with an error where they jam
/// before their sizes are reached. `rate`: how fast the radii grow, over the speed of the
/// spheres.
pub fn collision_packing(g: &Grid, radii: &[f64], gap: f64, rate: f64, r: &mut dyn Draws) -> Result<Vec<[f64; 3]>> {
    let n = radii.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    let planar = g.planar();
    let dim = if planar { 2.0 } else { 3.0 };
    let volume = |x: f64| if planar { std::f64::consts::PI * x * x * g.h } else { SPHERE * x * x * x };
    let goal: f64 = radii.iter().map(|x| volume(*x)).sum::<f64>() / g.volume();
    let s0 = (0.12 / goal).powf(1.0 / dim).min(1.0);
    let start: Vec<f64> = radii.iter().map(|x| x * s0).collect();
    let mut x = sequential_addition(g, &start, gap, 20_000, r);
    if x.len() < n {
        bail!("the spheres do not fit in the box, even small");
    }
    if s0 >= 1.0 {
        return Ok(x);
    }
    // Radii with half the gap: spheres that touch then are the gap apart.
    let a: Vec<f64> = radii.iter().map(|x| x + 0.5 * gap).collect();
    let amax = a.iter().cloned().fold(0.0, f64::max);
    let amean = a.iter().sum::<f64>() / n as f64;
    let grow = rate.clamp(1e-4, 0.2) / amean;
    let end = (1.0 - s0) / grow;
    let mut v: Vec<[f64; 3]> = (0..n).map(|_| [r.std_normal(), r.std_normal(), if planar { 0.0 } else { r.std_normal() }]).collect();
    // Each sphere: where it was at the time of its last event.
    let mut at = vec![0.0f64; n];
    let mut stamp = vec![0u32; n];
    let skin = amax;
    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut heap: std::collections::BinaryHeap<Event> = std::collections::BinaryHeap::new();
    let (mut valid, mut cap) = (0.0f64, 0.0f64);
    let mut t = 0.0f64;
    let mut events = 0usize;
    let mut rebuild = true;
    // The next event of the sphere i from the time t: a neighbour (below n) or a wall.
    let predict = |i: usize, t: f64, x: &[[f64; 3]], v: &[[f64; 3]], at: &[f64], lists: &[Vec<u32>]| -> (f64, usize) {
        let pi = add(x[i], scale(v[i], t - at[i]));
        let mut best = (f64::INFINITY, usize::MAX);
        for &j in &lists[i] {
            let j = j as usize;
            let pj = add(x[j], scale(v[j], t - at[j]));
            let d = g.delta(pi, pj);
            let w = sub(v[i], v[j]);
            let (sum, speed) = ((a[i] + a[j]) * (s0 + grow * t), (a[i] + a[j]) * grow);
            let (qa, qb, qc) = (dot(w, w) - speed * speed, dot(d, w) - sum * speed, dot(d, d) - sum * sum);
            let tau = if qc <= 0.0 {
                if qb < 0.0 {
                    0.0
                } else {
                    f64::INFINITY
                }
            } else {
                let disc = qb * qb - qa * qc;
                let den = if disc >= 0.0 { -qb + disc.sqrt() } else { 0.0 };
                if den > 0.0 {
                    qc / den
                } else {
                    f64::INFINITY
                }
            };
            if tau < best.0 {
                best = (tau, j);
            }
        }
        if !g.periodic {
            for k in 0..if planar { 2 } else { 3 } {
                let tau = if v[i][k] < 0.0 {
                    -pi[k] / v[i][k]
                } else if v[i][k] > 0.0 {
                    (g.size[k] - pi[k]) / v[i][k]
                } else {
                    f64::INFINITY
                };
                if tau.max(0.0) < best.0 {
                    best = (tau.max(0.0), n + k);
                }
            }
        }
        (t + best.0, best.1)
    };
    loop {
        if rebuild {
            // Everything brought to the time t, the speeds set back to their first level, the
            // neighbours of every sphere listed again.
            for i in 0..n {
                x[i] = add(x[i], scale(v[i], t - at[i]));
                at[i] = t;
                for k in 0..3 {
                    x[i][k] = if g.periodic { x[i][k].rem_euclid(g.size[k]) } else { x[i][k].clamp(0.0, g.size[k]) };
                }
            }
            let energy = v.iter().map(|w| dot(*w, *w)).sum::<f64>() / (dim * n as f64);
            if energy > 0.0 {
                let f = 1.0 / energy.sqrt();
                v.iter_mut().for_each(|w| *w = scale(*w, f));
            }
            let reach = 2.0 * amax * (s0 + grow * t).min(1.0) + skin;
            let mut buckets = Buckets::new(g, reach);
            x.iter().enumerate().for_each(|(k, c)| buckets.insert(*c, k));
            for i in 0..n {
                lists[i].clear();
                buckets.near(x[i], |j| {
                    if j != i && g.dist2(x[i], x[j]) < reach * reach {
                        lists[i].push(j as u32);
                    }
                });
            }
            cap = 4.0 * v.iter().map(|w| norm(*w)).fold(0.0, f64::max).max(1.0);
            valid = t + skin / (2.0 * cap + 2.0 * amax * grow);
            heap.clear();
            for i in 0..n {
                stamp[i] = stamp[i].wrapping_add(1);
                let (when, j) = predict(i, t, &x, &v, &at, &lists);
                heap.push(Event { when, i: i as u32, j: j as u32, si: stamp[i], sj: if j < n { stamp[j] } else { 0 } });
            }
            rebuild = false;
        }
        let Some(e) = heap.pop() else { break };
        let (i, j) = (e.i as usize, e.j as usize);
        if e.si != stamp[i] {
            continue;
        }
        if e.when >= end.min(valid) {
            // Nothing more before the lists go stale, or before the spheres are full size.
            heap.push(e);
            if valid >= end {
                break;
            }
            t = valid;
            rebuild = true;
            continue;
        }
        if j < n && e.sj != stamp[j] {
            // The partner bounced elsewhere meanwhile: look again.
            let (when, k) = predict(i, t, &x, &v, &at, &lists);
            heap.push(Event { when, i: e.i, j: k as u32, si: stamp[i], sj: if k < n { stamp[k] } else { 0 } });
            continue;
        }
        t = e.when.max(t);
        events += 1;
        if events > 30_000 * n.max(50) {
            bail!(
                "the spheres jam at a volume fraction of {:.3} ({:.3} asked): lower the fraction, or the growth for a denser packing",
                goal * (s0 + grow * t).powf(dim) * (radii.iter().map(|x| volume(*x)).sum::<f64>() / a.iter().map(|x| volume(*x)).sum::<f64>()).min(1.0),
                goal
            );
        }
        x[i] = add(x[i], scale(v[i], t - at[i]));
        at[i] = t;
        if j >= n {
            let k = j - n;
            v[i][k] = -v[i][k];
            x[i][k] = x[i][k].clamp(0.0, g.size[k]);
        } else {
            x[j] = add(x[j], scale(v[j], t - at[j]));
            at[j] = t;
            let d = g.delta(x[i], x[j]);
            let dir = unit(d);
            let (ui, uj) = (dot(v[i], dir), dot(v[j], dir));
            // The parts along the line of centres are exchanged, with the growth added.
            let kick = (a[i] + a[j]) * grow;
            v[i] = add(v[i], scale(dir, uj + kick - ui));
            v[j] = add(v[j], scale(dir, ui - kick - uj));
            stamp[j] = stamp[j].wrapping_add(1);
            let (when, k) = predict(j, t, &x, &v, &at, &lists);
            heap.push(Event { when, i: j as u32, j: k as u32, si: stamp[j], sj: if k < n { stamp[k] } else { 0 } });
            if norm(v[j]) > cap {
                rebuild = true;
            }
        }
        stamp[i] = stamp[i].wrapping_add(1);
        let (when, k) = predict(i, t, &x, &v, &at, &lists);
        heap.push(Event { when, i: i as u32, j: k as u32, si: stamp[i], sj: if k < n { stamp[k] } else { 0 } });
        if norm(v[i]) > cap {
            rebuild = true;
        }
    }
    for i in 0..n {
        x[i] = add(x[i], scale(v[i], end - at[i]));
        for k in 0..3 {
            x[i][k] = if g.periodic { x[i][k].rem_euclid(g.size[k]) } else { x[i][k].clamp(0.0, g.size[k]) };
        }
    }
    Ok(x)
}

struct Event {
    when: f64,
    i: u32,
    j: u32,
    si: u32,
    sj: u32,
}
impl PartialEq for Event {
    fn eq(&self, o: &Self) -> bool {
        self.when == o.when && self.i == o.i
    }
}
impl Eq for Event {}
impl PartialOrd for Event {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Event {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.when.total_cmp(&self.when).then(o.i.cmp(&self.i))
    }
}
