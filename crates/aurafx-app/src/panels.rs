//! Pipeline browser, properties, information, log and spreadsheet panels.

use crate::app::State;
use aurafx_filters::{Association, ParamKind, ParamSpec, ParamValue};
use aurafx_pipeline::{ColorBy, LogLevel, NodeId, Representation};
use egui::RichText;

// ------------------------------------------------------------------ pipeline

pub fn pipeline_ui(state: &mut State, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button(crate::icons::FOLDER_OPEN).on_hover_text("Open file").clicked() {
            state.open_file_dialog();
        }
        if ui.button(crate::icons::PLUS).on_hover_text("Add source or filter (Ctrl+K)").clicked() {
            state.palette.open = true;
        }
        let active = state.session.lock().active;
        if ui.add_enabled(active.is_some(), egui::Button::new(crate::icons::TRASH)).on_hover_text("Delete active node and its children (Delete)").clicked() {
            if let Some(a) = active {
                state.session.lock().remove_node(a);
            }
        }
        if ui.add_enabled(active.is_some(), egui::Button::new(crate::icons::PENCIL_SIMPLE)).on_hover_text("Rename").clicked() {
            if let Some(a) = active {
                let name = state.session.lock().nodes.get(&a).map(|n| n.name.clone()).unwrap_or_default();
                state.rename_target = Some((a, name));
            }
        }
    });
    ui.separator();
    // Snapshot of the tree to avoid holding the lock while drawing.
    struct Row {
        id: NodeId,
        name: String,
        kind: String,
        depth: usize,
        visible: bool,
        active: bool,
        running: bool,
        dirty: bool,
        error: Option<String>,
        summary: String,
    }
    let rows: Vec<Row> = {
        let s = state.session.lock();
        let depth_of = |mut id: NodeId| {
            let mut d = 0;
            while let Some(inp) = s.nodes.get(&id).and_then(|n| n.input) {
                d += 1;
                id = inp;
                if d > 64 {
                    break;
                }
            }
            d
        };
        // Depth-first order: children directly under their parents.
        let mut order: Vec<NodeId> = Vec::new();
        fn visit(s: &aurafx_pipeline::Session, parent: Option<NodeId>, order: &mut Vec<NodeId>) {
            for n in s.nodes.values() {
                if n.input == parent {
                    order.push(n.id);
                    visit(s, Some(n.id), order);
                }
            }
        }
        visit(&s, None, &mut order);
        order
            .into_iter()
            .map(|id| {
                let n = &s.nodes[&id];
                Row {
                    id,
                    name: n.name.clone(),
                    kind: n.type_label(),
                    depth: depth_of(id),
                    visible: n.visible,
                    active: s.active == Some(id),
                    running: n.running,
                    dirty: n.dirty,
                    error: n.error.clone(),
                    summary: n.info.as_ref().map(|i| format!("{}: {} points, {} cells", i.kind, i.n_points, i.n_cells)).unwrap_or_default(),
                }
            })
            .collect()
    };
    if rows.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("Nothing loaded yet.").weak());
        ui.label(RichText::new("Open a file, drop one onto the window, or add a source from the Sources menu.").weak().small());
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for r in rows {
            ui.horizontal(|ui| {
                ui.add_space(r.depth as f32 * 14.0);
                let eye = if r.visible { crate::icons::EYE } else { crate::icons::EYE_SLASH };
                if ui.add(egui::Button::new(eye).frame(false)).on_hover_text("Toggle visibility").clicked() {
                    state.session.lock().set_visible(r.id, !r.visible);
                }
                let mut text = RichText::new(&r.name);
                if r.error.is_some() {
                    text = text.color(crate::theme::ERROR);
                } else if !r.visible {
                    text = text.weak();
                }
                let resp = ui.selectable_label(r.active, text);
                let hover = if let Some(e) = &r.error { format!("{}\nERROR: {e}", r.kind) } else { format!("{}\n{}", r.kind, r.summary) };
                let resp = resp.on_hover_text(hover);
                if resp.clicked() {
                    state.session.lock().active = Some(r.id);
                }
                if resp.double_clicked() {
                    state.rename_target = Some((r.id, r.name.clone()));
                }
                resp.context_menu(|ui| {
                    ui.label(RichText::new(&r.kind).weak());
                    if ui.button("Rename").clicked() {
                        state.rename_target = Some((r.id, r.name.clone()));
                        ui.close();
                    }
                    if ui.button(if r.visible { "Hide" } else { "Show" }).clicked() {
                        state.session.lock().set_visible(r.id, !r.visible);
                        ui.close();
                    }
                    if ui.button("Show only this").clicked() {
                        let mut s = state.session.lock();
                        let ids: Vec<NodeId> = s.nodes.keys().copied().collect();
                        for id in ids {
                            s.set_visible(id, id == r.id);
                        }
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Add filter", |ui| {
                        for f in aurafx_filters::registry().iter().filter(|f| !f.is_source) {
                            if ui.button(f.label).clicked() {
                                state.session.lock().active = Some(r.id);
                                state.add_filter(f.id);
                                ui.close();
                            }
                        }
                    });
                    if ui.button("Re-run").clicked() {
                        state.session.lock().mark_dirty(r.id);
                        ui.close();
                    }
                    if ui.button("Save data…").clicked() {
                        state.session.lock().active = Some(r.id);
                        state.save_data_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(RichText::new("Delete").color(crate::theme::ERROR)).clicked() {
                        state.session.lock().remove_node(r.id);
                        ui.close();
                    }
                });
                if r.running {
                    ui.spinner();
                } else if r.dirty {
                    ui.label(RichText::new("…").weak());
                } else if r.error.is_some() {
                    ui.label(RichText::new(crate::icons::WARNING).color(crate::theme::ERROR));
                }
            });
        }
    });
}

