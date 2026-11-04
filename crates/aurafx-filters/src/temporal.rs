//! Temporal filters: they read their input at several times through a [`TemporalCtx`]
//! (Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator, Temporal Statistics,
//! Particle Tracer / Particle Path / Streak Line).

use crate::probe::{Hit, Locator};
use crate::simplex::parts;
use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::any::Any;
use std::sync::{Arc, Mutex};

/// What a temporal filter can ask of the pipeline while it runs.
pub struct TemporalCtx<'a> {
    /// The time the output is requested for.
    pub time: f64,
    /// Time steps the input provides (empty for data that does not change over time).
    pub input_times: &'a [f64],
    /// The input evaluated at any time (the upstream pipeline is re-run as needed).
    pub fetch: &'a (dyn Fn(f64) -> Result<Arc<DataSet>> + Sync),
    /// Other inputs (multi-input temporal filters, e.g. particle seeds) at the current time.
    pub extra: &'a [Arc<DataSet>],
    /// Per-node memory kept between runs (e.g. particles already advanced).
    pub state: &'a Mutex<Option<Box<dyn Any + Send>>>,
    /// Fingerprint of the upstream pipeline; state saved under another key is stale.
    pub key: u64,
}

impl TemporalCtx<'_> {
    /// Index of the last input step at or before `t` (0 before the first).
    pub fn step_at(&self, t: f64) -> usize {
        let eps = 1e-12 * (1.0 + t.abs());
        self.input_times.iter().rposition(|&x| x <= t + eps).unwrap_or(0)
    }
}

const EPS: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * (1.0 + a.abs().max(b.abs()))
}

// ------------------------------------------------------------------ interpolator

/// Output times of Temporal Interpolator: the input steps, or a regular spacing.
pub fn interpolator_times(interval: f64, input: &[f64]) -> Vec<f64> {
    let (Some(&a), Some(&b)) = (input.first(), input.last()) else { return Vec::new() };
    if interval <= 0.0 || b <= a {
        return input.to_vec();
    }
    let n = ((b - a) / interval + 1e-9).floor() as usize;
    let mut v: Vec<f64> = (0..=n).map(|k| a + k as f64 * interval).collect();
    if !close(*v.last().unwrap(), b) {
        v.push(b);
    }
    v
}

fn lerp_attrs(a: &mut Attributes, b: &Attributes, w: f64) {
    for arr in a.arrays.values_mut() {
        let Some(other) = b.get(&arr.name) else { continue };
        if other.ncomp != arr.ncomp || other.data.len() != arr.data.len() || !arr.scalar_type().is_float() {
            continue; // integer arrays (ids, flags) keep the earlier step's values
        }
        let x = arr.data.to_f64_vec();
        let y = other.data.to_f64_vec();
        let v: Vec<f64> = x.iter().zip(&y).map(|(p, q)| p + w * (q - p)).collect();
        arr.data = ArrayData::from_f64(arr.scalar_type(), &v);
    }
}

/// Linear blend of two datasets with the same structure (`w` = 0 gives `a`).
pub fn interpolate_datasets(a: &DataSet, b: &DataSet, w: f64) -> DataSet {
    match (a, b) {
        (DataSet::MultiBlock(ma), DataSet::MultiBlock(mb)) => {
            let mut out = ma.clone();
            for (k, (_, blk)) in out.blocks.iter_mut().enumerate() {
                if let (Some(x), Some((_, Some(y)))) = (blk.as_mut(), mb.blocks.get(k)) {
                    *x = interpolate_datasets(x, y, w);
                }
            }
            DataSet::MultiBlock(out)
        }
        _ if a.n_points() == b.n_points() && a.n_cells() == b.n_cells() && a.type_name() == b.type_name() => {
            let mut out = a.clone();
            if let (Some(pa), Some(pb)) = (out.point_data_mut(), b.point_data()) {
                lerp_attrs(pa, pb, w);
            }
            if let (Some(ca), Some(cb)) = (out.cell_data_mut(), b.cell_data()) {
                lerp_attrs(ca, cb, w);
            }
            // Moving meshes: blend explicit point coordinates.
            let pts_b = match b {
                DataSet::UnstructuredGrid(g) => Some(&g.points),
                DataSet::PolyData(p) => Some(&p.points),
                DataSet::StructuredGrid(g) => Some(&g.points),
                _ => None,
            };
            let pts_a = match &mut out {
                DataSet::UnstructuredGrid(g) => Some(&mut g.points),
                DataSet::PolyData(p) => Some(&mut p.points),
                DataSet::StructuredGrid(g) => Some(&mut g.points),
                _ => None,
            };
            if let (Some(pa), Some(pb)) = (pts_a, pts_b) {
                if pa.data != pb.data {
                    let x = pa.data.to_f64_vec();
                    let y = pb.data.to_f64_vec();
                    let v: Vec<f64> = x.iter().zip(&y).map(|(p, q)| p + w * (q - p)).collect();
                    pa.data = ArrayData::from_f64(pa.data.scalar_type(), &v);
                }
            }
            out
        }
        // Different structure: no meaningful blend, show the earlier step.
        _ => a.clone(),
    }
}

