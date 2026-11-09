//! Command palette: fuzzy search over filters, sources and actions.

use crate::app::State;
use aurafx_pipeline::session::CameraRequest;
use egui::RichText;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

enum Item {
    Filter(std::sync::Arc<aurafx_filters::FilterDef>),
    Action(&'static str, fn(&mut State)),
}

fn actions() -> Vec<(&'static str, &'static str, fn(&mut State))> {
    vec![
        ("Open file…", "open load file data", |s| s.open_file_dialog()),
        ("Save screenshot…", "image png export", |s| s.save_screenshot_dialog()),
        ("Export scene…", "gltf glb obj 3d model blender export", |s| {
            if let Some(path) = crate::dialogs::FileDialog::new()
                .add_filter("glTF binary", &["glb"])
                .add_filter("Wavefront OBJ", &["obj"])
                .add_filter("X3D", &["x3d"])
                .add_filter("VRML 2.0", &["wrl"])
                .set_file_name("scene.glb")
                .save_file()
            {
                s.viewport.request_screenshot(aurafx_pipeline::session::ScreenshotRequest { path, size: None, transparent: false, id: 0, overlays: true });
            }
        }),
        ("Reset camera", "fit view zoom", |s| s.session.lock().camera_request = Some(CameraRequest::ResetToBounds)),
        ("Look from +X", "view camera axis", |s| {
            s.session.lock().camera_request = Some(CameraRequest::View { direction: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0] })
        }),
        ("Look from +Y", "view camera axis", |s| {
            s.session.lock().camera_request = Some(CameraRequest::View { direction: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0] })
        }),
        ("Look from +Z", "view camera axis", |s| {
            s.session.lock().camera_request = Some(CameraRequest::View { direction: [0.0, 0.0, 1.0], up: [0.0, 1.0, 0.0] })
        }),
        ("Isometric view", "view camera", |s| s.session.lock().camera_request = Some(CameraRequest::View { direction: [1.0, 1.0, 1.0], up: [0.0, 0.0, 1.0] })),
        ("Toggle orthographic projection", "camera parallel perspective", |s| s.viewport.camera.orthographic = !s.viewport.camera.orthographic),
        ("Delete active node", "remove", |s| {
            let a = s.session.lock().active;
            if let Some(a) = a {
                s.session.lock().remove_node(a);
            }
        }),
        ("Hide all", "visibility", |s| {
            let mut ss = s.session.lock();
            let ids: Vec<_> = ss.nodes.keys().copied().collect();
            for id in ids {
                ss.set_visible(id, false);
            }
        }),
        ("Show all", "visibility", |s| {
            let mut ss = s.session.lock();
            let ids: Vec<_> = ss.nodes.keys().copied().collect();
            for id in ids {
                ss.set_visible(id, true);
            }
        }),
        ("Play / pause time", "animation", |s| s.playing = !s.playing),
        ("Save data…", "export write vtu vtk csv stl exodus vtkhdf ensight xdmf cgns", |s| s.save_data_dialog()),
        ("Save state…", "session afx save", |s| s.save_state_dialog(false)),
        ("Undo", "revert back", |s| s.undo()),
        ("Redo", "again", |s| s.redo()),
        ("Light theme", "appearance bright white", |s| s.session.lock().app_commands.push("theme light".into())),
        ("Dark theme", "appearance night black", |s| s.session.lock().app_commands.push("theme dark".into())),
        ("Settings", "preferences ai python theme", |s| s.show_settings = true),
        ("Describe a filter…", "ai natural language words assistant make filter", |s| s.assistant.describe_open = true),
        ("Ask the AI assistant", "chat help", |s| s.assistant.focus_input = true),
        ("Python quick reference", "help api cheat sheet", |s| s.console.show_help = true),
        ("Keyboard shortcuts", "help keys", |s| s.show_shortcuts = true),
        ("Clear pipeline", "remove all reset new", |s| {
            let mut ss = s.session.lock();
            let ids: Vec<_> = ss.nodes.keys().copied().collect();
            for id in ids {
                ss.remove_node(id);
            }
        }),
    ]
}

pub fn palette_ui(state: &mut State, ctx: &egui::Context) {
    if !state.palette.open {
        return;
    }
    let q = state.palette.query.clone();
    let mut items: Vec<(u32, Item)> = Vec::new();
    for f in aurafx_filters::search(&q) {
        items.push((f.search_score(&q), Item::Filter(f)));
    }
    let ql = q.to_lowercase();
    for (label, keys, f) in actions() {
        let score = if ql.is_empty() {
            1
        } else if label.to_lowercase().starts_with(&ql) {
            600
        } else if label.to_lowercase().contains(&ql) {
            350
        } else if keys.contains(&ql) {
            150
        } else {
            0
        };
        if score > 0 {
            items.push((score, Item::Action(label, f)));
        }
    }
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let n = items.len();
    let mut close = false;
    let mut run: Option<usize> = None;
    let screen = ctx.content_rect();
    #[allow(deprecated)]
    egui::Area::new(egui::Id::new("palette")).order(egui::Order::Foreground).fixed_pos(egui::pos2(screen.center().x - 280.0, screen.top() + 80.0)).show(
        ctx,
        |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| {
                ui.set_width(560.0);
                let resp =
                    ui.add(egui::TextEdit::singleline(&mut state.palette.query).hint_text("Type a filter, source or action…").desired_width(f32::INFINITY));
                resp.request_focus();
                let (esc, enter, up, down) = ui.input_mut(|i| {
                    (
                        i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                        i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    )
                });
                if esc {
                    close = true;
                }
                if down && n > 0 {
                    state.palette.selected = (state.palette.selected + 1) % n;
                }
                if up && n > 0 {
                    state.palette.selected = (state.palette.selected + n - 1) % n;
                }
                if enter && n > 0 {
                    run = Some(state.palette.selected.min(n - 1));
                }
                if resp.changed() {
                    state.palette.selected = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (i, (_, item)) in items.iter().enumerate() {
                        let sel = i == state.palette.selected;
                        let (title, sub) = match item {
                            Item::Filter(f) => (
                                format!("{}  {}", if f.is_source { crate::icons::PLUS } else { crate::icons::FUNNEL }, f.label),
                                format!("{} · {}", f.category, f.description),
                            ),
                            Item::Action(l, _) => (format!("{}  {l}", crate::icons::ARROW_RIGHT), "Action".to_string()),
                        };
                        let r = ui.selectable_label(sel, RichText::new(title).strong());
                        ui.label(RichText::new(sub).weak().small());
                        if r.clicked() {
                            run = Some(i);
                        }
                        if sel && (up || down) {
                            r.scroll_to_me(None);
                        }
                    }
                    if items.is_empty() {
                        ui.label(RichText::new("No matches").weak());
                    }
                });
            });
        },
    );
    if let Some(i) = run {
        match &items[i].1 {
            Item::Filter(f) => {
                let id = f.id;
                if f.is_source {
                    crate::source_dialog::insert_source(state, id);
                } else {
                    state.add_filter(id);
                }
            }
            Item::Action(_, f) => f(state),
        }
        close = true;
    }
    // Click outside closes.
    if ctx.input(|i| i.pointer.any_click()) && !ctx.is_pointer_over_egui() {
        close = true;
    }
    if close {
        state.palette.open = false;
        state.palette.query.clear();
        state.palette.selected = 0;
    }
}
