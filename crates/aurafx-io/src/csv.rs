//! Delimited text reader -> Table (numeric columns become Float64, others String).

use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::path::Path;

pub fn read(path: &Path) -> Result<DataSet> {
    let text = String::from_utf8_lossy(&crate::read_bytes(path)?).to_string();
    let mut lines = text.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    let header = lines.next().ok_or_else(|| anyhow!("empty file"))?;
    let delim = [',', '\t', ';', ' '].into_iter().max_by_key(|&d| header.matches(d).count()).unwrap_or(',');
    let split = |l: &str| -> Vec<String> {
        if delim == ' ' {
            l.split_whitespace().map(|s| s.trim_matches('"').to_string()).collect()
        } else {
            l.split(delim).map(|s| s.trim().trim_matches('"').to_string()).collect()
        }
    };
    let names = split(header);
    let header_is_numeric = names.iter().all(|n| n.parse::<f64>().is_ok());
    let names: Vec<String> = if header_is_numeric { (0..names.len()).map(|i| format!("col{i}")).collect() } else { names };
    let mut cols: Vec<Vec<String>> = vec![Vec::new(); names.len()];
    let rows = if header_is_numeric { std::iter::once(header).chain(lines).collect::<Vec<_>>() } else { lines.collect() };
    for l in rows {
        let f = split(l);
        for (i, c) in cols.iter_mut().enumerate() {
            c.push(f.get(i).cloned().unwrap_or_default());
        }
    }
    let mut table = Table::default();
    for (name, col) in names.into_iter().zip(cols) {
        let nums: Option<Vec<f64>> = col.iter().map(|s| s.parse::<f64>().ok()).collect();
        let arr = match nums {
            Some(v) => Array::new(name, 1, ArrayData::F64(v)),
            None => Array::new(name, 1, ArrayData::String(col)),
        };
        table.columns.insert(arr);
    }
    Ok(DataSet::Table(table))
}