pub fn temporal_interpolator(ctx: &TemporalCtx) -> Result<DataSet> {
    let t = ctx.time;
    let times = ctx.input_times;
    if times.len() < 2 || t <= times[0] || t >= times[times.len() - 1] {
        return Ok((*(ctx.fetch)(t)?).clone());
    }
    let k = ctx.step_at(t);
    let (t0, t1) = (times[k], times[(k + 1).min(times.len() - 1)]);
    if close(t, t0) || t1 <= t0 {
        return Ok((*(ctx.fetch)(t0)?).clone());
    }
    if close(t, t1) {
        return Ok((*(ctx.fetch)(t1)?).clone());
    }
    let (a, b) = ((ctx.fetch)(t0)?, (ctx.fetch)(t1)?);
    Ok(interpolate_datasets(&a, &b, (t - t0) / (t1 - t0)))
}

// ------------------------------------------------------------------ shift / scale

pub fn shift_scale_times(pre: f64, scale: f64, post: f64, input: &[f64]) -> Vec<f64> {
    input.iter().map(|t| (t + pre) * scale + post).collect()
}

pub fn temporal_shift_scale(ctx: &TemporalCtx, pre: f64, scale: f64, post: f64) -> Result<DataSet> {
    if scale == 0.0 {
        return Err(anyhow!("Temporal Shift Scale: scale must not be 0"));
    }
    Ok((*(ctx.fetch)((ctx.time - post) / scale - pre)?).clone())
}

// ------------------------------------------------------------------ array operator

pub struct ArrayOp<'a> {
    pub points: bool,
    pub array: &'a str,
    pub op: &'a str,
    pub relative: bool,
    pub shift: i64,
    pub first: i64,
    pub second: i64,
    pub suffix: &'a str,
}

