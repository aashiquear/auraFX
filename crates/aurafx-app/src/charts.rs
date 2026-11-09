//! Chart views: line, point and bar charts of an object's arrays (e.g. Plot Over Line output,
//! tables, histograms) and of series sent from Python / time analysis.
//!
//! View 0 is the Charts panel; more chart views open beside it (View › New Chart View, the
//! Pipeline's right-click menu, `afx.chart_view`), each plotting its own object or following the
//! selected one. Their settings live in the session (`aurafx_pipeline::charts`).

use crate::app::State;
use crate::icons;
use crate::menus::MenuExt;
use aurafx_data::DataSet;
use aurafx_pipeline::charts::{columns, default_axes, series_points, ChartData, ChartStyle, ChartView};
use aurafx_pipeline::ChartSeries;
use egui::{Color32, RichText};
use egui_plot::{Bar, BarChart, BoxElem, BoxPlot, BoxSpread, Legend, Line, Plot, PlotPoints, Points};

#[derive(Default)]
pub struct ChartsState {
    pub over_time_array: String,
    pub over_time_stat: String,
}

/// Series colours, the same in every chart view (n-th series, n-th colour).
const PALETTE: [Color32; 10] = [
    Color32::from_rgb(31, 119, 180),
    Color32::from_rgb(255, 127, 14),
    Color32::from_rgb(44, 160, 44),
    Color32::from_rgb(214, 39, 40),
    Color32::from_rgb(148, 103, 189),
    Color32::from_rgb(140, 86, 75),
    Color32::from_rgb(227, 119, 194),
    Color32::from_rgb(127, 127, 127),
    Color32::from_rgb(188, 189, 34),
    Color32::from_rgb(23, 190, 207),
];

/// Tab title of chart view `id`.
pub fn chart_title(state: &State, id: u32) -> String {
    let s = state.session.lock();
    let view = s.chart_views.iter().find(|v| v.id == id).cloned().unwrap_or(ChartView { id, ..Default::default() });
    let name = view.node.and_then(|n| s.nodes.get(&n)).map(|n| n.name.clone());
    view.title_with(name.as_deref())
}

/// Everything one frame of a chart view needs, read under one lock.
struct Snapshot {
    view: ChartView,
    node: Option<u64>,
    ds: Option<std::sync::Arc<DataSet>>,
    nodes: Vec<(u64, String)>,
    has_time: bool,
    script: Vec<ChartSeries>,
    x_label: String,
    y_label: String,
    title: String,
}

fn snapshot(state: &State, id: u32) -> Snapshot {
    let mut s = state.session.lock();
    if s.chart_view(id).is_none() {
        // A chart pane restored from a saved layout: give it a view that follows the selection.
        s.chart_views.push(ChartView { id, ..Default::default() });
    }
    let view = s.chart_view(id).cloned().unwrap_or_default();
    let node = s.chart_node(id);
    let ds = node.and_then(|n| s.nodes.get(&n)).and_then(|n| n.output.clone());
    let nodes = s.nodes.values().filter(|n| n.output.is_some()).map(|n| (n.id, n.name.clone())).collect();
    let has_time = s.times.len() > 1;
    let (script, x_label, y_label, title) = if id == 0 {
        (
            s.chart.series.clone(),
            s.chart.x_label.clone(),
            s.chart.y_label.clone(),
            if view.title.is_empty() { s.chart.title.clone() } else { view.title.clone() },
        )
    } else {
        (view.series.clone(), String::new(), String::new(), view.title.clone())
    };
    Snapshot { view, node, ds, nodes, has_time, script, x_label, y_label, title }
}

/// Choose the axes for a newly shown object: a view that follows the selection starts again
/// from the defaults; a view of its own object only fills in what is not set.
fn fill_defaults(view: &mut ChartView, node: Option<u64>, cols: &[(String, Vec<f64>)], is_table: bool) -> bool {
    if view.defaults_for == node || cols.is_empty() {
        return false;
    }
    let (dx, dy) = default_axes(cols, is_table);
    let has = |n: &str| n == "(index)" || cols.iter().any(|c| c.0 == n);
    if view.node.is_none() {
        view.x = dx;
        view.y = dy;
    } else {
        if view.x.is_empty() || !has(&view.x) {
            view.x = dx;
        }
        view.y.retain(|n| has(n));
        if view.y.is_empty() {
            view.y = dy;
        }
    }
    view.defaults_for = node;
    true
}

