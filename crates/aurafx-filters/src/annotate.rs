//! Annotation filters: they write a line of text (a one-row table with a `Text` column, and
//! where to put it as field data) that the 3D views draw in a corner: the time, a global value
//! of the data, the value of an array at a point or cell, a summary of selected points, the
//! environment (date, user, machine), or the result of a Python expression.

use crate::params::Association;
use anyhow::{anyhow, Result};
use aurafx_data::*;

/// The text table the 3D views draw: `Text`, and field data `TextCorner` and `TextSize`.
pub fn text_table(text: String, corner: &str, size: f64) -> DataSet {
    let mut t = Table::default();
    t.columns.insert(Array::new("Text", 1, ArrayData::String(vec![text])));
    t.field_data.insert(Array::new("TextCorner", 1, ArrayData::String(vec![corner.to_string()])));
    t.field_data.insert(Array::scalars_f64("TextSize", vec![size]));
    DataSet::Table(t)
}

/// The text, corner and size of an annotation output (`None` for other data).
pub fn annotation_of(ds: &DataSet) -> Option<(String, String, f32)> {
    let DataSet::Table(t) = ds else { return None };
    let corner = match &t.field_data.get("TextCorner")?.data {
        ArrayData::String(v) => v.first()?.clone(),
        _ => return None,
    };
    let text = match &t.columns.get("Text")?.data {
        ArrayData::String(v) => v.join("\n"),
        _ => return None,
    };
    let size = t.field_data.get("TextSize").map(|a| a.get(0, 0) as f32).unwrap_or(16.0);
    Some((text, corner, size))
}

/// Put `v` into `fmt`: `{}` (shortest), `{:.3}` (3 decimals), `{:.3e}` (exponent), `{:.4g}`
/// (4 significant digits). Without a placeholder the value is appended.
pub fn format_value(fmt: &str, v: f64) -> String {
    let Some(start) = fmt.find('{') else { return format!("{fmt}{}", short(v)) };
    let Some(len) = fmt[start..].find('}') else { return format!("{fmt}{}", short(v)) };
    let spec = &fmt[start + 1..start + len];
    let body = match spec.strip_prefix(":.") {
        None => short(v),
        Some(s) => {
            let (digits, kind) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
            let d: usize = digits.parse().unwrap_or(3);
            match kind {
                "e" => format!("{v:.d$e}"),
                "g" => {
                    let mag = if v == 0.0 { 0 } else { v.abs().log10().floor() as i32 };
                    if (-4..d as i32).contains(&mag) {
                        format!("{v:.*}", (d as i32 - 1 - mag).max(0) as usize)
                    } else {
                        format!("{v:.*e}", d.saturating_sub(1))
                    }
                }
                _ => format!("{v:.d$}"),
            }
        }
    };
    format!("{}{}{}", &fmt[..start], body, &fmt[start + len + 1..])
}

fn short(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", (v * 1e6).round() / 1e6)
    }
}

/// A global (field) value: with one value per time step, the value at `step`.
pub fn global_value(ds: &DataSet, name: &str, comp: usize, step: usize, n_steps: usize) -> Result<f64> {
    let fd = ds.field_data();
    let a = fd.get(name).or_else(|| match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().find_map(|(_, l)| l.field_data().get(name)),
        _ => None,
    });
    let a = a.ok_or_else(|| anyhow!("no field (global) array named {name:?}"))?;
    if a.is_empty() {
        return Err(anyhow!("the array {name:?} is empty"));
    }
    let row = if a.len() == n_steps && n_steps > 1 { step.min(a.len() - 1) } else { 0 };
    Ok(a.get(row, comp.min(a.ncomp - 1)))
}

/// The value of an array at point or cell `id` (a vector's magnitude unless `comp` is given).
pub fn attribute_value(ds: &DataSet, assoc: Association, name: &str, id: usize, comp: Option<usize>) -> Result<f64> {
    let attrs = match (ds, assoc) {
        (DataSet::Table(t), _) => Some(&t.columns),
        (d, Association::Points) => d.point_data(),
        (d, Association::Cells) => d.cell_data(),
    };
    let a = attrs.and_then(|a| a.get(name)).ok_or_else(|| anyhow!("no array named {name:?}"))?;
    if id >= a.len() {
        return Err(anyhow!("{name} has {} values; there is no number {id}", a.len()));
    }
    Ok(match comp {
        Some(c) => a.get(id, c.min(a.ncomp - 1)),
        None if a.ncomp == 1 => a.get(id, 0),
        None => a.magnitude(id),
    })
}