/// Combine an array at two time steps: `first op second`, as ParaView's Temporal Array
/// Operator. Relative mode compares the current step with the step `shift` away (clamped to
/// the available steps, so the first step compares with itself instead of failing).
pub fn temporal_array_operator(ctx: &TemporalCtx, o: &ArrayOp) -> Result<DataSet> {
    let n = ctx.input_times.len();
    if n == 0 {
        return Err(anyhow!("Temporal Array Operator needs data with time steps"));
    }
    let clamp = |i: i64| i.clamp(0, n as i64 - 1) as usize;
    let (i1, i2) = if o.relative {
        let cur = ctx.step_at(ctx.time) as i64;
        (cur as usize, clamp(cur + o.shift))
    } else {
        (clamp(o.first), clamp(o.second))
    };
    let a = (ctx.fetch)(ctx.input_times[i1])?;
    let b = (ctx.fetch)(ctx.input_times[i2])?;
    let suffix = if o.suffix.is_empty() {
        match o.op {
            "+" => "_add",
            "-" => "_sub",
            "*" => "_mul",
            _ => "_div",
        }
        .to_string()
    } else {
        o.suffix.to_string()
    };
    let op = o.op.to_string();
    let mut out = (*a).clone();
    let mut missing = false;
    let mut apply = |dst: &mut DataSet, other: &DataSet| {
        let src = if o.points { other.point_data() } else { other.cell_data() };
        let attrs = if o.points { dst.point_data_mut() } else { dst.cell_data_mut() };
        let (Some(attrs), Some(src)) = (attrs, src) else { return };
        let (Some(x), Some(y)) = (attrs.get(o.array), src.get(o.array)) else {
            missing = true;
            return;
        };
        if x.data.len() != y.data.len() {
            missing = true;
            return;
        }
        let (xv, yv) = (x.data.to_f64_vec(), y.data.to_f64_vec());
        let v: Vec<f64> = xv
            .iter()
            .zip(&yv)
            .map(|(p, q)| match op.as_str() {
                "+" => p + q,
                "-" => p - q,
                "*" => p * q,
                _ => p / q,
            })
            .collect();
        let mut arr = Array::new(format!("{}{suffix}", o.array), x.ncomp, ArrayData::F64(v));
        arr.component_names = x.component_names.clone();
        attrs.insert(arr);
    };
    match (&mut out, &*b) {
        (DataSet::MultiBlock(ma), DataSet::MultiBlock(mb)) => {
            let others: Vec<&DataSet> = mb.leaves().into_iter().map(|(_, l)| l).collect();
            let mut k = 0;
            fn walk(m: &mut MultiBlock, f: &mut dyn FnMut(&mut DataSet)) {
                for (_, b) in m.blocks.iter_mut() {
                    match b {
                        Some(DataSet::MultiBlock(inner)) => walk(inner, f),
                        Some(d) => f(d),
                        None => {}
                    }
                }
            }
            walk(ma, &mut |d| {
                if let Some(o) = others.get(k) {
                    apply(d, o);
                }
                k += 1;
            });
        }
        (d, other) => apply(d, other),
    }
    if missing {
        return Err(anyhow!("array {:?} is missing or changes size between the chosen time steps", o.array));
    }
    Ok(out)
}

// ------------------------------------------------------------------ statistics

/// Average, minimum, maximum and standard deviation of every array over all time steps
/// (`<name>_average`, `_minimum`, `_maximum`, `_stddev`, as ParaView's Temporal Statistics).
pub fn temporal_statistics(ctx: &TemporalCtx) -> Result<DataSet> {
    struct Acc {
        sum: Vec<f64>,
        sq: Vec<f64>,
        min: Vec<f64>,
        max: Vec<f64>,
        ncomp: usize,
    }
    let times: Vec<f64> = if ctx.input_times.is_empty() { vec![ctx.time] } else { ctx.input_times.to_vec() };
    let mut template: Option<DataSet> = None;
    let mut accs: indexmap::IndexMap<(usize, bool, String), Acc> = indexmap::IndexMap::new();
    for &t in &times {
        let ds = (ctx.fetch)(t)?;
        let leaves = crate::selection::flat_leaves(&ds);
        if template.is_none() {
            template = Some((*ds).clone());
        }
        for (li, (leaf, _, _)) in leaves.iter().enumerate() {
            for (is_cell, attrs) in [(false, leaf.point_data()), (true, leaf.cell_data())] {
                let Some(attrs) = attrs else { continue };
                for a in attrs.iter().filter(|a| a.scalar_type() != ScalarType::String) {
                    let len = a.data.len();
                    let e = accs.entry((li, is_cell, a.name.clone())).or_insert_with(|| Acc {
                        sum: vec![0.0; len],
                        sq: vec![0.0; len],
                        min: vec![f64::INFINITY; len],
                        max: vec![f64::NEG_INFINITY; len],
                        ncomp: a.ncomp,
                    });
                    if e.sum.len() != len {
                        continue; // topology changed; skip this array at this step
                    }
                    for i in 0..len {
                        let v = a.data.get_f64(i);
                        e.sum[i] += v;
                        e.sq[i] += v * v;
                        e.min[i] = e.min[i].min(v);
                        e.max[i] = e.max[i].max(v);
                    }
                }
            }
        }
    }
    let mut out = template.ok_or_else(|| anyhow!("no data"))?;
    let nf = times.len() as f64;
    let fill = |leaf: &mut DataSet, li: usize| {
        for ((l, is_cell, name), a) in &accs {
            if *l != li {
                continue;
            }
            let attrs = if *is_cell { leaf.cell_data_mut() } else { leaf.point_data_mut() };
            let Some(attrs) = attrs else { continue };
            attrs.remove(name);
            let mean: Vec<f64> = a.sum.iter().map(|s| s / nf).collect();
            let std: Vec<f64> = a.sq.iter().zip(&mean).map(|(q, m)| (q / nf - m * m).max(0.0).sqrt()).collect();
            attrs.insert(Array::new(format!("{name}_average"), a.ncomp, ArrayData::F64(mean)));
            attrs.insert(Array::new(format!("{name}_minimum"), a.ncomp, ArrayData::F64(a.min.clone())));
            attrs.insert(Array::new(format!("{name}_maximum"), a.ncomp, ArrayData::F64(a.max.clone())));
            attrs.insert(Array::new(format!("{name}_stddev"), a.ncomp, ArrayData::F64(std)));
        }
    };
    match &mut out {
        DataSet::MultiBlock(mb) => {
            fn walk(mb: &mut MultiBlock, li: &mut usize, f: &dyn Fn(&mut DataSet, usize)) {
                for (_, b) in mb.blocks.iter_mut() {
                    match b {
                        Some(DataSet::MultiBlock(inner)) => walk(inner, li, f),
                        Some(d) => {
                            f(d, *li);
                            *li += 1;
                        }
                        None => {}
                    }
                }
            }
            let mut li = 0;
            walk(mb, &mut li, &fill);
        }
        d => fill(d, 0),
    }
    Ok(out)
}

