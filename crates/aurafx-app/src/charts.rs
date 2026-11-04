//! Charts panel: line plots of the active node's arrays (e.g. Plot Over Line output, tables)
//! and of series sent from Python / time analysis.

use crate::app::State;
use crate::icons;
use aurafx_data::DataSet;
use egui::RichText;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct ChartsState {
    /// Arrays of the active node chosen for plotting (by name).
    pub selected: BTreeSet<String>,
    pub x_axis: String,
    pub last_node: Option<u64>,
    pub over_time_array: String,
    pub over_time_stat: String,
}

/// Columns usable as series from the active dataset: (name, values).
fn columns(ds: &DataSet) -> Vec<(String, Vec<f64>)> {
    let attrs = match ds {
        DataSet::Table(t) => Some(&t.columns),
        DataSet::MultiBlock(_) => None,
        d => d.point_data(),
    };
    let mut out = Vec::new();
    if let Some(a) = attrs {
        for arr in a.iter() {
            if arr.scalar_type() == aurafx_data::ScalarType::String || arr.len() > 2_000_000 {
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

pub fn charts_ui(state: &mut State, ui: &mut egui::Ui) {
    let (active, ds, chart, has_time) = {
        let s = state.session.lock();
        (s.active, s.active_node().and_then(|n| n.output.clone()), s.chart.clone(), s.times.len() > 1)
    };
    let cols = ds.as_deref().map(columns).unwrap_or_default();
    let cs = &mut state.charts;
    // (Re)initialize once the active node has columns to offer.
    if cs.last_node != active && !cols.is_empty() {
        cs.last_node = active;
        cs.selected.clear();
        let is_table = matches!(ds.as_deref(), Some(DataSet::Table(_)));
        cs.x_axis = if cols.iter().any(|c| c.0 == "arc_length") {
            "arc_length".into()
        } else if cols.iter().any(|c| c.0 == "bin_center") {
            "bin_center".into()
        } else if is_table && cols.len() > 1 {
            cols[0].0.clone()
        } else {
            "(index)".into()
        };
        // Default: plot the first meaningful array.
        let skip = |n: &str| n == cs.x_axis || matches!(n, "arc_length" | "vtkValidPointMask" | "bin_start" | "bin_end") || n.ends_with("Id");
        if let Some((n, _)) = cols.iter().find(|c| !skip(&c.0)) {
            cs.selected.insert(n.clone());
        }
    }
    ui.horizontal_wrapped(|ui| {
        if !cols.is_empty() {
            ui.label(RichText::new("X").strong());
            egui::ComboBox::from_id_salt("chart_x").selected_text(&cs.x_axis).show_ui(ui, |ui| {
                ui.selectable_value(&mut cs.x_axis, "(index)".to_string(), "(index)");
                for (n, _) in &cols {
                    ui.selectable_value(&mut cs.x_axis, n.clone(), n);
                }
            });
            ui.label(RichText::new("Y").strong());
            ui.menu_button(format!("{} arrays", cs.selected.len()), |ui| {
                for (n, _) in &cols {
                    let mut on = cs.selected.contains(n);
                    if ui.checkbox(&mut on, n).changed() {
                        if on {
                            cs.selected.insert(n.clone());
                        } else {
                            cs.selected.remove(n);
                        }
                    }
                }
            });
        }
        if has_time {
            ui.separator();
            ui.label("Over time:");
            if cs.over_time_stat.is_empty() {
                cs.over_time_stat = "max".into();
            }
            egui::ComboBox::from_id_salt("ot_stat").selected_text(&cs.over_time_stat).width(60.0).show_ui(ui, |ui| {
                for st in ["max", "min", "mean", "sum", "value"] {
                    ui.selectable_value(&mut cs.over_time_stat, st.to_string(), st);
                }
            });
            let names: Vec<String> = {
                let s = state.session.lock();
                s.active_node().and_then(|n| n.info.as_ref()).map(|i| i.point_arrays.iter().chain(&i.cell_arrays).chain(&i.field_arrays).map(|a| a.name.clone()).collect()).unwrap_or_default()
            };
            if cs.over_time_array.is_empty() || !names.contains(&cs.over_time_array) {
                cs.over_time_array = names.first().cloned().unwrap_or_default();
            }
            egui::ComboBox::from_id_salt("ot_arr").selected_text(&cs.over_time_array).width(120.0).show_ui(ui, |ui| {
                for n in &names {
                    ui.selectable_value(&mut cs.over_time_array, n.clone(), n);
                }
            });
            if ui.add_enabled(active.is_some() && !cs.over_time_array.is_empty(), egui::Button::new(format!("{} Plot", icons::WAVE_SINE))).on_hover_text("Re-runs the pipeline for every time step in the background").clicked() {
                let code = format!("afx.over_time(afx.get('{}'), {:?}, stat={:?})", active.unwrap(), cs.over_time_array, cs.over_time_stat);
                state.python.run(code, "charts");
            }
        }
        if !chart.series.is_empty() && ui.button(format!("{} Clear script plots", icons::TRASH)).clicked() {
            state.session.lock().chart = Default::default();
        }
        if ui.button(format!("{} CSV", icons::DOWNLOAD_SIMPLE)).on_hover_text("Export the plotted data").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("chart.csv").save_file() {
                let mut out = String::from("series,x,y\n");
                for s in &chart.series {
                    for (x, y) in s.x.iter().zip(&s.y) {
                        out += &format!("{},{x},{y}\n", s.name);
                    }
                }
                let xs = cols.iter().find(|c| c.0 == cs.x_axis).map(|c| c.1.clone());
                for (n, v) in cols.iter().filter(|c| cs.selected.contains(&c.0)) {
                    for (i, y) in v.iter().enumerate() {
                        let x = xs.as_ref().map(|x| x[i]).unwrap_or(i as f64);
                        out += &format!("{n},{x},{y}\n");
                    }
                }
                let _ = std::fs::write(p, out);
            }
        }
    });
    let cs = &state.charts;
    let xs: Option<&Vec<f64>> = cols.iter().find(|c| c.0 == cs.x_axis).map(|c| &c.1);
    let mut lines: Vec<(String, Vec<[f64; 2]>)> = Vec::new();
    for (n, v) in cols.iter().filter(|c| cs.selected.contains(&c.0)) {
        let pts: Vec<[f64; 2]> = v.iter().enumerate().filter(|(_, y)| y.is_finite()).map(|(i, y)| [xs.map(|x| x[i]).unwrap_or(i as f64), *y]).collect();
        lines.push((n.clone(), pts));
    }
    for s in &chart.series {
        lines.push((s.name.clone(), s.x.iter().zip(&s.y).map(|(x, y)| [*x, *y]).collect()));
    }
    if lines.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("Nothing to plot yet. Select a node with line or table data (try the Plot Over Line filter), plot a statistic over time, or call afx.plot(x, y) from Python.").weak());
        return;
    }
    if !chart.title.is_empty() {
        ui.vertical_centered(|ui| ui.label(RichText::new(&chart.title).strong()));
    }
    let x_label = if !chart.series.is_empty() && !chart.x_label.is_empty() { chart.x_label.clone() } else { cs.x_axis.clone() };
    let mut plot = Plot::new("aurafx_chart").legend(Legend::default()).x_axis_label(x_label);
    if !chart.y_label.is_empty() {
        plot = plot.y_axis_label(chart.y_label.clone());
    }
    plot.show(ui, |pui| {
        for (name, pts) in lines {
            pui.line(Line::new(name, PlotPoints::from(pts)).width(1.8));
        }
    });
}
