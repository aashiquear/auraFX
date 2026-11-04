//! Color map editor: edit the active object's color map over a live histogram of the colored
//! values, reverse it, split it into bands, save it as a preset, import/export ParaView `.json`.

use crate::app::State;
use aurafx_filters::Association;
use aurafx_pipeline::colormap::{self, ColorMap};
use aurafx_pipeline::{DisplayProps, NodeId};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

#[derive(Default)]
pub struct ColormapEditor {
    pub open: bool,
    selected: Option<usize>,
    dragging: Option<usize>,
    preset_name: String,
    log_hist: bool,
    /// (cache key, bin counts)
    hist: Option<(String, Vec<u32>)>,
}

const BINS: usize = 64;

/// Values of the colored array (all blocks), as f32.
fn colored_values(state: &State, id: NodeId, d: &DisplayProps) -> Option<(String, Vec<f32>)> {
    let cb = d.color_by.as_ref()?;
    let s = state.session.lock();
    let out = s.nodes.get(&id)?.output.clone()?;
    let ptr = std::sync::Arc::as_ptr(&out) as *const () as usize;
    drop(s);
    let mut vals = Vec::new();
    for (leaf, _, _) in aurafx_filters::selection::flat_leaves(&out) {
        let attrs = if cb.assoc == Association::Points { leaf.point_data() } else { leaf.cell_data() };
        if let Some(a) = attrs.and_then(|a| a.get(&cb.name)) {
            vals.extend(a.component_f32(cb.component.filter(|&c| c < a.ncomp)));
        }
    }
    Some((format!("{id}:{ptr}:{}:{:?}:{:?}", cb.name, cb.assoc, cb.component), vals))
}

