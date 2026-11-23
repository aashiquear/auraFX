//! Fibres and layers: overlapping straight fibres, fibres on Poisson lines, curved fibres,
//! fibres placed without overlap, bending fibres packed by repulsion, cross-linked fibres,
//! networks on the edges of a Delaunay triangulation, and laminates.

use super::particles::sequential_addition;
use super::*;
use crate::synthetic::{Draws, Rng};
use anyhow::{bail, Result};
use std::f64::consts::{PI, TAU};

/// A fibre: the points of its axis, joined by straight pieces.
pub type Fibre = Vec<[f64; 3]>;

/// The least distance between the segments `a0 a1` and `b0 b1`, with the closest point of each.
pub fn segment_gap(a0: [f64; 3], a1: [f64; 3], b0: [f64; 3], b1: [f64; 3]) -> (f64, [f64; 3], [f64; 3]) {
    let (d1, d2, w) = (sub(a1, a0), sub(b1, b0), sub(a0, b0));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, w));
    let (s, t);
    if a <= 1e-24 && e <= 1e-24 {
        (s, t) = (0.0, 0.0);
    } else if a <= 1e-24 {
        (s, t) = (0.0, (f / e).clamp(0.0, 1.0));
    } else {
        let c = dot(d1, w);
        if e <= 1e-24 {
            (s, t) = ((-c / a).clamp(0.0, 1.0), 0.0);
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let mut s0 = if den > 1e-24 { ((b * f - c * e) / den).clamp(0.0, 1.0) } else { 0.0 };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t) = (s0, t0);
        }
    }
    let (p, q) = (add(a0, scale(d1, s)), add(b0, scale(d2, t)));
    (norm(sub(p, q)), p, q)
}

/// The segment `b` moved to its image nearest to the segment `a`.
fn nearest_image(g: &Grid, a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    if !g.periodic {
        return b;
    }
    let (ma, mb) = (scale(add(a.0, a.1), 0.5), scale(add(b.0, b.1), 0.5));
    let shift = sub(sub(ma, g.delta(ma, mb)), mb);
    (add(b.0, shift), add(b.1, shift))
}

/// The pieces of a fibre, none longer than `longest`.
fn pieces(f: &Fibre, longest: f64) -> Vec<([f64; 3], [f64; 3])> {
    let mut out = Vec::new();
    for w in f.windows(2) {
        let n = ((norm(sub(w[1], w[0])) / longest).ceil() as usize).max(1);
        for k in 0..n {
            let at = |t: f64| add(w[0], scale(sub(w[1], w[0]), t));
            out.push((at(k as f64 / n as f64), at((k + 1) as f64 / n as f64)));
        }
    }
    out
}

/// Fibres placed one after another without overlap (random sequential addition): a fibre
/// drawn by `make` is kept when its axis stays at least `2 radius + gap` from the axes of
/// those already placed. Returns the fibres placed; fewer than `count` at jamming.
pub fn place_apart(
    g: &Grid,
    count: usize,
    radius: f64,
    gap: f64,
    attempts: usize,
    make: &mut dyn FnMut(&mut dyn Draws) -> Fibre,
    r: &mut dyn Draws,
) -> Vec<Fibre> {
    let longest = 4.0 * radius;
    let reach = 2.0 * radius + gap;
    let mut buckets = Buckets::new(g, longest + reach);
    let mut segs: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let mut out: Vec<Fibre> = Vec::new();
    for _ in 0..count {
        let mut placed = false;
        for _ in 0..attempts {
            let f = make(r);
            let mine = pieces(&f, longest);
            let mut free = true;
            for s in &mine {
                let mid = scale(add(s.0, s.1), 0.5);
                buckets.near(mid, |j| {
                    if free {
                        let o = nearest_image(g, *s, segs[j]);
                        if segment_gap(s.0, s.1, o.0, o.1).0 < reach {
                            free = false;
                        }
                    }
                });
                if !free {
                    break;
                }
            }
            if free {
                for s in mine {
                    buckets.insert(scale(add(s.0, s.1), 0.5), segs.len());
                    segs.push(s);
                }
                out.push(f);
                placed = true;
                break;
            }
        }
        if !placed {
            break;
        }
    }
    out
}

