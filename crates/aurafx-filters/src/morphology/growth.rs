//! Structures grown on the grid: cellular automata, activator-inhibitor and
//! reaction-diffusion patterns, grains by nucleation and growth, aggregates, networks of
//! pipes assembled from tiles, and branching trees.

use super::*;
use crate::synthetic::{grain_orientations, Draws, Rng};
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue};
use std::f64::consts::PI;

pub const NEIGHBOURS_MOORE: &str = "Faces, edges and corners";
pub const NEIGHBOURS_FACES: &str = "Faces";
pub const RULE_ANNEAL: &str = "Annealing";
pub const RULE_MAJORITY: &str = "Majority";
pub const RULE_CONSERVED: &str = "Majority at a constant fraction";
pub const RULE_CUSTOM: &str = "Births and survivals";

/// The offsets to the neighbours of a cell.
fn offsets(planar: bool, moore: bool) -> Vec<[i64; 3]> {
    let mut out = Vec::new();
    let zs: &[i64] = if planar { &[0] } else { &[-1, 0, 1] };
    for &z in zs {
        for y in -1..=1i64 {
            for x in -1..=1i64 {
                let m = x.abs() + y.abs() + z.abs();
                if m > 0 && (moore || m == 1) {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}

/// The cell at an offset from the cell `c`, through the periodic faces or none beyond the box.
fn shifted(g: &Grid, c: usize, o: [i64; 3]) -> Option<usize> {
    let p = g.ijk(c);
    let mut q = [0usize; 3];
    for k in 0..3 {
        let v = p[k] as i64 + o[k];
        let n = g.n[k] as i64;
        q[k] = if (0..n).contains(&v) {
            v as usize
        } else if g.periodic {
            v.rem_euclid(n) as usize
        } else {
            return None;
        };
    }
    Some(g.index(q))
}

/// "5-8, 12" as the numbers it lists.
fn counts_listed(text: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>(), b.trim().parse::<usize>()),
            None => (part.parse::<usize>(), part.parse::<usize>()),
        };
        match (a, b) {
            (Ok(a), Ok(b)) if a <= b && b <= 26 => out.extend(a..=b),
            _ => bail!("cannot read the neighbour counts \"{part}\" (write them as 5-8 or 4, 6, 7)"),
        }
    }
    Ok(out)
}

pub struct AutomatonParams {
    pub rule: String,
    pub neighbours: String,
    pub steps: usize,
    pub initial_fraction: f64,
    /// For the rule of births and survivals: the numbers of live neighbours at which a dead
    /// cell comes alive, and at which a live cell stays alive.
    pub births: String,
    pub survivals: String,
    /// The cells the automaton runs on (`LATTICES`) and, for other cells than those of the
    /// grid, their size.
    pub lattice: String,
    pub tile: f64,
    pub seed: u64,
}

/// A two-phase structure grown by a cellular automaton from random cells: at every step each
/// cell looks at its neighbours. Annealing (the majority rule with its two middle counts
/// swapped) coarsens noise into two interwoven phases; the majority rule freezes early; the
/// majority at a constant fraction keeps the volume fraction; births and survivals is any
/// rule of that kind.
pub fn cellular_automaton(g: &Grid, p: &AutomatonParams) -> Result<DataSet> {
    let mut r = Rng::new(p.seed);
    if p.lattice != LATTICE_GRID && !p.lattice.is_empty() {
        return automaton_on_tiles(g, p, &mut r);
    }
    let offs = offsets(g.planar(), p.neighbours != NEIGHBOURS_FACES);
    let mut state: Vec<u8> = (0..g.len()).map(|_| (r.random() < p.initial_fraction) as u8).collect();
    let around = offs.len() + 1;
    let half = around.div_ceil(2);
    let (births, survivals) = if p.rule == RULE_CUSTOM { (counts_listed(&p.births)?, counts_listed(&p.survivals)?) } else { (Vec::new(), Vec::new()) };
    for _ in 0..p.steps {
        let live: Vec<u8> = (0..g.len()).into_par_iter().map(|c| offs.iter().filter_map(|o| shifted(g, c, *o)).map(|q| state[q]).sum::<u8>()).collect();
        let next: Vec<u8> = match p.rule.as_str() {
            RULE_MAJORITY => (0..g.len()).map(|c| ((live[c] + state[c]) as usize >= half) as u8).collect(),
            RULE_CONSERVED => {
                // The cells with the most live cells around them, as many as before.
                let alive = state.iter().filter(|s| **s == 1).count();
                let score: Vec<f32> = (0..g.len()).map(|c| -((live[c] + state[c]) as f32) - 0.5 * state[c] as f32 - 0.25 * r.random() as f32).collect();
                below_quantile(&score, alive as f64 / g.len() as f64).iter().map(|x| (*x == PHASE_SOLID) as u8).collect()
            }
            RULE_CUSTOM => {
                (0..g.len()).map(|c| if state[c] == 1 { survivals.contains(&(live[c] as usize)) } else { births.contains(&(live[c] as usize)) } as u8).collect()
            }
            _ => (0..g.len())
                .map(|c| {
                    let s = (live[c] + state[c]) as usize;
                    (s > half || s + 1 == half) as u8
                })
                .collect(),
        };
        if next == state {
            break;
        }
        state = next;
    }
    Ok(DataSet::ImageData(phase_image(g, state.iter().map(|s| if *s == 1 { PHASE_SOLID } else { PHASE_MATRIX }).collect(), None)))
}

/// For every cell, the sum of `state` over the ball of `radius` cells around it, and the
/// number of cells of that ball.
fn ball_sum(g: &Grid, state: &[f32], radius: f64) -> (Vec<f32>, f64) {
    let reach = radius.floor() as i64;
    let mut rows: Vec<(i64, i64, i64)> = Vec::new();
    let zs = if g.planar() { 0..=0 } else { -reach..=reach };
    for dz in zs {
        for dy in -reach..=reach {
            let left = radius * radius - (dy * dy + dz * dz) as f64;
            if left >= 0.0 {
                rows.push((dy, dz, left.sqrt().floor() as i64));
            }
        }
    }
    let volume: f64 = rows.iter().map(|r| (2 * r.2 + 1) as f64).sum();
    let (nx, ny, nz) = (g.n[0] as i64, g.n[1] as i64, g.n[2] as i64);
    // Running sums along x, one row of cells after another.
    let line = (nx + 1) as usize;
    let mut run = vec![0.0f64; line * (ny * nz) as usize];
    for row in 0..(ny * nz) as usize {
        for i in 0..nx as usize {
            run[row * line + i + 1] = run[row * line + i] + state[row * nx as usize + i] as f64;
        }
    }
    let span = |row: usize, a: i64, b: i64| run[row * line + (b + 1) as usize] - run[row * line + a as usize];
    let out = (0..g.len())
        .into_par_iter()
        .map(|c| {
            let [i, j, k] = g.ijk(c).map(|x| x as i64);
            let mut s = 0.0;
            for (dy, dz, w) in &rows {
                let (mut y, mut z) = (j + dy, k + dz);
                if g.periodic {
                    y = y.rem_euclid(ny);
                    z = z.rem_euclid(nz);
                } else if y < 0 || y >= ny || z < 0 || z >= nz {
                    continue;
                }
                let row = (y + ny * z) as usize;
                let (a, b) = (i - w, i + w);
                if !g.periodic {
                    s += span(row, a.max(0), b.min(nx - 1));
                } else if b - a + 1 >= nx {
                    // The ball is wider than the box: whole rows, as often as it wraps.
                    let whole = (b - a + 1) / nx;
                    s += whole as f64 * span(row, 0, nx - 1);
                    let rest = (b - a + 1) % nx;
                    if rest > 0 {
                        let a2 = a.rem_euclid(nx);
                        let b2 = a2 + rest - 1;
                        s += if b2 < nx { span(row, a2, b2) } else { span(row, a2, nx - 1) + span(row, 0, b2 - nx) };
                    }
                } else if a < 0 {
                    s += span(row, 0, b) + span(row, a + nx, nx - 1);
                } else if b >= nx {
                    s += span(row, a, nx - 1) + span(row, 0, b - nx);
                } else {
                    s += span(row, a, b);
                }
            }
            s as f32
        })
        .collect();
    (out, volume)
}

/// Spots, stripes and labyrinths of an activator-inhibitor automaton: a live cell activates
/// the cells within `activation` and inhibits those between `activation` and `inhibition`.
/// At every step the cells where activation most exceeds inhibition are alive, as many as
/// the volume fraction asks.
pub fn activator_inhibitor(g: &Grid, activation: f64, inhibition: f64, fraction: f64, steps: usize, seed: u64) -> Result<DataSet> {
    if !(activation >= g.h && inhibition > activation) {
        bail!("the activation radius must be at least one cell and the inhibition radius larger");
    }
    if !(fraction > 0.0 && fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let (ra, ri) = (activation / g.h, inhibition / g.h);
    if ri > 40.0 {
        bail!("the inhibition radius is more than 40 cells: lower it or raise the cell size");
    }
    let mut r = Rng::new(seed);
    let mut state: Vec<f32> = (0..g.len()).map(|_| (r.random() < fraction) as u8 as f32).collect();
    for _ in 0..steps {
        let (near, va) = ball_sum(g, &state, ra);
        let (far, vi) = ball_sum(g, &state, ri);
        let score: Vec<f32> = near.iter().zip(&far).zip(&state).map(|((a, b), s)| -(a / va as f32 - (b - a) / (vi - va).max(1.0) as f32) - 1e-4 * s).collect();
        let next: Vec<f32> = below_quantile(&score, fraction).iter().map(|x| (*x == PHASE_SOLID) as u8 as f32).collect();
        if next == state {
            break;
        }
        state = next;
    }
    Ok(DataSet::ImageData(phase_image(g, state.iter().map(|s| if *s > 0.5 { PHASE_SOLID } else { PHASE_MATRIX }).collect(), None)))
}

pub const PATTERNS: &[(&str, f64, f64)] =
    &[("Spots", 0.030, 0.062), ("Worms", 0.040, 0.063), ("Labyrinth", 0.034, 0.060), ("Holes", 0.050, 0.060), ("Custom", f64::NAN, f64::NAN)];

/// Patterns of the Gray-Scott reaction and diffusion of two species, u fed at the rate
/// `feed` and v removed at the rate `feed + kill`, with u + 2v -> 3v. The solid is where v
/// is highest, at the volume fraction.
pub fn reaction_diffusion(g: &Grid, pattern: &str, feed: f64, kill: f64, steps: usize, fraction: f64, seed: u64) -> Result<DataSet> {
    if !(fraction > 0.0 && fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let (f, k) = PATTERNS.iter().find(|p| p.0 == pattern).filter(|p| p.1.is_finite()).map(|p| (p.1 as f32, p.2 as f32)).unwrap_or((feed as f32, kill as f32));
    let (du, dv) = (0.14f32, 0.07f32);
    let mut r = Rng::new(seed);
    let total = g.len();
    let mut u = vec![1.0f32; total];
    let mut v = vec![0.0f32; total];
    // Seeds of the second species, scattered.
    let blobs = (total / 1500).max(4);
    for _ in 0..blobs {
        let c = g.random_point(&mut r);
        g.stamp_sphere(c, 3.0 * g.h, |cell, _| {
            u[cell] = 0.5;
            v[cell] = 0.25;
        });
    }
    for (a, b) in u.iter_mut().zip(v.iter_mut()) {
        *a += 0.02 * (r.random() as f32 - 0.5);
        *b += 0.02 * r.random() as f32 * (*b > 0.0) as u8 as f32;
    }
    let offs = offsets(g.planar(), false);
    let nb: Vec<[u32; 6]> = (0..total)
        .into_par_iter()
        .map(|c| {
            let mut out = [c as u32; 6];
            for (k, o) in offs.iter().enumerate() {
                // Beyond a face that is not periodic the cell sees itself: nothing flows through.
                out[k] = shifted(g, c, *o).unwrap_or(c) as u32;
            }
            out
        })
        .collect();
    let count = offs.len();
    let (mut un, mut vn) = (u.clone(), v.clone());
    for _ in 0..steps {
        un.par_iter_mut().zip(vn.par_iter_mut()).enumerate().for_each(|(c, (a, b))| {
            let (uc, vc) = (u[c], v[c]);
            let (mut lu, mut lv) = (-(count as f32) * uc, -(count as f32) * vc);
            for q in &nb[c][..count] {
                lu += u[*q as usize];
                lv += v[*q as usize];
            }
            let uvv = uc * vc * vc;
            *a = uc + du * lu - uvv + f * (1.0 - uc);
            *b = vc + dv * lv + uvv - (f + k) * vc;
        });
        std::mem::swap(&mut u, &mut un);
        std::mem::swap(&mut v, &mut vn);
    }
    let (lo, hi) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |m, x| (m.0.min(*x), m.1.max(*x)));
    if !(hi - lo > 0.05) || !hi.is_finite() {
        bail!("the pattern died out or filled the box evenly: change the feed and kill rates, or the number of steps");
    }
    let phases = below_quantile(&v.iter().map(|x| -x).collect::<Vec<_>>(), fraction);
    let mut img = phase_image(g, phases, None);
    img.cell_data.insert(Array::new("Concentration", 1, ArrayData::F32(v)));
    Ok(DataSet::ImageData(img))
}

pub const NUCLEATION_START: &str = "All at the start";
pub const NUCLEATION_CONTINUOUS: &str = "At a constant rate";
pub const FRONT_ROUND: &str = "Round";
pub const FRONT_CUBE: &str = "Cubes";
pub const FRONT_OCTAHEDRON: &str = "Octahedra";

pub struct NucleationParams {
    pub nucleation: String,
    /// Nuclei at the start (all at the start).
    pub nuclei: usize,
    /// Nuclei per unit volume and unit time (constant rate).
    pub rate: f64,
    pub growth: f64,
    /// When the growth stops (0: when the box is full).
    pub time: f64,
    pub front: String,
    pub orientations: String,
    pub spread_deg: f64,
    pub structure: String,
    /// Sweeps of grain growth after the box is full (0: none), and its temperature.
    pub coarsening: usize,
    pub temperature: f64,
    pub seed: u64,
}

/// Grains by nucleation and growth: nuclei appear at random places, all at the start or at a
/// constant rate, and every grain grows at the same speed until it meets its neighbours. A
/// cell belongs to the grain that reaches it first; grains born where the material is
/// already transformed never show. Stopped early, the cells not reached are the last
/// feature, of phase 2.
pub fn nucleation_growth(g: &Grid, p: &NucleationParams) -> Result<DataSet> {
    let laue = Laue::from_label(&p.structure).ok_or_else(|| anyhow!("unknown crystal structure {}", p.structure))?;
    if !(p.growth > 0.0) {
        bail!("the growth speed must be above 0");
    }
    let mut r = Rng::new(p.seed);
    let planar = g.planar();
    let volume = if planar { g.size[0] * g.size[1] } else { g.volume() };
    let rate = if planar { p.rate * g.h } else { p.rate };
    let continuous = p.nucleation == NUCLEATION_CONTINUOUS;
    // (place, birth time)
    let mut nuclei: Vec<([f64; 3], f64)> = if continuous {
        if !(rate > 0.0) {
            bail!("the nucleation rate must be above 0");
        }
        // Nothing nucleates once all is transformed: the time at which 1e-6 is left.
        let full = if planar { (3.0 * 13.8 / (PI * rate * p.growth.powi(2))).cbrt() } else { (3.0 * 13.8 / (PI * rate * p.growth.powi(3))).powf(0.25) };
        let end = if p.time > 0.0 { p.time.min(full) } else { full };
        let count = poisson(rate * volume * end, &mut r);
        if count > 40_000 {
            bail!("{count} nuclei: lower the nucleation rate (or raise the growth speed)");
        }
        (0..count).map(|_| (g.random_point(&mut r), r.uniform(0.0, end))).collect()
    } else {
        (0..p.nuclei.max(1)).map(|_| (g.random_point(&mut r), 0.0)).collect()
    };
    if planar {
        nuclei.iter_mut().for_each(|n| n.0[2] = 0.5 * g.h);
    }
    if nuclei.is_empty() {
        bail!("no nucleus appeared: raise the nucleation rate or the time");
    }
    let orients = grain_orientations(nuclei.len(), &p.orientations, p.spread_deg.to_radians(), &mut r);
    let frames: Vec<[[f64; 3]; 3]> = orients.iter().map(|q| cx::quat_to_matrix(*q)).collect();
    let front = p.front.as_str();
    // The time a grain takes to reach a point: its distance in the norm of its front.
    let reach = |i: usize, d: [f64; 3]| -> f64 {
        match front {
            FRONT_CUBE | FRONT_OCTAHEDRON => {
                let m = &frames[i];
                let q: [f64; 3] = std::array::from_fn(|k| (m[k][0] * d[0] + m[k][1] * d[1] + m[k][2] * d[2]).abs());
                if front == FRONT_CUBE {
                    q[0].max(q[1]).max(q[2])
                } else {
                    q[0] + q[1] + q[2]
                }
            }
            _ => norm(d),
        }
    };
    // A nucleus born after another grain passed its place never grows.
    let mut order: Vec<usize> = (0..nuclei.len()).collect();
    order.sort_by(|a, b| nuclei[*a].1.total_cmp(&nuclei[*b].1));
    let mut alive: Vec<usize> = Vec::new();
    for &i in &order {
        let (x, t) = nuclei[i];
        if !alive.iter().any(|&j| nuclei[j].1 + reach(j, g.delta(x, nuclei[j].0)) / p.growth < t) {
            alive.push(i);
        }
    }
    if alive.len() as f64 * g.len() as f64 > 4e10 {
        bail!("too many grains ({}) for this grid", alive.len());
    }
    let arrival: Vec<(u32, f32)> = per_cell(g, |x| {
        let mut best = (0u32, f64::INFINITY);
        for (k, &i) in alive.iter().enumerate() {
            let t = nuclei[i].1 + reach(i, g.delta(x, nuclei[i].0)) / p.growth;
            if t < best.1 {
                best = (k as u32, t);
            }
        }
        (best.0, best.1 as f32)
    });
    let stop = if p.time > 0.0 { p.time as f32 } else { f32::INFINITY };
    let labels: Vec<u32> = arrival.iter().map(|(k, t)| if *t <= stop { *k } else { u32::MAX }).collect();
    let (mut ids, found) = crate::synthetic::compact(&labels);
    let mut grain_orients: Vec<cx::Quat> = found.iter().filter(|l| **l != u32::MAX).map(|l| orients[alive[*l as usize]]).collect();
    let rest = found.iter().position(|l| *l == u32::MAX).map(|k| k as i32 + 1);
    if p.coarsening > 0 && rest.is_none() {
        coarsen(g, &mut ids, p.coarsening, p.temperature, &mut r);
        let (renumbered, kept) = crate::synthetic::compact(&ids.iter().map(|i| *i as u32).collect::<Vec<u32>>());
        grain_orients = kept.iter().map(|l| grain_orients[*l as usize - 1]).collect();
        ids = renumbered;
    }
    let mut phases = vec![1i32; g.len()];
    if let Some(m) = rest {
        // The untransformed material is the last feature.
        let last = found.len() as i32;
        for (c, id) in ids.iter_mut().enumerate() {
            if *id == m {
                *id = last;
                phases[c] = 2;
            } else if *id > m {
                *id -= 1;
            }
        }
        grain_orients.push(cx::IDENTITY);
    }
    let transformed = phases.iter().filter(|x| **x == 1).count() as f64 / g.len() as f64;
    let mut img = crate::synthetic::microstructure(g.n, [g.h; 3], ids, &grain_orients, laue);
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
    img.cell_data.insert(Array::new("TransformationTime", 1, ArrayData::F32(arrival.iter().map(|a| a.1).collect())));
    if rest.is_some() {
        img.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![Laue::UNKNOWN_CODE, laue.code(), Laue::UNKNOWN_CODE])));
    }
    note(&mut img, "TransformedFraction", vec![transformed]);
    note(&mut img, "PhaseFractions", if rest.is_some() { vec![0.0, transformed, 1.0 - transformed] } else { vec![0.0, 1.0] });
    if p.time > 0.0 && front == FRONT_ROUND {
        // The Johnson-Mehl-Avrami-Kolmogorov law for these kinetics.
        let (gt, t) = (p.growth * p.time, p.time);
        let extended = match (continuous, planar) {
            (false, false) => nuclei.len() as f64 / volume * 4.0 / 3.0 * PI * gt.powi(3),
            (false, true) => nuclei.len() as f64 / volume * PI * gt.powi(2),
            (true, false) => PI / 3.0 * rate * p.growth.powi(3) * t.powi(4),
            (true, true) => PI / 3.0 * rate * p.growth.powi(2) * t.powi(3),
        };
        note(&mut img, "ExpectedFraction", vec![1.0 - (-extended).exp()]);
    }
    Ok(DataSet::ImageData(img))
}