pub fn charts_ui(state: &mut State, ui: &mut egui::Ui) {
    chart_view_ui(state, ui, 0);
}

pub fn chart_view_ui(state: &mut State, ui: &mut egui::Ui, id: u32) {
    let snap = snapshot(state, id);
    let mut view = snap.view.clone();
    let cols = snap.ds.as_deref().map(|d| columns(d, view.data)).unwrap_or_default();
    let is_table = matches!(snap.ds.as_deref(), Some(DataSet::Table(_)));
    let has_cells = snap.ds.as_deref().and_then(|d| d.cell_data()).is_some_and(|c| !c.is_empty());
    fill_defaults(&mut view, snap.node, &cols, is_table);
    let mut new_view = false;
    let mut clear_script = false;
    let mut picture = false;
    // A picture of the views is being taken: leave the controls out of it.
    let hide = state.capture.hide_controls();
    if !hide {
        ui.horizontal_wrapped(|ui| {
            // The object plotted.
            let follow = "Selected object".to_string();
            let current = match view.node {
                None => follow.clone(),
                Some(n) => snap.nodes.iter().find(|(i, _)| *i == n).map(|(_, name)| name.clone()).unwrap_or_else(|| "(removed)".into()),
            };
            ui.label(RichText::new(format!("{}", icons::TREE_STRUCTURE)).strong()).on_hover_text("The object this chart plots");
            egui::ComboBox::from_id_salt(("chart_node", id)).selected_text(current).width(130.0).show_ui(ui, |ui| {
                if ui.selectable_label(view.node.is_none(), follow).on_hover_text("Plot whichever object is selected in the Pipeline").clicked() {
                    view.node = None;
                    view.defaults_for = Some(u64::MAX);
                }
                for (nid, name) in &snap.nodes {
                    if ui.selectable_label(view.node == Some(*nid), name).clicked() && view.node != Some(*nid) {
                        view.node = Some(*nid);
                        view.defaults_for = Some(u64::MAX);
                    }
                }
            });
            if !cols.is_empty() || has_cells {
                if !is_table && has_cells {
                    let label = match view.data {
                        ChartData::Auto => "auto",
                        ChartData::Points => "points",
                        ChartData::Cells => "cells",
                    };
                    egui::ComboBox::from_id_salt(("chart_data", id)).selected_text(label).width(60.0).show_ui(ui, |ui| {
                        for (d, l) in [(ChartData::Auto, "auto"), (ChartData::Points, "points"), (ChartData::Cells, "cells")] {
                            if ui.selectable_label(view.data == d, l).clicked() {
                                view.data = d;
                                view.defaults_for = Some(u64::MAX);
                            }
                        }
                    });
                }
                ui.label(RichText::new("X").strong());
                egui::ComboBox::from_id_salt(("chart_x", id)).selected_text(&view.x).show_ui(ui, |ui| {
                    ui.selectable_value(&mut view.x, "(index)".to_string(), "(index)");
                    for (n, _) in &cols {
                        ui.selectable_value(&mut view.x, n.clone(), n);
                    }
                });
                ui.label(RichText::new("Y").strong());
                ui.scroll_menu(format!("{} arrays", view.y.len()), |ui| {
                    for (n, _) in &cols {
                        let mut on = view.y.contains(n);
                        if ui.checkbox(&mut on, n).changed() {
                            if on {
                                view.y.push(n.clone());
                            } else {
                                view.y.retain(|y| y != n);
                            }
                        }
                    }
                });
            }
            egui::ComboBox::from_id_salt(("chart_style", id)).selected_text(view.style.label()).width(80.0).show_ui(ui, |ui| {
                for st in ChartStyle::ALL {
                    ui.selectable_value(&mut view.style, st, st.label());
                }
            });
            ui.checkbox(&mut view.log_x, "log x");
            ui.checkbox(&mut view.log_y, "log y");
            if snap.has_time {
                ui.separator();
                over_time_ui(state, ui, snap.node);
            }
            if !snap.script.is_empty() && ui.button(format!("{} Clear script plots", icons::TRASH)).clicked() {
                clear_script = true;
            }
            if ui.button(format!("{} CSV", icons::DOWNLOAD_SIMPLE)).on_hover_text("Export the plotted data").clicked() {
                if let Some(p) = crate::dialogs::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("chart.csv").save_file() {
                    let _ = std::fs::write(p, csv(&view, &cols, &snap.script));
                }
            }
            if ui
                .button(format!("{} New chart view", icons::PLUS))
                .on_hover_text("Open another chart view beside this one, for this object or any other")
                .clicked()
            {
                new_view = true;
            }
            if ui.button(format!("{} Picture…", icons::CAMERA)).on_hover_text("Save this chart as a PNG picture, without these controls").clicked() {
                picture = true;
            }
        });
    }
    {
        let mut s = state.session.lock();
        if let Some(v) = s.chart_view(id) {
            if *v != view {
                *v = view.clone();
                s.revision += 1;
            }
        }
        if clear_script {
            if id == 0 {
                s.chart = Default::default();
            } else if let Some(v) = s.chart_view(id) {
                v.series.clear();
            }
        }
        if new_view {
            let node = snap.node;
            s.new_chart_view(node);
        }
    }
    if picture {
        if let Some(path) = crate::dialogs::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("chart.png").save_file() {
            let pane = if id == 0 { crate::app::Pane::Charts } else { crate::app::Pane::Chart(id) };
            state.capture.request(crate::capture::Request {
                path,
                views: crate::capture::Views::One(pane),
                transparent: false,
                controls: false,
                done: crate::capture::Done::Toast,
            });
        }
    }
    let mut lines = series_points(&view, &cols);
    for sr in &snap.script {
        lines.push((sr.name.clone(), sr.x.iter().zip(&sr.y).map(|(x, y)| [*x, *y]).filter(|p| p[0].is_finite() && p[1].is_finite()).collect()));
    }
    if lines.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Nothing to plot yet. Choose an object with line or table data (try the Plot Over Line filter), plot a statistic over time, or call afx.plot(x, y) from Python.",
            )
            .weak(),
        );
        return;
    }
    if !snap.title.is_empty() {
        ui.vertical_centered(|ui| ui.label(RichText::new(&snap.title).strong()));
    }
    draw_plot(ui, id, &view, lines, &snap.x_label, &snap.y_label, hide);
}