/// Phases and FeatureIds of fibres of radius `radius`: a cell belongs to the first fibre that
/// holds it. `flat` cuts the ends square; `lumen` (a fraction of the radius) hollows every
/// fibre, the bores joining where fibres cross.
pub fn fibre_cells(g: &Grid, fibres: &[Fibre], radius: f64, flat: bool, lumen: f64) -> (Vec<i32>, Vec<i32>) {
    let mut phases = vec![PHASE_MATRIX; g.len()];
    let mut ids = vec![0i32; g.len()];
    let mut bore = vec![false; if lumen > 0.0 { g.len() } else { 0 }];
    let inner = lumen.clamp(0.0, 0.999) * radius;
    for (k, f) in fibres.iter().enumerate() {
        let last = f.len().saturating_sub(2);
        for (s, w) in f.windows(2).enumerate() {
            g.stamp_segment(w[0], w[1], radius, |cell, d, t| {
                if flat && ((s == 0 && t < 0.0) || (s == last && t > 1.0)) {
                    return;
                }
                if ids[cell] == 0 {
                    ids[cell] = k as i32 + 1;
                    phases[cell] = PHASE_SOLID;
                }
                if lumen > 0.0 && d < inner {
                    bore[cell] = true;
                }
            });
        }
    }
    for (c, b) in bore.iter().enumerate() {
        if *b {
            phases[c] = PHASE_MATRIX;
            ids[c] = 0;
        }
    }
    (phases, ids)
}

fn finish(g: &Grid, fibres: &[Fibre], radius: f64, flat: bool, lumen: f64) -> ImageData {
    let (phases, ids) = fibre_cells(g, fibres, radius, flat, lumen);
    let mut img = phase_image(g, phases, Some(ids));
    note(&mut img, "Fibres", vec![fibres.len() as f64]);
    img
}

fn straight(centre: [f64; 3], d: [f64; 3], length: f64) -> Fibre {
    vec![sub(centre, scale(d, 0.5 * length)), add(centre, scale(d, 0.5 * length))]
}

fn mid_plane(g: &Grid, mut p: [f64; 3]) -> [f64; 3] {
    if g.planar() {
        p[2] = 0.5 * g.h;
    }
    p
}

/// Overlapping straight fibres: cylinders (flat ends) or spherocylinders (round ends) at the
/// points of a Poisson process, their union as the solid, at the fraction `1 - exp(-theta V)`.
pub fn overlapping_fibres(g: &Grid, radius: f64, length: f64, dirs: &Directions, round: bool, fraction: f64, lumen: f64, seed: u64) -> Result<DataSet> {
    if !(0.0..1.0).contains(&fraction) {
        bail!("the volume fraction must be at least 0 and below 1");
    }
    let mut r = Rng::new(seed);
    let one = PI * radius * radius * length + if round { 4.0 / 3.0 * PI * radius.powi(3) } else { 0.0 };
    let margin = 0.5 * length + radius;
    let count = poisson(-(1.0 - fraction).ln() / one * g.volume_around(margin), &mut r);
    let fibres: Vec<Fibre> = (0..count).map(|_| straight(mid_plane(g, g.random_point_around(margin, &mut r)), dirs.draw(&mut r), length)).collect();
    let mut img = finish(g, &fibres, radius, !round, lumen);
    note(&mut img, "ExpectedFraction", vec![fraction]);
    Ok(DataSet::ImageData(img))
}

/// Lines of a Poisson process through the ball around the box, each as the two ends of its
/// chord: `length_density` is the length of line per unit volume.
fn poisson_lines(g: &Grid, length_density: f64, dirs: &Directions, r: &mut dyn Draws) -> Vec<Fibre> {
    let centre = g.size.map(|s| 0.5 * s);
    let ball = norm(centre) + 1e-9;
    // Lines of a given direction cross the disc normal to it at so many points per unit area.
    let count = poisson(length_density * PI * ball * ball, r);
    (0..count)
        .map(|_| {
            let d = dirs.draw(r);
            let (u, v) = frame(d);
            let (rho, a) = (ball * r.random().sqrt(), r.uniform(0.0, TAU));
            let p = add(centre, add(scale(u, rho * a.cos()), scale(v, rho * a.sin())));
            straight(p, d, 2.0 * ball)
        })
        .collect()
}

