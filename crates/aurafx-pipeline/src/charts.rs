//! Chart views: line, point and bar charts of one object's arrays.
//!
//! Chart view 0 is the Charts panel; more views can be opened beside it, each plotting its own
//! object (or following the selected one). The views live in the session, so scripts can open
//! and change them and state files keep them.

use crate::session::{ChartSeries, Session};
use crate::NodeId;
use aurafx_data::{DataSet, ScalarType};
use serde::{Deserialize, Serialize};

/// How the series of a chart view are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartStyle {
    #[default]
    Lines,
    Points,
    /// Lines with a marker at every value.
    Markers,
    Bars,
    /// A box per series: its quartiles, median and whiskers (1.5 interquartile ranges).
    Box,
}

impl ChartStyle {
    pub const ALL: [ChartStyle; 5] = [ChartStyle::Lines, ChartStyle::Points, ChartStyle::Markers, ChartStyle::Bars, ChartStyle::Box];

    pub fn label(self) -> &'static str {
        match self {
            ChartStyle::Lines => "Lines",
            ChartStyle::Points => "Points",
            ChartStyle::Markers => "Lines and markers",
            ChartStyle::Bars => "Bars",
            ChartStyle::Box => "Box plot",
        }
    }

    pub fn parse(s: &str) -> Option<ChartStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lines" | "line" => Some(ChartStyle::Lines),
            "points" | "point" | "scatter" => Some(ChartStyle::Points),
            "markers" | "lines+markers" | "linespoints" => Some(ChartStyle::Markers),
            "bars" | "bar" => Some(ChartStyle::Bars),
            "box" | "boxplot" | "box plot" => Some(ChartStyle::Box),
            _ => None,
        }
    }
}

/// Which arrays of a dataset a chart view plots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartData {
    /// Table columns, else point arrays, else cell arrays.
    #[default]
    Auto,
    Points,
    Cells,
}

impl ChartData {
    pub fn parse(s: &str) -> Option<ChartData> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Some(ChartData::Auto),
            "points" | "point" => Some(ChartData::Points),
            "cells" | "cell" => Some(ChartData::Cells),
            _ => None,
        }
    }
}

/// One chart view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartView {
    /// 0 is the Charts panel; the others are numbered from 1.
    pub id: u32,
    /// The object plotted; `None` follows the selected object.
    pub node: Option<NodeId>,
    pub data: ChartData,
    /// X axis: an array name, or "(index)"; empty = chosen from the data.
    pub x: String,
    /// Arrays plotted; empty = chosen from the data.
    pub y: Vec<String>,
    pub style: ChartStyle,
    pub log_x: bool,
    pub log_y: bool,
    pub title: String,
    /// Lines sent by scripts to this view (`afx.plot(..., view=n)`); view 0 shows `Session::chart`.
    pub series: Vec<ChartSeries>,
    /// The object the defaults were chosen for (the UI's bookkeeping).
    #[serde(skip)]
    pub defaults_for: Option<NodeId>,
}

impl ChartView {
    /// Tab title: "Charts" for view 0, "Chart 2: name" for the others.
    pub fn title_with(&self, node_name: Option<&str>) -> String {
        let base = if self.id == 0 { "Charts".to_string() } else { format!("Chart {}", self.id + 1) };
        match (self.node, node_name) {
            (Some(_), Some(n)) if self.id != 0 => format!("{base}: {n}"),
            _ => base,
        }
    }
}

