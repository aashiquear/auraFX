//! The welcome window: what auraFX is for and one button into each kind of work, shown at the
//! first start (and from Help › Welcome). Also the way to open the user guide.

use crate::app::{LayoutAction, Pane, State};
use egui::RichText;

/// Open a chapter of the user guide in the browser: the copy beside the program when there is
/// one (a source checkout), else the published one.
pub fn open_guide(ctx: &egui::Context, page: &str) {
    let local = std::env::current_exe().ok().and_then(|exe| {
        exe.ancestors().skip(1).take(4).map(|d| d.join("docs/userguide/book").join(page)).find(|p| p.exists()).or_else(|| {
            // Installed: share/doc/aurafx beside bin/.
            exe.parent().and_then(|b| b.parent()).map(|r| r.join("share/doc/aurafx/userguide").join(page)).filter(|p| p.exists())
        })
    });
    let url = match local {
        Some(p) => format!("file://{}", p.display()),
        None => format!("https://aashiquear.github.io/auraFX/docs/userguide/book/{page}"),
    };
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// Start a new CAD part and open the CAD environment on it.
pub fn new_cad_part(state: &mut State) {
    state.add_filter("cad_model");
    let ok = {
        let s = state.session.lock();
        s.active.is_some_and(|a| s.is_cad_model(a))
    };
    if ok {
        crate::cad::enter_active(state);
    }
}

/// Add a Geometry source (the command language) and show its Properties, where the commands
/// and the steps to a mesh are.
pub fn new_geometry(state: &mut State) {
    state.add_filter("geometry");
    state.layout_actions.push(LayoutAction::Show(Pane::Properties));
    state.status = "Geometry: its commands are in Properties, with Insert menus and the steps from bodies to a mesh".into();
}

struct Card {
    title: &'static str,
    text: &'static str,
    actions: &'static [(&'static str, &'static str, fn(&mut State))],
}

const CARDS: &[Card] = &[
    Card {
        title: "View/analyze results",
        text: "Open what a solver wrote or an experiment measured (Exodus, VTK, OpenFOAM, LAMMPS, CGNS, NetCDF, EBSD scans, image stacks and more), then cut, contour, plot, measure and animate it. Right-click an object, in the Pipeline or in the 3D view, for what fits it.",
        actions: &[
            ("Open a file…", "Ctrl+O, or drop a file on the window", |s| s.open_file_dialog()),
            ("Try a sample", "A small simulation result to play with", |s| s.open_sample("disk_out_ref.ex2")),
        ],
    },
    Card {
        title: "Build/model a part",
        text: "A parametric CAD modeler: sketch an outline, extrude it, add holes and fillets, change a dimension and the part rebuilds. STEP in and out.",
        actions: &[("New CAD part", "Opens the CAD environment on an empty part, with the next steps shown at the top left", new_cad_part)],
    },
    Card {
        title: "Build a mesh",
        text: "Solids from short commands, then hexahedra or tetrahedra with named blocks and side sets, checked for quality and saved for a solver.",
        actions: &[("New geometry", "Adds a Geometry source: its Properties have Insert menus for the commands and the steps to a mesh", new_geometry)],
    },
    Card {
        title: "Synthesize crystal",
        text: "Synthetic microstructures: equiaxed or columnar grains, or grains packed from size and shape statistics, with orientations and texture; then meshes and solver cases from them. EBSD scans get the same grain analysis.",
        actions: &[
            ("Voronoi grains", "A box of equiaxed grains with orientations; its Properties set the number, sizes and texture", |s| s.add_filter("voronoi_grains")),
            ("Columnar grains", "Grains grown along z, like additively manufactured or directionally solidified material", |s| s.add_filter("columnar_grains")),
            ("From statistics", "Pack Grains: goal size and shape distributions, edited with live plots under View > Goal Statistics", |s| {
                s.add_filter("pack_grains");
                s.goal_stats.open = true;
            }),
        ],
    },
    Card {
        title: "Work on big or remote data",
        text: "Leave the data on the cluster: a server there reads and filters it, on several processes if you like, and only what is drawn comes back. Files larger than memory are read in pieces.",
        actions: &[("Open on another machine…", "Add a machine over SSH and open a file on it", |s| s.server.open = true)],
    },
    Card {
        title: "Run and automate",
        text: "Start solvers here, on another machine over SSH or through Slurm; script everything in Python or a notebook; or tell the assistant what you want to see.",
        actions: &[
            ("Simulations", "Machines, jobs and templates", |s| s.layout_actions.push(LayoutAction::Show(Pane::Jobs))),
            ("Ask the assistant", "It sees your pipeline and writes the code", |s| s.assistant.focus_input = true),
        ],
    },
];

pub fn window(state: &mut State, ctx: &egui::Context) {
    if !state.show_welcome {
        return;
    }
    let mut open = true;
    let mut run: Option<fn(&mut State)> = None;
    egui::Window::new("Welcome to auraFX")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(640.0)
        .show(ctx, |ui| {
            ui.label("One window for viewing and analyzing simulation results and experimental data, modeling parts, meshing them, synthesizing microstructures and running solvers. Pick where to start:");
            ui.add_space(4.0);
            egui::Grid::new("welcome cards").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                for (k, c) in CARDS.iter().enumerate() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        // A grid cell lays its contents out in a row: stack them.
                        ui.vertical(|ui| {
                            ui.set_width(300.0);
                            ui.set_min_height(132.0);
                            ui.label(RichText::new(c.title).strong().size(15.0));
                            ui.add(egui::Label::new(c.text).wrap());
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                for (label, tip, f) in c.actions {
                                    if ui.button(*label).on_hover_text(*tip).clicked() {
                                        run = Some(*f);
                                    }
                                }
                            });
                        });
                    });
                    if k % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new("Also here: volume rendering and ray tracing, charts and spreadsheets, pole figures and texture, keyframe animation, pictures, videos and web pages of a scene, plugins in Python and Rust.")
                        .weak(),
                )
                .wrap(),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("Good to know").strong());
            for tip in [
                "Ctrl+K searches every filter, source and action; what fits the selected object comes first.",
                "Right-click an object (in the Pipeline or the 3D view) for suggestions, and hover over anything for what it does.",
                "In the CAD environment the top left says what to do next, and S searches its commands.",
                "Everything you do with the mouse can be turned into a Python script (Python panel › Trace).",
            ] {
                ui.label(format!("•  {tip}"));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("User guide").on_hover_text("Every feature with pictures, in your browser").clicked() {
                    open_guide(ui.ctx(), "index.html");
                }
                if ui.button("Keyboard shortcuts").clicked() {
                    state.show_shortcuts = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.checkbox(&mut state.settings.show_welcome, "Show at start").on_hover_text("Also under Help › Welcome").changed() {
                        state.settings.save();
                    }
                });
            });
        });
    if let Some(f) = run {
        f(state);
        open = false;
    }
    state.show_welcome = open;
}

/// After a crash: say so once, where the report is and how to get the work back.
pub fn crash_window(state: &mut State, ctx: &egui::Context) {
    let Some(report) = state.crash_report.clone() else { return };
    let mut open = true;
    let mut close = false;
    egui::Window::new("auraFX closed unexpectedly last time")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Sorry about that. What went wrong was written to a file on this computer (nothing was sent anywhere):");
            ui.add(egui::Label::new(RichText::new(report.display().to_string()).monospace().small()).wrap());
            ui.label("Attaching it to a bug report helps to fix the cause.");
            ui.horizontal(|ui| {
                let last = State::last_session_path();
                if last.exists() && ui.button("Restore last session").on_hover_text("The pipeline as it was shortly before").clicked() {
                    state.load_state(last);
                    close = true;
                }
                if ui.button("Copy the path").clicked() {
                    ui.ctx().copy_text(report.display().to_string());
                }
                if ui.button("Report a bug").on_hover_text("Opens the project's issue page in your browser").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/aashiquear/auraFX/issues/new"));
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
    if close || !open {
        state.crash_report = None;
    }
}
