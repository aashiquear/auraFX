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
                let first = m.leaves().into_iter().next().map(|(_, d)| d);
                (
                    first.and_then(|d| d.point_data()).map(arrays).unwrap_or_default(),
                    first.and_then(|d| d.cell_data()).map(arrays).unwrap_or_default(),
                )
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