pub const PROCESS_DLA: &str = "Diffusion-limited";
pub const PROCESS_EDEN: &str = "Eden growth";
pub const PROCESS_BALLISTIC: &str = "Ballistic";
pub const GEOMETRY_CLUSTER: &str = "Clusters";
pub const GEOMETRY_DEPOSIT: &str = "Deposit on the base";

/// Aggregates grown one cell at a time. Diffusion-limited: a particle wanders at random and
/// sticks where it first touches the aggregate (branched, fractal clusters). Eden: a random
/// cell of the surface of the aggregate is added (compact clusters with a rough surface).
/// Ballistic: particles fly straight and stick on contact (porous deposits). From seeds in
/// the box, or as a deposit on its base.
pub fn aggregation(g: &Grid, process: &str, geometry: &str, seeds: usize, fraction: f64, seed: u64) -> Result<DataSet> {
    if !(fraction > 0.0 && fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let total = g.len();
    if total > 30_000_000 {
        bail!("aggregates are limited to 30 million cells");
    }
    let mut r = Rng::new(seed);
    let offs = offsets(g.planar(), false);
    let up = if g.planar() { 1 } else { 2 };
    let deposit = geometry == GEOMETRY_DEPOSIT;
    // A deposit does not wrap along its growth.
    let step = |c: usize, o: [i64; 3]| -> Option<usize> {
        if deposit {
            let v = g.ijk(c)[up] as i64 + o[up];
            if v < 0 || v >= g.n[up] as i64 {
                return None;
            }
        }
        shifted(g, c, o)
    };
    let mut ids = vec![0i32; total];
    let mut occupied = 0usize;
    if deposit {
        for c in 0..total {
            if g.ijk(c)[up] == 0 {
                ids[c] = 1;
                occupied += 1;
            }
        }
    } else {
        for k in 0..seeds.max(1) {
            let c = (r.random() * total as f64) as usize % total;
            if ids[c] == 0 {
                ids[c] = k as i32 + 1;
                occupied += 1;
            }
        }
    }
    let goal = ((fraction * total as f64) as usize).max(occupied).min(total);
    let touching = |ids: &[i32], c: usize| -> i32 { offs.iter().filter_map(|o| step(c, *o)).map(|q| ids[q]).find(|i| *i > 0).unwrap_or(0) };
    let random_cell = |r: &mut Rng| (r.random() * total as f64) as usize % total;
    let mut top = 0usize;
    match process {
        PROCESS_EDEN => {
            let mut surface: Vec<usize> = (0..total).filter(|c| ids[*c] == 0 && touching(&ids, *c) > 0).collect();
            while occupied < goal && !surface.is_empty() {
                let k = (r.random() * surface.len() as f64) as usize % surface.len();
                let c = surface.swap_remove(k);
                if ids[c] != 0 {
                    continue;
                }
                ids[c] = touching(&ids, c);
                occupied += 1;
                surface.extend(offs.iter().filter_map(|o| step(c, *o)).filter(|q| ids[*q] == 0));
            }
        }
        PROCESS_BALLISTIC if deposit => {
            while occupied < goal {
                // A particle falls down a column and stops at the first cell that touches the deposit.
                let mut p = g.ijk(random_cell(&mut r));
                p[up] = g.n[up] - 1;
                let mut c = g.index(p);
                if ids[c] != 0 || touching(&ids, c) > 0 {
                    break;
                }
                let mut down = [0i64; 3];
                down[up] = -1;
                loop {
                    let Some(below) = step(c, down) else { break };
                    c = below;
                    if touching(&ids, c) > 0 {
                        break;
                    }
                }
                ids[c] = 1;
                occupied += 1;
            }
        }
        PROCESS_BALLISTIC => {
            let far = 4 * g.n.iter().max().unwrap();
            let mut idle = 0usize;
            while occupied < goal && idle < 200_000 {
                let start = random_cell(&mut r);
                if ids[start] != 0 || touching(&ids, start) > 0 {
                    idle += 1;
                    continue;
                }
                let d = if g.planar() {
                    let a = r.uniform(0.0, std::f64::consts::TAU);
                    [a.cos(), a.sin(), 0.0]
                } else {
                    isotropic(&mut r)
                };
                let x0 = g.ijk(start).map(|v| v as f64 + 0.5);
                let mut c = start;
                let mut stuck = false;
                for s in 1..far {
                    let q: [i64; 3] = std::array::from_fn(|k| (x0[k] + d[k] * s as f64).floor() as i64);
                    let p = g.ijk(start);
                    let Some(next) = shifted(g, start, std::array::from_fn(|k| q[k] - p[k] as i64)) else { break };
                    if ids[next] != 0 {
                        stuck = true;
                        break;
                    }
                    c = next;
                    if touching(&ids, c) > 0 {
                        stuck = true;
                        break;
                    }
                }
                if stuck && ids[c] == 0 && touching(&ids, c) > 0 {
                    ids[c] = touching(&ids, c);
                    occupied += 1;
                    idle = 0;
                } else {
                    idle += 1;
                }
            }
        }
        _ => {
            let mut idle = 0usize;
            while occupied < goal && idle < 200_000 {
                let mut c = random_cell(&mut r);
                if deposit {
                    // Released a little above the deposit.
                    let mut p = g.ijk(c);
                    p[up] = (top + 4).min(g.n[up] - 1);
                    c = g.index(p);
                }
                if ids[c] != 0 {
                    idle += 1;
                    continue;
                }
                let mut stuck = touching(&ids, c);
                let mut walked = 0usize;
                while stuck == 0 && walked < 400_000 {
                    walked += 1;
                    let o = offs[(r.random() * offs.len() as f64) as usize % offs.len()];
                    match step(c, o) {
                        Some(q) if ids[q] == 0 => c = q,
                        Some(_) => {}
                        None if deposit => {}
                        None => break,
                    }
                    if deposit && g.ijk(c)[up] > top + 12 {
                        break;
                    }
                    stuck = touching(&ids, c);
                }
                if stuck > 0 {
                    ids[c] = stuck;
                    occupied += 1;
                    top = top.max(g.ijk(c)[up]);
                    idle = 0;
                } else {
                    idle += 1;
                }
            }
        }
    }
    let phases: Vec<i32> = ids.iter().map(|i| if *i > 0 { PHASE_SOLID } else { PHASE_MATRIX }).collect();
    Ok(DataSet::ImageData(phase_image(g, phases, Some(ids))))
}

/// A network of pipes assembled from tiles: every tile of a cubic lattice has an opening
/// towards each neighbour, open or closed on both sides alike, and the number of open sides
/// of a tile must be one of `degrees`. The openings start at random and are flipped at the
/// tiles that break the rule until none does.
pub fn pipe_network(g: &Grid, tile_cells: usize, pipe_radius: f64, degrees: &[f64], open: f64, seed: u64) -> Result<DataSet> {
    let m = tile_cells.max(2);
    let tiles: [usize; 3] = std::array::from_fn(|k| (g.n[k] / m).max(1));
    let count = tiles[0] * tiles[1] * tiles[2];
    let allowed: Vec<usize> = degrees.iter().map(|d| d.round().max(0.0) as usize).collect();
    if allowed.is_empty() {
        bail!("give the numbers of open sides a tile may have");
    }
    let mut r = Rng::new(seed);
    let tgrid = Grid { n: tiles, h: g.h * m as f64, size: tiles.map(|t| t as f64 * g.h * m as f64), periodic: g.periodic };
    let axes: Vec<usize> = (0..3).filter(|k| tiles[*k] > 1).collect();
    // The opening of tile t towards +axis: edges[axis][t].
    let other = |t: usize, a: usize| -> Option<usize> {
        let mut o = [0i64; 3];
        o[a] = 1;
        shifted(&tgrid, t, o)
    };
    let back = |t: usize, a: usize| -> Option<usize> {
        let mut o = [0i64; 3];
        o[a] = -1;
        shifted(&tgrid, t, o)
    };
    let mut edges: Vec<Vec<bool>> = (0..3).map(|a| (0..count).map(|t| axes.contains(&a) && other(t, a).is_some() && r.random() < open).collect()).collect();
    let degree =
        |edges: &[Vec<bool>], t: usize| -> usize { axes.iter().map(|&a| edges[a][t] as usize + back(t, a).map(|b| edges[a][b] as usize).unwrap_or(0)).sum() };
    let mut bad: Vec<usize> = (0..count).filter(|t| !allowed.contains(&degree(&edges, *t))).collect();
    let mut flips = 0usize;
    while !bad.is_empty() {
        flips += 1;
        if flips > 400 * count + 10_000 {
            bail!("no assembly found for these numbers of open sides: allow more of them");
        }
        let t = bad[(r.random() * bad.len() as f64) as usize % bad.len()];
        // Flip one of the openings of this tile.
        let sides: Vec<(usize, usize)> = axes.iter().flat_map(|&a| [other(t, a).map(|_| (a, t)), back(t, a).map(|b| (a, b))]).flatten().collect();
        if sides.is_empty() {
            bail!("no assembly found: a tile has no neighbour");
        }
        let (a, owner) = sides[(r.random() * sides.len() as f64) as usize % sides.len()];
        edges[a][owner] = !edges[a][owner];
        bad = (0..count).filter(|t| !allowed.contains(&degree(&edges, *t))).collect();
    }
    let side = g.h * m as f64;
    let radius = pipe_radius * side;
    let mut phases = vec![PHASE_MATRIX; g.len()];
    let mut pipes = 0;
    for &a in &axes {
        for t in 0..count {
            if edges[a][t] {
                pipes += 1;
                let from = tgrid.pos(t);
                let mut to = from;
                to[a] += side;
                g.stamp_segment(from, to, radius, |c, _, _| phases[c] = PHASE_SOLID);
            }
        }
    }
    let mut img = phase_image(g, phases, None);
    let mut all = Vec::new();
    for &a in &axes {
        for t in (0..count).filter(|t| edges[a][*t]) {
            let from = tgrid.pos(t);
            let mut to = from;
            to[a] += side;
            all.push(shapes::capsule(from, to, radius));
        }
    }
    shapes::keep(&mut img, &all, g.periodic);
    note(&mut img, "Pipes", vec![pipes as f64]);
    note(&mut img, "Tiles", vec![count as f64]);
    Ok(DataSet::ImageData(img))
}

pub const ROOT_BASE: &str = "Middle of the base";
pub const ROOT_CENTRE: &str = "Centre of the box";

pub struct BranchingParams {
    pub attractors: usize,
    /// Length of a segment of the tree.
    pub step: f64,
    /// A point pulls the tree only within this distance (0: at any distance).
    pub influence: f64,
    /// A point is used up when the tree comes this near (0: twice the step).
    pub kill: f64,
    pub roots: usize,
    pub root: String,
    pub tip_radius: f64,
    /// The exponent n of `r^n = sum of the r^n of the branches` (3: Murray's law of vessels).
    pub exponent: f64,
    pub seed: u64,
}

/// A branching tree (vessels, roots, corals) grown by space colonization: points scattered
/// in the box pull the nearest node of the tree, each pulled node grows a segment towards
/// the mean direction of its points, and points the tree has reached are removed. The radii
/// follow from the tips by `r^n = sum of the r^n of the branches`.
pub fn branching_network(g: &Grid, p: &BranchingParams) -> Result<DataSet> {
    if !(p.step > 0.0 && p.tip_radius > 0.0) {
        bail!("the step and the tip radius must be above 0");
    }
    let mut r = Rng::new(p.seed);
    let flat = |mut x: [f64; 3]| {
        if g.planar() {
            x[2] = 0.5 * g.h;
        }
        x
    };
    let mut points: Vec<[f64; 3]> = (0..p.attractors).map(|_| flat(g.random_point(&mut r))).collect();
    let up = if g.planar() { 1 } else { 2 };
    let mut nodes: Vec<[f64; 3]> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    for k in 0..p.roots.max(1) {
        let mut x = g.size.map(|s| 0.5 * s);
        if p.root == ROOT_BASE {
            x[up] = 0.0;
        }
        if k > 0 {
            x = g.random_point(&mut r);
            if p.root == ROOT_BASE {
                x[up] = 0.0;
            }
        }
        nodes.push(flat(x));
        parent.push(usize::MAX);
    }
    let kill = if p.kill > 0.0 { p.kill } else { 2.0 * p.step };
    let influence = if p.influence > 0.0 { p.influence } else { f64::INFINITY };
    let mut nearest: Vec<(usize, f64)> =
        points.iter().map(|x| (0..nodes.len()).map(|n| (n, norm(sub(*x, nodes[n])))).fold((0, f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a })).collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for _ in 0..20_000 {
        let mut pull: std::collections::BTreeMap<usize, [f64; 3]> = std::collections::BTreeMap::new();
        for (x, (n, d)) in points.iter().zip(&nearest) {
            if *d < influence {
                let e = pull.entry(*n).or_insert([0.0; 3]);
                *e = add(*e, unit(sub(*x, nodes[*n])));
            }
        }
        let first_new = nodes.len();
        for (n, dir) in pull {
            if norm(dir) < 1e-9 {
                continue;
            }
            let x = flat(add(nodes[n], scale(unit(dir), p.step)));
            // The same branch is not grown twice from a node.
            if children[n].iter().any(|c| norm(sub(nodes[*c], x)) < 0.25 * p.step) {
                continue;
            }
            children[n].push(nodes.len());
            nodes.push(x);
            parent.push(n);
            children.push(Vec::new());
        }
        if nodes.len() == first_new {
            break;
        }
        if nodes.len() > 400_000 {
            bail!("the tree has more than 400 000 nodes: raise the step");
        }
        let mut keep = Vec::with_capacity(points.len());
        let mut kept_nearest = Vec::with_capacity(points.len());
        for (x, mut best) in points.iter().zip(nearest.iter().copied()) {
            for n in first_new..nodes.len() {
                let d = norm(sub(*x, nodes[n]));
                if d < best.1 {
                    best = (n, d);
                }
            }
            if best.1 >= kill {
                keep.push(*x);
                kept_nearest.push(best);
            }
        }
        points = keep;
        nearest = kept_nearest;
        if points.is_empty() {
            break;
        }
    }
    // Radii from the tips down.
    let n_exp = p.exponent.max(0.5);
    let mut power = vec![0.0; nodes.len()];
    let mut tips = 0;
    for n in (0..nodes.len()).rev() {
        if children[n].is_empty() {
            power[n] = p.tip_radius.powf(n_exp);
            tips += 1;
        }
        if parent[n] != usize::MAX {
            power[parent[n]] += power[n];
        }
    }
    let radius: Vec<f64> = power.iter().map(|x| x.powf(1.0 / n_exp)).collect();
    let open = Grid { periodic: false, ..g.clone() };
    let mut phases = vec![PHASE_MATRIX; g.len()];
    let mut thick = vec![0.0f32; g.len()];
    for n in 0..nodes.len() {
        if parent[n] != usize::MAX {
            open.stamp_segment(nodes[parent[n]], nodes[n], radius[n].max(0.5 * g.h), |c, _, _| {
                phases[c] = PHASE_SOLID;
                thick[c] = thick[c].max(radius[n] as f32);
            });
        }
    }
    let mut img = phase_image(g, phases, None);
    let all: Vec<shapes::Shape> =
        (0..nodes.len()).filter(|n| parent[*n] != usize::MAX).map(|n| shapes::capsule(nodes[parent[n]], nodes[n], radius[n].max(0.5 * g.h))).collect();
    shapes::keep(&mut img, &all, false);
    img.cell_data.insert(Array::new("BranchRadius", 1, ArrayData::F32(thick)));
    note(&mut img, "Nodes", vec![nodes.len() as f64]);
    note(&mut img, "Tips", vec![tips as f64]);
    note(&mut img, "RootRadius", vec![radius.iter().cloned().fold(0.0, f64::max)]);
    Ok(DataSet::ImageData(img))
}

pub const LATTICE_GRID: &str = "Squares or cubes";
pub const LATTICE_HEXAGONS: &str = "Hexagons";
pub const LATTICE_TRIANGLES: &str = "Triangles";
pub const LATTICE_PENROSE: &str = "Penrose rhombi";
pub const LATTICE_EIGHTFOLD: &str = "Squares and rhombi (eightfold)";
pub const LATTICE_BCC: &str = "Truncated octahedra";
pub const LATTICE_FCC: &str = "Rhombic dodecahedra";
pub const LATTICES: &[&str] = &[LATTICE_GRID, LATTICE_HEXAGONS, LATTICE_TRIANGLES, LATTICE_PENROSE, LATTICE_EIGHTFOLD, LATTICE_BCC, LATTICE_FCC];

/// A tiling of the plane by rhombi that never repeats (fivefold: the Penrose tiling;
/// eightfold: squares and rhombi), drawn on the columns of cells of the grid: the tile of
/// every column. Families of parallel lines, one for each direction of a star, cut the
/// plane; every crossing of two lines is a rhombus of the tiling, placed by the numbers of
/// the lines of every family that lie before the crossing.
fn rhombus_tiling(g: &Grid, fold: usize, size: f64, r: &mut dyn Draws) -> Vec<u32> {
    let star: Vec<[f64; 2]> =
        (0..fold).map(|j| if fold == 5 { 2.0 * PI * j as f64 / 5.0 } else { PI * j as f64 / fold as f64 }).map(|a| [a.cos(), a.sin()]).collect();
    let mut shift: Vec<f64> = (0..fold).map(|_| r.random()).collect();
    if fold == 5 {
        // The shifts of the five families sum to nothing: the tiling is then Penrose's.
        shift[4] = -shift[..4].iter().sum::<f64>();
    }
    let centre = [0.5 * g.size[0], 0.5 * g.size[1]];
    let half = (0.5 * g.size[0].hypot(g.size[1]) + (fold as f64 + 2.0) * size) / (size * 0.5 * fold as f64);
    let (nx, ny) = (g.n[0], g.n[1]);
    let mut tile = vec![u32::MAX; nx * ny];
    let mut next = 0u32;
    for a in 0..fold {
        for b in a + 1..fold {
            let (ea, eb) = (star[a], star[b]);
            let det = ea[0] * eb[1] - ea[1] * eb[0];
            let lines = |s: f64| ((-half + s).ceil() as i64 - 1)..=((half + s).floor() as i64 + 1);
            for ka in lines(shift[a]) {
                for kb in lines(shift[b]) {
                    let (ca, cb) = (ka as f64 - shift[a], kb as f64 - shift[b]);
                    let x = [(ca * eb[1] - cb * ea[1]) / det, (cb * ea[0] - ca * eb[0]) / det];
                    if x[0].hypot(x[1]) > half {
                        continue;
                    }
                    let mut base = [0.0; 2];
                    for (j, e) in star.iter().enumerate() {
                        let k = if j == a {
                            ka as f64
                        } else if j == b {
                            kb as f64
                        } else {
                            (x[0] * e[0] + x[1] * e[1] + shift[j]).ceil()
                        };
                        base = [base[0] + k * e[0], base[1] + k * e[1]];
                    }
                    let w = [centre[0] + size * base[0], centre[1] + size * base[1]];
                    let corners = [
                        w,
                        [w[0] + size * ea[0], w[1] + size * ea[1]],
                        [w[0] + size * (ea[0] + eb[0]), w[1] + size * (ea[1] + eb[1])],
                        [w[0] + size * eb[0], w[1] + size * eb[1]],
                    ];
                    let lo = [corners.iter().map(|c| c[0]).fold(f64::INFINITY, f64::min), corners.iter().map(|c| c[1]).fold(f64::INFINITY, f64::min)];
                    let hi = [corners.iter().map(|c| c[0]).fold(f64::NEG_INFINITY, f64::max), corners.iter().map(|c| c[1]).fold(f64::NEG_INFINITY, f64::max)];
                    let range = |k: usize, n: usize| {
                        ((lo[k] / g.h - 0.5).ceil().max(0.0) as usize)..(((hi[k] / g.h - 0.5).floor() + 1.0).clamp(0.0, n as f64) as usize)
                    };
                    let mut used = false;
                    for j in range(1, ny) {
                        for i in range(0, nx) {
                            let q = [(i as f64 + 0.5) * g.h - w[0], (j as f64 + 0.5) * g.h - w[1]];
                            let (u, v) = ((q[0] * eb[1] - q[1] * eb[0]) / (size * det), (ea[0] * q[1] - ea[1] * q[0]) / (size * det));
                            if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
                                tile[i + nx * j] = next;
                                used = true;
                            }
                        }
                    }
                    if used {
                        next += 1;
                    }
                }
            }
        }
    }
    tile
}