// ------------------------------------------------------------------ particles

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleOutput {
    /// Particles at the current time (points).
    Tracer,
    /// Each particle's path from its release to now (lines).
    Path,
    /// For each seed, a line through every particle it released (streak lines).
    Streak,
}

pub struct ParticleOptions {
    pub vectors: String,
    /// Release new particles at the seeds every N input steps (0 = only at the start).
    pub reinject_every: usize,
    /// Integration starts at the first input step at or after this time.
    pub start_time: f64,
    pub terminal_speed: f64,
}

#[derive(Clone)]
struct Particle {
    id: u32,
    seed: u32,
    step: u32,
    born: f64,
    pos: [f64; 3],
    alive: bool,
    /// (position, time, sampled input arrays) at release and at every input step passed.
    history: Vec<([f64; 3], f64, Option<Vec<f64>>)>,
}

struct TracerState {
    key: u64,
    /// Time the particles have been advanced to and the index of the next step to inject at.
    time: f64,
    next_injection: usize,
    next_id: u32,
    particles: Vec<Particle>,
    /// Arrays recorded along paths (name, components), fixed by the first step read.
    names: Vec<(String, usize)>,
}

/// A velocity field at one input step, ready for point location.
struct Field {
    loc: Locator,
    ds: DataSet,
    vec: String,
    /// Typical cell size, used to pick integration sub-steps.
    h: f64,
}

impl Field {
    fn new(ds: &DataSet, vec: &str) -> Result<Self> {
        let ds = match ds {
            DataSet::MultiBlock(_) => DataSet::UnstructuredGrid(crate::append::append_datasets(&[ds])?),
            d => d.clone(),
        };
        let (loc, ex) = Locator::new(&ds)?;
        let ex = ex.into_owned();
        let (_, pd, _, _) = parts(&ex).ok_or_else(|| anyhow!("particle tracer: unsupported dataset"))?;
        if pd.get(vec).filter(|a| a.ncomp >= 3).is_none() {
            return Err(anyhow!("no 3-component point array {vec:?} to advect particles with"));
        }
        let nc = ex.n_cells().max(1) as f64;
        let b = ex.bounds();
        let s = b.size();
        let dim = s.iter().filter(|&&x| x > 1e-12 * b.diagonal().max(1e-300)).count().max(1) as f64;
        let vol: f64 = s.iter().filter(|&&x| x > 1e-12 * b.diagonal().max(1e-300)).product();
        let h = (vol / nc).powf(1.0 / dim).max(1e-12);
        Ok(Self { loc, ds: ex, vec: vec.to_string(), h })
    }
    fn locate(&self, p: [f64; 3]) -> Option<Hit> {
        self.loc.find(p)
    }
    fn attrs(&self) -> &Attributes {
        parts(&self.ds).unwrap().1
    }
    /// Numeric point arrays (name, components).
    fn names(&self) -> Vec<(String, usize)> {
        self.attrs().iter().filter(|a| a.scalar_type() != ScalarType::String).map(|a| (a.name.clone(), a.ncomp)).collect()
    }
    /// Values of `names` at `p`, flattened (NaN where outside or missing).
    fn sample(&self, p: [f64; 3], names: &[(String, usize)]) -> Vec<f64> {
        let hit = self.locate(p);
        let fa = self.attrs();
        let mut out = Vec::new();
        for (name, nc) in names {
            let arr = fa.get(name);
            for c in 0..*nc {
                out.push(match (&hit, arr) {
                    (Some(h), Some(a)) if a.ncomp == *nc => (0..h.n).map(|j| h.w[j] * a.get(h.ids[j] as usize, c)).sum(),
                    _ => f64::NAN,
                });
            }
        }
        out
    }
    fn velocity(&self, h: &Hit) -> [f64; 3] {
        let a = self.attrs().get(&self.vec).unwrap();
        let mut v = [0.0; 3];
        for k in 0..h.n {
            for (c, vc) in v.iter_mut().enumerate() {
                *vc += h.w[k] * a.get(h.ids[k] as usize, c);
            }
        }
        v
    }
}

