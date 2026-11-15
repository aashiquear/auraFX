//! Goal Statistics: edit the grain size and shape distributions (and the orientations) of a
//! Pack Grains source with live plots, pack, and compare the packed volume's measured
//! statistics with the goals.

use crate::app::State;
use crate::touch::FitScreen;
use aurafx_data::DataSet;
use aurafx_filters::ParamValue;
use aurafx_pipeline::{NodeId, NodeKind};
use egui::{Color32, RichText};
use egui_plot::{Bar, BarChart, Legend, Line, Plot, PlotPoints};
use std::sync::Arc;

#[derive(Clone)]
pub struct Goals {
    pub cells: [f64; 3],
    pub spacing: f64,
    pub mu: f64,
    pub sigma: f64,
    pub min_cut: f64,
    pub max_cut: f64,
    pub ba: [f64; 2],
    pub ca: [f64; 2],
    pub shape: String,
    pub aligned: bool,
    pub fraction: f64,
    pub orientations: String,
    pub spread: f64,
    pub seed: i64,
    pub calibrate: bool,
}

impl Default for Goals {
    fn default() -> Self {
        Goals {
            cells: [64.0; 3],
            spacing: 1.0,
            mu: 2.3,
            sigma: 0.2,
            min_cut: 3.0,
            max_cut: 3.0,
            ba: [15.0, 2.0],
            ca: [12.0, 4.0],
            shape: aurafx_filters::synthetic::SHAPE_ELLIPSOID.into(),
            aligned: false,
            fraction: 1.0,
            orientations: aurafx_filters::synthetic::ORIENT_RANDOM.into(),
            spread: 10.0,
            seed: 1,
            calibrate: true,
        }
    }
}

#[derive(Default)]
pub struct GoalStats {
    pub open: bool,
    pub goals: Goals,
    /// The Pack Grains source the panel edits (None: a new one on Pack).
    pub node: Option<NodeId>,
    /// (output pointer, equivalent diameters, b/a, c/a) of the packed volume.
    measured: Option<(usize, Vec<f64>, Vec<f64>, Vec<f64>)>,
}

fn pack_id(n: &aurafx_pipeline::Node) -> bool {
    matches!(&n.kind, NodeKind::Source { def } if def.id == "pack_grains")
}

impl Goals {
    fn from_params(p: &aurafx_filters::params::Params) -> Self {
        let d = Goals::default();
        let fl = |k: &str, n: usize, def: &[f64]| {
            let v = p.floats(k);
            if v.len() >= n {
                v
            } else {
                def.to_vec()
            }
        };
        let c = fl("cells", 3, &d.cells);
        let ba = fl("ba", 2, &d.ba);
        let ca = fl("ca", 2, &d.ca);
        Goals {
            cells: [c[0], c[1], c[2]],
            spacing: p.f64("spacing", d.spacing),
            mu: p.f64("esd_mu", d.mu),
            sigma: p.f64("esd_sigma", d.sigma),
            min_cut: p.f64("min_cut", d.min_cut),
            max_cut: p.f64("max_cut", d.max_cut),
            ba: [ba[0], ba[1]],
            ca: [ca[0], ca[1]],
            shape: Some(p.str("shape")).filter(|s| !s.is_empty()).unwrap_or(d.shape),
            aligned: p.bool("aligned", d.aligned),
            fraction: p.f64("fraction", d.fraction),
            orientations: Some(p.str("orientations")).filter(|s| !s.is_empty()).unwrap_or(d.orientations),
            spread: p.f64("spread", d.spread),
            seed: p.i64("seed", d.seed),
            calibrate: p.bool("calibrate", d.calibrate),
        }
    }