// ------------------------------------------------------------------ properties

/// Fill an "auto" (NaN) parameter from the data when the user wants to edit it explicitly.
fn auto_fill(state: &State, id: NodeId, spec: &ParamSpec) -> ParamValue {
    let s = state.session.lock();
    let node = &s.nodes[&id];
    let input = node.input.and_then(|i| s.nodes.get(&i)).or(Some(node));
    let b = input.and_then(|n| n.output.as_ref()).map(|d| d.bounds()).unwrap_or_default();
    let center = if b.is_empty() { [0.0; 3] } else { b.center() };
    let key = spec.key.as_str();
    match &spec.kind {
        ParamKind::Vec3 => {
            if key.contains("min") {
                ParamValue::Vec3(if b.is_empty() { [0.0; 3] } else { b.min })
            } else if key.contains("max") {
                ParamValue::Vec3(if b.is_empty() { [1.0; 3] } else { b.max })
            } else {
                ParamValue::Vec3(center)
            }
        }
        ParamKind::Float { .. } => {
            let range =
                input.and_then(|n| n.info.as_ref()).and_then(|i| i.point_arrays.first().or(i.cell_arrays.first())).map(|a| a.range).unwrap_or((0.0, 1.0));
            ParamValue::Float(if key == "lower" {
                range.0
            } else if key == "upper" {
                range.1
            } else if key.contains("scale") {
                1.0
            } else {
                0.0
            })
        }
        _ => spec.default.clone(),
    }
}