#[allow(clippy::too_many_arguments)]
fn draw_plot(ui: &mut egui::Ui, id: u32, view: &ChartView, lines: Vec<(String, Vec<[f64; 2]>)>, x_label: &str, y_label: &str, picture: bool) {
    let tf = |v: f64, log: bool| if log { v.log10() } else { v };
    let x_name = if !x_label.is_empty() { x_label.to_string() } else { view.x.clone() };
    let mut plot = Plot::new(("aurafx_chart", id))
        .legend(Legend::default())
        .x_axis_label(if view.log_x { format!("{x_name} (log)") } else { x_name })
        // No hover read-out in pictures.
        .show_x(!picture)
        .show_y(!picture);
    if !y_label.is_empty() || view.log_y {
        plot = plot.y_axis_label(if view.log_y { format!("{y_label} (log)").trim().to_string() } else { y_label.to_string() });
    }
    if view.log_x {
        plot = plot.x_axis_formatter(|m, _| format!("{:.3e}", 10f64.powf(m.value)));
    }
    if view.log_y {
        plot = plot.y_axis_formatter(|m, _| format!("{:.3e}", 10f64.powf(m.value)));
    }
    let n = lines.len().max(1);
    plot.show(ui, |pui| {
        for (k, (name, pts)) in lines.into_iter().enumerate() {
            let color = PALETTE[k % PALETTE.len()];
            let pts: Vec<[f64; 2]> = pts.into_iter().map(|p| [tf(p[0], view.log_x), tf(p[1], view.log_y)]).collect();
            match view.style {
                ChartStyle::Lines => pui.line(Line::new(name, PlotPoints::from(pts)).color(color).width(1.8)),
                ChartStyle::Points => pui.points(Points::new(name, PlotPoints::from(pts)).color(color).radius(2.5)),
                ChartStyle::Markers => {
                    pui.line(Line::new(name.clone(), PlotPoints::from(pts.clone())).color(color).width(1.5));
                    pui.points(Points::new(name, PlotPoints::from(pts)).color(color).radius(3.0));
                }
                ChartStyle::Bars => {
                    // Side by side: each series gets a share of the smallest step between values.
                    let mut xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
                    xs.sort_by(|a, b| a.total_cmp(b));
                    let step = xs.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
                    let step = if step.is_finite() { step } else { 1.0 };
                    let w = step * 0.8 / n as f64;
                    let off = (k as f64 - (n as f64 - 1.0) / 2.0) * w;
                    let bars = pts.iter().map(|p| Bar::new(p[0] + off, p[1]).width(w)).collect();
                    pui.bar_chart(BarChart::new(name, bars).color(color));
                }
                ChartStyle::Box => {
                    // The distribution of the series' values, one box per series at x = 1, 2, ...
                    let mut v: Vec<f64> = pts.iter().map(|p| p[1]).filter(|y| y.is_finite()).collect();
                    v.sort_by(|a, b| a.total_cmp(b));
                    if v.is_empty() {
                        continue;
                    }
                    let q = |f: f64| aurafx_filters::stats::quantile(&v, f);
                    let (q1, q2, q3) = (q(0.25), q(0.5), q(0.75));
                    let iqr = q3 - q1;
                    let lo = v.iter().copied().find(|&x| x >= q1 - 1.5 * iqr).unwrap_or(q1);
                    let hi = v.iter().rev().copied().find(|&x| x <= q3 + 1.5 * iqr).unwrap_or(q3);
                    let elem = BoxElem::new((k + 1) as f64, BoxSpread::new(lo, q1, q2, q3, hi)).box_width(0.5).whisker_width(0.3);
                    pui.box_plot(BoxPlot::new(name, vec![elem]).color(color));
                }
            }
        }
    });
}