    fn values(&self) -> Vec<(&'static str, ParamValue)> {
        vec![
            ("cells", ParamValue::Floats(self.cells.to_vec())),
            ("spacing", ParamValue::Float(self.spacing)),
            ("esd_mu", ParamValue::Float(self.mu)),
            ("esd_sigma", ParamValue::Float(self.sigma)),
            ("min_cut", ParamValue::Float(self.min_cut)),
            ("max_cut", ParamValue::Float(self.max_cut)),
            ("ba", ParamValue::Floats(self.ba.to_vec())),
            ("ca", ParamValue::Floats(self.ca.to_vec())),
            ("shape", ParamValue::Str(self.shape.clone())),
            ("aligned", ParamValue::Bool(self.aligned)),
            ("fraction", ParamValue::Float(self.fraction)),
            ("orientations", ParamValue::Str(self.orientations.clone())),
            ("spread", ParamValue::Float(self.spread)),
            ("seed", ParamValue::Int(self.seed)),
            ("calibrate", ParamValue::Bool(self.calibrate)),
        ]
    }
}

/// ln Gamma (Lanczos, g = 7).
fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

pub fn beta_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 || x >= 1.0 || a <= 0.0 || b <= 0.0 {
        return 0.0;
    }
    ((a - 1.0) * x.ln() + (b - 1.0) * (1.0 - x).ln() + ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b)).exp()
}

pub fn lognormal_pdf(d: f64, mu: f64, sigma: f64) -> f64 {
    if d <= 0.0 || sigma <= 0.0 {
        return 0.0;
    }
    let z = (d.ln() - mu) / sigma;
    (-0.5 * z * z).exp() / (d * sigma * (2.0 * std::f64::consts::PI).sqrt())
}

/// Equivalent diameters and aspect ratios of the packed volume's features.
fn measure(ds: &DataSet) -> Option<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let d = aurafx_filters::features::feature_sizes(ds, "FeatureIds").ok()?;
    let d = aurafx_filters::features::feature_shapes(&d, "FeatureIds").ok()?;
    let fd = d.field_data();
    let (esd, ar, n) = (fd.get("EquivalentDiameters")?, fd.get("AspectRatios")?, fd.get("NumElements")?);
    let keep: Vec<usize> = (1..esd.len()).filter(|&f| n.get(f, 0) > 0.0).collect();
    Some((
        keep.iter().map(|&f| esd.get(f, 0)).collect(),
        keep.iter().map(|&f| ar.get(f, 0)).filter(|v| *v > 0.0 && *v <= 1.0).collect(),
        keep.iter().map(|&f| ar.get(f, 1)).filter(|v| *v > 0.0 && *v <= 1.0).collect(),
    ))
}

/// A density histogram of `v` over [lo, hi] in `bins` bars.
fn histogram(v: &[f64], lo: f64, hi: f64, bins: usize, color: Color32) -> Option<BarChart> {
    if v.is_empty() || hi <= lo {
        return None;
    }
    let w = (hi - lo) / bins as f64;
    let mut c = vec![0usize; bins];
    for x in v {
        let k = ((x - lo) / w).floor();
        if k >= 0.0 && (k as usize) < bins {
            c[k as usize] += 1;
        }
    }
    let bars = c.iter().enumerate().map(|(k, &n)| Bar::new(lo + (k as f64 + 0.5) * w, n as f64 / (v.len() as f64 * w)).width(w * 0.95)).collect();
    Some(BarChart::new("Packed", bars).color(color.gamma_multiply(0.55)))
}

fn curve(name: &str, lo: f64, hi: f64, f: impl Fn(f64) -> f64, color: Color32) -> Line<'static> {
    let pts: Vec<[f64; 2]> = (0..=200).map(|k| lo + (hi - lo) * k as f64 / 200.0).map(|x| [x, f(x)]).collect();
    Line::new(name.to_string(), PlotPoints::new(pts)).color(color).width(2.0)
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