/// A summary of the points of `ds` (the output of Extract Selection, say): how many, and the
/// first rows with their ids and the chosen arrays' values.
pub fn selection_text(ds: &DataSet, arrays: &[String], rows: usize) -> String {
    let n = ds.n_points();
    let pd = ds.point_data();
    let ids = pd.and_then(|p| p.get("vtkOriginalPointIds").or_else(|| p.get("GlobalPointIds")));
    let names: Vec<String> = if arrays.is_empty() {
        pd.map(|p| {
            p.iter().filter(|a| a.scalar_type() != ScalarType::String && !a.name.starts_with("vtk") && a.ncomp == 1).take(3).map(|a| a.name.clone()).collect()
        })
        .unwrap_or_default()
    } else {
        arrays.to_vec()
    };
    let mut lines = vec![format!("{n} selected point{}", if n == 1 { "" } else { "s" })];
    for i in 0..n.min(rows) {
        let id = ids.map(|a| a.get(i, 0) as i64).unwrap_or(i as i64);
        let vals: Vec<String> = names
            .iter()
            .filter_map(|nm| {
                pd.and_then(|p| p.get(nm)).map(|a| format!("{nm} {}", format_value("{:.4g}", if a.ncomp == 1 { a.get(i, 0) } else { a.magnitude(i) })))
            })
            .collect();
        lines.push(format!("#{id}: {}", vals.join(", ")));
    }
    if n > rows {
        lines.push(format!("… and {} more", n - rows));
    }
    lines.join("\n")
}

/// Date and time, user, machine, operating system and auraFX version, as asked.
pub fn environment_text(date: bool, user: bool, machine: bool, system: bool, version: bool) -> String {
    let mut parts = Vec::new();
    if date {
        parts.push(now_text());
    }
    if user {
        if let Ok(u) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            parts.push(u);
        }
    }
    if machine {
        let h = std::env::var("HOSTNAME")
            .ok()
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
            .filter(|h| !h.is_empty());
        if let Some(h) = h {
            parts.push(h);
        }
    }
    if system {
        parts.push(format!("{} {}", std::env::consts::OS, std::env::consts::ARCH));
    }
    if version {
        parts.push(format!("auraFX {}", env!("CARGO_PKG_VERSION")));
    }
    parts.join(" · ")
}

/// The current date and time in UTC, YYYY-MM-DD HH:MM.
fn now_text() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_value("Time: {:.3}", 1.5), "Time: 1.500");
        assert_eq!(format_value("t = {} s", 2.0), "t = 2 s");
        assert_eq!(format_value("{:.2e} Pa", 12345.0), "1.23e4 Pa");
        assert_eq!(format_value("{:.3g}", 0.012345), "0.0123");
        assert_eq!(format_value("{:.3g}", 123456.0), "1.23e5");
        assert_eq!(format_value("T max ", 3.25), "T max 3.25");
    }

    #[test]
    fn values_and_texts() {
        let mut pd = PolyData::default();
        for i in 0..5 {
            pd.points.push([i as f64, 0.0, 0.0]);
        }
        pd.point_data.insert(Array::scalars_f64("T", vec![10.0, 11.0, 12.0, 13.0, 14.0]));
        pd.point_data.insert(Array::from_vec("V", 3, vec![3.0, 4.0, 0.0].repeat(5)));
        pd.field_data.insert(Array::scalars_f64("Energy", vec![1.0, 2.0, 3.0]));
        let ds = DataSet::PolyData(pd);
        assert_eq!(attribute_value(&ds, Association::Points, "T", 3, None).unwrap(), 13.0);
        assert_eq!(attribute_value(&ds, Association::Points, "V", 0, None).unwrap(), 5.0);
        assert!(attribute_value(&ds, Association::Points, "T", 9, None).is_err());
        assert_eq!(global_value(&ds, "Energy", 0, 2, 3).unwrap(), 3.0);
        assert_eq!(global_value(&ds, "Energy", 0, 2, 10).unwrap(), 1.0, "not one value per step: the first");
        let s = selection_text(&ds, &[], 2);
        assert!(s.starts_with("5 selected points\n#0: T 10"), "{s}");
        assert!(s.ends_with("… and 3 more"));
        let e = environment_text(true, false, false, true, true);
        assert!(e.contains("UTC") && e.contains(std::env::consts::OS) && e.contains("auraFX"), "{e}");
        let t = text_table("hello".into(), "lower left", 20.0);
        assert_eq!(annotation_of(&t), Some(("hello".into(), "lower left".into(), 20.0)));
        assert_eq!(annotation_of(&ds), None);
    }
}
