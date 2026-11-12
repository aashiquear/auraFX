//! Finding one's way in the CAD environment without a manual:
//!
//! - **What next**: a strip at the top of the model tree that follows what the user is doing
//!   (an empty part, an open sketch, a picker, a part that does not build) and offers the two
//!   or three steps that make sense now, as buttons.
//! - **Command search** (the S key, or Ctrl+K while editing a part): every command of the
//!   environment in a small box at the pointer; type a few letters and press Enter.
//! - **Keys** for the sketch tools (L, R, C, A, P, Esc) and a right-click menu on the part's
//!   faces.
//!
//! All of them run the same operations as the ribbon's buttons.

use super::{op, pick, ribbon, sketch, PickKind, RibbonTab};
use crate::app::State;
use aurafx_cad::SketchPlane;
use egui::RichText;
use serde_json::json;

/// One command of the environment.
pub struct Cmd {
    pub label: &'static str,
    /// The ribbon tab it lives on (shown in the search, so the button can be found again).
    pub tab: &'static str,
    /// What it does, in a sentence.
    pub tip: &'static str,
    /// Other words people type for it.
    pub words: &'static str,
    /// Its key, when it has one.
    pub key: &'static str,
    /// Only offered while a sketch is open (`Some(true)`), only outside one (`Some(false)`).
    pub in_sketch: Option<bool>,
    pub run: fn(&mut State),
}