fn param_widget(
    state: &mut State,
    ui: &mut egui::Ui,
    id: NodeId,
    spec: &ParamSpec,
    value: &ParamValue,
    arrays: &[(Association, String, usize)],
) -> Option<ParamValue> {
    let mut out = None;
    match &spec.kind {
        ParamKind::Bool => {
            let mut b = value.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                out = Some(ParamValue::Bool(b));
            }
        }
        ParamKind::Int { min, max } => {
            let mut v = value.as_i64().unwrap_or(0);
            let r = ui.add(egui::DragValue::new(&mut v).range(*min..=*max));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                out = Some(ParamValue::Int(v));
            }
        }
        ParamKind::Float { min, max, .. } => {
            let mut v = value.as_f64().unwrap_or(0.0);
            if v.is_nan() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("auto").weak());
                    if ui.small_button("edit").on_hover_text("Replace the automatic value with an explicit one").clicked() {
                        out = Some(auto_fill(state, id, spec));
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    let speed = (v.abs() * 0.01).max(0.001);
                    let r = ui.add(egui::DragValue::new(&mut v).range(*min..=*max).speed(speed).max_decimals(6));
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        out = Some(ParamValue::Float(v));
                    }
                    if spec.default.as_f64().map(|d| d.is_nan()).unwrap_or(false) && ui.small_button("auto").clicked() {
                        out = Some(ParamValue::Float(f64::NAN));
                    }
                });
            }
        }
        ParamKind::Vec3 => {
            let mut v = value.as_vec3().unwrap_or([0.0; 3]);
            if v.iter().any(|x| x.is_nan()) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("auto (data center)").weak());
                    if ui.small_button("edit").clicked() {
                        out = Some(auto_fill(state, id, spec));
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    let mut changed = false;
                    for c in &mut v {
                        let speed = (c.abs() * 0.01).max(0.001);
                        let r = ui.add(egui::DragValue::new(c).speed(speed).max_decimals(5));
                        changed |= r.drag_stopped() || (r.changed() && !r.dragged());
                    }
                    if changed {
                        out = Some(ParamValue::Vec3(v));
                    }
                    if spec.key == "normal" {
                        for (l, n) in [("X", [1.0, 0.0, 0.0]), ("Y", [0.0, 1.0, 0.0]), ("Z", [0.0, 0.0, 1.0])] {
                            if ui.small_button(l).clicked() {
                                out = Some(ParamValue::Vec3(n));
                            }
                        }
                    }
                    if spec.default.as_vec3().map(|d| d[0].is_nan()).unwrap_or(false) && ui.small_button("auto").clicked() {
                        out = Some(ParamValue::Vec3([f64::NAN; 3]));
                    }
                });
            }
        }
        ParamKind::Str => {
            let mut s = value.as_str().unwrap_or("").to_string();
            let r = ui.text_edit_singleline(&mut s);
            if r.lost_focus() {
                out = Some(ParamValue::Str(s));
            }
        }
        ParamKind::Expression => {
            let mut s = value.as_str().unwrap_or("").to_string();
            let r = ui.add(egui::TextEdit::multiline(&mut s).code_editor().desired_rows(2).desired_width(f32::INFINITY));
            let apply = r.lost_focus() || (r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)));
            if apply {
                out = Some(ParamValue::Str(s));
            }
        }
        ParamKind::Choice(opts) => {
            let mut cur = value.as_str().unwrap_or("").to_string();
            let before = cur.clone();
            egui::ComboBox::from_id_salt(&spec.key).selected_text(&cur).show_ui(ui, |ui| {
                for o in opts {
                    ui.selectable_value(&mut cur, o.clone(), o);
                }
            });
            if cur != before {
                out = Some(ParamValue::Str(cur));
            }
        }
        ParamKind::Array { scalar_only, vector_only } => {
            let (cur_assoc, cur_name) = value.as_array().map(|(a, n)| (a, n.to_string())).unwrap_or((Association::Points, String::new()));
            let label = if cur_name.is_empty() { "(automatic)".to_string() } else { format!("{} {}", crate::viewport::assoc_glyph(cur_assoc), cur_name) };
            egui::ComboBox::from_id_salt(&spec.key).selected_text(label).width(200.0).show_ui(ui, |ui| {
                if ui.selectable_label(cur_name.is_empty(), "(automatic)").clicked() {
                    out = Some(ParamValue::Array { assoc: Association::Points, name: String::new() });
                }
                for (a, n, nc) in arrays {
                    if (*scalar_only && *nc != 1) || (*vector_only && *nc < 3) {
                        continue;
                    }
                    let sel = *a == cur_assoc && *n == cur_name;
                    if ui
                        .selectable_label(
                            sel,
                            format!("{} {}{}", crate::viewport::assoc_glyph(*a), n, if *nc > 1 { format!(" ({nc})") } else { String::new() }),
                        )
                        .clicked()
                    {
                        out = Some(ParamValue::Array { assoc: *a, name: n.clone() });
                    }
                }
            });
        }
        ParamKind::Floats => {
            let cur = value.as_floats().unwrap_or_default();
            let id_salt = ui.scope_id().with(&spec.key);
            let mut text: String =
                ui.data_mut(|d| d.get_temp::<String>(id_salt)).unwrap_or_else(|| cur.iter().map(|v| aurafx_data::info::g(*v)).collect::<Vec<_>>().join(", "));
            let r = ui.text_edit_singleline(&mut text);
            ui.data_mut(|d| d.insert_temp(id_salt, text.clone()));
            if r.lost_focus() {
                let vals: Vec<f64> = text.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
                out = Some(ParamValue::Floats(vals));
                ui.data_mut(|d| d.remove::<String>(id_salt));
            }
        }
    }
    out
}

