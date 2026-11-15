//! Help with the Geometry source's commands, where they are typed: menus that insert a command
//! with sensible numbers (so the commands need not be remembered), and a checklist of the steps
//! from bodies to a mesh a solver can read, each with a button that does it.

use crate::app::State;
use crate::menus::MenuExt;
use aurafx_pipeline::NodeId;
use egui::RichText;

/// The commands by group: (the line inserted, what it does).
const GROUPS: &[(&str, &str, &[(&str, &str)])] = &[
    (
        "Shape",
        "A solid: each new one gets the next body number",
        &[
            ("brick x 4 y 2 z 1", "A box of those sizes about the origin"),
            ("cylinder radius 0.5 height 2", "A cylinder along z"),
            ("sphere radius 1", "A ball"),
            ("cone radius 1 top 0.3 height 2", "A cone or frustum along z"),
            ("torus major 2 minor 0.4", "A ring"),
            ("prism sides 6 radius 1 height 2", "A regular prism (6 sides: a hexagonal bar)"),
            ("import 'part.stl'", "A closed surface from a file (STL, PLY, OBJ, VTK, or a CAD file such as 'part.step')"),
        ],
    ),
    (
        "Outline",
        "A flat outline and the solid made from it",
        &[
            ("outline 1 rectangle x 2 y 1", "Start outline 1 as a rectangle"),
            ("outline 1 circle radius 0.5", "Start outline 1 as a circle"),
            ("outline 1 polygon 0 0  2 0  2 1  0 1", "Start outline 1 from corner points (x y pairs)"),
            ("outline 1 fillet radius 0.2", "Round the corners of outline 1"),
            ("extrude outline 1 height 1", "A solid: the outline pulled along z"),
            ("revolve outline 1 angle 360", "A solid: the outline turned about z (x is the distance from the axis, y the height)"),
        ],
    ),
    (
        "Combine",
        "Booleans and cuts between bodies",
        &[
            ("unite body 1 2", "One body from several"),
            ("subtract body 2 from body 1", "Remove body 2 from body 1"),
            ("intersect body 1 2", "Keep the common part"),
            ("webcut body 1 with plane xplane offset 0", "Cut a body in two along a plane (blocks that mesh more regularly)"),
            ("imprint all", "Touching bodies mesh node to node on the faces they share"),
            ("copy body 1", "A copy as a new body"),
            ("delete body 2", "Remove a body"),
        ],
    ),
    (
        "Move",
        "Place a body",
        &[
            ("move body 2 x 1 y 0 z 0.5", "Translate"),
            ("rotate body 2 angle 30 about z center 0 0 0", "Turn about an axis"),
            ("scale body 2 1.5", "Scale about the origin"),
        ],
    ),
    (
        "Mesh",
        "Hexahedra (set Output to Hex mesh to see them)",
        &[
            ("mesh body 1 size 0.2", "Mesh a body with elements of about that size"),
            ("mesh body 1 size 0.2 element HEX20", "The same with quadratic elements"),
            ("mesh body 1 size 0.2 scheme mapped", "One structured block bent onto a ball-like body"),
            ("mesh body 1 size 0.2 intervals 20 8 4 bias 3 1 1", "Pieces per axis and their grading, for a brick or cylinder"),
        ],
    ),
    (
        "Names and sets",
        "What the solver's boundary conditions refer to",
        &[
            ("body 1 name 'bracket'", "Name a body (its element block gets the name)"),
            ("face 6 name 'inlet'", "Name a face (the faces are listed under Bodies below)"),
            ("sideset 1 add face 6 name 'inlet'", "A side set of the mesh faces on a geometry face"),
            ("nodeset 1 add face 1 name 'fixed'", "A node set of the nodes on a face"),
            ("block 1 add body 1 2 name 'steel'", "Several bodies in one element block"),
        ],
    ),
];

/// The row of Insert menus. Returns the command line chosen.
pub fn insert_menu(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Insert:").weak().small()).on_hover_text("Adds a command with example numbers at the end; change the numbers and press Apply");
        for (title, about, items) in GROUPS {
            ui.scroll_menu(*title, |ui| {
                ui.label(RichText::new(*about).weak().small());
                for (line, does) in items.iter() {
                    if ui.button(RichText::new(*line).monospace()).on_hover_text(*does).clicked() {
                        ui.close();
                        picked = Some(*line);
                    }
                    ui.label(RichText::new(*does).weak().small());
                }
            })
            .response
            .on_hover_text(*about);
        }
    });
    picked
}

/// The title line of a step: a tick when it is done. What follows it is the step's action.
fn tick(ui: &mut egui::Ui, done: bool, text: &str) {
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        let (mark, color) = if done { (crate::icons::CHECK_CIRCLE, crate::theme::OK) } else { (crate::icons::CIRCLE, ui.visuals().weak_text_color()) };
        ui.label(RichText::new(mark).color(color));
        ui.label(RichText::new(text).strong());
    });
}