fn add_shape(state: &mut State, kind: &str, base: &str, extra: serde_json::Value) {
    let (at, has_part) = ribbon::top_center(state);
    let name = ribbon::unique(state, base);
    let mut a = json!({"kind": kind, "name": name, "at": at});
    if let (Some(o), Some(e)) = (a.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    let _ = has_part;
    op(state, "add", a);
}

fn tool(state: &mut State, t: sketch::Tool) {
    if let Some(e) = state.cad.sketch.as_mut() {
        e.select_tool(t);
    }
}

/// Every command, in the order of the ribbon.
pub fn commands() -> Vec<Cmd> {
    let c = |label, tab, tip, words, key, in_sketch, run| Cmd { label, tab, tip, words, key, in_sketch, run };
    vec![
        // Sketch.
        c("Sketch on the Top plane", "Sketch", "Draw an outline on the horizontal plane, then extrude it", "draw new sketch xy", "", Some(false), |s| {
            sketch::begin_new(s, SketchPlane::XY, 0.0, false)
        }),
        c("Sketch on the Front plane", "Sketch", "Draw on the front plane (x across, z up)", "draw new sketch xz", "", Some(false), |s| {
            sketch::begin_new(s, SketchPlane::XZ, 0.0, false)
        }),
        c("Sketch on the Right plane", "Sketch", "Draw on the side plane (y across, z up)", "draw new sketch yz", "", Some(false), |s| {
            sketch::begin_new(s, SketchPlane::YZ, 0.0, false)
        }),
        c("Sketch on a face…", "Sketch", "Click a flat face of the part and draw on it", "draw face boss pocket", "", Some(false), |s| {
            pick::open(s, PickKind::SketchFace)
        }),
        c("Reference plane…", "Sketch", "Make a plane to sketch on: offset, turned, or through three points", "datum offset plane", "", Some(false), |s| {
            super::open_plane_form(s)
        }),
        c("Line", "Sketch", "Click points to draw connected lines; right-click ends the chain", "draw segment", "L", Some(true), |s| {
            tool(s, sketch::Tool::Line)
        }),
        c("Rectangle", "Sketch", "Click two opposite corners", "draw box square", "R", Some(true), |s| tool(s, sketch::Tool::Rect)),
        c("Circle", "Sketch", "Click the center, then a point on the circle", "draw round", "C", Some(true), |s| tool(s, sketch::Tool::Circle)),
        c("Arc", "Sketch", "Click the center, the start, then the end", "draw curve", "A", Some(true), |s| tool(s, sketch::Tool::Arc)),
        c("Spline", "Sketch", "Click control points; Enter finishes", "draw curve smooth", "", Some(true), |s| tool(s, sketch::Tool::Spline)),
        c("Point", "Sketch", "Place a point", "draw", "P", Some(true), |s| tool(s, sketch::Tool::Point)),
        c("Select", "Sketch", "Select points and shapes to constrain, dimension, move or delete them", "pick move drag", "Esc", Some(true), |s| {
            tool(s, sketch::Tool::Select)
        }),
        c("Extrude the sketch", "Sketch", "Finish the sketch as a solid of the given depth (or a cut)", "finish boss pocket pad", "", Some(true), |s| {
            sketch::finish(s, false)
        }),
        c("Revolve the sketch", "Sketch", "Finish the sketch turned about its vertical axis", "finish lathe turn", "", Some(true), |s| sketch::finish(s, true)),
        c("Cancel the sketch", "Sketch", "Close the sketch without changing the part", "close leave", "", Some(true), sketch::cancel),
        // Features.
        c("Box", "Features", "A 20 mm box on top of the part", "block brick cube", "", Some(false), |s| {
            add_shape(s, "box", "Box", json!({"size": [20, 20, 20]}))
        }),
        c("Cylinder", "Features", "A cylinder standing on the part", "rod post boss", "", Some(false), |s| {
            add_shape(s, "cylinder", "Cylinder", json!({"radius": 8, "height": 20}))
        }),
        c("Sphere", "Features", "A sphere of radius 10", "ball", "", Some(false), |s| add_shape(s, "sphere", "Sphere", json!({"radius": 10}))),
        c("Hole", "Features", "A simple hole cut down from the top of the part", "drill bore", "", Some(false), |s| {
            add_shape(s, "hole", "Hole", json!({"hole": "simple", "diameter": 6, "depth": 10}))
        }),
        c("Counterbore hole", "Features", "A hole with a flat recess for a bolt head", "bolt screw", "", Some(false), |s| {
            add_shape(s, "hole", "Hole", json!({"hole": "counterbore", "diameter": 6, "depth": 10, "cb_diameter": 11, "cb_depth": 4}))
        }),
        c("Tapped hole", "Features", "A threaded hole", "thread screw", "", Some(false), |s| {
            add_shape(s, "hole", "Hole", json!({"hole": "tapped", "diameter": 8, "depth": 10, "pitch": 1.25}))
        }),
        // Modify.
        c("Fillet edges…", "Modify", "Click edges of the part and round them", "round blend radius", "", Some(false), |s| pick::open(s, PickKind::Fillet)),
        c("Chamfer edges…", "Modify", "Click edges and cut them at an angle", "bevel", "", Some(false), |s| pick::open(s, PickKind::Chamfer)),
        c("Shell, open faces…", "Modify", "Hollow the part, clicking the faces to open", "hollow wall thickness enclosure", "", Some(false), |s| {
            pick::open(s, PickKind::Shell)
        }),
        c("Draft faces…", "Modify", "Taper side faces by an angle (for moulded parts)", "taper mould", "", Some(false), |s| pick::open(s, PickKind::Draft)),
        c("Pattern on a face…", "Modify", "Holes or bosses in a row, a grid or a circle on a flat face", "array bolt circle copies", "", Some(false), |s| {
            pick::open(s, PickKind::PatternFace)
        }),
        c("Mirror about a face…", "Modify", "Add the reflection of the part about a flat face", "reflect symmetric", "", Some(false), |s| {
            pick::open(s, PickKind::MirrorFace)
        }),
        c("Fillet all edges", "Modify", "Round every edge of the selected feature (or the part) by 1 mm", "round", "", Some(false), |s| {
            let t = ribbon::selected_name(s).map(serde_json::Value::String).unwrap_or(serde_json::Value::Null);
            op(s, "modifier", json!({"feature": t, "modifier": {"kind": "Fillet", "radius": 1}}));
        }),
        // Inspect, Assemble, Files.
        c(
            "Measure and mass",
            "Inspect",
            "Volume, surface, size, mass, a section's area and the distance between two faces",
            "distance angle weight",
            "",
            Some(false),
            |s| s.cad.tab = RibbonTab::Inspect,
        ),
        c(
            "Insert a part",
            "Assemble",
            "Place another part beside this one (a template, a blank part or a CAD file)",
            "assembly component",
            "",
            Some(false),
            |s| s.cad.tab = RibbonTab::Assemble,
        ),
        c("Check fit", "Assemble", "Find the parts that overlap", "interference clash collision", "", Some(false), ribbon::check_fit),
        c("Start from a template", "Files", "Replace the part by a starter part (a bracket, a flange, an enclosure…)", "new example", "", Some(false), |s| {
            s.cad.tab = RibbonTab::Files
        }),
        c("Save the part…", "Files", "Write the part to an .afxcad file", "write file", "", None, ribbon::save_as),
        c(
            "Export STEP, IGES, 3MF or STL…",
            "Files",
            "Write the part for another CAD program, a printer or a mesher",
            "save write file exchange",
            "",
            None,
            ribbon::export_as,
        ),
        // The environment.
        c("Apply", "top right", "Hand the part to the pipeline and leave; the nodes below it re-run", "finish done ok accept", "Ctrl+Enter", None, |s| {
            super::apply(s, false)
        }),
        c("Apply, keep editing", "top right", "Hand the part to the pipeline and stay here", "finish save", "", None, |s| super::apply(s, true)),
        c("Discard and leave", "top right", "Leave the CAD environment; the part is as it was", "cancel close exit", "Esc", None, |s| {
            s.cad.leave_dialog = true
        }),
        c("Undo", "Edit menu", "Take back the last change", "back revert", "Ctrl+Z", None, |s| s.undo()),
        c("Redo", "Edit menu", "Do the undone change again", "again", "Ctrl+Shift+Z", None, |s| s.redo()),
    ]
}

/// What the user is doing now, in a sentence, and the labels of the commands worth offering.
pub fn next_steps(state: &State) -> (String, Vec<&'static str>) {
    if let Some(p) = &state.cad.pick {
        return (format!("{}: click the part in the 3D view (click again to untick), then confirm in the panel on the right.", p.kind.title()), vec![]);
    }
    if let Some(e) = &state.cad.sketch {
        let (closed, open) = (e.sketch.profiles().len(), e.sketch.open_paths().len());
        let free = e.report.as_ref().map(|r| r.dof).unwrap_or(0);
        return if e.sketch.entities.is_empty() {
            ("Draw a closed outline: pick a tool (L line, R rectangle, C circle) and click in the view.".into(), vec!["Rectangle", "Circle", "Line"])
        } else if closed == 0 {
            (
                if open > 0 {
                    "The outline is still open: end the last line on the first point (it snaps), or draw a rectangle or circle."
                } else {
                    "Draw a closed outline to extrude."
                }
                .into(),
                vec!["Line", "Rectangle", "Cancel the sketch"],
            )
        } else if e.feature.is_some() {
            ("Change the outline or its dimensions, then press Apply sketch on the right.".into(), vec!["Select", "Cancel the sketch"])
        } else if free > 0 {
            (
                format!(
                    "{closed} closed outline(s). {free} degree(s) of freedom left (blue): select a line or two points and add a dimension so the size is fixed, or extrude as it is."
                ),
                vec!["Select", "Extrude the sketch", "Revolve the sketch"],
            )
        } else {
            ("The sketch is fully defined. Set the depth on the right and extrude it.".into(), vec!["Extrude the sketch", "Revolve the sketch"])
        };
    }
    let s = state.session.lock();
    let Some(e) = s.cad_edit.as_ref() else { return (String::new(), vec![]) };
    if e.document.features.is_empty() && e.document.components.is_empty() {
        return (
            "An empty part. Most parts start as a sketch that is extruded; a ready shape or a template is quicker.".into(),
            vec!["Sketch on the Top plane", "Box", "Start from a template"],
        );
    }
    if let Some(err) = &e.error {
        return (format!("The part does not build: {err}"), vec!["Undo"]);
    }
    if e.rollback.is_some() {
        return ("Rolled back: only the features above the marker are built, and a new feature goes in at the marker. Drag the marker in the timeline to the end to see the whole part.".into(), vec![]);
    }
    let only_one = e.document.features.len() == 1;
    let mut steps =
        if only_one { vec!["Sketch on a face…", "Hole", "Fillet edges…"] } else { vec!["Sketch on a face…", "Fillet edges…", "Shell, open faces…"] };
    let text = if e.changed {
        steps.push("Apply");
        "Add to the part, round its edges or hollow it; select a feature to change its numbers on the right. Apply hands the part to the pipeline."
    } else {
        "Select a feature to change its numbers on the right, or add to the part."
    };
    (text.into(), steps)
}

/// The "what next" strip.
pub fn guide_ui(state: &mut State, ui: &mut egui::Ui) {
    let (text, steps) = next_steps(state);
    if text.is_empty() {
        return;
    }
    let is_error = state.cad.sketch.is_none()
        && state.session.lock().cad_edit.as_ref().is_some_and(|e| e.error.is_some() && !(e.document.features.is_empty() && e.document.components.is_empty()));
    let all = commands();
    let mut run: Option<fn(&mut State)> = None;
    egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} Next", crate::icons::SPARKLE)).strong().color(if is_error {
                crate::theme::ERROR
            } else {
                crate::theme::ACCENT
            }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(format!("{} S", crate::icons::MAGNIFYING_GLASS))
                    .on_hover_text("Search every CAD command (press S in the 3D view, or Ctrl+K)")
                    .clicked()
                {
                    state.cad.search = Some(Search::default());
                }
            });
        });
        ui.add(egui::Label::new(RichText::new(&text).small()).wrap());
        ui.horizontal_wrapped(|ui| {
            for label in &steps {
                if let Some(c) = all.iter().find(|c| c.label == *label) {
                    let shown = if c.key.is_empty() { c.label.to_string() } else { format!("{}  ({})", c.label, c.key) };
                    if ui.button(shown).on_hover_text(c.tip).clicked() {
                        run = Some(c.run);
                    }
                }
            }
        });
    });
    if let Some(f) = run {
        f(state);
    }
    // The same sentence in the status bar, where the eye goes while working in the 3D view.
    let line = format!("CAD: {text}");
    if state.cad.last_hint != line {
        state.cad.last_hint = line.clone();
        state.status = line;
    }
}