pub fn properties_ui(state: &mut State, ui: &mut egui::Ui) {
    let snapshot = {
        let s = state.session.lock();
        s.active_node().map(|n| {
            let input_info = n.input.and_then(|i| s.nodes.get(&i)).and_then(|i| i.info.clone());
            (
                n.id,
                n.name.clone(),
                n.type_label(),
                n.def().cloned(),
                n.params.clone(),
                n.display.clone(),
                n.info.clone(),
                input_info,
                n.error.clone(),
                n.last_exec_ms,
            )
        })
    };
    let Some((id, name, kind, def, params, display, info, input_info, error, ms)) = snapshot else {
        ui.label(RichText::new("Select a node in the pipeline to edit its properties.").weak());
        return;
    };
    ui.horizontal(|ui| {
        ui.heading(&name);
        ui.label(RichText::new(&kind).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ms > 0.0 {
                ui.label(RichText::new(format!("{ms:.0} ms")).weak().small());
            }
        });
    });
    if let Some(e) = error {
        ui.colored_label(crate::theme::ERROR, e);
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Arrays available to parameter widgets: from the input for filters, own output for sources.
        let src_info = if def.as_ref().map(|d| d.is_source).unwrap_or(true) { info.as_ref() } else { input_info.as_ref() };
        let mut arrays: Vec<(Association, String, usize)> = Vec::new();
        if let Some(i) = src_info {
            arrays.extend(i.point_arrays.iter().map(|a| (Association::Points, a.name.clone(), a.ncomp)));
            arrays.extend(i.cell_arrays.iter().map(|a| (Association::Cells, a.name.clone(), a.ncomp)));
        }
        if let Some(def) = &def {
            egui::CollapsingHeader::new(RichText::new("Parameters").strong())
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(RichText::new(def.description).weak().small());
                    let show_adv = ui.scope_id().with("adv");
                    let mut adv: bool = ui.data_mut(|d| d.get_temp(show_adv)).unwrap_or(false);
                    egui::Grid::new("params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        for spec in &def.params {
                            if spec.advanced && !adv {
                                continue;
                            }
                            let label = ui.label(&spec.label);
                            if !spec.help.is_empty() {
                                label.on_hover_text(&spec.help);
                            }
                            let value = params.get(&spec.key).cloned().unwrap_or(spec.default.clone());
                            if let Some(nv) = param_widget(state, ui, id, spec, &value, &arrays) {
                                state.session.lock().set_param(id, &spec.key, nv);
                            }
                            ui.end_row();
                        }
                    });
                    if def.params.iter().any(|p| p.advanced) {
                        if ui.checkbox(&mut adv, "Show advanced").changed() {
                            ui.data_mut(|d| d.insert_temp(show_adv, adv));
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Reset to defaults").clicked() {
                            let mut s = state.session.lock();
                            if let Some(n) = s.nodes.get_mut(&id) {
                                n.params = def.default_params();
                            }
                            s.mark_dirty(id);
                        }
                        if ui.button("Re-run").clicked() {
                            state.session.lock().mark_dirty(id);
                        }
                    });
                })
                .header_response
                .on_hover_text("Changes apply immediately.");
        }

        egui::CollapsingHeader::new(RichText::new("Display").strong()).default_open(true).show(ui, |ui| {
            let mut d = display.clone();
            egui::Grid::new("display").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Representation");
                egui::ComboBox::from_id_salt("repr").selected_text(d.representation.label()).show_ui(ui, |ui| {
                    for r in Representation::ALL {
                        ui.selectable_value(&mut d.representation, r, r.label());
                    }
                });
                ui.end_row();

                ui.label("Color by");
                let own: Vec<(Association, String, usize)> = info
                    .as_ref()
                    .map(|i| {
                        i.point_arrays
                            .iter()
                            .map(|a| (Association::Points, a.name.clone(), a.ncomp))
                            .chain(i.cell_arrays.iter().map(|a| (Association::Cells, a.name.clone(), a.ncomp)))
                            .collect()
                    })
                    .unwrap_or_default();
                let cur = d.color_by.as_ref().map(|c| format!("{} {}", crate::viewport::assoc_glyph(c.assoc), c.name)).unwrap_or_else(|| "Solid color".into());
                egui::ComboBox::from_id_salt("colorby").selected_text(cur).width(200.0).show_ui(ui, |ui| {
                    if ui.selectable_label(d.color_by.is_none(), "Solid color").clicked() {
                        d.color_by = None;
                        d.range = None;
                    }
                    for (a, n, nc) in &own {
                        let sel = d.color_by.as_ref().map(|c| c.assoc == *a && c.name == *n).unwrap_or(false);
                        if ui
                            .selectable_label(
                                sel,
                                format!("{} {}{}", crate::viewport::assoc_glyph(*a), n, if *nc > 1 { format!(" ({nc})") } else { String::new() }),
                            )
                            .clicked()
                        {
                            d.color_by = Some(ColorBy { assoc: *a, name: n.clone(), component: None });
                            d.range = None;
                        }
                    }
                });
                ui.end_row();

                if let Some(cb) = d.color_by.clone() {
                    let cb = &cb;
                    let ncomp = own.iter().find(|(a, n, _)| *a == cb.assoc && *n == cb.name).map(|x| x.2).unwrap_or(1);
                    if ncomp > 1 {
                        ui.label("Component");
                        let mut comp = cb.component;
                        egui::ComboBox::from_id_salt("comp").selected_text(comp.map(|c| c.to_string()).unwrap_or_else(|| "Magnitude".into())).show_ui(
                            ui,
                            |ui| {
                                ui.selectable_value(&mut comp, None, "Magnitude");
                                for c in 0..ncomp {
                                    ui.selectable_value(&mut comp, Some(c), ["X", "Y", "Z"].get(c).map(|s| s.to_string()).unwrap_or_else(|| c.to_string()));
                                }
                            },
                        );
                        if comp != cb.component {
                            d.color_by.as_mut().unwrap().component = comp;
                            d.range = None;
                        }
                        ui.end_row();
                    }
                    ui.label("Color map");
                    egui::ComboBox::from_id_salt("cmap").selected_text(&d.colormap).show_ui(ui, |ui| {
                        for n in aurafx_pipeline::colormap::names() {
                            ui.selectable_value(&mut d.colormap, n.to_string(), n);
                        }
                    });
                    ui.end_row();
                    ui.label("Range");
                    ui.horizontal(|ui| {
                        let data_range = info
                            .as_ref()
                            .and_then(|i| {
                                let arrs = if cb.assoc == Association::Points { &i.point_arrays } else { &i.cell_arrays };
                                arrs.iter().find(|a| a.name == cb.name).map(|a| a.range)
                            })
                            .unwrap_or((0.0, 1.0));
                        let mut r = d.range.unwrap_or(data_range);
                        let mut changed = false;
                        let (s0, s1) = ((r.0.abs() * 0.01).max(0.001), (r.1.abs() * 0.01).max(0.001));
                        let r0 = ui.add(egui::DragValue::new(&mut r.0).speed(s0).max_decimals(6));
                        let r1 = ui.add(egui::DragValue::new(&mut r.1).speed(s1).max_decimals(6));
                        changed |= r0.drag_stopped() || (r0.changed() && !r0.dragged()) || r1.drag_stopped() || (r1.changed() && !r1.dragged());
                        if changed {
                            d.range = Some(r);
                        }
                        if d.range.is_some() && ui.small_button("auto").clicked() {
                            d.range = None;
                        }
                        ui.checkbox(&mut d.log_scale, "log");
                        ui.checkbox(&mut d.show_scalar_bar, "bar");
                    });
                    ui.end_row();
                } else {
                    ui.label("Solid color");
                    ui.color_edit_button_rgb(&mut d.solid_color);
                    ui.end_row();
                }

                if d.representation == Representation::Volume {
                    ui.label("Volume density");
                    ui.add(egui::Slider::new(&mut d.volume_opacity, 0.05..=10.0).logarithmic(true)).on_hover_text("Higher values make the volume more opaque");
                    ui.end_row();
                    ui.label("Transparent below");
                    ui.add(egui::Slider::new(&mut d.volume_threshold, 0.0..=0.99))
                        .on_hover_text("Fraction of the color range below which the volume is invisible");
                    ui.end_row();
                } else {
                    ui.label("Opacity");
                    ui.add(egui::Slider::new(&mut d.opacity, 0.0..=1.0));
                    ui.end_row();
                }
                if matches!(d.representation, Representation::Points) || info.as_ref().map(|i| i.n_cells == 0 || i.kind == "Polygonal Mesh").unwrap_or(false) {
                    ui.label("Point size");
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(&mut d.point_size, 1.0..=30.0));
                        ui.checkbox(&mut d.points_as_spheres, "spheres");
                    });
                    ui.end_row();
                }
                ui.label("Lighting");
                ui.checkbox(&mut d.lighting, "");
                ui.end_row();
            });
            if d != display {
                state.session.lock().set_display(id, |x| *x = d);
            }
        });

        if let Some(info) = &info {
            ui.collapsing(RichText::new("Information").strong(), |ui| info_body(ui, info));
        }
    });
}