/// Fibres without ends: cylinders about the lines of a Poisson process (the box is not
/// periodic). With `L` the length of line per unit volume the fibres take `1 - exp(-pi r^2 L)`.
pub fn poisson_line_fibres(g: &Grid, radius: f64, dirs: &Directions, fraction: f64, seed: u64) -> Result<DataSet> {
    if !(0.0..1.0).contains(&fraction) {
        bail!("the volume fraction must be at least 0 and below 1");
    }
    let mut r = Rng::new(seed);
    let fibres = poisson_lines(g, -(1.0 - fraction).ln() / (PI * radius * radius), dirs, &mut r);
    let open = Grid { periodic: false, ..g.clone() };
    let mut img = finish(&open, &fibres, radius, true, 0.0);
    note(&mut img, "ExpectedFraction", vec![fraction]);
    Ok(DataSet::ImageData(img))
}

pub const CURVE_CHAIN: &str = "Worm-like chain";
pub const CURVE_ARC: &str = "Arc";
pub const CURVE_HELIX: &str = "Helix";

pub struct CurveParams {
    pub shape: String,
    pub radius: f64,
    /// Length along the axis of the fibre.
    pub length: f64,
    /// Length over which a worm-like chain forgets its direction.
    pub persistence: f64,
    /// Radius of curvature of the arcs, or radius of the helices.
    pub curvature_radius: f64,
    pub pitch: f64,
    pub segments: usize,
}

/// One curved fibre through `centre`, its direction there `d`.
fn curved(p: &CurveParams, centre: [f64; 3], d: [f64; 3], planar: bool, r: &mut dyn Draws) -> Fibre {
    let n = p.segments.max(1);
    let (u, v) = if planar { ([-d[1], d[0], 0.0], [0.0, 0.0, 1.0]) } else { frame(d) };
    let turn = if planar { 0.0 } else { r.uniform(0.0, TAU) };
    let side = add(scale(u, turn.cos()), scale(v, turn.sin()));
    match p.shape.as_str() {
        CURVE_ARC => {
            let rc = p.curvature_radius.max(1e-9);
            let origin = add(centre, scale(side, rc));
            (0..=n)
                .map(|k| {
                    let a = (k as f64 / n as f64 - 0.5) * p.length / rc;
                    add(origin, add(scale(side, -rc * a.cos()), scale(d, rc * a.sin())))
                })
                .collect()
        }
        CURVE_HELIX => {
            let rh = p.curvature_radius.max(0.0);
            let per_turn = ((TAU * rh).powi(2) + p.pitch * p.pitch).sqrt().max(1e-9);
            let total = TAU * p.length / per_turn;
            let other = cross(d, side);
            (0..=n)
                .map(|k| {
                    let a = (k as f64 / n as f64 - 0.5) * total;
                    add(centre, add(add(scale(side, rh * a.cos()), scale(other, rh * a.sin())), scale(d, p.pitch * a / TAU)))
                })
                .collect()
        }
        _ => {
            let ds = p.length / n as f64;
            let wander = (2.0 * ds / p.persistence.max(1e-9)).sqrt();
            let mut t = d;
            let mut pts = vec![[0.0; 3]];
            for _ in 0..n {
                pts.push(add(*pts.last().unwrap(), scale(t, ds)));
                let mut kick = [r.std_normal(), r.std_normal(), if planar { 0.0 } else { r.std_normal() }];
                kick = sub(kick, scale(t, dot(kick, t)));
                t = unit(add(t, scale(kick, wander)));
            }
            let mean = scale(pts.iter().fold([0.0; 3], |a, b| add(a, *b)), 1.0 / pts.len() as f64);
            pts.iter().map(|q| add(sub(*q, mean), centre)).collect()
        }
    }
}