pub fn window(state: &mut State, ctx: &egui::Context) {
    if !state.goal_stats.open {
        return;
    }
    // Bind to the active Pack Grains source when there is one.
    {
        let s = state.session.lock();
        if let Some(n) = s.active_node().filter(|n| pack_id(n)) {
            if state.goal_stats.node != Some(n.id) {
                state.goal_stats.node = Some(n.id);
                state.goal_stats.goals = Goals::from_params(&n.params);
                state.goal_stats.measured = None;
            }
        } else if state.goal_stats.node.is_some_and(|id| !s.nodes.contains_key(&id)) {
            state.goal_stats.node = None;
            state.goal_stats.measured = None;
        }
    }
    // The packed output, measured once per output.
    let out: Option<Arc<DataSet>> = state.goal_stats.node.and_then(|id| state.session.lock().nodes.get(&id).and_then(|n| n.output.clone()));
    if let Some(o) = &out {
        let ptr = Arc::as_ptr(o) as *const () as usize;
        if state.goal_stats.measured.as_ref().is_none_or(|m| m.0 != ptr) {
            state.goal_stats.measured = measure(o).map(|(a, b, c)| (ptr, a, b, c));
        }
    }
    let mut open = true;
    let mut pack = false;
    egui::Window::new("Goal Statistics").open(&mut open).default_width(560.0).show_fit(ctx, |ui| {
        let gs = &mut state.goal_stats;
        let g = &mut gs.goals;
        ui.label(match gs.node {
            Some(_) => RichText::new("Editing the selected Pack Grains source: Pack applies these goals to it.").weak(),
            None => RichText::new("Pack adds a Pack Grains source with these goals.").weak(),
        });
        let (accent, measured_color) = (Color32::from_rgb(40, 120, 230), Color32::from_rgb(230, 120, 40));
        ui.separator();
        ui.strong("Size: equivalent diameter, lognormal");
        ui.horizontal(|ui| {
            ui.label("mu");
            ui.add(egui::DragValue::new(&mut g.mu).speed(0.01));
            ui.label("sigma");
            ui.add(egui::DragValue::new(&mut g.sigma).speed(0.005).range(0.0..=3.0));
            ui.label("cut below / above (sigmas)");
            ui.add(egui::DragValue::new(&mut g.min_cut).speed(0.05).range(0.0..=10.0));
            ui.add(egui::DragValue::new(&mut g.max_cut).speed(0.05).range(0.0..=10.0));
        });
        let (dlo, dhi) = ((g.mu - g.min_cut * g.sigma).exp() * g.spacing, (g.mu + g.max_cut * g.sigma).exp() * g.spacing);
        let (mu_s, sigma) = (g.mu + g.spacing.ln(), g.sigma);
        let m = gs.measured.clone();
        ui.label(format!(
            "Goal mean diameter {:.3}{}",
            (mu_s + 0.5 * sigma * sigma).exp(),
            m.as_ref().map(|m| format!(", packed {:.3} over {} grains", mean(&m.1), m.1.len())).unwrap_or_default()
        ));
        Plot::new("goal_size").height(150.0).legend(Legend::default()).allow_scroll(false).show(ui, |p| {
            if let Some(h) = m.as_ref().and_then(|m| histogram(&m.1, dlo * 0.8, dhi * 1.1, 24, measured_color)) {
                p.bar_chart(h);
            }
            p.line(curve("Goal", dlo * 0.8, dhi * 1.1, |d| if d < dlo || d > dhi { 0.0 } else { lognormal_pdf(d, mu_s, sigma) }, accent));
        });
        ui.separator();
        ui.strong("Shape: aspect ratios b/a and c/a, beta (alpha, beta)");
        ui.horizontal(|ui| {
            ui.label("b/a");
            ui.add(egui::DragValue::new(&mut g.ba[0]).speed(0.1).range(0.1..=200.0));
            ui.add(egui::DragValue::new(&mut g.ba[1]).speed(0.1).range(0.1..=200.0));
            ui.label("c/a");
            ui.add(egui::DragValue::new(&mut g.ca[0]).speed(0.1).range(0.1..=200.0));
            ui.add(egui::DragValue::new(&mut g.ca[1]).speed(0.1).range(0.1..=200.0));
        });
        ui.horizontal(|ui| {
            ui.label("Shape");
            egui::ComboBox::from_id_salt("goal_shape").selected_text(g.shape.clone()).show_ui(ui, |ui| {
                for s in [
                    aurafx_filters::synthetic::SHAPE_ELLIPSOID,
                    aurafx_filters::synthetic::SHAPE_SUPER,
                    aurafx_filters::synthetic::SHAPE_CUBOCT,
                    aurafx_filters::synthetic::SHAPE_CYLINDER,
                ] {
                    ui.selectable_value(&mut g.shape, s.to_string(), s);
                }
            });
            ui.checkbox(&mut g.aligned, "Axes along x, y, z").on_hover_text("The axis distribution: grain axes along the sample axes, else turned at random");
        });
        let (ba, ca) = (g.ba, g.ca);
        ui.label(format!(
            "Goal mean b/a {:.3}, c/a {:.3}{}",
            ba[0] / (ba[0] + ba[1]),
            ca[0] / (ca[0] + ca[1]),
            m.as_ref().map(|m| format!("; packed {:.3}, {:.3}", mean(&m.2), mean(&m.3))).unwrap_or_default()
        ));
        ui.columns(2, |cols| {
            for (k, (name, ab, meas)) in [("b/a", ba, m.as_ref().map(|m| m.2.clone())), ("c/a", ca, m.as_ref().map(|m| m.3.clone()))].into_iter().enumerate() {
                Plot::new(format!("goal_shape_{k}")).height(120.0).allow_scroll(false).show(&mut cols[k], |p| {
                    if let Some(h) = meas.as_ref().and_then(|v| histogram(v, 0.0, 1.0, 20, measured_color)) {
                        p.bar_chart(h);
                    }
                    p.line(curve(name, 0.0, 1.0, |x| beta_pdf(x, ab[0], ab[1]), accent));
                });
            }
        });
        ui.separator();
        ui.strong("Orientations and volume");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("goal_odf").selected_text(g.orientations.clone()).show_ui(ui, |ui| {
                for s in [aurafx_filters::synthetic::ORIENT_RANDOM, aurafx_filters::synthetic::ORIENT_FIBER100, aurafx_filters::synthetic::ORIENT_FIBER111] {
                    ui.selectable_value(&mut g.orientations, s.to_string(), s);
                }
            });
            ui.label("spread (degrees)");
            ui.add(egui::DragValue::new(&mut g.spread).speed(0.2).range(0.0..=90.0));
        });
        ui.horizontal(|ui| {
            ui.label("Cells");
            for c in g.cells.iter_mut() {
                ui.add(egui::DragValue::new(c).speed(1.0).range(4.0..=1024.0));
            }
            ui.label("cell size");
            ui.add(egui::DragValue::new(&mut g.spacing).speed(0.01).range(1e-6..=1e6));
        });
        ui.horizontal(|ui| {
            ui.label("Grain volume fraction");
            ui.add(egui::DragValue::new(&mut g.fraction).speed(0.01).range(0.01..=1.0));
            ui.label("seed");
            ui.add(egui::DragValue::new(&mut g.seed).range(0..=i64::MAX));
            ui.checkbox(&mut g.calibrate, "Match the goals").on_hover_text("Repack with corrected goals until the measured sizes and shapes match");
        });
        ui.separator();
        pack = ui.button(RichText::new("Pack").strong()).on_hover_text("Pack grains with these goals").clicked();
    });
    if pack {
        let values = state.goal_stats.goals.values();
        let mut s = state.session.lock();
        let id = match state.goal_stats.node.filter(|id| s.nodes.contains_key(id)) {
            Some(id) => Some(id),
            None => s.add_source("pack_grains").ok(),
        };
        if let Some(id) = id {
            for (k, v) in values {
                s.set_param(id, k, v);
            }
            s.active = Some(id);
            drop(s);
            state.goal_stats.node = Some(id);
            state.goal_stats.measured = None;
        }
    }
    if !open {
        state.goal_stats.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn densities_integrate_to_one() {
        let n = 20_000;
        let beta: f64 = (0..n).map(|k| beta_pdf((k as f64 + 0.5) / n as f64, 15.0, 2.0) / n as f64).sum();
        assert!((beta - 1.0).abs() < 1e-6, "{beta}");
        let w = 200.0 / n as f64;
        let ln: f64 = (0..n).map(|k| lognormal_pdf((k as f64 + 0.5) * w, 2.3, 0.2) * w).sum();
        assert!((ln - 1.0).abs() < 1e-6, "{ln}");
        assert!((ln_gamma(5.0) - 24f64.ln()).abs() < 1e-12);
    }
}