/// Velocity at `p` and time fraction `w` between two fields; `None` outside either.
fn velocity(f0: &Field, f1: &Field, w: f64, p: [f64; 3]) -> Option<[f64; 3]> {
    let a = f0.velocity(&f0.locate(p)?);
    if w <= 0.0 {
        return Some(a);
    }
    let b = f1.velocity(&f1.locate(p)?);
    Some([a[0] + w * (b[0] - a[0]), a[1] + w * (b[1] - a[1]), a[2] + w * (b[2] - a[2])])
}

/// Advance `p` from time `ta` to `tb` (inside the interval [t0, t1] of the two fields) with
/// RK4, sub-stepped so a step moves about a quarter of a cell. `None` if the particle leaves.
#[allow(clippy::too_many_arguments)]
fn advance(f0: &Field, f1: &Field, t0: f64, t1: f64, ta: f64, tb: f64, p: [f64; 3], terminal: f64) -> Option<[f64; 3]> {
    let span = (t1 - t0).max(1e-300);
    let wt = |t: f64| ((t - t0) / span).clamp(0.0, 1.0);
    let v0 = velocity(f0, f1, wt(ta), p)?;
    let speed = (v0[0] * v0[0] + v0[1] * v0[1] + v0[2] * v0[2]).sqrt();
    if speed < terminal {
        return Some(p);
    }
    let h = f0.h.min(f1.h);
    let n = ((speed * (tb - ta).abs() / (0.25 * h)).ceil() as usize).clamp(1, 10_000);
    let dt = (tb - ta) / n as f64;
    let mut x = p;
    let mut t = ta;
    let add = |x: [f64; 3], v: [f64; 3], s: f64| [x[0] + s * v[0], x[1] + s * v[1], x[2] + s * v[2]];
    for _ in 0..n {
        let k1 = velocity(f0, f1, wt(t), x)?;
        let k2 = velocity(f0, f1, wt(t + dt / 2.0), add(x, k1, dt / 2.0))?;
        let k3 = velocity(f0, f1, wt(t + dt / 2.0), add(x, k2, dt / 2.0))?;
        let k4 = velocity(f0, f1, wt(t + dt), add(x, k3, dt))?;
        for c in 0..3 {
            x[c] += dt / 6.0 * (k1[c] + 2.0 * k2[c] + 2.0 * k3[c] + k4[c]);
        }
        t += dt;
    }
    Some(x)
}