/// The tile of every cell of the grid for a lattice of tiles of size `size`, and the number
/// of tiles. Tilings of the plane are stacked in layers when the grid has several.
fn tiles(g: &Grid, lattice: &str, size: f64, r: &mut dyn Draws) -> Result<(Vec<u32>, usize)> {
    if !(size >= 3.0 * g.h) {
        bail!("the tiles must be at least three cells across");
    }
    let planar_lattice = !matches!(lattice, LATTICE_BCC | LATTICE_FCC);
    if !planar_lattice && g.planar() {
        bail!("{lattice} need more than one layer of cells");
    }
    let layer = |z: f64| if g.planar() { 0 } else { (z / size).floor() as i64 };
    let flat: Option<Vec<u32>> = match lattice {
        LATTICE_PENROSE => Some(rhombus_tiling(g, 5, size, r)),
        LATTICE_EIGHTFOLD => Some(rhombus_tiling(g, 4, size, r)),
        _ => None,
    };
    let row = size * 3f64.sqrt() / 2.0;
    let keys: Vec<[i64; 4]> = (0..g.len())
        .into_par_iter()
        .map(|c| {
            let x = g.pos(c);
            match lattice {
                LATTICE_PENROSE | LATTICE_EIGHTFOLD => [flat.as_ref().unwrap()[c % (g.n[0] * g.n[1])] as i64, layer(x[2]), 0, 0],
                LATTICE_TRIANGLES => {
                    let (u, v) = (x[0] / size - x[1] / (size * 3f64.sqrt()), 2.0 * x[1] / (size * 3f64.sqrt()));
                    [u.floor() as i64, v.floor() as i64, layer(x[2]), ((u - u.floor()) + (v - v.floor()) < 1.0) as i64]
                }
                LATTICE_BCC => {
                    let y = x.map(|t| t / size);
                    let (a, b) = (y.map(f64::round), y.map(|t| t.floor() + 0.5));
                    let near = |q: [f64; 3]| (0..3).map(|k| (y[k] - q[k]).powi(2)).sum::<f64>();
                    let q = if near(a) <= near(b) { a } else { b };
                    [(2.0 * q[0]) as i64, (2.0 * q[1]) as i64, (2.0 * q[2]) as i64, 0]
                }
                LATTICE_FCC => {
                    // The points of whole coordinates of even sum, in units of half the size.
                    let y = x.map(|t| 2.0 * t / size);
                    let mut q = y.map(|t| t.round() as i64);
                    if (q[0] + q[1] + q[2]).rem_euclid(2) == 1 {
                        let k = (0..3).max_by(|a, b| (y[*a] - q[*a] as f64).abs().total_cmp(&(y[*b] - q[*b] as f64).abs())).unwrap();
                        q[k] += if y[k] > q[k] as f64 { 1 } else { -1 };
                    }
                    [q[0], q[1], q[2], 0]
                }
                _ => {
                    // Hexagons: the nearest point of a triangular lattice.
                    let j0 = (x[1] / row).round() as i64;
                    let mut best = (f64::INFINITY, [0i64; 2]);
                    for j in j0 - 1..=j0 + 1 {
                        let off = if j.rem_euclid(2) == 1 { 0.5 * size } else { 0.0 };
                        let i0 = ((x[0] - off) / size).round() as i64;
                        for i in i0 - 1..=i0 + 1 {
                            let d = (x[0] - (i as f64 * size + off)).powi(2) + (x[1] - j as f64 * row).powi(2);
                            if d < best.0 {
                                best = (d, [i, j]);
                            }
                        }
                    }
                    [best.1[0], best.1[1], layer(x[2]), 0]
                }
            }
        })
        .collect();
    let mut index: std::collections::HashMap<[i64; 4], u32> = std::collections::HashMap::new();
    let tile: Vec<u32> = keys
        .iter()
        .map(|k| {
            let n = index.len() as u32;
            *index.entry(*k).or_insert(n)
        })
        .collect();
    Ok((tile, index.len()))
}

