//! The right-click menu of a 3D view: what can be done with the object under the pointer (or the
//! selected one), at the point that was clicked. A right-click that drags still zooms; the menu
//! opens on a click without a drag.

use crate::app::State;
use crate::menus::MenuExt;
use aurafx_pipeline::session::CameraRequest;
use aurafx_pipeline::{NodeId, Representation};
use egui::RichText;
use std::sync::Arc;

/// Where the menu was opened: the object and the point on its surface, if the click hit one.
#[derive(Clone, Copy, Default)]
pub struct ViewMenuHit {
    pub node: Option<NodeId>,
    pub point: Option<[f64; 3]>,
}

/// Find what is under the pointer when the right button is pressed, and attach the menu.
pub fn view_context_menu(state: &mut State, response: &egui::Response, rect: egui::Rect) {
    if crate::cad::editing(state) {
        // Editing a part: the menu is about the face under the pointer.
        if crate::cad::sketching(state) || crate::cad::picking(state) {
            return;
        }
        if response.secondary_clicked() {
            let node = state.session.lock().cad_editing();
            state.cad.menu_face = None;
            if let (Some(node), Some(pos)) = (node, response.interact_pointer_pos()) {
                let ray = crate::pick::screen_ray(&state.viewport.camera, rect, pos);
                let meshes: Vec<(NodeId, Arc<aurafx_render::MeshData>)> = state.viewport.meshes_for_pick().into_iter().filter(|(id, _)| *id == node).collect();
                if let Some(h) = crate::pick::raycast(&ray, &meshes, 0.0) {
                    state.cad.menu_face = crate::cad::face_under(state, node, h.world.to_array());
                }
            }
        }
        response.context_menu(|ui| {
            let face = state.cad.menu_face.clone();
            crate::cad::face_menu(state, ui, face.as_ref());
        });
        return;
    }
    if response.secondary_clicked() {
        let mut hit = ViewMenuHit::default();
        if let Some(pos) = response.interact_pointer_pos() {
            let ray = crate::pick::screen_ray(&state.viewport.camera, rect, pos);
            let visible: std::collections::HashSet<NodeId> = state.session.lock().nodes.values().filter(|n| n.visible).map(|n| n.id).collect();
            let meshes: Vec<(NodeId, Arc<aurafx_render::MeshData>)> =
                state.viewport.meshes_for_pick().into_iter().filter(|(id, _)| visible.contains(id)).collect();
            let radius = state.viewport.camera.world_per_pixel(rect.height() as f64) * 6.0;
            if let Some(h) = crate::pick::raycast(&ray, &meshes, radius) {
                hit = ViewMenuHit { node: Some(h.node), point: Some(h.world.to_array()) };
            }
        }
        state.view_menu_hit = hit;
    }
    let hit = state.view_menu_hit;
    response.context_menu(|ui| {
        let target = hit.node.or(state.session.lock().active);
        let Some(id) = target else {
            ui.label(RichText::new("Nothing here yet").weak());
            if ui.button(crate::icons::with("Open")).clicked() {
                ui.close();
                state.open_file_dialog();
            }
            if ui.button(format!("{} Search sources and filters  Ctrl+K", crate::icons::MAGNIFYING_GLASS)).clicked() {
                ui.close();
                state.palette.open = true;
            }
            return;
        };
        let (name, kind) = {
            let s = state.session.lock();
            match s.nodes.get(&id) {
                Some(n) => (n.name.clone(), n.info.as_ref().map(|i| i.kind.clone()).unwrap_or_default()),
                None => return,
            }
        };
        ui.label(RichText::new(&name).strong());
        if !kind.is_empty() {
            ui.label(RichText::new(kind).weak().small());
        }
        ui.separator();
        // At the clicked point.
        if let Some(p) = hit.point {
            let at = |state: &mut State, filter: &str| {
                let mut s = state.session.lock();
                s.active = Some(id);
                match s.add_filter(filter, id) {
                    Ok(new) => {
                        s.set_param_json(new, "origin", &serde_json::json!(p));
                        drop(s);
                        state.status = format!("Added {filter} at the clicked point: drag its handle in the view, or change it in Properties");
                    }
                    Err(e) => {
                        drop(s);
                        state.toast(format!("{e:#}"), true);
                    }
                }
            };
            if ui.button(crate::icons::with("Slice here")).on_hover_text("A cutting plane through the point you clicked").clicked() {
                ui.close();
                at(state, "slice");
            }
            if ui.button(crate::icons::with("Clip here")).on_hover_text("Cut the object away on one side of the point you clicked").clicked() {
                ui.close();
                at(state, "clip");
            }
            if ui.button(format!("{} Turn the view about this point", crate::icons::ARROWS_COUNTER_CLOCKWISE)).clicked() {
                ui.close();
                let c = &mut state.viewport.camera;
                let d = glam::DVec3::from_array(p) - c.target;
                c.target += d;
                c.eye += d;
            }
            ui.separator();
        }
        // What fits this data.
        let ctx_info = crate::menus::node_context(state, Some(id));
        if let Some((info, timed)) = &ctx_info {
            if let Some(f) = crate::menus::suggestion_buttons(ui, info, *timed) {
                state.session.lock().active = Some(id);
                state.add_filter(f);
            }
        }
        ui.scroll_menu("All filters", |ui| {
            if let Some(f) = crate::menus::filter_menu_for(ui, ctx_info.as_ref().map(|c| &c.0), |_| true) {
                state.session.lock().active = Some(id);
                state.add_filter(f);
            }
        });
        ui.separator();
        // How it is drawn.
        let (rep, color_by, arrays) = {
            let s = state.session.lock();
            let n = &s.nodes[&id];
            let arrays: Vec<(aurafx_filters::Association, String)> = n
                .info
                .as_ref()
                .map(|i| {
                    i.point_arrays
                        .iter()
                        .map(|a| (aurafx_filters::Association::Points, a.name.clone()))
                        .chain(i.cell_arrays.iter().map(|a| (aurafx_filters::Association::Cells, a.name.clone())))
                        .collect()
                })
                .unwrap_or_default();
            (n.display.representation, n.display.color_by.clone(), arrays)
        };
        ui.scroll_menu("Draw as", |ui| {
            for r in Representation::ALL {
                if ui.selectable_label(rep == r, r.label()).clicked() {
                    ui.close();
                    state.session.lock().set_display(id, |d| d.representation = r);
                }
            }
        });
        ui.scroll_menu("Color by", |ui| {
            if ui.selectable_label(color_by.is_none(), "One color").clicked() {
                ui.close();
                state.session.lock().set_display(id, |d| d.color_by = None);
            }
            for (assoc, name) in &arrays {
                let on = color_by.as_ref().is_some_and(|c| c.assoc == *assoc && &c.name == name);
                let tag = if *assoc == aurafx_filters::Association::Points { "points" } else { "cells" };
                if ui.selectable_label(on, format!("{name}  ({tag})")).clicked() {
                    ui.close();
                    let (assoc, name) = (*assoc, name.clone());
                    state.session.lock().set_display(id, |d| {
                        d.color_by = Some(aurafx_pipeline::ColorBy { assoc, name, component: None });
                        d.range = None;
                    });
                }
            }
        });
        if ui.button(crate::icons::with("Hide")).clicked() {
            ui.close();
            state.session.lock().set_visible(id, false);
        }
        ui.separator();
        if ui.button(format!("{} Reset camera", crate::icons::ARROWS_COUNTER_CLOCKWISE)).clicked() {
            ui.close();
            state.session.lock().camera_request = Some(CameraRequest::ResetToBounds);
        }
        if ui.button(format!("{} Save screenshot…", crate::icons::CAMERA)).clicked() {
            ui.close();
            state.save_screenshot_dialog();
        }
    });
}