fn info_body(ui: &mut egui::Ui, info: &aurafx_data::DataSetInfo) {
    egui::Grid::new("info").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Type");
        ui.label(&info.kind);
        ui.end_row();
        ui.label("Points");
        ui.label(info.n_points.to_string());
        ui.end_row();
        ui.label("Cells");
        ui.label(info.n_cells.to_string());
        ui.end_row();
        ui.label("Memory");
        ui.label(format!("{:.1} MB", info.bytes as f64 / 1e6));
        ui.end_row();
        if !info.bounds.is_empty() {
            let b = &info.bounds;
            ui.label("Bounds X");
            ui.label(format!("{} .. {}", aurafx_data::info::g(b.min[0]), aurafx_data::info::g(b.max[0])));
            ui.end_row();
            ui.label("Bounds Y");
            ui.label(format!("{} .. {}", aurafx_data::info::g(b.min[1]), aurafx_data::info::g(b.max[1])));
            ui.end_row();
            ui.label("Bounds Z");
            ui.label(format!("{} .. {}", aurafx_data::info::g(b.min[2]), aurafx_data::info::g(b.max[2])));
            ui.end_row();
        }
    });
    for (title, arrs) in [("Point arrays", &info.point_arrays), ("Cell arrays", &info.cell_arrays), ("Field arrays", &info.field_arrays)] {
        if arrs.is_empty() {
            continue;
        }
        ui.add_space(4.0);
        ui.label(RichText::new(title).strong());
        egui::Grid::new(title).striped(true).num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
            for a in arrs {
                ui.label(&a.name);
                ui.label(RichText::new(&a.dtype).weak());
                ui.label(format!("{}c", a.ncomp));
                ui.label(format!("{} .. {}", aurafx_data::info::g(a.range.0), aurafx_data::info::g(a.range.1)));
                ui.end_row();
            }
        });
    }
    if !info.blocks.is_empty() {
        ui.label(RichText::new(format!("Blocks: {}", info.blocks.join(", "))).weak());
    }
}