/// The series a chart can plot from a dataset: (name, values), long arrays and strings left out.
/// The blocks of a multi-block give one set each, named `block/array` (a line per block, each
/// against its own x array).
pub fn columns(ds: &DataSet, data: ChartData) -> Vec<(String, Vec<f64>)> {
    if let DataSet::MultiBlock(mb) = ds {
        let mut out = Vec::new();
        for (name, leaf) in mb.leaves().into_iter().take(64) {
            for (c, v) in columns(leaf, data) {
                out.push((format!("{name}/{c}"), v));
            }
        }
        return out;
    }
    let attrs = match ds {
        DataSet::Table(t) => Some(&t.columns),
        d => match data {
            ChartData::Points => d.point_data(),
            ChartData::Cells => d.cell_data(),
            ChartData::Auto => match d.point_data() {
                Some(p) if !p.is_empty() => Some(p),
                _ => d.cell_data(),
            },
        },
    };
    let mut out = Vec::new();
    if let Some(a) = attrs {
        for arr in a.iter() {
            if arr.scalar_type() == ScalarType::String || arr.len() > 2_000_000 {
                continue;
            }
            if arr.ncomp == 1 {
                out.push((arr.name.clone(), (0..arr.len()).map(|i| arr.get(i, 0)).collect()));
            } else {
                out.push((format!("{} (magnitude)", arr.name), (0..arr.len()).map(|i| arr.magnitude(i)).collect()));
                for c in 0..arr.ncomp.min(3) {
                    out.push((format!("{}_{}", arr.name, ["X", "Y", "Z"][c]), (0..arr.len()).map(|i| arr.get(i, c)).collect()));
                }
            }
        }
    }
    out
}

/// The array name of a column (without its `block/` prefix).
fn base(name: &str) -> &str {
    name.rsplit_once('/').map(|(_, b)| b).unwrap_or(name)
}

/// The x axis and first series a chart shows by default for these columns.
pub fn default_axes(cols: &[(String, Vec<f64>)], is_table: bool) -> (String, Vec<String>) {
    // Blocks: the same array in every block (one line per block).
    if cols.iter().any(|c| c.0.contains('/')) {
        let x = if cols.iter().any(|c| base(&c.0) == "arc_length") { "arc_length".to_string() } else { "(index)".to_string() };
        let first =
            cols.iter().map(|c| base(&c.0)).find(|n| *n != x && !matches!(*n, "arc_length" | "vtkValidPointMask") && !n.ends_with("Id") && !n.ends_with("Ids"));
        let y = match first {
            Some(f) => cols.iter().filter(|c| base(&c.0) == f).map(|c| c.0.clone()).collect(),
            None => Vec::new(),
        };
        return (x, y);
    }
    let x = if cols.iter().any(|c| c.0 == "arc_length") {
        "arc_length".to_string()
    } else if cols.iter().any(|c| c.0 == "bin_center") {
        "bin_center".to_string()
    } else if cols.iter().any(|c| c.0 == "Time") && is_table {
        "Time".to_string()
    } else if is_table && cols.len() > 1 {
        cols[0].0.clone()
    } else {
        "(index)".to_string()
    };
    let skip = |n: &str| n == x || matches!(n, "arc_length" | "vtkValidPointMask" | "bin_start" | "bin_end") || n.ends_with("Id") || n.ends_with("Ids");
    let y = cols.iter().find(|c| !skip(&c.0)).map(|c| vec![c.0.clone()]).unwrap_or_default();
    (x, y)
}

/// The (x, y) points of every plotted series of a view, from the given columns.
pub fn series_points(view: &ChartView, cols: &[(String, Vec<f64>)]) -> Vec<(String, Vec<[f64; 2]>)> {
    let xs: Option<&Vec<f64>> = cols.iter().find(|c| c.0 == view.x).map(|c| &c.1);
    let ok = |v: f64, log: bool| v.is_finite() && (!log || v > 0.0);
    let mut out = Vec::new();
    for name in &view.y {
        let Some((_, v)) = cols.iter().find(|c| &c.0 == name) else { continue };
        // A block's line uses that block's x array.
        let own_x = name.rsplit_once('/').and_then(|(blk, _)| cols.iter().find(|c| c.0 == format!("{blk}/{}", view.x)).map(|c| &c.1));
        let xs = own_x.or(xs);
        let pts =
            v.iter().enumerate().map(|(i, y)| [xs.map(|x| x[i]).unwrap_or(i as f64), *y]).filter(|p| ok(p[0], view.log_x) && ok(p[1], view.log_y)).collect();
        out.push((name.clone(), pts));
    }
    out
}

impl Session {
    /// Chart view `id` (view 0 always exists).
    pub fn chart_view(&mut self, id: u32) -> Option<&mut ChartView> {
        if id == 0 && !self.chart_views.iter().any(|v| v.id == 0) {
            self.chart_views.insert(0, ChartView::default());
        }
        self.chart_views.iter_mut().find(|v| v.id == id)
    }

