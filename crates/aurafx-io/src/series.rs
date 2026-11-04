//! Helpers for grouping numbered files (`run_0001.vtu`, `run_0002.vtu`, ...) into a series.

use std::path::{Path, PathBuf};

/// Given one file, find sibling files that differ only by a trailing number and return
/// them sorted by that number together with the parsed numbers. Returns `None` if the
/// file is not part of a numbered family.
pub fn detect_numbered_series(path: &Path) -> Option<Vec<(f64, PathBuf)>> {
    let name = path.file_name()?.to_str()?;
    let (stem, ext) = name.rsplit_once('.')?;
    let digits_end = stem.len();
    let digits_start = stem.rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
    if digits_start == digits_end {
        return None;
    }
    let prefix = &stem[..digits_start];
    let dir = path.parent()?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let n = entry.file_name();
        let n = n.to_str().unwrap_or("");
        if let Some((s, e)) = n.rsplit_once('.') {
            if e == ext && s.starts_with(prefix) {
                let num = &s[prefix.len()..];
                if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(v) = num.parse::<f64>() {
                        out.push((v, entry.path()));
                    }
                }
            }
        }
    }
    if out.len() < 2 {
        return None;
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    Some(out)
}
