//! Multiple 3D views.
//!
//! `State::viewport` is always the *active* view; other views live in `State::views`. Node
//! visibility (`Node::visible`) describes the active view; each inactive view keeps its own
//! set of visible nodes, swapped in while it is drawn. This keeps every tool (Python, menus,
//! picking, screenshots, animations) working on "the view you are looking at".

use crate::app::{Pane, State};
use crate::viewport::Viewport;
use aurafx_pipeline::NodeId;
use std::collections::HashSet;

pub const VIEW_DIRECTIONS: &[(&str, [f64; 3], [f64; 3])] = &[
    ("Isometric", [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]),
    ("+X", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ("-X", [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ("+Y", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ("-Y", [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
    ("+Z", [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ("-Z", [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
];

pub fn pane_for(id: u32) -> Pane {
    if id == 0 {
        Pane::Viewport
    } else {
        Pane::View(id)
    }
}

impl State {
    fn visible_set(&self) -> HashSet<NodeId> {
        self.session.lock().nodes.values().filter(|n| n.visible).map(|n| n.id).collect()
    }

    fn apply_visible_set(&self, set: &HashSet<NodeId>) {
        let mut s = self.session.lock();
        for n in s.nodes.values_mut() {
            n.visible = set.contains(&n.id);
        }
        s.revision += 1;
    }

    /// Create a new view that starts as a copy of the active one (camera, visibility, link).
    pub fn new_view(&mut self) -> u32 {
        let id = self.next_view_id.max(1);
        self.next_view_id = id + 1;
        let mut v = Viewport::with_render_state(self.viewport.render_state(), self.settings.background, id);
        v.camera = self.viewport.camera.clone();
        v.linked = self.viewport.linked;
        v.mark_camera_initialized();
        self.views.insert(id, v);
        let vis = self.visible_set();
        self.view_visibility.insert(id, vis);
        id
    }

    /// Make view `id` the active one.
    pub fn activate_view(&mut self, id: u32) {
        if id == self.active_view {
            return;
        }
        let Some(mut next) = self.views.remove(&id) else { return };
        let current_vis = self.visible_set();
        self.view_visibility.insert(self.active_view, current_vis);
        std::mem::swap(&mut self.viewport, &mut next);
        // `next` now holds the previously active view.
        self.views.insert(self.active_view, next);
        let vis = self.view_visibility.remove(&id).unwrap_or_default();
        self.apply_visible_set(&vis);
        self.active_view = id;
    }

    /// Draw view `id` (active or not) in a pane.
    pub fn view_pane_ui(&mut self, ui: &mut egui::Ui, id: u32) {
        if id == self.active_view {
            crate::viewport::viewport_ui(self, ui, true);
            return;
        }
        if !self.views.contains_key(&id) {
            // Restored from a saved layout: create it on first use.
            let mut v = Viewport::with_render_state(self.viewport.render_state(), self.settings.background, id);
            v.camera = self.viewport.camera.clone();
            self.views.insert(id, v);
            self.next_view_id = self.next_view_id.max(id + 1);
        }
        let mut v = self.views.remove(&id).unwrap();
        v.settings.background = self.viewport.settings.background;
        std::mem::swap(&mut self.viewport, &mut v);
        let saved = self.visible_set();
        let mine = self.view_visibility.get(&id).cloned().unwrap_or_else(|| saved.clone());
        self.apply_visible_set(&mine);
        let interacted = crate::viewport::viewport_ui(self, ui, false);
        self.apply_visible_set(&saved);
        std::mem::swap(&mut self.viewport, &mut v);
        self.views.insert(id, v);
        if interacted {
            self.pending_activate = Some(id);
        }
    }

    /// After all views are drawn: activation requests, linked cameras, "match" requests, closed views.
    pub fn views_end_frame(&mut self, open: &[u32]) {
        if let Some(id) = self.pending_activate.take() {
            self.activate_view(id);
        }
        // The active view's pane was closed: fall back to another open view.
        if !open.contains(&self.active_view) {
            if let Some(&id) = open.first() {
                self.activate_view(id);
            }
        }
        // Forget views whose panes were closed.
        let gone: Vec<u32> = self.views.keys().copied().filter(|id| !open.contains(id)).collect();
        for id in gone {
            self.views.remove(&id);
            self.view_visibility.remove(&id);
        }
        if let Some(from) = self.camera_match_from.take() {
            let cam = if from == self.active_view { Some(self.viewport.camera.clone()) } else { self.views.get(&from).map(|v| v.camera.clone()) };
            if let Some(cam) = cam {
                for v in self.views.values_mut() {
                    v.camera = cam.clone();
                }
                self.viewport.camera = cam;
            }
        }
        if self.viewport.linked {
            let cam = self.viewport.camera.clone();
            for v in self.views.values_mut().filter(|v| v.linked) {
                v.camera = cam.clone();
            }
        }
    }

    /// Link (or unlink) every view's camera.
    pub fn link_all_views(&mut self, on: bool) {
        self.viewport.linked = on;
        for v in self.views.values_mut() {
            v.linked = on;
        }
    }
}