/// The command search box.
#[derive(Default)]
pub struct Search {
    pub query: String,
    pub selected: usize,
    /// Where it opened (the pointer), so it does not follow the mouse.
    pub at: Option<egui::Pos2>,
}

fn score(c: &Cmd, q: &str) -> u32 {
    if q.is_empty() {
        return 1;
    }
    let l = c.label.to_lowercase();
    if l.starts_with(q) {
        300
    } else if l.split_whitespace().any(|w| w.starts_with(q)) {
        200
    } else if l.contains(q) {
        120
    } else if c.words.split_whitespace().any(|w| w.starts_with(q)) {
        80
    } else if c.tip.to_lowercase().contains(q) {
        30
    } else {
        0
    }
}

/// The commands matching `query` in the current mode, best first; with an empty query the
/// suggested next steps come first.
pub fn search(state: &State, query: &str) -> Vec<Cmd> {
    let q = query.trim().to_lowercase();
    let sketching = state.cad.sketch.is_some();
    let (_, steps) = next_steps(state);
    let mut v: Vec<(u32, Cmd)> = commands()
        .into_iter()
        .filter(|c| c.in_sketch.is_none_or(|s| s == sketching))
        .filter_map(|c| {
            let mut s = score(&c, &q);
            if s > 0 {
                if let Some(k) = steps.iter().position(|l| *l == c.label) {
                    s += 50 - k as u32;
                }
                Some((s, c))
            } else {
                None
            }
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|x| x.1).collect()
}

pub fn search_ui(state: &mut State, ctx: &egui::Context) {
    let Some(mut sr) = state.cad.search.take() else { return };
    let at = *sr.at.get_or_insert_with(|| {
        let screen = ctx.content_rect();
        let p = ctx.pointer_latest_pos().unwrap_or(screen.center());
        egui::pos2(p.x.clamp(screen.left() + 8.0, screen.right() - 340.0), p.y.clamp(screen.top() + 8.0, screen.bottom() - 330.0))
    });
    let items = search(state, &sr.query);
    let n = items.len();
    let (mut close, mut run): (bool, Option<fn(&mut State)>) = (false, None);
    #[allow(deprecated)]
    let area = egui::Area::new(egui::Id::new("cad search")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::window(ui.style()).show(ui, |ui| {
            ui.set_width(320.0);
            let r = ui.add(egui::TextEdit::singleline(&mut sr.query).hint_text("Search CAD commands…").desired_width(f32::INFINITY));
            r.request_focus();
            let (esc, enter, up, down) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                )
            });
            close |= esc;
            if r.changed() {
                sr.selected = 0;
            }
            if n > 0 {
                if down {
                    sr.selected = (sr.selected + 1) % n;
                }
                if up {
                    sr.selected = (sr.selected + n - 1) % n;
                }
                if enter {
                    run = Some(items[sr.selected.min(n - 1)].run);
                }
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (i, c) in items.iter().enumerate() {
                    let sel = i == sr.selected.min(n.saturating_sub(1));
                    let resp = ui
                        .horizontal(|ui| {
                            let r = ui.selectable_label(sel, RichText::new(c.label).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let place = if c.key.is_empty() { c.tab.to_string() } else { format!("{} · {}", c.key, c.tab) };
                                ui.label(RichText::new(place).weak().small());
                            });
                            r
                        })
                        .inner
                        .on_hover_text(c.tip);
                    if resp.clicked() {
                        run = Some(c.run);
                    }
                    if sel && (up || down) {
                        resp.scroll_to_me(None);
                    }
                }
                if items.is_empty() {
                    ui.label(RichText::new("No command matches. Ask the assistant, or see the CAD modeling chapter of the user guide (Help).").weak().small());
                }
            });
        });
    });
    if ctx.input(|i| i.pointer.any_pressed()) && !area.response.rect.contains(ctx.pointer_latest_pos().unwrap_or_default()) {
        close = true;
    }
    if let Some(f) = run {
        f(state);
        close = true;
    }
    if !close {
        state.cad.search = Some(sr);
    }
}

