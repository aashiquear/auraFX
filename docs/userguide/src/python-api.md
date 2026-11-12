# Python API reference

<!-- Generated from the aurafx module by crates/aurafx-python/tests/api_reference.rs; do not edit. -->

Every function of the `aurafx` module (`import aurafx as afx`) and every method of the objects it returns, with the text `help()` shows for it. Filters and sources are not repeated here: each is `node.<name>(...)` or `afx.<name>(...)` with the settings listed in the [filter reference](filter-reference.md). The [Python chapter](python.md) introduces the API by task.

## Functions

- `afx.active()`
- `afx.active_view(index)`: Make a 3D view active by number (0 = main view, 1, 2, ... = views added with split_view). Returns when the view is active, so `node.hide()` and the like that follow apply to it.
- `afx.animation(frames=None, time=None)`: Keyframe animation settings: number of frames, and whether time steps play along.
- `afx.animation_time(t)`: Show the keyframe animation at time t (0..1).
- `afx.annotate(text, corner='upper left', size=16.0)`: Add a text label to the 3D views. corner: "upper left", "upper right", "lower right", "upper center", "lower center".
- `afx.apply(filter_id, input=None, **kwargs)`
- `afx.apply_multi(filter_id, inputs, **kwargs)`: Add a multi-input filter: `afx.apply_multi("append_datasets", [a, b, c])`.
- `afx.ask(text, agent=False, view=False)`: Send a request to the AI assistant panel. With agent=True it runs its code and iterates; with view=True a picture of the 3D view goes with it (for models that see images).
- `afx.assistant_prompt(name)`: Put a saved prompt (by name) in the assistant's box, the selected object filled in.
- `afx.assistant_status()`: The assistant panel's state: busy (an answer is arriving), agent (agent mode is iterating), the number of messages, the last answer, the last error and the last messages as (role, text). Empty values without the app.
- `afx.cad_check(node=None)`: The downstream check of the CAD environment that is open (see node.cad.check()).
- `afx.cad_from(node, name=None)`: A CAD Model whose part starts as the solid another node shows: a Geometry source (each body apart) or any closed surface, kept as imported bodies that features add to and cut from. The node itself is left as it is and hidden. afx.cad_from(g).edit_cad() opens it in the CAD environment.
- `afx.cancel(id)`
- `afx.chart_view(node=None, y=None, x=None, style=None, data=None, log_x=None, log_y=None, title=None, view=None)`: Open a chart view, or change one: `afx.chart_view(line, y=["T", "p"], x="arc_length")`. `node` is the object it plots (None: whichever object is selected); `y` one array name or a list; `x` an array name or "index"; `style` "lines", "points", "markers" or "bars"; `data` "auto", "points" or "cells". With `view=n` the existing view n is changed (0 = the Charts panel). Returns the view's number.
- `afx.chart_views()`: The chart views: a list of dicts (view, node, x, y, style, data, log_x, log_y, title).
- `afx.clear()`
- `afx.clear_annotations()`
- `afx.clear_keyframes()`: Remove every camera keyframe and parameter track.
- `afx.clear_plots()`
- `afx.clear_selection()`
- `afx.close_chart_view(view)`: Close chart view `view` (0, the Charts panel, is hidden instead).
- `afx.colormaps()`: Names of the available color maps (built-in and user presets).
- `afx.define_colormap(name, points)`: Define (or replace) a user color map from [[t, r, g, b], ...] with values in 0..1.
- `afx.describe()`
- `afx.display(obj)`: Show something in the notebook when auraFX runs as a Jupyter kernel: a picture file (.png, .jpg, .gif, .svg), PNG bytes, or any object (as text). In the app's console it prints a line.
- `afx.export_scene(path)`: Export the visible objects as a 3D scene: `afx.export_scene("scene.glb")` (glTF binary, for Blender, three.js, PowerPoint, ...) or `.obj` (with vertex colors).
- `afx.filter(_fn=None, *, label=None, category='Plugins', description=None, params=None, inputs=1, keywords=(), id=None)`: Register a filter. The function gets an afx.Data (or a list of them when inputs != 1) and keyword parameters, and returns an afx.Data (or None after changing its input in place). inputs: 1, an exact count, or (min, max) with max None for any number.
- `afx.filters()`
- `afx.get(name)`
- `afx.gpu_benchmark(half=100)`: Time Contour and Slice with and without the GPU on a wavelet of (2 half + 1)^3 points: a list of {"filter", "cpu_s", "gpu_s"} (empty without a GPU).
- `afx.gpu_filters(on=None)`: Use the GPU for filters (Contour and Slice on big grids; the results are the same): `afx.gpu_filters(False)` turns it off; returns {"device": name or None, "on": bool}.
- `afx.gpus(host=None)`: GPUs on a host (local by default): list of dicts.
- `afx.help_filter(id)`
- `afx.hide_panel(name)`: Close a panel (it can be reopened from the Windows menu or with show_panel).
- `afx.hosts()`
- `afx.import_colormaps(path)`: Load ParaView color map presets (.json) as user presets. Returns their names.
- `afx.import_pipeline(path, input=None)`: Build a pipeline from a pipeline file: a .json pipeline (numbered steps, the layout .dream3d files record) or the pipeline inside a .dream3d file. Steps that read files open them when they exist; otherwise the pipeline starts from `input` (a node). Returns the new nodes; steps without an auraFX filter are listed in the console.
- `afx.install_ffmpeg()`: Make MP4 and WebM export work on a machine without ffmpeg: downloads a ready-made copy of the encoder (about 30 MB) into auraFX's own data folder. Does nothing when ffmpeg is already on the PATH or was downloaded before. The Animation window has the same as a button.
- `afx.job(id, tail=50)`: Status of a job (dict with state, exit_code, log tail, outputs).
- `afx.jobs()`
- `afx.key_camera(t, interp='linear')`: Keyframe the current 3D view camera at animation time t (0..1); `interp` says how it moves on to the next key: "linear", "step", "ease in", "ease out", "ease in-out", "exponential".
- `afx.layout(name)`: Window layout preset: "full", "visualize", "analyze", "script" or "simulate".
- `afx.lighting(**options)`: 3D view lighting and screen effects, e.g. `afx.lighting(shadows=True, ssao=True)`.  Options: light_kit, lights (list of dicts: direction, color, intensity, camera_relative, shadows; [] for the light kit), ambient, shadows, edl, edl_strength, ssao, ssao_strength, ssao_radius, fxaa, tone_mapping ("none", "reinhard", "aces"), exposure. `afx.lighting()` with no options returns the defaults.
- `afx.link_views(on=True)`: Link (or unlink) the cameras of all 3D views.
- `afx.load(path, host=None, processes=None, launcher=None, stream=None, **kwargs)`: Open a file. With `host=` (a host from the Simulations panel, an ssh alias or user@machine, or "local") or an `ssh://host/path` URL, the file is read on that machine by aurafx-server and filters added below it run there too. `stream=N` reads a file larger than memory in N pieces one after another (on this computer unless `host=` says otherwise).
- `afx.load_plugin(path)`: Load (or reload) a plugin file from Python: `afx.load_plugin("my_tools.py")`.
- `afx.load_remote(host, path)`: Copy a file from a host into the local cache and open it.
- `afx.load_state(path)`: Replace the pipeline with the contents of an .afx file.
- `afx.make_grid(points, cells, types)`: An unstructured grid: `afx.make_grid(points, cells=[[...], ...], types=[12, ...])` with VTK cell type numbers (10 tetra, 12 hexahedron, 5 triangle, ...); one type applies to all cells.
- `afx.make_image(dims, spacing=Ellipsis, origin=Ellipsis)`: A uniform grid: `afx.make_image(dims=(nx, ny, nz), spacing=(1, 1, 1), origin=(0, 0, 0))`.
- `afx.make_multiblock(blocks, names=None)`: A multi-block dataset from several pieces: `afx.make_multiblock([a, b], names=None)`.
- `afx.make_points(xyz)`: A point cloud (one vertex per point): `afx.make_points(xyz)`.
- `afx.make_polydata(points, polys=None, lines=None, verts=None)`: A surface: `afx.make_polydata(points, polys=[[0,1,2], ...], lines=[[...]], verts=[[...]])`.
- `afx.match_views()`: Copy the active view's camera to every other view once.
- `afx.nodes()`
- `afx.over_time(node, array, stat='max', plot=True)`: Statistic of an array over every time step, plotted in the Charts panel. stat: "max", "min", "mean", "sum" or "value" (for global/field variables). Returns (times, values).
- `afx.panel(title)`: Add a panel to the window (Windows > Plugin panels). The function gets a `ui` and is run when the panel opens and whenever a button is pressed or a value changes: ui.label(text), ui.button(text) -> True on the run its press causes, ui.slider(name, value, lo, hi), ui.number(name, value), ui.checkbox(name, value), ui.text(name, value), ui.choice(name, options, value) give the current value, ui.image(path), ui.separator().
- `afx.plot(x, y=None, label=None, title=None, xlabel=None, ylabel=None, clear=False, view=0)`: Add a line to the Charts panel: afx.plot(x, y, label="T max").
- `afx.plugins()`: Loaded plugins: [{path, filters, readers, error, enabled}].
- `afx.points_from(xyz, name='points', **arrays)`: Add a dataset built from NumPy arrays as a point cloud node: afx.points_from(xyz, temperature=t).
- `afx.probe(node, point)`: Values of every array of `node` at a point: {"name": value, ...}. Points outside the data return {}.
- `afx.reader(extensions, label=None)`: Register a reader for file extensions: a function of the path returning an afx.Data.
- `afx.redo()`
- `afx.reset_camera()`
- `afx.run()`
- `afx.run_case(node, solver, workdir, host=None, exe=None, np=1, auto_load=True, wait=False, slurm=None, **kwargs)`: Write a simulation case from a node's data (see simulation_case) and run it: on this computer in `workdir`, or on `host` (the case files are uploaded to `workdir` there). `exe` is the solver program (default: solid_mechanics-opt, spk_mpi, pinnacle), `np` the MPI processes. The results open when the run ends (auto_load). Returns the job id.
- `afx.save_animation(path, fps=10.0, orbit=None, cue=None, frames=None, width=None, height=None, views=None, poster=None, background=None)`: Record an animation: keyframes if any were added (key_camera / node.key), otherwise time steps, or a 360° camera orbit with orbit=frames. The format follows the extension: .gif, .mp4 or .webm (these two need the video encoder: `afx.install_ffmpeg()` fetches it once) or .png (a folder of frames).  With `cue`, a Python function called with the animation time t (0..1) before each frame (after the keyframes are applied), the script renders the frames itself and returns when the file is written: `afx.save_animation("a.mp4", cue=lambda t: s.set(value=t * 900))`.  `views="all"` records every visible 3D view and chart view as laid out in the window ("charts", "3d", "chart N" and "view N" as for `afx.screenshot`).  `width` and `height` set the frames' pixels (one of them keeps the view's shape) and `background` their background, as for `afx.screenshot` (a color, or `[bottom, top]` for a gradient; not "transparent"), for an animation of the active 3D view.  `poster="clip.jpg"` also writes the first frame as a picture (.jpg or .png), the still a web page shows before the video plays.
- `afx.save_state(path)`: Save the pipeline, display settings and camera to an .afx file.
- `afx.screenshot(path, width=None, height=None, transparent=False, annotations=True, views=None, controls=False, background=None)`: Save a picture of the active 3D view: `afx.screenshot("fig.png", width=1920)`. `width` and `height` set the pixels (one of them keeps the view's shape, both set the shape too: `width=1920, height=1080`). `background` is "screen" (the default), "transparent", a color ("white", "black", "#1a2b3c", (r, g, b)) or two colors `[bottom, top]` for a gradient. With `views`, a picture of several views as they are laid out in the window: "all" (every visible 3D view and chart view), "charts", "3d", "chart" (the Charts panel), "chart N" (the chart titled Chart N) or "view N"; `controls=True` keeps the charts' boxes and buttons.
- `afx.select(node, ids, assoc='cells')`: Select cells (or points) of a node by flattened index; shown highlighted in the 3D view.
- `afx.selection()`: Current selection as (node, assoc, ids), or None.
- `afx.servers()`: Servers this session talks to: name, whether connected, and the machine's details.
- `afx.set_shortcut(action=None, keys='', vim=None)`: Bind keys to an action of the window: `afx.set_shortcut("screenshot", "Ctrl+P")`, several keys with commas, "" for none; `vim=True` turns on the vim-style view keys (h j k l, + -, 0).
- `afx.set_theme(mode)`: UI theme: "dark", "light" or "system".
- `afx.show_panel(name)`: Bring a panel to the front: "3D View", "Python", "Assistant", "Simulations", "Spreadsheet", "Log", ...
- `afx.simulation_case(node, solver, folder, **kwargs)`: Write a simulation case from a node's data into `folder`: solver "moose" (a grain mesh: mesh.e with one block per grain, grain_euler.txt, grains.tex, slip_systems.txt, polycrystal.i), "spparks" (a volume with FeatureIds: grains.spparks, in.potts) or "pinnacle" (a grain mesh: mesh.e, orientations_points.csv, case.i). Options: material, axis ('x', 'y', 'z'), strain, steps, end_time, fixed and loaded (set names), mc_steps, temperature, dump_every, seed. Returns a dict: folder, files, input, command, outputs, template.
- `afx.simulation_template(name)`: Open the Simulations panel on a template: "lammps", "moose", "moose_polycrystal", "spparks", "pinnacle", "openfoam", "python", "shell" (or its label).
- `afx.snapshot(width=None, height=None, views=None)`: A picture of the active 3D view (views="all": every view as laid out) shown in the notebook.
- `afx.source(_fn=None, *, label=None, category='Plugins', description=None, params=None, keywords=(), id=None)`: Register a source: a function of keyword parameters returning an afx.Data.
- `afx.split_view(direction='right')`: Add a 3D view next to the active one: "right" (side by side) or "down" (above/below). The new view becomes active and starts with the same camera and visible objects. Returns its number once it exists (None without the app).
- `afx.submit(command, host=None, workdir=None, gpu=None, outputs=None, auto_load=False, name=None, env=None, slurm=None, wait=False)`: Run a shell command as a job (locally, on an SSH host, or via Slurm). gpu: None, "any", "nvidia", "amd", or e.g. "nvidia:0,1". Returns the job id.
- `afx.sweep(command, var, values, host=None, workdir=None, gpu=None, outputs=None, auto_load=False)`: Parameter sweep: one job per value; "{var}" in the command is replaced and each run gets its own folder <workdir>/<var>_<value>. Returns the job ids.
- `afx.time(index=None)`
- `afx.time_label(on=True)`: Show (or hide) the current time in the 3D views.
- `afx.times()`
- `afx.undo()`
- `afx.view(direction, up=None)`: View from a named direction ("+x", "-y", "iso", ...) or a vector [x, y, z] pointing from the data to the camera.

## ArrayMap

Dict-like view of one attribute set; values are NumPy copies.

- `append(value, name)`: `append(array, name)`, the same as `self[name] = array`.
- `get(name)`
- `keys()`
- `set_units(name, units)`: Set the units of an array (an empty string clears them).
- `units(name)`: The units of an array ("K", "m/s"), or None.

## ArrayParam

A parameter naming an array of the input: array=afx.ArrayParam(vector=False).


## Choice

A parameter chosen from a list: mode=afx.Choice(["fast", "accurate"]).


## Code

A multi-line text parameter edited in a code box.


## Data

A dataset: `points`, `point_data`, `cell_data`, `field_data` (NumPy), `n_points`, `n_cells`.

- `CellData` (attribute)
- `FieldData` (attribute)
- `PointData` (attribute): ParaView-style names (`output.PointData.append(array, "name")`).
- `blocks()`: The pieces of a multi-block dataset (copies), or [self] for other data.
- `bounds` (attribute): (xmin, xmax, ymin, ymax, zmin, zmax)
- `cell_data` (attribute)
- `cells` (attribute): Cells as (connectivity offsets, connectivity, VTK cell types) NumPy arrays.
- `copy()`
- `field_data` (attribute)
- `n_cells` (attribute)
- `n_points` (attribute)
- `point_data` (attribute)
- `points` (attribute): Point coordinates (N x 3, a copy). Assign to move the points of meshes with explicit points.
- `type` (attribute): "Unstructured Grid", "Polygonal Mesh", "Image Data", ...

## FilterCall

Callable returned by `node.<filter>` so that `node.slice(normal=[0,0,1])` works.


## Node

A pipeline node handle.

- `activate()`: Make this the active node (the one the Properties panel and new filters use).
- `add_array(name, values, assoc='points')`: Add a NumPy array as point (or cell) data.
- `array(name, assoc=None)`: Get an array as a NumPy array (copy). `assoc` is "points", "cells" or "field".
- `arrays()`: Array names: {"points": [...], "cells": [...]}.
- `cad` (attribute): The CAD document of a CAD Model node: `node.cad.parameter("width", 40)`, `node.cad.box(...)`.
- `color_by(name=None, assoc=None, component=None, cmap=None, range=None)`
- `delete()`
- `edit_cad()`: Open the CAD environment on this node (a CAD Model): `m.edit_cad()`.
- `hide()`
- `id` (attribute)
- `info()`: Dataset summary as a dict (points, cells, bounds, arrays).
- `inputs` (attribute): All inputs of this node: the pipeline input first, then the extra inputs of multi-input filters.
- `key(param, t, value=None, interp='linear')`: Keyframe parameter `param` at animation time t (0..1). Without `value`, the current value is used. Display properties work too: "opacity", "point_size", "line_width", "solid_color", "edge_color", "lic_strength", "volume_opacity", "volume_threshold". `interp` says how the value moves on to the next key ("linear", "step", "ease in", "ease out", "ease in-out", "exponential").
- `name` (attribute)
- `params` (attribute)
- `pipeline_json()`: The pipeline that makes this node, as JSON text (the form .dream3d files record).
- `plot(*y, x=None, style=None, data=None, log_x=None, log_y=None, title=None)`: Plot this object's arrays in a new chart view: `line.plot("T", "p", x="arc_length")`. Returns the view's number (afx.chart_view changes it later).
- `points` (attribute): Point coordinates as an (N, 3) NumPy array.
- `save(path, all_times=False, pieces=4)`: Write this node's data: .vtu .vtp .vti .vtr .vts .vtm .vtk .vtkhdf .exo .case (EnSight Gold) .xmf (Xdmf + .h5) .cgns .csv .stl .ply .dream3d (with the pipeline) .spparks .tex .inp (Abaqus) .msh (Gmsh) .cas (Fluent) .bdf (Nastran) .mha/.mhd (MetaImage) .png/.tif (image stacks) .pvtu/.pvtp/.pvti (parallel pieces; pieces=4) .pvd (format from the extension). all_times=True writes every time step: into one Exodus, VTKHDF or CGNS file (CGNS keeps one mesh), or as an EnSight, Xdmf or .pvd series.
- `save_pole_figures(folder, families='100, 110, 111', projection='Stereographic', sigma=7.5, size=256, phase=0, structure='', mask='')`: Pole figures of every phase as PNG pictures in a folder (phase1_100.png, ...): sample x to the right, y up, each phase's MRD from 0 (dark blue) to its largest value (red). Returns the files written with their largest MRD.
- `send_to_view(view, keep=False)`: Show this object in another 3D view. `view` is a view number (0 = main view) or "right"/"down" to open a new view holding only this object. By default it is moved (hidden in the other views); keep=True shows it there as well.
- `set(**kwargs)`: Set parameters / display options: node.set(normal=[0,0,1], opacity=0.5).
- `set_inputs(*nodes)`: Replace the extra inputs of a multi-input filter: `app.set_inputs(b, c)`.
- `show()`
- `update()`: Run the pipeline up to this node and return the dataset summary.
- `watch(on=True, follow_latest=True)`: Reload automatically when the file changes (live view of a running simulation).

## node.cad (the CAD document of a CAD Model)

The parametric document of a CAD Model node. Numbers can be expressions of the document's parameters ("width / 2"): the geometry then follows the parameter.

- `activate(name=None)`
- `apply(keep_editing=False, fix='default', run='now')`: Inside the CAD environment: hand the part to the node and leave (keep_editing=True stays), after the downstream check. fix: "default" (apply the default fixes), "none" (leave what the check found as it is), a list of {"node", "param", "value"} fixes, or the text of an assistant's answer (a JSON list). run: "now", or "later" (the nodes below wait for run_held()). Returns {"features", "fixes", "held", "kept_editing"}. Outside the environment: bring the node up to date and return its summary.
- `ask_apply(keep_editing=False)`: What the Apply button does in the window: run the downstream check and open its dialog when the nodes below are affected (only useful from a script running in the app).
- `bind(feature, path, expr)`: Drive a numeric field of a feature by an expression; path is a JSON pointer such as /shape/size/0.
- `box(size=(10, 10, 10), name=None, op='add', at=(0, 0, 0), rotate=(0, 0, 0), corner_radius=0, blend=0)`
- `chamfer_edges(edges, size=1, name=None)`: Chamfer picked edges (see fillet_edges).
- `check()`: The downstream check of the changes made in the environment: what changed, the nodes below the part, the conflicts (node, param, reference, what happened, the default fix) and the prompt to ask an assistant for more fixes.
- `check_fit()`: Run the Assemble tab's Check fit and show the overlaps. In the window only.
- `circular_pattern(count, total_deg=360, axis='Z', feature=None)`
- `clear(name='Part')`
- `component_add(name=None, template=None, file=None, document=None, at=(0, 0, 0), rotate=(0, 0, 0))`: Place another part: a template ('flange'), a CAD file (.afxcad, .step, .igs, .3mf, .sc), a document (a dict) or a blank part to model in with component_op(). at / rotate (degrees) place it.
- `component_interference(resolution=0.5)`: The pairs of parts that overlap, with the overlap volume in mm3.
- `component_move(component, at=None, rotate=None, by=None)`: Place a component at a position (and rotation in degrees), or move it by an offset.
- `component_op(component, op, **args)`: Edit a component's own part with any operation of this API, e.g. component_op('Plate', 'add', kind='box', size=[40, 40, 5]).
- `component_remove(component)`
- `component_rename(component, to)`
- `component_visible(component, visible=True)`
- `components()`: The placed parts: name, position, rotation, visibility.
- `cone(r_bottom=5, r_top=0, height=10, name=None, op='add', at=(0, 0, 0), rotate=(0, 0, 0), blend=0)`
- `configuration(name, **overrides)`
- `cylinder(radius=5, height=10, name=None, op='add', at=(0, 0, 0), rotate=(0, 0, 0), blend=0)`
- `discard()`: Leave the environment without applying.
- `document()`
- `draft(faces, angle=3, neutral_origin=(0, 0, 0), neutral_normal=(0, 0, 1), name=None)`: Taper side faces (names from faces(), or points) by `angle` degrees about a neutral plane they stay fixed on. The exact kernel builds it; the implicit one leaves the faces vertical and says so.
- `edges()`: The edges of the part as the exact (B-rep) kernel builds it: index, a point on the edge, length, straight, and the names of the faces on either side. Raises when the exact kernel cannot build it.
- `edit(op, args=None, **kw)`
- `edit_component(name=None)`: Inside the CAD environment: work on one component's own part. Every call of this object (box, hole, extrude, features, faces, ...) then acts on that part, shown alone in its own coordinates; edit_component(None) goes back to the assembly. Undo is the assembly's.
- `editing()`: True while the CAD environment is open on this node.
- `evaluate(resolution=64, kernel='Auto')`: Build the part now: volume, area, centroid, bounds, the faces (name, kind, area, centroid, direction), the kernel that built it and notes on what it could not build exactly.
- `extrude(sketch, depth=10, name=None, op='add', end='blind', draft=0, reverse=None, at=(0, 0, 0), top=None, bottom=None)`: Extrude a sketch along its plane's normal. end: blind, midplane or through_all. top / bottom: {"fillet": r} or {"chamfer": d} on the cap edges. reverse: against the normal (left out, a sketch on a face grows out of the part, and a cut goes into it).
- `faces(resolution=64)`
- `features()`
- `fillet(radius, feature=None)`
- `fillet_edges(edges, size=1, name=None)`: Round picked edges (exact kernel): edges are indices from edges(), points on the edges, or a pair of face names ('Base z+', 'Bore side') for the edge between them.
- `grid_pattern(counts, step, feature=None)`
- `hole(diameter=5, depth=10, kind='simple', name=None, at=(0, 0, 0), rotate=(0, 0, 0), **kw)`: A hole cut downward (-z) from `at`: kind simple, drilled, counterbore (cb_diameter, cb_depth), countersink (cs_diameter, cs_angle) or tapped (pitch).
- `info()`
- `interference(other, at=(0, 0, 0), rotate=(0, 0, 0), resolution=0.5)`: The overlap volume with another CAD Model node placed at `at` (and rotated by `rotate` degrees).
- `linear_pattern(count, step, feature=None)`
- `load(path)`: Replace the document by a file: .afxcad, or a solid (.step/.stp, .igs, .3mf, .sc, .stl, .obj) as the base feature of a new document.
- `loft(sections, name=None, op='add', at=(0, 0, 0))`: Loft through the closed outlines of two or more sketches on parallel planes at different offsets (names of kept sketches, or sketches).
- `mass(density=7.85)`: Volume, surface, center, bounds and mass (density in g/cm3) of the built part.
- `mate(kind, a, b=None, value=None, flip=False, name=None)`: A mate between two parts by a face of each, 'Component/Face' (faces() lists them; a face of the assembly's own part is 'Face'; 'XY', 'XZ', 'YZ' are the assembly's planes). kind: coincident (two flat faces meet; flip=True: flush, looking the same way), concentric (two round faces on one axis), distance (flat faces value mm apart), angle (value degrees: 0 as coincident leaves them) or fixed (the part a stays put: mate('fixed', 'Base')). value may be an expression of the assembly's dimensions. Returns {"mate", "dof", "status"}.
- `mate_remove(mate)`
- `mate_set(mate, value=None, flip=None, suppressed=None)`: Change a mate: its distance or angle (this moves the assembly), flip, or switch it off.
- `mates(resolution=48)`: The mates as the assembly builds now: name, kind, a, b, value, and "error" (why one is not met, else None); with "dof" on each: the ways the mated parts can still move.
- `measure_faces(a, b)`: The distance between two parallel flat faces (or their centers), and the angle between them.
- `mirror(plane='YZ', feature=None)`
- `mirror_about(origin, normal, name=None)`: Mirror the part so far about a plane, e.g. a flat face: origin is a point on it (or a face name from faces()), normal its direction. The reflection is joined to the part.
- `modifier(kind, feature=None, **kw)`
- `move(feature, to)`
- `open_picker(kind='fillet')`: Open the edge or face picker in the window: fillet, chamfer, shell, draft, mirror, pattern, or mate (two faces of two parts).
- `parameter(name, expr, comment='')`
- `parameters()`
- `pick_choose(label)`: Tick an item of the open picker by its label (a face's name), as a click on it does. In the window only.
- `pick_face_for(feature)`: Open the list of flat faces a feature's sketch can be put on (the model tree's 'Pick another face…'). In the window only; sketch_face() does it without the window.
- `placements(resolution=48)`: Where the mates put every component: name, at, rotate (degrees).
- `plane(name=None, from_plane='XY', offset=0, angle=0, about='x', points=None, origin=None, normal=None, x_axis=None)`: A reference plane to sketch on: a principal plane (XY, XZ, YZ) or another reference plane moved by offset and turned by angle about its x or y axis; or through three points; or by origin and normal. Sketch on it with sketch(plane=name).
- `plane_form()`: Open the New reference plane form in the window.
- `redo()`
- `remove(feature)`
- `remove_parameter(name)`
- `remove_plane(name)`
- `rename(feature, to)`
- `revolve(sketch, angle=360, name=None, op='add', at=(0, 0, 0))`: Revolve a sketch about its local y axis (on the XZ plane: the world z axis); x is the radius.
- `rollback(features=None)`: Build only the first `features` features (None: all), like the timeline's rollback marker.
- `run_held()`: Run the nodes a postponed apply held back.
- `save(path, kernel='Auto')`: Write the part by the file's extension: .afxcad (the document), .step/.stp, .3mf, .stl, .obj or .sc (SolidCraft). Exact faces and edges when the B-rep kernel builds the part; otherwise a faceted tessellation (the result says which). Returns {"bytes", "exact", "note"}.
- `scale(factor, feature=None)`
- `select(feature=None)`
- `select_component(name)`: Select a component in the feature tree (it shows its position and turn). In the window only.
- `set(feature, path, value)`
- `shell(thickness, feature=None, open_top=None)`
- `shell_faces(faces=(), thickness=2, name=None)`: Hollow the part so far with this wall, removing the picked faces (names from faces(), or points); no faces: a closed hollow body.
- `show_tab(name)`: Show a tab of the CAD ribbon (Sketch, Features, Modify, Inspect, Files) in the window.
- `sketch(plane='XY', offset=0, face=None)`: A sketch to draw in: on a plane (XY, XZ, YZ or a reference plane's name, moved by offset), or on a flat face of the part by its name (face="Plate z+"); such a sketch stays on its face when the part changes. See the sketch's own calls: ready shapes, or lines with constraints.
- `sketch_add(sketch, name=None)`: Keep a sketch with the part under a name (Sketch 1, ...): the profile or the path of a sweep, a section of a loft. Returns {"sketch": name}.
- `sketch_cursor(x, y, line_from=None)`: For pictures: where the sketch canvas takes the cursor to be (sketch units) when the mouse is elsewhere, optionally drawing a line from point line_from, so snaps and guides show.
- `sketch_dim_edit(index='first')`: Open the value editor of the sketch's dimension number index (as a double-click does); "first": the first dimension.
- `sketch_edit(feature)`: Open the sketch canvas on the sketch of an extruded or revolved feature.
- `sketch_face(feature, face=None)`: Put the sketch of an extruded or revolved feature on a face of the part (by name): it follows that face from then on. face=None lets go of the face; the sketch stays where it is.
- `sketch_new(plane='XY', offset=0, items=None)`: Open the sketch canvas (in the window) on a new sketch on a plane, optionally with ready shapes (the items of cad.sketch()). Only useful from a script running in the app.
- `sketch_on_face()`: Open the face picker to start a sketch on a flat face of the part (in the window).
- `sketch_open(name)`: Open the sketch canvas on a sketch kept with the part. In the window only.
- `sketch_remove(name)`
- `sketch_rename(name, to)`
- `sketch_set(name, sketch)`: Replace a kept sketch; the sweeps and lofts that use it follow.
- `sketches()`: The sketches kept with the part: name, plane, closed outlines, open paths.
- `sphere(radius=5, name=None, op='add', at=(0, 0, 0), rotate=(0, 0, 0), blend=0)`
- `split(plane='XY', offset=0, keep_positive=True, feature=None)`
- `state()`
- `suppress(feature, suppressed=True)`
- `sweep(profile, path, twist=0, name=None, op='add', at=(0, 0, 0))`: Sweep the closed outline of one sketch along the open path of another. profile and path are names of kept sketches (sketch_add) or sketches (kept as '<name> profile' and '<name> path'). Draw the profile on a plane the path leaves at right angles, where the path starts. twist: degrees turned over the whole path. op: add or cut.
- `template(name)`: Start from a template: enclosure, flange, knob, threaded rod, vase or bracket.
- `thread(major_diameter=8, pitch=1.25, length=10, internal=False, name=None, op='add', at=(0, 0, 0))`
- `torus(major=10, minor=2, name=None, op='add', at=(0, 0, 0), rotate=(0, 0, 0), blend=0)`
- `undo()`
- `volume(resolution=64)`

## A sketch description (node.cad.sketch)

A sketch: ready shapes (rectangle, circle, polygon, regular, path), or lines, circles and arcs drawn one by one and tied by constraints and dimensions. A dimension's value is a number or an expression of the part's dimensions ("width / 2"). Pass it to extrude(), revolve(), sketch_add(), sweep() or loft().

- `add_circle(center, r, construction=False)`: A circle to constrain (circle() adds a ready, dimensioned one). Returns the circle.
- `angle(a, b, value)`: The angle between two lines, in degrees.
- `arc(center, start, end, construction=False)`: An arc counter-clockwise from start to end around center.
- `circle(r, center=(0, 0))`
- `coincident(p, q)`: Two points in one place (the end of one line on the start of another).
- `concentric(a, b)`
- `diameter(circle, value)`
- `dimension(a, b=None, value=None)`: The same as distance().
- `distance(a, b=None, value=None)`: A driving dimension: distance(line, 40) is the line's length, distance(p, q, 40) the distance between two points, distance(p, line, 40) from a point to a line. The value may be an expression of the part's dimensions: distance(l, "width").
- `equal(a, b)`: Two lines of one length, or two circles of one radius.
- `fix(point)`
- `hdistance(p, q, value)`: The distance between two points along the sketch's x.
- `horizontal(line)`
- `length(line, value)`
- `line(a, b, construction=False)`: A line between two points: (x, y) pairs, or points of other entities (l.end) to share a corner.
- `lines(points, closed=True)`: Lines through the points, sharing their corners (closed: back to the first). Returns the lines.
- `midpoint(point, line)`
- `parallel(a, b)`
- `path(points)`: Lines joined end to end and left open: the path of a sweep.
- `perpendicular(a, b)`
- `point(x, y, fixed=False)`: A point; fixed=True pins it where it is.
- `point_on(point, entity)`: A point on a line (anywhere along it) or on a circle or arc.
- `polygon(points)`
- `radius(circle, value)`
- `rectangle(w, h, corner=None, center=None)`
- `regular(sides, r, center=(0, 0))`
- `solve()`: Solve the constraints now: {"status": "fully defined" \| "under-defined" \| "over-defined" \| "failed", "dof": degrees of freedom left, "converged", "outlines": closed outlines, "paths": open ones}. The points' .xy are then where the constraints put them. (extrude() and the others solve by themselves; this is to check a sketch first.)
- `spec()`
- `symmetric(p, q, line)`: Two points mirror images of each other in a line.
- `tangent(a, b)`: A line touching a circle or an arc, or two of those touching.
- `vdistance(p, q, value)`: The distance between two points along the sketch's y.
- `vertical(line)`

## afx.mesh (geometry commands)

Geometry and meshing. afx.mesh.geometry() starts geometry built from commands.

- `geometry(commands='')`

## afx.mesh.geometry() (a list of commands)

Geometry built from commands. g.cmd('brick x 2 y 1 z 1') adds commands (checked at once), g.bodies() lists the bodies and their faces, g.journal() returns every command, g.result(resolution=64, output='Surface') adds the Geometry source to the pipeline.

- `bodies()`
- `cmd(text)`
- `journal()`
- `result(resolution=64, output='Surface')`

## ui (inside an @afx.panel function)

- `button(text)`
- `checkbox(name, value=False)`
- `choice(name, options, value=None)`
- `image(path)`
- `label(text)`
- `number(name, value)`
- `separator()`
- `slider(name, value, lo, hi)`
- `text(name, value='')`