fn to_color32(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

impl State {
    /// Set the active object's color map to preset `name` (user presets are copied in).
    pub fn apply_colormap_preset(&mut self, id: NodeId, name: &str) {
        let user = colormap::user().into_iter().find(|m| m.name.eq_ignore_ascii_case(name)).map(|m| m.stops);
        let name = name.to_string();
        self.session.lock().set_display(id, |d| {
            d.colormap = name;
            d.colormap_stops = user;
        });
    }

    /// Store a user preset in the settings and make it available everywhere.
    pub fn save_user_colormap(&mut self, map: ColorMap) {
        self.settings.user_colormaps.retain(|m| !m.name.eq_ignore_ascii_case(&map.name));
        self.settings.user_colormaps.push(map.clone());
        self.settings.save();
        colormap::register_user(map);
    }
}

pub fn editor_ui(state: &mut State, ctx: &egui::Context) {
    if !state.cmap_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Color Map Editor").open(&mut open).resizable(false).default_width(440.0).show(ctx, |ui| body(state, ui));
    if !open {
        state.cmap_editor.open = false;
    }
}

fn body(state: &mut State, ui: &mut egui::Ui) {
    let target = {
        let s = state.session.lock();
        s.active_node().map(|n| (n.id, n.name.clone(), n.display.clone()))
    };
    let Some((id, name, display)) = target else {
        ui.label(RichText::new("Select an object in the Pipeline to edit its color map.").weak());
        return;
    };
    let Some(cb) = display.color_by.clone() else {
        ui.label(RichText::new(format!("{name} has a solid color. Pick an array under \"Color by\" first.")).weak());
        return;
    };
    let mut d = display.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).strong());
        ui.label(RichText::new(format!("colored by {}{}", cb.name, cb.component.map(|c| format!(" [{c}]")).unwrap_or_default())).weak());
    });

    // Preset.
    ui.horizontal(|ui| {
        ui.label("Preset");
        let mut preset = d.colormap.clone();
        if crate::widgets::colormap_combo(ui, "cmap_editor_preset", &mut preset, 180.0) {
            state.apply_colormap_preset(id, &preset);
            state.cmap_editor.selected = None;
            if let Some(nd) = state.session.lock().nodes.get(&id).map(|n| n.display.clone()) {
                d = nd;
            }
        }
        if d.colormap_stops.is_some() {
            ui.label(RichText::new("edited").italics().color(crate::theme::ACCENT));
        }
    });

    // Data values and range.
    let values = colored_values(state, id, &d);
    let data_range = values.as_ref().map(|(_, v)| {
        let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x as f64), b.max(x as f64)));
        if lo <= hi {
            (lo, hi)
        } else {
            (0.0, 1.0)
        }
    });
    let (lo, hi) = d.range.or(data_range).unwrap_or((0.0, 1.0));
    let span = if hi > lo { hi - lo } else { 1.0 };

    // Histogram (cached per data/array/range).
    if let Some((key, vals)) = &values {
        let key = format!("{key}:{lo}:{hi}");
        if state.cmap_editor.hist.as_ref().map(|(k, _)| *k != key).unwrap_or(true) {
            let mut bins = vec![0u32; BINS];
            for &v in vals.iter().filter(|v| v.is_finite()) {
                let f = (v as f64 - lo) / span;
                if (0.0..=1.0).contains(&f) {
                    bins[((f * BINS as f64) as usize).min(BINS - 1)] += 1;
                }
            }
            state.cmap_editor.hist = Some((key, bins));
        }
    }

    // Stops being edited: the object's own, or the preset's.
    let base_stops = d.colormap_stops.clone().unwrap_or_else(|| colormap::get(&d.colormap).stops);
    let mut stops = base_stops.clone();
    let rev = d.colormap_reverse;
    let shown_t = |t: f32| if rev { 1.0 - t } else { t };

    // Histogram + gradient + handles.
    let width = ui.available_width().clamp(320.0, 520.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 120.0), Sense::click_and_drag());
    let hist_rect = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 70.0));
    let grad_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 72.0), Pos2::new(rect.right(), rect.top() + 100.0));
    let handle_y = grad_rect.bottom() + 9.0;
    let p = ui.painter_at(rect.expand(2.0));
    p.rect_filled(hist_rect, 3.0, ui.visuals().extreme_bg_color);
    if let Some((_, bins)) = &state.cmap_editor.hist {
        let f = |c: u32| if state.cmap_editor.log_hist { (c as f32 + 1.0).ln() } else { c as f32 };
        let max = bins.iter().map(|&c| f(c)).fold(0.0, f32::max).max(1e-6);
        let eff = d.effective_colormap();
        let bw = hist_rect.width() / BINS as f32;
        for (i, &c) in bins.iter().enumerate() {
            let h = f(c) / max * (hist_rect.height() - 4.0);
            let x = hist_rect.left() + i as f32 * bw;
            let col = to_color32(eff.sample((i as f32 + 0.5) / BINS as f32)).gamma_multiply(0.85);
            p.rect_filled(Rect::from_min_max(Pos2::new(x + 0.5, hist_rect.bottom() - h), Pos2::new(x + bw - 0.5, hist_rect.bottom())), 0.0, col);
        }
    }
    crate::widgets::paint_map(&p, grad_rect, &d.effective_colormap());
    p.rect_stroke(grad_rect, 2.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Outside);
    let x_of = |t: f32| grad_rect.left() + shown_t(t) * grad_rect.width();
    let t_of = |x: f32| {
        let u = ((x - grad_rect.left()) / grad_rect.width()).clamp(0.0, 1.0);
        if rev {
            1.0 - u
        } else {
            u
        }
    };
    for (i, (t, c)) in stops.iter().enumerate() {
        let x = x_of(*t);
        let sel = state.cmap_editor.selected == Some(i);
        let tri = vec![Pos2::new(x, grad_rect.bottom() + 1.0), Pos2::new(x - 6.0, handle_y + 5.0), Pos2::new(x + 6.0, handle_y + 5.0)];
        p.add(egui::Shape::convex_polygon(
            tri,
            to_color32(*c),
            Stroke::new(if sel { 2.5 } else { 1.0 }, if sel { crate::theme::ACCENT } else { ui.visuals().text_color() }),
        ));
    }
    let mut changed = false;
    let pointer = resp.interact_pointer_pos();
    let nearest = |pos: Pos2, stops: &[(f32, [f32; 3])]| {
        stops.iter().enumerate().map(|(i, (t, _))| (i, (x_of(*t) - pos.x).abs())).filter(|(_, dx)| *dx < 8.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    };
    if resp.drag_started() {
        if let Some(pos) = pointer {
            state.cmap_editor.dragging = nearest(pos, &stops);
            state.cmap_editor.selected = state.cmap_editor.dragging.or(state.cmap_editor.selected);
        }
    }
    if let (Some(i), Some(pos)) = (state.cmap_editor.dragging, pointer) {
        if resp.dragged() && i < stops.len() {
            stops[i].0 = t_of(pos.x);
            changed = true;
        }
    }
    if resp.drag_stopped() {
        state.cmap_editor.dragging = None;
    }
    if resp.clicked() {
        if let Some(pos) = pointer {
            match nearest(pos, &stops) {
                Some(i) => state.cmap_editor.selected = Some(i),
                None => {
                    // Click on the gradient: a new stop with the color already there.
                    let t = t_of(pos.x);
                    let c = ColorMap::new("", stops.clone()).sample(t);
                    stops.push((t, c));
                    state.cmap_editor.selected = Some(stops.len() - 1);
                    changed = true;
                }
            }
        }
    }
    if resp.double_clicked() {
        if let Some(i) = pointer.and_then(|pos| nearest(pos, &stops)) {
            if stops.len() > 2 {
                stops.remove(i);
                state.cmap_editor.selected = None;
                changed = true;
            }
        }
    }
    resp.on_hover_text("Drag a marker to move it, click the gradient to add one, double-click a marker to remove it");
    ui.horizontal(|ui| {
        ui.label(RichText::new(aurafx_data::info::g(if rev { hi } else { lo })).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(aurafx_data::info::g(if rev { lo } else { hi })).small());
        });
    });

    // Selected stop: value and color.
    if let Some(i) = state.cmap_editor.selected.filter(|&i| i < stops.len()) {
        ui.horizontal(|ui| {
            ui.label("Marker at");
            let mut v = lo + stops[i].0 as f64 * span;
            if ui.add(egui::DragValue::new(&mut v).speed(span / 200.0).max_decimals(6)).changed() {
                stops[i].0 = (((v - lo) / span) as f32).clamp(0.0, 1.0);
                changed = true;
            }
            let mut c = stops[i].1;
            if ui.color_edit_button_rgb(&mut c).changed() {
                stops[i].1 = c;
                changed = true;
            }
            if ui.add_enabled(stops.len() > 2, egui::Button::new(crate::icons::TRASH)).on_hover_text("Remove this marker").clicked() {
                stops.remove(i);
                state.cmap_editor.selected = None;
                changed = true;
            }
        });
    } else {
        ui.label(RichText::new("Click a marker under the gradient to change its value or color.").weak().small());
    }

    // Keep the selection on the same stop after sorting.
    if changed {
        let sel = state.cmap_editor.selected.and_then(|i| stops.get(i).copied());
        let drag = state.cmap_editor.dragging.and_then(|i| stops.get(i).copied());
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        state.cmap_editor.selected = sel.and_then(|s| stops.iter().position(|x| *x == s));
        state.cmap_editor.dragging = drag.and_then(|s| stops.iter().position(|x| *x == s));
        d.colormap_stops = Some(stops.clone());
    }

    ui.separator();
    egui::Grid::new("cmap_opts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("Range");
        ui.horizontal(|ui| {
            let mut r = d.range.unwrap_or((lo, hi));
            let a = ui.add(egui::DragValue::new(&mut r.0).speed(span / 200.0).max_decimals(6));
            let b = ui.add(egui::DragValue::new(&mut r.1).speed(span / 200.0).max_decimals(6));
            if a.changed() || b.changed() {
                d.range = Some(r);
            }
            if ui.small_button("Fit to data").on_hover_text("Use the data's minimum and maximum").clicked() {
                d.range = None;
            }
        });
        ui.end_row();
        ui.label("Options");
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.colormap_reverse, "Reverse");
            ui.checkbox(&mut d.log_scale, "Log scale");
            ui.checkbox(&mut state.cmap_editor.log_hist, "Log histogram");
        });
        ui.end_row();
        ui.label("Bands");
        ui.horizontal(|ui| {
            let mut on = d.colormap_bands >= 2;
            if ui.checkbox(&mut on, "").on_hover_text("Split the colors into flat bands").changed() {
                d.colormap_bands = if on { 8 } else { 0 };
            }
            if on {
                ui.add(egui::Slider::new(&mut d.colormap_bands, 2..=32));
            }
        });
        ui.end_row();
    });

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(d.colormap_stops.is_some(), egui::Button::new("Reset to preset")).clicked() {
            d.colormap_stops = None;
            state.cmap_editor.selected = None;
        }
        if ui.button(format!("{} Export…", crate::icons::FLOPPY_DISK)).on_hover_text("Save as a ParaView color map preset (.json)").clicked() {
            let mut m = d.effective_colormap();
            m.name = format!("{} ({})", d.colormap, cb.name);
            if let Some(path) = crate::dialogs::FileDialog::new().add_filter("ParaView color map", &["json"]).set_file_name("colormap.json").save_file() {
                match std::fs::write(&path, colormap::to_paraview_json(&[m])) {
                    Ok(()) => state.toast(format!("Saved {}", path.display()), false),
                    Err(e) => state.toast(format!("Could not save: {e}"), true),
                }
            }
        }
        if ui.button(format!("{} Import…", crate::icons::FOLDER_OPEN)).on_hover_text("Load ParaView color map presets (.json)").clicked() {
            if let Some(path) = crate::dialogs::FileDialog::new().add_filter("ParaView color map", &["json"]).pick_file() {
                match std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|t| colormap::from_paraview_json(&t)) {
                    Ok(maps) => {
                        let first = maps[0].name.clone();
                        let n = maps.len();
                        for m in maps {
                            state.save_user_colormap(m);
                        }
                        state.apply_colormap_preset(id, &first);
                        state.toast(format!("Imported {n} color map(s)"), false);
                        return;
                    }
                    Err(e) => state.toast(format!("{e:#}"), true),
                }
            }
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.cmap_editor.preset_name).hint_text("preset name").desired_width(150.0));
        let name_ok = !state.cmap_editor.preset_name.trim().is_empty();
        if ui
            .add_enabled(name_ok, egui::Button::new(format!("{} Save as preset", crate::icons::PLUS)))
            .on_hover_text("Keep this color map for other objects and future sessions")
            .clicked()
        {
            let name = state.cmap_editor.preset_name.trim().to_string();
            let mut m = d.effective_colormap();
            m.name = name.clone();
            state.save_user_colormap(m);
            d.colormap = name.clone();
            d.colormap_stops = Some(colormap::get(&name).stops);
            d.colormap_reverse = false;
            d.colormap_bands = 0;
            state.cmap_editor.preset_name.clear();
            state.toast(format!("Saved preset {name}"), false);
        }
    });
    let users = colormap::user();
    if !users.is_empty() {
        ui.collapsing(format!("My presets ({})", users.len()), |ui| {
            for m in users {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(80.0, 12.0), Sense::hover());
                    crate::widgets::paint_map(ui.painter(), r, &m);
                    if ui.small_button(&m.name).on_hover_text("Use on this object").clicked() {
                        state.apply_colormap_preset(id, &m.name);
                    }
                    if ui.small_button(crate::icons::TRASH).on_hover_text("Delete this preset").clicked() {
                        state.settings.user_colormaps.retain(|x| x.name != m.name);
                        state.settings.save();
                        colormap::remove_user(&m.name);
                    }
                });
            }
        });
    }

    // Apply edits (re-read: preset/import buttons may have changed the display meanwhile).
    let now = state.session.lock().nodes.get(&id).map(|n| n.display.clone());
    if now.as_ref() == Some(&display) && d != display {
        state.session.lock().set_display(id, |x| *x = d);
    }
}