/// Keys of the environment that are not on the ribbon: S opens the command search; in a sketch
/// L, R, C, A and P choose a tool and Esc goes back to Select.
pub fn keys(state: &mut State, ctx: &egui::Context) {
    if ctx.text_edit_focused() || state.cad.search.is_some() || state.cad.dialog_open() || state.cad.leave_dialog {
        return;
    }
    let pressed = |k: egui::Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
    if pressed(egui::Key::S) {
        state.cad.search = Some(Search::default());
        return;
    }
    if state.cad.sketch.is_some() {
        for (k, t) in [
            (egui::Key::L, sketch::Tool::Line),
            (egui::Key::R, sketch::Tool::Rect),
            (egui::Key::C, sketch::Tool::Circle),
            (egui::Key::A, sketch::Tool::Arc),
            (egui::Key::P, sketch::Tool::Point),
        ] {
            if pressed(k) {
                tool(state, t);
            }
        }
    }
}

/// The right-click menu of the part in the 3D view: what can be done with the face under the
/// pointer. `face` is the face found there (None: the click missed the part).
pub fn face_menu(state: &mut State, ui: &mut egui::Ui, face: Option<&aurafx_pipeline::CadFace>) {
    let Some(f) = face else {
        ui.label(RichText::new("Right-click a face of the part for what can be done with it.").weak().small());
        if ui.button(format!("{} Search CAD commands  (S)", crate::icons::MAGNIFYING_GLASS)).clicked() {
            ui.close();
            state.cad.search = Some(Search::default());
        }
        return;
    };
    ui.label(RichText::new(&f.name).strong());
    ui.label(RichText::new(format!("{} face, {} mm²", f.kind, super::num_text(f.area))).weak().small());
    ui.separator();
    let flat = f.kind.eq_ignore_ascii_case("plane") || f.kind.eq_ignore_ascii_case("planar") || f.kind.eq_ignore_ascii_case("flat");
    let b = |ui: &mut egui::Ui, on: bool, label: &str, tip: &str| {
        ui.add_enabled(on, egui::Button::new(crate::icons::with(label)))
            .on_hover_text(tip)
            .on_disabled_hover_text(format!("{tip}\nOnly on a flat face; this one is curved."))
            .clicked()
    };
    if b(ui, flat, "Sketch on this face", "Draw on this face to add a boss or cut a pocket") {
        ui.close();
        sketch::begin_on_face(state, f);
    }
    if b(ui, true, "Fillet edges…", "Round edges: click them in the view") {
        ui.close();
        pick::open(state, PickKind::Fillet);
    }
    if b(ui, true, "Shell, open faces…", "Hollow the part; click this face to open it") {
        ui.close();
        pick::open(state, PickKind::Shell);
    }
    if b(ui, flat, "Pattern holes on a face…", "Holes in a row, a grid or a circle on a flat face") {
        ui.close();
        pick::open(state, PickKind::PatternFace);
    }
    if b(ui, flat, "Mirror about a face…", "Add the reflection of the part about a flat face") {
        ui.close();
        pick::open(state, PickKind::MirrorFace);
    }
    ui.separator();
    if ui
        .button(crate::icons::with("Select its feature"))
        .on_hover_text("Show the feature that made this face in the panel on the right, to change its numbers")
        .clicked()
    {
        ui.close();
        state.session.lock().cad_env_select(Some(f.feature));
    }
    if ui.button(format!("{} Search CAD commands  (S)", crate::icons::MAGNIFYING_GLASS)).clicked() {
        ui.close();
        state.cad.search = Some(Search::default());
    }
}