/// The steps from bodies to a mesh for a solver, for the Geometry source `id`.
pub fn steps_ui(state: &mut State, ui: &mut egui::Ui, id: NodeId, commands: &str, output: &str, bodies: Option<&[u32]>) {
    let meshed = commands.lines().any(|l| l.trim_start().starts_with("mesh "));
    let is_mesh = output == aurafx_filters::geometry_model::OUT_MESH || output == aurafx_filters::geometry_model::OUT_TET;
    let (has_quality, error) = {
        let s = state.session.lock();
        let below = s.downstream(id);
        (below.iter().any(|n| s.nodes.get(n).and_then(|x| x.def()).is_some_and(|d| d.id == "mesh_quality")), s.nodes.get(&id).and_then(|n| n.error.clone()))
    };
    let set_output = |state: &mut State, to: &str| {
        state.session.lock().set_param(id, "output", aurafx_filters::ParamValue::Str(to.into()));
    };
    egui::CollapsingHeader::new(RichText::new("From geometry to a mesh").strong()).default_open(true).show(ui, |ui| {
        ui.vertical(|ui| {
            // 1. Bodies.
            let n = bodies.map(|b| b.len()).unwrap_or(0);
            tick(ui, n > 0, &format!("1. Bodies ({n})"));
            ui.label(RichText::new("Insert › Shape, Outline, Combine").weak().small());
            // 2. Mesh commands.
            tick(ui, meshed, "2. Say what to mesh");
            ui.horizontal(|ui| {
                if !meshed {
                    let ids = bodies.map(|b| b.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                    if ui
                        .add_enabled(n > 0, egui::Button::new("Mesh every body"))
                        .on_hover_text("Adds a mesh command for all the bodies, sized to a twentieth of the model, and shows the mesh")
                        .on_disabled_hover_text("Make a body first")
                        .clicked()
                    {
                        let size = {
                            let s = state.session.lock();
                            s.nodes
                                .get(&id)
                                .and_then(|x| x.info.as_ref())
                                .map(|i| i.bounds.diagonal() / 20.0)
                                .filter(|d| d.is_finite() && *d > 0.0)
                                .unwrap_or(0.1)
                        };
                        let line = format!("mesh body {ids} size {}", aurafx_data::info::g(size));
                        let new = format!("{}\n{line}\n", commands.trim_end());
                        let mut s = state.session.lock();
                        s.set_param(id, "commands", aurafx_filters::ParamValue::Str(new));
                        s.set_param(id, "output", aurafx_filters::ParamValue::Str(aurafx_filters::geometry_model::OUT_MESH.into()));
                    }
                } else {
                    ui.label(RichText::new("change the size in the mesh command").weak().small());
                }
            });
            // 3. Look at it.
            tick(ui, is_mesh && error.is_none(), "3. Look at the mesh");
            ui.horizontal(|ui| {
                for (label, out, tip) in [
                    ("Surface", aurafx_filters::geometry_model::OUT_SURFACE, "The bodies' surfaces (quick, while building the geometry)"),
                    ("Hex mesh", aurafx_filters::geometry_model::OUT_MESH, "The hexahedral mesh of the bodies named by mesh commands"),
                    ("Tet mesh", aurafx_filters::geometry_model::OUT_TET, "Their tetrahedral mesh"),
                ] {
                    if ui.selectable_label(output == out, label).on_hover_text(tip).clicked() && output != out {
                        set_output(state, out);
                    }
                }
            });
            // 4. Quality.
            tick(ui, has_quality, "4. Check the elements");
            if ui
                .add_enabled(is_mesh, egui::Button::new("Mesh Quality"))
                .on_hover_text(
                    "Adds a Mesh Quality filter below: color the mesh by scaled Jacobian or see a summary table with the cells outside the acceptable range",
                )
                .on_disabled_hover_text("Show the Hex or Tet mesh first (step 3)")
                .clicked()
            {
                state.session.lock().active = Some(id);
                state.add_filter("mesh_quality");
            }
            // 5. Sets.
            let sets = commands.lines().any(|l| {
                let l = l.trim_start();
                l.starts_with("sideset ") || l.starts_with("nodeset ")
            });
            tick(ui, sets, "5. Name what loads act on");
            ui.add(egui::Label::new(RichText::new("every face is a side set already; Insert › Names and sets adds your own").weak().small()).wrap());
            // 6. Save.
            tick(ui, false, "6. Save for the solver");
            if ui
                .add_enabled(is_mesh && error.is_none(), egui::Button::new("Save data…"))
                .on_hover_text("Exodus II (.e) keeps the blocks and sets; Nastran, Fluent, CGNS and VTK are there too")
                .on_disabled_hover_text("Show the Hex or Tet mesh first (step 3)")
                .clicked()
            {
                state.session.lock().active = Some(id);
                state.save_data_dialog();
            }
        });
        ui.horizontal(|ui| {
            if crate::touch::small_button(ui, "Open the guide chapter")
                .on_hover_text("Geometry and meshing in the user guide: every command with pictures")
                .clicked()
            {
                crate::welcome::open_guide(ui.ctx(), "geometry.html");
            }
            ui.label(RichText::new("or ask the assistant to write the commands").weak().small());
        });
    });
}