/// Curved fibres: worm-like chains, arcs of circles or helices, overlapping at the points of
/// a Poisson process or placed one after another without overlap.
#[allow(clippy::too_many_arguments)]
pub fn curved_fibres(g: &Grid, p: &CurveParams, dirs: &Directions, fraction: f64, overlap: bool, gap: f64, lumen: f64, seed: u64) -> Result<DataSet> {
    if !(0.0..1.0).contains(&fraction) {
        bail!("the volume fraction must be at least 0 and below 1");
    }
    let mut r = Rng::new(seed);
    let one = PI * p.radius * p.radius * p.length + 4.0 / 3.0 * PI * p.radius.powi(3);
    let margin = 0.5 * p.length + p.radius;
    let fibres = if overlap {
        let count = poisson(-(1.0 - fraction).ln() / one * g.volume_around(margin), &mut r);
        (0..count)
            .map(|_| {
                let (c, d) = (mid_plane(g, g.random_point_around(margin, &mut r)), dirs.draw(&mut r));
                curved(p, c, d, g.planar(), &mut r)
            })
            .collect()
    } else {
        let count = (fraction * g.volume() / one).round() as usize;
        let mut make = |r: &mut dyn Draws| {
            let (c, d) = (mid_plane(g, g.random_point(r)), dirs.draw(r));
            curved(p, c, d, g.planar(), r)
        };
        let placed = place_apart(g, count, p.radius, gap, 4000, &mut make, &mut r);
        if placed.len() < count {
            bail!("{} of {} fibres found a place without overlap: lower the fraction or the gap, or let the fibres overlap", placed.len(), count);
        }
        placed
    };
    Ok(DataSet::ImageData(finish(g, &fibres, p.radius, false, lumen)))
}

