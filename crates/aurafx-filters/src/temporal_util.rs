//! Time utilities: choose time steps (Extract Time Steps), make new ones (Generate Time
//! Steps), hold one time (Force Time), snap a requested time to a step of the data (Temporal
//! Snap to Time Step) and keep recent steps in memory (Temporal Cache).

use crate::temporal::TemporalCtx;
use anyhow::{anyhow, Result};
use aurafx_data::DataSet;
use std::sync::Arc;

/// Step indices from text: "0, 3, 5-8, 10:20:2" (ranges inclusive; start:stop:stride stops
/// before stop). Indices past the last step are left out.
pub fn parse_steps(text: &str, n: usize) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in text.split([',', ';', ' ']).map(str::trim).filter(|p| !p.is_empty()) {
        let num = |s: &str| s.trim().parse::<usize>().map_err(|_| anyhow!("{s:?} is not a step number"));
        if let Some((a, rest)) = part.split_once(':') {
            let (b, stride) = match rest.split_once(':') {
                Some((b, st)) => (if b.is_empty() { n } else { num(b)? }, num(st)?.max(1)),
                None => (if rest.is_empty() { n } else { num(rest)? }, 1),
            };
            let a = if a.is_empty() { 0 } else { num(a)? };
            out.extend((a..b.min(n)).step_by(stride));
        } else if let Some((a, b)) = part.split_once('-') {
            out.extend(num(a)?..=num(b)?.min(n.saturating_sub(1)));
        } else {
            out.push(num(part)?);
        }
    }
    out.retain(|&i| i < n);
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Output times of Extract Time Steps: the chosen steps, or every `stride`-th from `first`.
pub fn extract_times(steps: &str, first: usize, stride: usize, input: &[f64]) -> Vec<f64> {
    let idx: Vec<usize> =
        if steps.trim().is_empty() { (first..input.len()).step_by(stride.max(1)).collect() } else { parse_steps(steps, input.len()).unwrap_or_default() };
    idx.into_iter().map(|i| input[i]).collect()
}

/// Output times of Generate Time Steps: `count` values from `start` to `end` (both included);
/// with an unset range, the input's first and last times.
pub fn generate_times(start: f64, end: f64, count: usize, input: &[f64]) -> Vec<f64> {
    let a = if start.is_finite() { start } else { input.first().copied().unwrap_or(0.0) };
    let b = if end.is_finite() { end } else { input.last().copied().unwrap_or(1.0) };
    match count {
        0 => Vec::new(),
        1 => vec![a],
        n => (0..n).map(|k| a + (b - a) * k as f64 / (n - 1) as f64).collect(),
    }
}

/// The input step `t` snaps to: the nearest, the one at or before, or the one at or after.
pub fn snap(t: f64, times: &[f64], mode: &str) -> f64 {
    if times.is_empty() {
        return t;
    }
    let eps = 1e-12 * (1.0 + t.abs());
    match mode {
        "Previous" => times.iter().rev().find(|&&x| x <= t + eps).copied().unwrap_or(times[0]),
        "Next" => times.iter().find(|&&x| x >= t - eps).copied().unwrap_or(*times.last().unwrap()),
        _ => *times.iter().min_by(|a, b| (*a - t).abs().total_cmp(&(*b - t).abs())).unwrap(),
    }
}

/// Temporal Cache: the input at `ctx.time`, from memory when it was read recently.
pub fn cached(ctx: &TemporalCtx, size: usize) -> Result<DataSet> {
    type Cache = (u64, Vec<(u64, Arc<DataSet>)>);
    let key = ctx.time.to_bits();
    let mut st = ctx.state.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = st.as_ref().and_then(|b| b.downcast_ref::<Cache>()).map(|c| c.0 == ctx.key).unwrap_or(false);
    if !fresh {
        *st = Some(Box::new((ctx.key, Vec::<(u64, Arc<DataSet>)>::new())));
    }
    let cache = st.as_mut().unwrap().downcast_mut::<Cache>().unwrap();
    if let Some(pos) = cache.1.iter().position(|(k, _)| *k == key) {
        let hit = cache.1.remove(pos);
        let d = hit.1.clone();
        cache.1.push(hit);
        return Ok((*d).clone());
    }
    drop(st);
    let d = (ctx.fetch)(ctx.time)?;
    let mut st = ctx.state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cache) = st.as_mut().and_then(|b| b.downcast_mut::<Cache>()) {
        cache.1.push((key, d.clone()));
        while cache.1.len() > size.max(1) {
            cache.1.remove(0);
        }
    }
    Ok((*d).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurafx_data::{Array, Table};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[test]
    fn steps_and_times() {
        assert_eq!(parse_steps("0, 3, 5-7, 10:16:3", 20).unwrap(), vec![0, 3, 5, 6, 7, 10, 13]);
        assert_eq!(parse_steps("8, 30, 2:", 10).unwrap(), vec![2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(parse_steps("a", 3).is_err());
        let t = [0.0, 0.5, 1.0, 1.5, 2.0];
        assert_eq!(extract_times("", 1, 2, &t), vec![0.5, 1.5]);
        assert_eq!(extract_times("0,4", 0, 1, &t), vec![0.0, 2.0]);
        assert_eq!(generate_times(f64::NAN, f64::NAN, 3, &t), vec![0.0, 1.0, 2.0]);
        assert_eq!(generate_times(10.0, 12.0, 5, &t), vec![10.0, 10.5, 11.0, 11.5, 12.0]);
        assert_eq!(snap(0.7, &t, "Nearest"), 0.5);
        assert_eq!(snap(0.7, &t, "Previous"), 0.5);
        assert_eq!(snap(0.7, &t, "Next"), 1.0);
        assert_eq!(snap(9.0, &t, "Next"), 2.0);
        assert_eq!(snap(-1.0, &t, "Previous"), 0.0);
    }

    #[test]
    fn cache_reads_each_step_once() {
        let reads = AtomicUsize::new(0);
        let fetch = |t: f64| -> Result<Arc<DataSet>> {
            reads.fetch_add(1, Ordering::SeqCst);
            let mut tb = Table::default();
            tb.columns.insert(Array::scalars_f64("t", vec![t]));
            Ok(Arc::new(DataSet::Table(tb)))
        };
        let state = Mutex::new(None);
        let times = [0.0, 1.0, 2.0];
        for t in [0.0, 1.0, 0.0, 1.0, 2.0, 0.0] {
            let ctx = TemporalCtx { time: t, input_times: &times, fetch: &fetch, extra: &[], state: &state, key: 7 };
            let DataSet::Table(out) = cached(&ctx, 2).unwrap() else { panic!() };
            assert_eq!(out.columns.get("t").unwrap().get(0, 0), t);
        }
        // 0, 1 read; 0, 1 from memory; 2 read (0 dropped, it was used before 1); 0 read again.
        assert_eq!(reads.load(Ordering::SeqCst), 4);
    }
}