pub fn information_ui(state: &mut State, ui: &mut egui::Ui) {
    let info = state.session.lock().active_node().and_then(|n| n.info.clone());
    match info {
        Some(i) => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| info_body(ui, &i));
        }
        None => {
            ui.label(RichText::new("No dataset selected.").weak());
        }
    }
}

// ------------------------------------------------------------------ log

pub fn log_ui(state: &mut State, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button("Clear").clicked() {
            state.session.lock().log.clear();
        }
    });
    let entries: Vec<(LogLevel, String, f64)> = state.session.lock().log.iter().rev().take(500).map(|e| (e.level, e.message.clone(), e.when)).collect();
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        for (lvl, msg, t) in entries.into_iter().rev() {
            let color = match lvl {
                LogLevel::Info => ui.visuals().text_color(),
                LogLevel::Warn => crate::theme::WARN,
                LogLevel::Error => crate::theme::ERROR,
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{t:8.1}s")).weak().monospace());
                ui.label(RichText::new(msg).color(color));
            });
        }
    });
}

// ------------------------------------------------------------------ spreadsheet

pub fn spreadsheet_ui(state: &mut State, ui: &mut egui::Ui) {
    let (name, ds) = {
        let s = state.session.lock();
        match s.active_node() {
            Some(n) => (n.name.clone(), n.output.clone()),
            None => (String::new(), None),
        }
    };
    let Some(ds) = ds else {
        ui.label(RichText::new("Select a node with data.").weak());
        return;
    };
    let ds = match &*ds {
        aurafx_data::DataSet::MultiBlock(mb) => mb.leaves().into_iter().next().map(|(_, d)| d.clone()),
        d => Some(d.clone()),
    };
    let Some(ds) = ds else { return };
    let table_mode = matches!(ds, aurafx_data::DataSet::Table(_));
    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).strong());
        if !table_mode {
            ui.selectable_value(&mut state.spreadsheet_assoc, Association::Points, "Points");
            ui.selectable_value(&mut state.spreadsheet_assoc, Association::Cells, "Cells");
        }
    });
    let (attrs, n_rows, points) = match (&ds, state.spreadsheet_assoc) {
        (aurafx_data::DataSet::Table(t), _) => (Some(t.columns.clone()), t.n_rows(), None),
        (d, Association::Points) => (d.point_data().cloned(), d.n_points(), d.points()),
        (d, Association::Cells) => (d.cell_data().cloned(), d.n_cells(), None),
    };
    let Some(attrs) = attrs else { return };
    // Optional filter to the 3D-view selection.
    let sel_rows: Option<Vec<usize>> = {
        let s = state.session.lock();
        let assoc = if table_mode { Association::Points } else { state.spreadsheet_assoc };
        s.selection.as_ref().filter(|sel| Some(sel.node) == s.active && sel.assoc == assoc).map(|sel| sel.ids.iter().copied().filter(|&i| i < n_rows).collect())
    };
    if let Some(rows) = &sel_rows {
        ui.checkbox(&mut state.spreadsheet_only_selected, format!("Only selected ({})", rows.len()));
    }
    let row_map: Option<Vec<usize>> = sel_rows.filter(|_| state.spreadsheet_only_selected);
    let n_rows = row_map.as_ref().map(|r| r.len()).unwrap_or(n_rows);
    let page_size = 200;
    let pages = n_rows.div_ceil(page_size).max(1);
    state.spreadsheet_page = state.spreadsheet_page.min(pages - 1);
    ui.horizontal(|ui| {
        if ui.button(crate::icons::CARET_LEFT).clicked() && state.spreadsheet_page > 0 {
            state.spreadsheet_page -= 1;
        }
        ui.label(format!("rows {}–{} of {}", state.spreadsheet_page * page_size, ((state.spreadsheet_page + 1) * page_size).min(n_rows), n_rows));
        if ui.button(crate::icons::CARET_RIGHT).clicked() && state.spreadsheet_page + 1 < pages {
            state.spreadsheet_page += 1;
        }
        if ui.button("Export CSV…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(format!("{name}.csv")).save_file() {
                let mut out = String::new();
                let mut header: Vec<String> = Vec::new();
                if points.is_some() {
                    header.extend(["x", "y", "z"].map(String::from));
                }
                for a in attrs.iter() {
                    if a.ncomp == 1 {
                        header.push(a.name.clone());
                    } else {
                        header.extend((0..a.ncomp).map(|c| format!("{}_{c}", a.name)));
                    }
                }
                out += &header.join(",");
                out.push('\n');
                for i in 0..n_rows {
                    let mut row: Vec<String> = Vec::new();
                    if let Some(p) = &points {
                        row.extend(p.get(i).iter().map(|v| v.to_string()));
                    }
                    for a in attrs.iter() {
                        for c in 0..a.ncomp {
                            row.push(a.get(i, c).to_string());
                        }
                    }
                    out += &row.join(",");
                    out.push('\n');
                }
                let _ = std::fs::write(p, out);
            }
        }
    });
    let start = state.spreadsheet_page * page_size;
    let end = (start + page_size).min(n_rows);
    let cols: Vec<&aurafx_data::Array> = attrs.iter().collect();
    use egui_extras::{Column, TableBuilder};
    let mut tb = TableBuilder::new(ui).striped(true).resizable(true).column(Column::auto());
    if points.is_some() {
        tb = tb.columns(Column::auto(), 3);
    }
    tb = tb.columns(Column::auto().at_least(80.0), cols.len());
    tb.header(20.0, |mut h| {
        h.col(|ui| {
            ui.label(RichText::new("#").strong());
        });
        if points.is_some() {
            for n in ["x", "y", "z"] {
                h.col(|ui| {
                    ui.label(RichText::new(n).strong());
                });
            }
        }
        for a in &cols {
            h.col(|ui| {
                ui.label(RichText::new(&a.name).strong()).on_hover_text(format!("{} x {}", a.scalar_type().vtk_name(), a.ncomp));
            });
        }
    })
    .body(|body| {
        body.rows(18.0, end - start, |mut row| {
            let i = start + row.index();
            let i = row_map.as_ref().map(|m| m[i]).unwrap_or(i);
            row.col(|ui| {
                ui.label(i.to_string());
            });
            if let Some(p) = &points {
                for v in p.get(i) {
                    row.col(|ui| {
                        ui.label(aurafx_data::info::g(v));
                    });
                }
            }
            for a in &cols {
                row.col(|ui| {
                    if a.scalar_type() == aurafx_data::ScalarType::String {
                        if let Some(s) = a.as_slice::<String>() {
                            ui.label(&s[i]);
                        }
                    } else if a.ncomp == 1 {
                        ui.label(aurafx_data::info::g(a.get(i, 0)));
                    } else {
                        ui.label(a.tuple(i).iter().map(|v| aurafx_data::info::g(*v)).collect::<Vec<_>>().join(", "));
                    }
                });
            }
        });
    });
}
