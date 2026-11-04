//! Human-readable dataset summaries (used by the UI, Python and the AI assistant).

use crate::{Attributes, Bounds, DataSet};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrayInfo {
    pub name: String,
    pub ncomp: usize,
    pub dtype: String,
    pub range: (f64, f64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSetInfo {
    pub kind: String,
    pub n_points: usize,
    pub n_cells: usize,
    pub bounds: Bounds,
    pub bytes: usize,
    pub point_arrays: Vec<ArrayInfo>,
    pub cell_arrays: Vec<ArrayInfo>,
    pub field_arrays: Vec<ArrayInfo>,
    pub blocks: Vec<String>,
}

fn arrays(a: &Attributes) -> Vec<ArrayInfo> {
    a.iter()
        .map(|arr| ArrayInfo {
            name: arr.name.clone(),
            ncomp: arr.ncomp,
            dtype: arr.scalar_type().vtk_name().to_string(),
            range: arr.range(None),
        })
        .collect()
}

impl DataSetInfo {
    pub fn of(ds: &DataSet) -> Self {
        let (pa, ca) = match ds {
            DataSet::MultiBlock(m) => {
                // Arrays present in the first leaf, with ranges merged over every leaf that has them.
                let leaves = m.leaves();
                let merge = |get: &dyn Fn(&DataSet) -> Option<&Attributes>| -> Vec<ArrayInfo> {
                    let Some(first) = leaves.first().and_then(|(_, d)| get(d)) else { return vec![] };
                    let mut out = arrays(first);
                    for a in &mut out {
                        for (_, d) in leaves.iter().skip(1) {
                            if let Some(x) = get(d).and_then(|at| at.get(&a.name)) {
                                let r = x.range(None);
                                a.range = (a.range.0.min(r.0), a.range.1.max(r.1));
                            }
                        }
                    }
                    out
                };
                (merge(&|d: &DataSet| d.point_data()), merge(&|d: &DataSet| d.cell_data()))
            }
            DataSet::Table(t) => (arrays(&t.columns), vec![]),
            _ => (ds.point_data().map(arrays).unwrap_or_default(), ds.cell_data().map(arrays).unwrap_or_default()),
        };
        Self {
            kind: ds.type_name().to_string(),
            n_points: ds.n_points(),
            n_cells: ds.n_cells(),
            bounds: ds.bounds(),
            bytes: ds.n_bytes(),
            point_arrays: pa,
            cell_arrays: ca,
            field_arrays: arrays(ds.field_data()),
            blocks: match ds {
                DataSet::MultiBlock(m) => m.leaves().into_iter().map(|(n, _)| n).collect(),
                _ => vec![],
            },
        }
    }

    /// Compact text summary suitable for an LLM prompt or a tooltip.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{}: {} points, {} cells, {:.1} MB",
            self.kind,
            self.n_points,
            self.n_cells,
            self.bytes as f64 / 1e6
        );
        if !self.bounds.is_empty() {
            let b = &self.bounds;
            s += &format!(
                "\nbounds: x[{}, {}] y[{}, {}] z[{}, {}]",
                g(b.min[0]), g(b.max[0]), g(b.min[1]), g(b.max[1]), g(b.min[2]), g(b.max[2])
            );
        }
        for (label, arrs) in [("point arrays", &self.point_arrays), ("cell arrays", &self.cell_arrays), ("field arrays", &self.field_arrays)] {
            if !arrs.is_empty() {
                s += &format!("\n{label}:");
                for a in arrs {
                    s += &format!("\n  {} ({} x {}, range {} .. {})", a.name, a.dtype, a.ncomp, g(a.range.0), g(a.range.1));
                }
            }
        }
        if !self.blocks.is_empty() {
            s += &format!("\nblocks: {}", self.blocks.join(", "));
        }
        s
    }
}

/// Short general-purpose float formatting (like C's %.4g).
pub fn g(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if !v.is_finite() {
        return format!("{v}");
    }
    let a = v.abs();
    if (1e-3..1e5).contains(&a) {
        let s = format!("{:.4}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    } else {
        format!("{:.3e}", v)
    }
}