/// The automaton on the tiles of a lattice drawn over the grid: two tiles are neighbours
/// when they share a side (a face), or, with all the neighbours, a corner too.
fn automaton_on_tiles(g: &Grid, p: &AutomatonParams, r: &mut dyn Draws) -> Result<DataSet> {
    let (tile, count) = tiles(g, &p.lattice, p.tile, r)?;
    let moore = p.neighbours != NEIGHBOURS_FACES;
    let offs = offsets(g.planar(), moore);
    let mut touching: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    for c in 0..g.len() {
        for o in &offs {
            if let Some(q) = shifted(g, c, *o) {
                if tile[q] > tile[c] {
                    *touching.entry((tile[c], tile[q])).or_insert(0) += 1;
                }
            }
        }
    }
    // Sides, not corners: the two tiles touch along a stretch of cells.
    let across = p.tile / g.h;
    let least = if moore {
        1
    } else if g.planar() {
        (0.4 * across).max(2.0) as u32
    } else {
        (0.1 * across * across).max(2.0) as u32
    };
    let mut near: Vec<Vec<u32>> = vec![Vec::new(); count];
    for ((a, b), n) in &touching {
        if *n >= least {
            near[*a as usize].push(*b);
            near[*b as usize].push(*a);
        }
    }
    let mut state: Vec<u8> = (0..count).map(|_| (r.random() < p.initial_fraction) as u8).collect();
    let (births, survivals) = if p.rule == RULE_CUSTOM { (counts_listed(&p.births)?, counts_listed(&p.survivals)?) } else { (Vec::new(), Vec::new()) };
    for _ in 0..p.steps {
        let live: Vec<usize> = near.iter().map(|n| n.iter().map(|q| state[*q as usize] as usize).sum()).collect();
        let next: Vec<u8> = match p.rule.as_str() {
            RULE_CONSERVED => {
                let alive = state.iter().filter(|s| **s == 1).count();
                let score: Vec<f32> = (0..count)
                    .map(|t| -((live[t] + state[t] as usize) as f32 / (near[t].len() + 1) as f32) - 0.01 * state[t] as f32 - 0.005 * r.random() as f32)
                    .collect();
                below_quantile(&score, alive as f64 / count as f64).iter().map(|x| (*x == PHASE_SOLID) as u8).collect()
            }
            RULE_CUSTOM => (0..count).map(|t| if state[t] == 1 { survivals.contains(&live[t]) } else { births.contains(&live[t]) } as u8).collect(),
            rule => (0..count)
                .map(|t| {
                    let (s, half) = (live[t] + state[t] as usize, (near[t].len() + 1).div_ceil(2));
                    if rule == RULE_MAJORITY {
                        (s >= half) as u8
                    } else {
                        (s > half || s + 1 == half) as u8
                    }
                })
                .collect(),
        };
        if next == state {
            break;
        }
        state = next;
    }
    let phases = tile.iter().map(|t| if state[*t as usize] == 1 { PHASE_SOLID } else { PHASE_MATRIX }).collect();
    let mut img = phase_image(g, phases, Some(tile.iter().map(|t| *t as i32 + 1).collect()));
    note(&mut img, "Tiles", vec![count as f64]);
    note(&mut img, "NeighboursPerTile", vec![near.iter().map(|n| n.len()).sum::<usize>() as f64 / count.max(1) as f64]);
    Ok(DataSet::ImageData(img))
}