/// Straight fibres with round ends, none nearer than `gap` to another, placed one after
/// another (random sequential addition).
#[allow(clippy::too_many_arguments)]
pub fn hard_fibres(g: &Grid, radius: f64, length: f64, dirs: &Directions, fraction: f64, gap: f64, lumen: f64, seed: u64) -> Result<DataSet> {
    if !(fraction > 0.0 && fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let mut r = Rng::new(seed);
    let one = PI * radius * radius * length + 4.0 / 3.0 * PI * radius.powi(3);
    let count = (fraction * g.volume() / one).round() as usize;
    let mut make = |r: &mut dyn Draws| straight(mid_plane(g, g.random_point(r)), dirs.draw(r), length);
    let fibres = place_apart(g, count, radius, gap, 20_000, &mut make, &mut r);
    if fibres.len() < count {
        bail!(
            "{} of {} fibres found a place without overlap (a fraction of {:.3}): lower the fraction or the gap, or align the fibres",
            fibres.len(),
            count,
            fraction * fibres.len() as f64 / count as f64
        );
    }
    Ok(DataSet::ImageData(finish(g, &fibres, radius, false, lumen)))
}

/// A direction about `mean` (a unit vector) of the von Mises-Fisher law of concentration `kappa`.
fn von_mises_fisher(mean: [f64; 3], kappa: f64, r: &mut dyn Draws) -> [f64; 3] {
    if kappa < 1e-9 {
        return isotropic(r);
    }
    let u = r.random();
    let w = (1.0 + (u + (1.0 - u) * (-2.0 * kappa).exp()).ln() / kappa).clamp(-1.0, 1.0);
    let (a, b) = frame(mean);
    let phi = r.uniform(0.0, TAU);
    let s = (1.0 - w * w).max(0.0).sqrt();
    add(scale(mean, w), add(scale(a, s * phi.cos()), scale(b, s * phi.sin())))
}

pub struct BendingParams {
    pub radius: f64,
    pub length: f64,
    /// How closely a step follows the main direction of its fibre, and the previous step.
    pub kappa_main: f64,
    pub kappa_step: f64,
    pub fraction: f64,
    pub sweeps: usize,
    /// Above 0, fibres nearer than this to one another are drawn into contact.
    pub contact_distance: f64,
    pub seed: u64,
}

/// Bending fibres packed without overlap. Every fibre is a random walk of steps one radius
/// long, each step drawn about a mix of the main direction of the fibre and of the previous
/// step (von Mises-Fisher). The fibres start overlapping; the balls along their axes then
/// push one another apart while each fibre keeps its length and resists bending, until the
/// overlaps are gone.
pub fn bending_fibres(g: &Grid, p: &BendingParams, dirs: &Directions) -> Result<DataSet> {
    if !(p.fraction > 0.0 && p.fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let mut r = Rng::new(p.seed);
    let rad = p.radius;
    let steps = ((p.length / rad).round() as usize).max(1);
    let ds = p.length / steps as f64;
    let one = PI * rad * rad * p.length + 4.0 / 3.0 * PI * rad.powi(3);
    let count = (p.fraction * g.volume() / one).round() as usize;
    if count * (steps + 1) > 4_000_000 {
        bail!("too many fibres ({count}): raise the radius or lower the fraction");
    }
    // The balls of all the fibres in one list; fibre f holds the balls first[f]..first[f + 1].
    let mut balls: Vec<[f64; 3]> = Vec::new();
    for _ in 0..count {
        let main = dirs.draw(&mut r);
        let mut d = von_mises_fisher(main, p.kappa_main, &mut r);
        let mut pts = vec![[0.0; 3]];
        for _ in 0..steps {
            pts.push(add(*pts.last().unwrap(), scale(d, ds)));
            let pull = add(scale(main, p.kappa_main), scale(d, p.kappa_step));
            d = von_mises_fisher(unit(pull), norm(pull), &mut r);
            if g.planar() {
                d = unit([d[0], d[1], 0.0]);
            }
        }
        let mean = scale(pts.iter().fold([0.0; 3], |a, b| add(a, *b)), 1.0 / pts.len() as f64);
        let centre = mid_plane(g, g.random_point(&mut r));
        balls.extend(pts.iter().map(|q| add(sub(*q, mean), centre)));
    }
    let per = steps + 1;
    let fibre_of = |i: usize| i / per;
    // Balls of one fibre closer than this along it are not pushed apart.
    let own = ((2.0 * rad / ds).ceil() as usize).max(1);
    let mut buckets = Buckets::new(g, 2.0 * rad + p.contact_distance.max(0.0));
    let mut worst = f64::INFINITY;
    for sweep in 0..p.sweeps {
        buckets.clear();
        for (i, b) in balls.iter().enumerate() {
            buckets.insert(*b, i);
        }
        worst = 0.0;
        let mut moves = vec![[0.0; 3]; balls.len()];
        for i in 0..balls.len() {
            let me = balls[i];
            buckets.near(me, |j| {
                if j <= i || (fibre_of(i) == fibre_of(j) && j - i <= own) {
                    return;
                }
                let d = g.delta(me, balls[j]);
                let dist = norm(d);
                let overlap = 2.0 * rad - dist;
                let dir = if dist > 1e-12 { scale(d, 1.0 / dist) } else { [1.0, 0.0, 0.0] };
                if overlap > 0.0 {
                    worst = worst.max(overlap);
                    moves[i] = add(moves[i], scale(dir, 0.5 * overlap));
                    moves[j] = sub(moves[j], scale(dir, 0.5 * overlap));
                } else if -overlap < p.contact_distance && fibre_of(i) != fibre_of(j) {
                    // Near but apart: a weak pull into contact.
                    moves[i] = add(moves[i], scale(dir, 0.1 * overlap));
                    moves[j] = sub(moves[j], scale(dir, 0.1 * overlap));
                }
            });
        }
        for (b, m) in balls.iter_mut().zip(&moves) {
            // Several neighbours may push the same way: bounded steps keep the motion stable.
            let l = norm(*m);
            let cap = 0.5 * rad;
            *b = add(*b, if l > cap { scale(*m, cap / l) } else { *m });
            if g.planar() {
                b[2] = 0.5 * g.h;
            }
        }
        // Each fibre keeps the spacing of its balls and straightens a little.
        for f in 0..count {
            let chain = &mut balls[f * per..(f + 1) * per];
            for _ in 0..2 {
                for k in 0..steps {
                    let d = sub(chain[k + 1], chain[k]);
                    let l = norm(d).max(1e-12);
                    let fix = scale(d, 0.5 * (l - ds) / l);
                    chain[k] = add(chain[k], fix);
                    chain[k + 1] = sub(chain[k + 1], fix);
                }
            }
            for k in 1..steps {
                let mid = scale(add(chain[k - 1], chain[k + 1]), 0.5);
                chain[k] = add(chain[k], scale(sub(mid, chain[k]), 0.05));
            }
        }
        if worst < 0.02 * rad && sweep > 5 {
            break;
        }
    }
    let fibres: Vec<Fibre> = (0..count).map(|f| balls[f * per..(f + 1) * per].to_vec()).collect();
    let mut img = finish(g, &fibres, rad, false, 0.0);
    note(&mut img, "LargestOverlap", vec![if worst.is_finite() { worst / rad } else { 0.0 }]);
    Ok(DataSet::ImageData(img))
}

pub struct CrossLinkParams {
    pub radius: f64,
    /// 0: fibres without ends, on Poisson lines.
    pub length: f64,
    pub fraction: f64,
    /// Fraction of the contacts that carry a cross-link.
    pub bonded: f64,
    pub link_radius: f64,
    pub seed: u64,
}

/// Entangled straight fibres bonded at their contacts: two fibres touch where their axes
/// pass within two radii, and a fraction of these contacts carries a cross-link, a ball of
/// a third phase.
pub fn cross_linked_fibres(g: &Grid, p: &CrossLinkParams, dirs: &Directions) -> Result<DataSet> {
    if !(0.0..1.0).contains(&p.fraction) {
        bail!("the volume fraction must be at least 0 and below 1");
    }
    let mut r = Rng::new(p.seed);
    let rate = -(1.0 - p.fraction).ln();
    let endless = p.length <= 0.0;
    let fibres: Vec<Fibre> = if endless {
        poisson_lines(g, rate / (PI * p.radius * p.radius), dirs, &mut r)
    } else {
        let margin = 0.5 * p.length + p.radius;
        let count = poisson(rate / (PI * p.radius * p.radius * p.length) * g.volume_around(margin), &mut r);
        (0..count).map(|_| straight(mid_plane(g, g.random_point_around(margin, &mut r)), dirs.draw(&mut r), p.length)).collect()
    };
    if fibres.len() > 20_000 {
        bail!("too many fibres ({}): raise the radius or lower the fraction", fibres.len());
    }
    let grid = if endless { Grid { periodic: false, ..g.clone() } } else { g.clone() };
    let inside = |x: [f64; 3]| (0..3).all(|k| x[k] >= 0.0 && x[k] <= grid.size[k]);
    let mut contacts: Vec<[f64; 3]> = Vec::new();
    for i in 0..fibres.len() {
        for j in i + 1..fibres.len() {
            let o = nearest_image(&grid, (fibres[i][0], fibres[i][1]), (fibres[j][0], fibres[j][1]));
            let (d, a, b) = segment_gap(fibres[i][0], fibres[i][1], o.0, o.1);
            let mid = scale(add(a, b), 0.5);
            if d < 2.0 * p.radius && (grid.periodic || inside(mid)) {
                contacts.push(mid);
            }
        }
    }
    let (mut phases, ids) = fibre_cells(&grid, &fibres, p.radius, !endless, 0.0);
    let mut links = 0;
    for c in &contacts {
        if r.random() < p.bonded {
            links += 1;
            grid.stamp_sphere(*c, p.link_radius, |cell, _| phases[cell] = 3);
        }
    }
    let mut img = phase_image(&grid, phases, Some(ids));
    note(&mut img, "Fibres", vec![fibres.len() as f64]);
    note(&mut img, "Contacts", vec![contacts.len() as f64]);
    note(&mut img, "CrossLinks", vec![links as f64]);
    Ok(DataSet::ImageData(img))
}

/// The points with their images through the faces of a periodic box of size `size`, those
/// within a few mean spacings of the box: enough for the Delaunay triangulation inside the
/// box to be that of the periodic set. The points themselves come first.
pub fn periodic_images(size: [f64; 3], points: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let mut all: Vec<[f64; 3]> = points.to_vec();
    let margin = 3.5 * (size[0] * size[1] * size[2] / points.len().max(1) as f64).cbrt();
    for sz in -1..=1i32 {
        for sy in -1..=1i32 {
            for sx in -1..=1i32 {
                if (sx, sy, sz) == (0, 0, 0) {
                    continue;
                }
                for p in points {
                    let q = [p[0] + sx as f64 * size[0], p[1] + sy as f64 * size[1], p[2] + sz as f64 * size[2]];
                    if (0..3).all(|k| q[k] > -margin && q[k] < size[k] + margin) {
                        all.push(q);
                    }
                }
            }
        }
    }
    all
}

/// The edges of the Delaunay triangulation of `points` in the box of `g`, as pairs of end
/// points; over the periodic images of the points when the box is periodic (every edge once,
/// the one whose middle lies in the box).
pub fn delaunay_edges(g: &Grid, points: &[[f64; 3]]) -> Result<Vec<([f64; 3], [f64; 3])>> {
    let all = if g.periodic { periodic_images(g.size, points) } else { points.to_vec() };
    let tets = crate::delaunay3d::tetrahedralize(&all, 0.0)?;
    let mut seen = std::collections::HashSet::new();
    let mut edges = Vec::new();
    for t in &tets {
        for (a, b) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
            let key = (t[a].min(t[b]), t[a].max(t[b]));
            if !seen.insert(key) {
                continue;
            }
            let (p, q) = (all[key.0], all[key.1]);
            let mid = scale(add(p, q), 0.5);
            if !g.periodic || (0..3).all(|k| mid[k] >= 0.0 && mid[k] < g.size[k]) {
                edges.push((p, q));
            }
        }
    }
    Ok(edges)
}

/// Points at random, or kept apart: `regularity` (0 to 1) is their least distance as a
/// fraction of the distance at which placing them one after another jams.
pub fn germs(g: &Grid, count: usize, regularity: f64, r: &mut dyn Draws) -> Result<Vec<[f64; 3]>> {
    if regularity <= 0.0 {
        return Ok((0..count).map(|_| g.random_point(r)).collect());
    }
    let jam = (0.38 * 6.0 * g.volume() / (PI * count.max(1) as f64)).cbrt();
    let pts = sequential_addition(g, &vec![0.5 * jam * regularity.min(1.0); count], 0.0, 50_000, r);
    if pts.len() < count {
        bail!("the points jam after {} of {}: lower the regularity", pts.len(), count);
    }
    Ok(pts)
}

/// A network of fibres on the edges of the Delaunay triangulation of random points, joined
/// at every point. With `fraction` above 0 the radius is the one that gives this fraction.
pub fn delaunay_fibre_network(g: &Grid, count: usize, radius: f64, fraction: f64, regularity: f64, seed: u64) -> Result<DataSet> {
    if g.planar() {
        bail!("the Delaunay fibre network needs more than one layer of cells");
    }
    if count < 8 {
        bail!("at least 8 points");
    }
    let mut r = Rng::new(seed);
    let points = germs(g, count, regularity, &mut r)?;
    let edges = delaunay_edges(g, &points)?;
    let spacing = (g.volume() / count as f64).cbrt();
    let phases = if fraction > 0.0 {
        below_quantile(&segment_distance(g, &edges, 0.5 * spacing), fraction.min(0.95))
    } else {
        segment_distance(g, &edges, radius).iter().map(|d| if (*d as f64) < radius { PHASE_SOLID } else { PHASE_MATRIX }).collect()
    };
    let mut img = phase_image(g, phases, None);
    note(&mut img, "Nodes", vec![count as f64]);
    note(&mut img, "Fibres", vec![edges.len() as f64]);
    note(&mut img, "MeanDegree", vec![2.0 * edges.len() as f64 / count as f64]);
    Ok(DataSet::ImageData(img))
}

pub struct LaminateParams {
    pub thickness: f64,
    pub thickness_sigma: f64,
    pub spacing: f64,
    pub spacing_sigma: f64,
    /// 0, 1 or 2: the axis the layers are stacked along.
    pub axis: usize,
    /// Fraction of the area of a holed layer its holes take, and their radius.
    pub hole_fraction: f64,
    pub hole_radius: f64,
    /// Holes only through the layers thinner than the median thickness.
    pub thin_only: bool,
    pub seed: u64,
}

/// A laminate: layers of the second phase of random thicknesses, a random distance apart,
/// with round holes through the layers (or the thinner ones only).
pub fn random_laminate(g: &Grid, p: &LaminateParams) -> Result<DataSet> {
    if !(p.thickness > 0.0 && p.spacing > 0.0) {
        bail!("the thickness and the spacing of the layers must be above 0");
    }
    if !(0.0..1.0).contains(&p.hole_fraction) {
        bail!("the hole fraction must be at least 0 and below 1");
    }
    let mut r = Rng::new(p.seed);
    let ax = p.axis.min(2);
    let height = g.size[ax];
    // Alternately a gap and a layer, from below the box (or from its base when periodic).
    let mut cuts = vec![if g.periodic { 0.0 } else { -r.random() * (p.spacing + p.thickness) }];
    let mut thick = Vec::new();
    while *cuts.last().unwrap() < height {
        let gap = lognormal(p.spacing, p.spacing_sigma, &mut r);
        let t = lognormal(p.thickness, p.thickness_sigma, &mut r);
        let z = *cuts.last().unwrap();
        cuts.push(z + gap);
        cuts.push(z + gap + t);
        thick.push(t);
    }
    if g.periodic {
        // A whole number of layers in the height: the stack is stretched to end on the top face.
        let s = height / *cuts.last().unwrap();
        cuts.iter_mut().for_each(|c| *c *= s);
        thick.iter_mut().for_each(|t| *t *= s);
    }
    let (u, v) = ((ax + 1) % 3, (ax + 2) % 3);
    let area = g.size[u] * g.size[v];
    let holes: Vec<Vec<[f64; 2]>> = thick
        .iter()
        .map(|t| {
            if p.hole_fraction <= 0.0 || p.hole_radius <= 0.0 || (p.thin_only && *t >= p.thickness) {
                return Vec::new();
            }
            let m = if g.periodic { 0.0 } else { p.hole_radius };
            let n = poisson(-(1.0 - p.hole_fraction).ln() / (PI * p.hole_radius * p.hole_radius) * (g.size[u] + 2.0 * m) * (g.size[v] + 2.0 * m), &mut r);
            (0..n).map(|_| [r.uniform(-m, g.size[u] + m), r.uniform(-m, g.size[v] + m)]).collect()
        })
        .collect();
    let _ = area;
    let r2 = p.hole_radius * p.hole_radius;
    let layer_at = |z: f64| -> Option<usize> {
        // cuts: [start, gap end = layer 0 start, layer 0 end, layer 1 start, ...]
        let k = cuts.partition_point(|c| *c <= z);
        (k >= 2 && k % 2 == 0).then(|| k / 2 - 1)
    };
    let cells: Vec<(i32, i32)> = per_cell(g, |x| match layer_at(x[ax]) {
        None => (PHASE_MATRIX, 0),
        Some(l) => {
            let holed = holes[l].iter().any(|h| {
                let (mut du, mut dv) = (x[u] - h[0], x[v] - h[1]);
                if g.periodic {
                    du -= g.size[u] * (du / g.size[u]).round();
                    dv -= g.size[v] * (dv / g.size[v]).round();
                }
                du * du + dv * dv < r2
            });
            if holed {
                (PHASE_MATRIX, 0)
            } else {
                (PHASE_SOLID, l as i32 + 1)
            }
        }
    });
    let mut img = phase_image(g, cells.iter().map(|c| c.0).collect(), Some(cells.iter().map(|c| c.1).collect()));
    note(&mut img, "Layers", vec![thick.len() as f64]);
    Ok(DataSet::ImageData(img))
}