    /// Open a new chart view of `node` (None: the selected object). Returns its number.
    pub fn new_chart_view(&mut self, node: Option<NodeId>) -> u32 {
        let id = self.chart_views.iter().map(|v| v.id + 1).max().unwrap_or(1).max(1);
        self.chart_views.push(ChartView { id, node, ..Default::default() });
        self.app_commands.push(format!("chart_open {id}"));
        self.touch();
        id
    }

    /// Close chart view `id` (view 0, the Charts panel, is only hidden).
    pub fn close_chart_view(&mut self, id: u32) -> bool {
        let before = self.chart_views.len();
        if id != 0 {
            self.chart_views.retain(|v| v.id != id);
        }
        self.app_commands.push(format!("chart_close {id}"));
        self.touch();
        id == 0 || self.chart_views.len() != before
    }

    /// The node a chart view plots now (its own, or the selected one).
    pub fn chart_node(&self, id: u32) -> Option<NodeId> {
        let v = self.chart_views.iter().find(|v| v.id == id);
        match v.and_then(|v| v.node) {
            Some(n) => Some(n),
            None => self.active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurafx_data::{Array, Table};

    fn table() -> DataSet {
        let mut t = Table::default();
        t.columns.insert(Array::scalars_f64("Time", vec![0.0, 1.0, 2.0, 3.0]));
        t.columns.insert(Array::scalars_f64("T", vec![1.0, 4.0, 9.0, 16.0]));
        t.columns.insert(Array::from_vec("v", 3, vec![1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0f64]));
        DataSet::Table(t)
    }

    #[test]
    fn columns_defaults_and_points() {
        let ds = table();
        let cols = columns(&ds, ChartData::Auto);
        let names: Vec<&str> = cols.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(names, ["Time", "T", "v (magnitude)", "v_X", "v_Y", "v_Z"]);
        let (x, y) = default_axes(&cols, true);
        assert_eq!((x.as_str(), y), ("Time", vec!["T".to_string()]));
        let mut view = ChartView { x, y: vec!["T".into(), "v (magnitude)".into()], ..Default::default() };
        let s = series_points(&view, &cols);
        assert_eq!(s[0].1, vec![[0.0, 1.0], [1.0, 4.0], [2.0, 9.0], [3.0, 16.0]]);
        assert_eq!(s[1].1[2], [2.0, 3.0]);
        // A log axis leaves out values that are not positive.
        view.log_x = true;
        assert_eq!(series_points(&view, &cols)[0].1.len(), 3);
    }

    #[test]
    fn blocks_plot_one_line_each() {
        let mut mb = aurafx_data::MultiBlock::default();
        for (k, name) in ["line 1", "line 2"].iter().enumerate() {
            let mut t = Table::default();
            t.columns.insert(Array::scalars_f64("arc_length", vec![0.0, 1.0, 2.0]));
            t.columns.insert(Array::scalars_f64("T", vec![k as f64, 1.0, 2.0]));
            mb.blocks.push((name.to_string(), Some(DataSet::Table(t))));
        }
        let ds = DataSet::MultiBlock(mb);
        let cols = columns(&ds, ChartData::Auto);
        let (x, y) = default_axes(&cols, false);
        assert_eq!(x, "arc_length");
        assert_eq!(y, vec!["line 1/T".to_string(), "line 2/T".to_string()]);
        let view = ChartView { x, y, ..Default::default() };
        let s = series_points(&view, &cols);
        assert_eq!(s[1].1, vec![[0.0, 1.0], [1.0, 1.0], [2.0, 2.0]]);
    }

    #[test]
    fn views_open_close_and_follow() {
        let mut s = Session::new();
        assert!(s.chart_view(0).is_some());
        let a = s.new_chart_view(Some(7));
        let b = s.new_chart_view(None);
        assert_eq!((a, b), (1, 2));
        s.active = Some(3);
        assert_eq!(s.chart_node(a), Some(7));
        assert_eq!(s.chart_node(b), Some(3));
        assert!(s.close_chart_view(a));
        assert!(!s.close_chart_view(a));
        assert_eq!(s.new_chart_view(None), 3);
        assert!(s.app_commands.iter().any(|c| c == "chart_open 1"));
        assert!(s.app_commands.iter().any(|c| c == "chart_close 1"));
    }
}
