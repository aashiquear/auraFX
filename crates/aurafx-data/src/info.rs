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
    /// Cell types and their counts, most common first.
    #[serde(default)]
    pub cell_types: Vec<(String, usize)>,
    /// Number of curved (higher-order) cells.
    #[serde(default)]
    pub curved_cells: usize,
}

fn cell_types(ds: &DataSet, counts: &mut std::collections::BTreeMap<u8, usize>) {
    match ds {
        DataSet::UnstructuredGrid(ug) => {
            for t in &ug.cells.types {
                *counts.entry(*t as u8).or_default() += 1;
            }
        }
        DataSet::PolyData(pd) => {
            for c in [&pd.verts, &pd.lines, &pd.polys, &pd.strips] {
                for t in &c.types {
                    *counts.entry(*t as u8).or_default() += 1;
                }
            }
        }
        DataSet::MultiBlock(m) => m.leaves().into_iter().for_each(|(_, d)| cell_types(d, counts)),
        _ => {}
    }
}

fn arrays(a: &Attributes) -> Vec<ArrayInfo> {
    a.iter()
        .map(|arr| ArrayInfo { name: arr.name.clone(), ncomp: arr.ncomp, dtype: arr.scalar_type().vtk_name().to_string(), range: arr.range(None) })
        .collect()
}

impl DataSetInfo {
    pub fn of(ds: &DataSet) -> Self {
        // A cell grid is described by its cells with the fields at their corners (what is drawn
        // and colored), under its own name.
        if let DataSet::CellGrid(g) = ds {
            let mut info = Self::of(&DataSet::UnstructuredGrid(g.to_unstructured()));
            let types: Vec<&str> = g.cell_types.keys().map(|k| k.trim_start_matches("vtkDG")).collect();
            let sides = g.n_sides_shown();
            let sides = if sides > 0 { format!("; {sides} sides shown") } else { String::new() };
            info.kind = format!("Cell Grid ({}; {} fields{sides})", types.join(", "), g.attributes.iter().filter(|a| !a.shape).count());
            info.n_cells = g.n_cells();
            return info;
        }
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
        let mut counts = std::collections::BTreeMap::new();
        cell_types(ds, &mut counts);
        let mut ct: Vec<(u8, usize)> = counts.into_iter().collect();
        ct.sort_by(|a, b| b.1.cmp(&a.1));
        Self {
            kind: match ds {
                DataSet::HyperTreeGrid(h) => {
                    format!(
                        "Hyper Tree Grid ({}-D, branch factor {}, {} visible leaves, depth {})",
                        h.dimension(),
                        h.branch_factor,
                        h.n_leaves(),
                        h.max_depth()
                    )
                }
                _ => ds.type_name().to_string(),
            },
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
            cell_types: ct.iter().map(|(t, n)| (crate::CellType::from_vtk(*t).map(|c| c.name()).unwrap_or("?").to_string(), *n)).collect(),
            curved_cells: ct.iter().filter(|(t, _)| crate::CellType::from_vtk(*t).and_then(|c| c.high_order()).is_some()).map(|(_, n)| n).sum(),
        }
    }

    /// Summary of a dataset split into pieces, from the pieces' summaries.
    pub fn merge(parts: &[DataSetInfo]) -> DataSetInfo {
        let Some(first) = parts.iter().find(|p| p.n_cells > 0 || p.n_points > 0).or(parts.first()) else {
            return DataSetInfo::of(&DataSet::PolyData(Default::default()));
        };
        let mut out = first.clone();
        out.n_points = parts.iter().map(|p| p.n_points).sum();
        out.n_cells = parts.iter().map(|p| p.n_cells).sum();
        out.bytes = parts.iter().map(|p| p.bytes).sum();
        out.bounds = parts.iter().fold(Bounds::EMPTY, |b, p| b.union(&p.bounds));
        out.curved_cells = parts.iter().map(|p| p.curved_cells).sum();
        let merge_arrays = |get: fn(&DataSetInfo) -> &Vec<ArrayInfo>| -> Vec<ArrayInfo> {
            let mut v = get(first).clone();
            for a in &mut v {
                for p in parts {
                    if let Some(b) = get(p).iter().find(|b| b.name == a.name) {
                        if b.range.0 <= b.range.1 {
                            a.range = if a.range.0 <= a.range.1 { (a.range.0.min(b.range.0), a.range.1.max(b.range.1)) } else { b.range };
                        }
                    }
                }
            }
            v
        };
        out.point_arrays = merge_arrays(|p| &p.point_arrays);
        out.cell_arrays = merge_arrays(|p| &p.cell_arrays);
        let mut types: Vec<(String, usize)> = Vec::new();
        for p in parts {
            for (t, n) in &p.cell_types {
                match types.iter_mut().find(|x| &x.0 == t) {
                    Some(x) => x.1 += n,
                    None => types.push((t.clone(), *n)),
                }
            }
        }
        types.sort_by(|a, b| b.1.cmp(&a.1));
        out.cell_types = types;
        out
    }

    /// Compact text summary suitable for an LLM prompt or a tooltip.
    pub fn summary(&self) -> String {
        let mut s = format!("{}: {} points, {} cells, {:.1} MB", self.kind, self.n_points, self.n_cells, self.bytes as f64 / 1e6);
        if !self.bounds.is_empty() {
            let b = &self.bounds;
            s += &format!("\nbounds: x[{}, {}] y[{}, {}] z[{}, {}]", g(b.min[0]), g(b.max[0]), g(b.min[1]), g(b.max[1]), g(b.min[2]), g(b.max[2]));
        }
        for (label, arrs) in [("point arrays", &self.point_arrays), ("cell arrays", &self.cell_arrays), ("field arrays", &self.field_arrays)] {
            if !arrs.is_empty() {
                s += &format!("\n{label}:");
                for a in arrs {
                    s += &format!("\n  {} ({} x {}, range {} .. {})", a.name, a.dtype, a.ncomp, g(a.range.0), g(a.range.1));
                }
            }
        }
        if self.curved_cells > 0 || self.cell_types.len() > 1 {
            let t: Vec<String> = self.cell_types.iter().map(|(n, c)| format!("{c} {n}")).collect();
            s += &format!("\ncell types: {}", t.join(", "));
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