/// Particle Tracer, Particle Path and Streak Line: particles released at the seed points move
/// with the time-varying velocity (linear in time between steps, RK4 in space). Advancing is
/// incremental: playing forward continues from the particles of the previous run.
pub fn particles(ctx: &TemporalCtx, seeds: &[[f64; 3]], opt: &ParticleOptions, out: ParticleOutput, params_key: u64) -> Result<DataSet> {
    let times: Vec<f64> = ctx.input_times.iter().copied().filter(|&t| t >= opt.start_time - EPS * (1.0 + t.abs())).collect();
    let key = ctx.key ^ params_key.rotate_left(17);
    let target = ctx.time;
    // Pick up where the last run stopped when nothing upstream changed and time moved forward.
    let mut guard = ctx.state.lock().unwrap();
    let mut st = match guard.take().and_then(|b| b.downcast::<TracerState>().ok()) {
        Some(s) if s.key == key && s.time <= target + EPS => *s,
        _ => TracerState { key, time: f64::NEG_INFINITY, next_injection: 0, next_id: 0, particles: Vec::new(), names: Vec::new() },
    };
    let inject = |st: &mut TracerState, k: usize| {
        for (i, &p) in seeds.iter().enumerate() {
            let id = st.next_id;
            st.next_id += 1;
            st.particles.push(Particle { id, seed: i as u32, step: k as u32, born: times[k], pos: p, alive: true, history: vec![(p, times[k], None)] });
        }
    };
    let should_inject = |k: usize| k == 0 || (opt.reinject_every > 0 && k.is_multiple_of(opt.reinject_every));
    if times.is_empty() {
        // Static data: particles stay at the seeds.
        if st.particles.is_empty() {
            st.time = target;
            st.next_injection = 1;
            st.particles = seeds
                .iter()
                .enumerate()
                .map(|(i, &p)| Particle { id: i as u32, seed: i as u32, step: 0, born: target, pos: p, alive: true, history: vec![(p, target, None)] })
                .collect();
        }
    } else if target >= times[0] - EPS {
        if st.time == f64::NEG_INFINITY {
            st.time = times[0];
        }
        let mut fields: Option<(usize, Field, Field)> = None;
        loop {
            // Release particles at every step reached.
            while st.next_injection < times.len() && times[st.next_injection] <= st.time + EPS {
                let k = st.next_injection;
                if should_inject(k) {
                    inject(&mut st, k);
                }
                st.next_injection += 1;
            }
            if st.time >= target - EPS * (1.0 + target.abs()) {
                break;
            }
            let k = times.iter().rposition(|&x| x <= st.time + EPS).unwrap_or(0);
            if k + 1 >= times.len() {
                break; // past the last step: nothing more to integrate
            }
            let (t0, t1) = (times[k], times[k + 1]);
            let tb = t1.min(target);
            if fields.as_ref().map(|f| f.0) != Some(k) {
                let a = Field::new(&*(ctx.fetch)(t0)?, &opt.vectors)?;
                let b = Field::new(&*(ctx.fetch)(t1)?, &opt.vectors)?;
                fields = Some((k, a, b));
            }
            let (_, f0, f1) = fields.as_ref().unwrap();
            if st.names.is_empty() {
                st.names = f0.names();
            }
            let names = st.names.clone();
            let ta = st.time;
            for p in st.particles.iter_mut().filter(|p| p.alive) {
                // Record the arrays at release while that step's data is at hand.
                if let Some(h) = p.history.last_mut().filter(|h| h.2.is_none() && close(h.1, t0)) {
                    h.2 = Some(f0.sample(h.0, &names));
                }
                match advance(f0, f1, t0, t1, ta, tb, p.pos, opt.terminal_speed) {
                    Some(x) => {
                        p.pos = x;
                        if close(tb, t1) {
                            p.history.push((x, t1, Some(f1.sample(x, &names))));
                        }
                    }
                    None => p.alive = false,
                }
            }
            st.time = tb;
        }
    }
    // Sample the input arrays at the particles (at the current time, or at each history point).
    let now = (ctx.fetch)(target)?;
    let field_now = Field::new(&now, &opt.vectors).or_else(|_| {
        // A dataset without the vector field can still be sampled for display arrays.
        let (loc, ex) = Locator::new(&now)?;
        Ok::<Field, anyhow::Error>(Field { loc, ds: ex.into_owned(), vec: String::new(), h: 1.0 })
    })?;
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut pid = Vec::new();
    let mut seed_id = Vec::new();
    let mut step_id = Vec::new();
    let mut age = Vec::new();
    let mut lines = Cells::new();
    let mut verts = Cells::new();
    let mut sample_at: Vec<f64> = Vec::new();
    let mut pre: Vec<Option<Vec<f64>>> = Vec::new();
    let mut push = |p: [f64; 3], part: &Particle, t: f64, vals: Option<&Vec<f64>>, pts: &mut Vec<[f64; 3]>| -> u32 {
        pre.push(vals.cloned());
        pts.push(p);
        pid.push(part.id as i64);
        seed_id.push(part.seed as i64);
        step_id.push(part.step as i32);
        age.push((t - part.born) as f32);
        sample_at.push(t);
        (pts.len() - 1) as u32
    };
    let alive_now = |p: &Particle| p.alive && st.time >= p.born - EPS;
    match out {
        ParticleOutput::Tracer => {
            for p in st.particles.iter().filter(|p| alive_now(p)) {
                let i = push(p.pos, p, st.time, None, &mut pts);
                verts.push(CellType::Vertex, &[i]);
            }
        }
        ParticleOutput::Path => {
            for p in st.particles.iter() {
                let mut ids = Vec::new();
                for (x, t, v) in &p.history {
                    ids.push(push(*x, p, *t, v.as_ref(), &mut pts));
                }
                if p.alive && !close(p.history.last().map(|h| h.1).unwrap_or(st.time), st.time) {
                    ids.push(push(p.pos, p, st.time, None, &mut pts));
                }
                if ids.len() > 1 {
                    lines.push(CellType::PolyLine, &ids);
                } else if let Some(&i) = ids.first() {
                    verts.push(CellType::Vertex, &[i]);
                }
            }
        }
        ParticleOutput::Streak => {
            for s in 0..seeds.len() as u32 {
                // Youngest particle (at the seed) first.
                let mut group: Vec<&Particle> = st.particles.iter().filter(|p| p.seed == s && alive_now(p)).collect();
                group.sort_by_key(|p| std::cmp::Reverse(p.step));
                let ids: Vec<u32> = group.iter().map(|p| push(p.pos, p, st.time, None, &mut pts)).collect();
                if ids.len() > 1 {
                    lines.push(CellType::PolyLine, &ids);
                } else if let Some(&i) = ids.first() {
                    verts.push(CellType::Vertex, &[i]);
                }
            }
        }
    }
    // Input arrays at every output point: recorded along paths, sampled now for the rest.
    let mut attrs = Attributes::new();
    let names: Vec<(String, usize)> = if st.names.is_empty() { field_now.names() } else { st.names.clone() };
    let width: usize = names.iter().map(|n| n.1).sum();
    let mut cols: Vec<Vec<f64>> = names.iter().map(|(_, nc)| Vec::with_capacity(pts.len() * nc)).collect();
    let mut cache: Vec<(f64, Arc<Field>)> = vec![(target, Arc::new(field_now))];
    for (i, &p) in pts.iter().enumerate() {
        let vals = match pre[i].take().filter(|v| v.len() == width) {
            Some(v) => v,
            None => {
                let t = sample_at[i];
                let f = match cache.iter().find(|(ct, _)| close(*ct, t)) {
                    Some((_, f)) => f.clone(),
                    None => {
                        let ds = (ctx.fetch)(t)?;
                        let (loc, ex) = Locator::new(&ds)?;
                        let f = Arc::new(Field { loc, ds: ex.into_owned(), vec: String::new(), h: 1.0 });
                        cache.push((t, f.clone()));
                        f
                    }
                };
                f.sample(p, &names)
            }
        };
        let mut o = 0;
        for (k, (_, nc)) in names.iter().enumerate() {
            cols[k].extend_from_slice(&vals[o..o + nc]);
            o += nc;
        }
    }
    for ((name, nc), v) in names.into_iter().zip(cols) {
        attrs.insert(Array::new(name, nc, ArrayData::F64(v)));
    }
    attrs.insert(Array::from_vec("ParticleId", 1, pid));
    attrs.insert(Array::from_vec("InjectedPointId", 1, seed_id));
    attrs.insert(Array::from_vec("InjectionStepId", 1, step_id));
    attrs.insert(Array::from_vec("ParticleAge", 1, age));
    if let Some(v) = attrs.get(&opt.vectors).map(|a| a.name.clone()) {
        attrs.set_active(AttributeKind::Vectors, &v);
    }
    let n = pts.len();
    let poly = PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), verts, lines, point_data: attrs, ..Default::default() };
    debug_assert_eq!(poly.points.len(), n);
    *guard = Some(Box::new(st));
    Ok(DataSet::PolyData(poly))
}