fn csv(view: &ChartView, cols: &[(String, Vec<f64>)], script: &[ChartSeries]) -> String {
    let mut out = String::from("series,x,y\n");
    for s in script {
        for (x, y) in s.x.iter().zip(&s.y) {
            out += &format!("{},{x},{y}\n", s.name);
        }
    }
    let xs = cols.iter().find(|c| c.0 == view.x).map(|c| c.1.clone());
    for n in &view.y {
        if let Some((_, v)) = cols.iter().find(|c| &c.0 == n) {
            for (i, y) in v.iter().enumerate() {
                let x = xs.as_ref().map(|x| x[i]).unwrap_or(i as f64);
                out += &format!("{n},{x},{y}\n");
            }
        }
    }
    out
}

/// Plot a statistic of an array over every time step (into the Charts panel).
fn over_time_ui(state: &mut State, ui: &mut egui::Ui, node: Option<u64>) {
    ui.label("Over time:");
    let cs = &mut state.charts;
    if cs.over_time_stat.is_empty() {
        cs.over_time_stat = "max".into();
    }
    egui::ComboBox::from_id_salt(ui.scope_id().with("ot_stat")).selected_text(&cs.over_time_stat).width(60.0).show_ui(ui, |ui| {
        for st in ["max", "min", "mean", "sum", "value"] {
            ui.selectable_value(&mut cs.over_time_stat, st.to_string(), st);
        }
    });
    let names: Vec<String> = {
        let s = state.session.lock();
        node.and_then(|n| s.nodes.get(&n))
            .and_then(|n| n.info.as_ref())
            .map(|i| i.point_arrays.iter().chain(&i.cell_arrays).chain(&i.field_arrays).map(|a| a.name.clone()).collect())
            .unwrap_or_default()
    };
    let cs = &mut state.charts;
    if cs.over_time_array.is_empty() || !names.contains(&cs.over_time_array) {
        cs.over_time_array = names.first().cloned().unwrap_or_default();
    }
    egui::ComboBox::from_id_salt(ui.scope_id().with("ot_arr")).selected_text(&cs.over_time_array).width(120.0).show_ui(ui, |ui| {
        for n in &names {
            ui.selectable_value(&mut cs.over_time_array, n.clone(), n);
        }
    });
    if ui
        .add_enabled(node.is_some() && !cs.over_time_array.is_empty(), egui::Button::new(format!("{} Plot", icons::WAVE_SINE)))
        .on_hover_text("Re-runs the pipeline for every time step in the background; the result goes to the Charts panel")
        .clicked()
    {
        let code = format!("afx.over_time(afx.get('{}'), {:?}, stat={:?})", node.unwrap(), cs.over_time_array, cs.over_time_stat);
        state.python.run(code, "charts");
    }
}