/// Grain growth: again and again a cell takes the grain of one of its neighbours when that
/// leaves it with no more unlike neighbours than before (at a temperature above 0, sometimes
/// even when it leaves more). Boundaries straighten, small grains vanish, the others grow.
fn coarsen(g: &Grid, ids: &mut [i32], sweeps: usize, temperature: f64, r: &mut dyn Draws) {
    let offs = offsets(g.planar(), true);
    let n = g.len();
    for _ in 0..sweeps * n {
        let c = ((r.random() * n as f64) as usize).min(n - 1);
        let Some(q) = shifted(g, c, offs[((r.random() * offs.len() as f64) as usize).min(offs.len() - 1)]) else { continue };
        let (mine, other) = (ids[c], ids[q]);
        if mine == other {
            continue;
        }
        let (mut same, mut gained) = (0i32, 0i32);
        for o in &offs {
            if let Some(k) = shifted(g, c, *o) {
                same += (ids[k] == mine) as i32;
                gained += (ids[k] == other) as i32;
            }
        }
        let rise = (same - gained) as f64;
        if rise <= 0.0 || (temperature > 0.0 && r.random() < (-rise / temperature).exp()) {
            ids[c] = other;
        }
    }
}

/// Polarized blocks: cubes that each deform in one way only, long along one axis and short
/// along the two others or the reverse, so that a block pushes its neighbours out through
/// two opposite faces and pulls them in through the four others (or the reverse). Blocks
/// fit together when every face moves the same way on both sides: along every line of blocks
/// the motion alternates. The blocks are drawn as cubes with a pore, long along the axis
/// that pushes out; `BlockAxis` and `BlockSign` give the state of every block.
pub fn polarized_blocks(g: &Grid, tile_cells: usize, pore: f64, seed: u64) -> Result<DataSet> {
    let m = tile_cells.max(4);
    let blocks: [usize; 3] = std::array::from_fn(|k| (g.n[k] / m).max(1));
    let axes: Vec<usize> = (0..3).filter(|k| blocks[*k] > 1).collect();
    if axes.len() < 3 {
        bail!("polarized blocks need at least two blocks along every axis: more cells, or fewer cells per block");
    }
    if g.periodic && blocks.iter().any(|b| b % 2 == 1) {
        bail!("a periodic assembly needs an even number of blocks along every axis");
    }
    let mut r = Rng::new(seed);
    // The motion along each axis: a sign for every line of blocks, alternating along the line.
    let lines = [blocks[1] * blocks[2], blocks[0] * blocks[2], blocks[0] * blocks[1]];
    let mut sign: [Vec<i8>; 3] = std::array::from_fn(|a| (0..lines[a]).map(|_| if r.random() < 0.5 { 1 } else { -1 }).collect());
    let line = |a: usize, b: [usize; 3]| -> usize {
        match a {
            0 => b[1] + blocks[1] * b[2],
            1 => b[0] + blocks[0] * b[2],
            _ => b[0] + blocks[0] * b[1],
        }
    };
    let alternate = |k: usize| if k % 2 == 0 { 1i8 } else { -1 };
    let strain = |sign: &[Vec<i8>; 3], b: [usize; 3]| -> [i8; 3] { std::array::from_fn(|a| sign[a][line(a, b)] * alternate(b[a])) };
    let all: Vec<[usize; 3]> =
        (0..blocks[0] * blocks[1] * blocks[2]).map(|t| [t % blocks[0], (t / blocks[0]) % blocks[1], t / (blocks[0] * blocks[1])]).collect();
    // A block whose three motions have the same sign is no block: lines are flipped until none is left.
    let bad = |sign: &[Vec<i8>; 3], b: [usize; 3]| {
        let s = strain(sign, b);
        s[0] == s[1] && s[1] == s[2]
    };
    let mut tries = 0usize;
    loop {
        let wrong: Vec<[usize; 3]> = all.iter().filter(|b| bad(&sign, **b)).copied().collect();
        if wrong.is_empty() {
            break;
        }
        tries += 1;
        if tries > 200 * all.len() + 10_000 {
            bail!("no assembly found: change the seed");
        }
        let b = wrong[((r.random() * wrong.len() as f64) as usize).min(wrong.len() - 1)];
        // The flip of one of the three lines through the block that leaves the fewest wrong blocks.
        let mut best = (usize::MAX, 0usize);
        for a in 0..3 {
            let l = line(a, b);
            sign[a][l] = -sign[a][l];
            let left = (0..blocks[a])
                .filter(|k| {
                    let mut q = b;
                    q[a] = *k;
                    bad(&sign, q)
                })
                .count();
            sign[a][l] = -sign[a][l];
            if left < best.0 || (left == best.0 && r.random() < 0.4) {
                best = (left, a);
            }
        }
        let a = if r.random() < 0.1 { ((r.random() * 3.0) as usize).min(2) } else { best.1 };
        let l = line(a, b);
        sign[a][l] = -sign[a][l];
    }
    let side = g.h * m as f64;
    let pore = pore.clamp(0.05, 0.45);
    let cells: Vec<(i32, i32, i8, i8)> = per_cell(g, |x| {
        let b: [usize; 3] = std::array::from_fn(|k| ((x[k] / side) as usize).min(blocks[k] - 1));
        let s = strain(&sign, b);
        // The axis whose motion differs from the two others.
        let odd = (0..3).find(|a| s[*a] != s[(a + 1) % 3] && s[*a] != s[(a + 2) % 3]).unwrap_or(0);
        let d: f64 = (0..3)
            .map(|k| {
                let u = (x[k] - (b[k] as f64 + 0.5) * side) / side;
                let semi = if (k == odd) == (s[odd] > 0) { 1.5 * pore } else { 0.75 * pore };
                (u / semi.min(0.47)).powi(2)
            })
            .sum();
        let number = (b[0] + blocks[0] * (b[1] + blocks[1] * b[2])) as i32 + 1;
        (if d < 1.0 { PHASE_MATRIX } else { PHASE_SOLID }, number, odd as i8, s[odd])
    });
    let mut img = phase_image(g, cells.iter().map(|c| c.0).collect(), Some(cells.iter().map(|c| c.1).collect()));
    img.cell_data.insert(Array::new("BlockAxis", 1, ArrayData::I32(cells.iter().map(|c| c.2 as i32).collect())));
    img.cell_data.insert(Array::new("BlockSign", 1, ArrayData::I32(cells.iter().map(|c| c.3 as i32).collect())));
    note(&mut img, "Blocks", vec![all.len() as f64]);
    Ok(DataSet::ImageData(img))
}
