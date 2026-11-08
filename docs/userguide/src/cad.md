# CAD modeling

auraFX has a parametric part modeler. A part is a short list of **features** (a sketch
extruded, a hole cut, a boss added) with **named dimensions**. Change a dimension and the whole
part rebuilds, and everything built on it (meshes, grains, solver cases) can be re-run. The part
keeps **named faces**, so a later step can say "the wall of the bore" instead of a number that
changes when the part does.

It sits beside the command-based Geometry source of the previous chapter. Use Geometry for
quick specimens written as text; use a CAD model when the part has dimensions you want to
change, sketches, patterns and holes.

## The CAD Model source

Add one with **Sources › CAD Model** (or the command palette, `Ctrl+K`). It starts as a plate.
Its Properties show:

- **Dimensions**: the part's named dimensions. Type a new value or an expression such as
  `width / 2` and press **Enter**; the part and everything downstream update. The value the
  expression gives is shown next to it. **reset** returns to the value stored in the part.
  A dimension changed here belongs to this node and leaves the part untouched, so two nodes
  can show two sizes of one part.
- **Configuration**: shown when the part has configurations (named sets of dimensions such as
  *small* and *large*); pick one to build that variant.
- **Features**: the part's history in order. The checkbox suppresses a feature (the part is
  rebuilt without it).
- **Faces**: every face of the result with its number and name, its volume and area.
- **Resolution** and **Output**: cells along the longest side when the surface is drawn, and
  *Surface* or *Distance field* (the signed distance to the part, negative inside).

![A flange from the templates, colored by face: the bolt holes, the counterbore and the plate each have their own faces](images/cad_part.png)

![The CAD Model in the window: Properties list the dimensions with their values, the configuration, the features and the faces](images/cad_panel.png)

## Faces and their names

The surface has the cell arrays `BodyId` and `FaceId`, and the field arrays `FaceNames`
(indexed by face number), `BodyNames`, `Volume` and `SurfaceArea`: the same layout as the
Geometry source, so everything that works on a Geometry result works on a CAD model. Color by
`FaceId` to see the faces.

A face is named after the feature that made it and its place on that feature: a brick has
`x-`, `x+`, `y-`, `y+`, `z-` and `z+`; a cylinder or a hole has `side`, `bottom` and `top`; an
extruded sketch has `bottom`, `top` and one `side N` for each line, arc or circle of the
sketch (the sketch's own order). `Bore side` is the wall of a hole named *Bore*; `Base z+` is
the top of a brick named *Base*. Rename a feature and its faces are renamed with it, which the
later *downstream check* uses to follow them.

## The CAD environment

Editing happens in one mode of the same window. Open it from **Sources › CAD Model…** (which
adds a new part and opens it), from **Edit in CAD…** (right-click a CAD Model in the Pipeline,
or the button at the top of its Properties), or from Python with `part.edit_cad()`. While it
is open:

- the toolbar becomes the **ribbon** with tabs *Sketch*, *Features*, *Modify*, *Inspect* and
  *Files*. Each button adds a feature with sensible sizes at the top of the part; you then set
  its numbers in the PropertyManager.
- the Pipeline panel becomes the **FeatureManager**: the part's dimensions, its features in
  order (the checkbox suppresses one; double-click renames; right-click for *Suppress*,
  *Roll back to here*, *Move up*, *Move down*, *Delete*), the modifiers on the whole part and
  the configurations.
- the Properties panel becomes the **PropertyManager**: every field of the selected feature.
  Each number accepts an expression of the dimensions: type `width / 2` instead of a number
  and the field follows the dimension (the value it gives is shown beside it). Choices (the
  end condition of an extrusion, the type of a hole, a mirror plane) are menus. With no
  feature selected it shows the part itself and its faces.
- the **timeline bar** at the bottom lists the features in order. The orange marker is the
  *rollback marker*: only the features before it are built, so you can look at the part as it
  was after the third feature, or insert a hole earlier in the history. ◀ ▶ move it.
- the menu bar gains a **CAD** menu, and the part is built again after every edit, in the
  background (the timeline shows the time it took and the part's volume).
- **the pipeline is paused.** Other 3D views, charts and spreadsheets stay as they were and
  say so in a grey banner; nothing downstream re-runs on every edit.

![The CAD environment: ribbon, feature tree with dimensions, the properties of a counterbored hole, the part colored by face and the timeline](images/cad_environment.png)

The right end of the ribbon leaves the environment:

- **Apply** hands the part to the CAD Model node and closes the environment; the nodes below
  it re-run. A part that does not build is refused, with the reason.
- **Apply, keep editing** does the same and stays.
- **Discard** (or `Esc`, which asks) closes it and leaves everything as it was.

All the edits of a session are **one undo step** in the main window (*Edit › Undo* after Apply
takes back the whole session). Inside the environment, *Undo* and *Redo* (`Ctrl+Z`,
`Ctrl+Y`, the timeline bar, the CAD menu) step through single edits.

The *Inspect* tab reads the part: volume, surface, size, center of volume, mass for a density,
and the area of a cross-section at a height. The *Files* tab starts from a template, opens an
`.afxcad` file, or saves the part as one.

From Python:

```python
e = part.edit_cad()            # open the environment; e is part.cad
e.box(size=(10, 10, 6), name="Boss", at=(0, 0, 11))
e.evaluate()                   # build it now (the window does this by itself)
e.rollback(1)                  # build only the first feature
e.undo(); e.redo()
e.apply()                      # or e.apply(keep_editing=True) / e.discard()
```

## What the nodes below depend on

Other nodes refer to a part by the names inside it: an Extract Set that names `Bore side`, a
mesh command that names a face, a clip plane placed inside the plate. Editing the part can
break these. So **Apply checks them first**. It compares the part as the nodes below were built
against with the part you made, finds what they refer to that changed, and opens a dialog:

![Applying the CAD changes affects 3 downstream objects: the Extract Set whose face was renamed, a clip and a slice that now fall outside the narrower plate](images/cad_check.png)

- **What changed in the part** lists faces that were renamed, replaced, removed or moved,
  dimensions added or removed, and the new bounds and volume.
- Each row is something below the part that refers to a changed thing: the object, its
  parameter, what happened to the thing it refers to (*renamed to…*, *removed*, *now outside
  the part*, *one of several*), the **default fix**, the **assistant's fix** when you asked for
  one, and a choice per row (*default*, *assistant*, *leave*).
- The default fixes are the safe ones. A renamed face is renamed where it is used, including
  inside command and expression text (whole names only: `Bore side` is not found inside `Bore
  sides`). A face that is gone but has one successor in the same place is replaced by it, and
  marked *(check)* because that is a guess. A position that fell outside the part is moved onto
  its nearest point. A face that is simply gone, or has several possible successors, has no
  default: you choose, or ask the assistant.
- **Ask the assistant** sends the changes, the conflicts and each affected node's filter and
  parameters to the assistant, which answers with a list of changes; they appear in the
  *Assistant's fix* column and the same chat shows its reasoning.
- **Apply fixes and re-run** applies the part and your choices, together, as one undo step;
  **Apply without fixing** applies the part and leaves the references (those nodes will show
  their own errors); **Back to CAD** returns; **Discard CAD changes** closes the environment
  as it was. The checkbox *re-run the nodes below now* postpones their re-run: they keep their
  old results, marked by a banner with **Run now**.

When nothing below is affected the dialog is only the question *Re-run N downstream filters?*
with **Run now**, **Later** and **Back to CAD**. A Python filter's code is searched for the
names as text: such rows say *(in code)* and have no default fix, because the name may not be
a reference.

From Python:

```python
e = part.edit_cad()
e.rename("Bore", "Pin hole")
report = e.check()             # {'diff': ..., 'conflicts': [...], 'downstream': [...], 'prompt': ...}
e.apply()                      # fix="default": applies the default fixes
e.apply(fix="none")            # leave what the check found as it is
e.apply(fix=answer_text)       # the text of an assistant's answer (a JSON list of fixes)
e.apply(run="later")           # the nodes below wait; part.cad.run_held() runs them
```

## Sketching

A sketch is a drawing on a plane that a feature extrudes or revolves. Start one from the
ribbon's **Sketch** tab: **Top (XY)**, **Front (XZ)** or **Right (YZ)**, or **Top of the part**
(the plane of the highest face, to add a boss or cut a pocket). To sketch on a flat face, select
no feature, open *Faces* in the PropertyManager and press **Sketch here** next to a face whose
normal points along x, y or z. To change an existing feature's sketch, right-click it in the
FeatureManager and choose **Edit sketch…**, or press **Edit sketch…** in its properties.

The canvas takes the place of the 3D view while a sketch is open:

![The sketch canvas on the L-bracket: its outline with the dimensions from the part's own dimensions (length, height, t), the status "Fully defined" and the constraint list](images/cad_sketch.png)

- **Draw** with the tools in the ribbon: *Line* (connected lines; right-click ends the chain),
  *Rectangle* (two corners), *Circle* (center, then a point on it), *Arc* (center, start,
  end), *Spline*, *Point*. New points snap to existing ones (which joins shapes) and to a grid
  (set in the PropertyManager). A line drawn nearly horizontal or vertical becomes so.
- **Select** shapes and points by clicking (Shift adds); drag a point to move it. Right or
  middle mouse drag pans, the wheel zooms, `Esc` ends a chain, `Del` deletes.
- **Constrain** the selection with the ribbon: horizontal, vertical, parallel, perpendicular,
  equal, tangent, concentric, coincident, midpoint, on line, on circle, symmetric, fix.
  **Dimension** it: length, radius, diameter, distance (also horizontal and vertical) and angle.
- A dimension starts at the current size. Edit it in the **Constraints** list in the
  PropertyManager: type a number, or an expression of the part's dimensions (`width / 2`,
  `3 in`), and the sketch follows the dimension from then on.
- The **solver runs after every change**. Shapes are **blue** while the sketch can still move
  (the status says how many degrees of freedom are left), **white or black** when it is fully
  defined, and **red** when constraints conflict (remove one). *Construction* turns the
  selected shapes into guides that are not part of the outline.

Finish from the PropertyManager: **Extrude** (or **Cut**, for a pocket) with a depth that may be
an expression, **reverse** to go against the plane's normal, **Revolve** about the plane's
vertical axis (draw on the Front plane with x as the radius), or **Apply sketch** when you
edited the sketch of an existing feature. The sketch then becomes a feature with named faces:
`side 1`, `side 2`… follow the order the shapes were drawn.

From Python, `e.sketch_new(plane="XY", items=[...])` and `e.sketch_edit("Bracket")` open the
canvas in the window, and `e.sketch(...)` with `e.extrude(...)` builds the same features
without it.

## Kernels, STEP and other files

A part is built by one of two kernels, chosen in the CAD Model's Properties (**Kernel**):

- **B-rep** builds *exact* solids with true faces and edges (a cylinder is a cylinder, a hole's
  wall is a real cylindrical face). It maps primitives, extruded and revolved sketches, holes,
  patterns (as copies), rounding and hollowing of a feature or of the whole part, and cap
  chamfers. This is what **STEP** is written from, and it reads imported solids.
- **Implicit** describes the part as a distance field. It builds every feature, including
  threads, lofts and sweeps and the rounded junction a feature's *blend* makes, and never fails;
  edges are not exact.
- **Auto** (the default) uses B-rep when it can build the part and Implicit otherwise. The
  result carries a field `Kernel` (which one built it) and `KernelNotes` (why B-rep could not).

Both kernels name faces the same way (`Base z+`, `Bore side`), so everything built on the names
works on either. The CAD environment shows the part with the Implicit kernel while you edit (it
is quick), and the node builds it with the kernel you chose when you apply.

**Files.** *File › Open* (and `afx.load`) opens a **`.step` / `.stp`** (AP203, AP214, AP242),
**`.igs` / `.iges`**, **`.3mf`**, a SolidCraft **`.sc`** design, or an **`.afxcad`** document as
a **CAD Model**: the file's solid becomes the base feature of a part, with its bytes kept inside
the document, so you can add features to it (cut a hole through an imported bracket) and the
part still opens on another computer. STL and OBJ keep their ordinary readers; to edit one, add
it as a CAD Model from Python.

```python
m = afx.cad_model("bracket.step")                 # a solid from a STEP file as a CAD Model
m.cad.hole(diameter=6, depth=20, at=(10, 5, 8))   # cut into it
m.cad.save("bracket2.step")                       # exact STEP (faces and edges)
m.cad.save("bracket2.3mf"); m.cad.save("part.sc") # 3MF, a SolidCraft design (also .stl, .obj)
r = m.cad.save("lofted.step")                     # {"exact": False, "note": "faceted: ..."} when
                                                  # only the Implicit kernel builds the part
```

A part that only the Implicit kernel builds is written as STEP or STL from its tessellation: a
valid solid of triangles whose header says *faceted B-rep* and why. IGES can be read but not yet
written.

## Edges, faces and measures

With the exact kernel the part has real edges, and you can work on them. In the **Modify** tab:

- **Fillet edges…** and **Chamfer edges…** open a picker in the PropertyManager that **lists the
  part's edges**, each with the two faces it lies between (`Base z+ × Bore side`), its length and
  whether it is curved. Tick the edges, set the size (a number or an expression of the
  dimensions) and press **Apply**: a *Fillet* or *Chamfer* feature is added after the features it
  modifies, so it follows them when you roll back or change a dimension.
- **Shell, open faces…** lists the faces; tick the ones to remove (none: a closed hollow body)
  and give the wall.

![The edge picker: the edges of a plate with a bored hole, each with the faces on either side](images/cad_pick.png)

These features need the **B-rep** kernel. On the Implicit kernel a picked fillet or chamfer
leaves the edges sharp, and the result says so (`KernelNotes`); a picked-face shell is approximated.
**Fillet** and **Chamfer** without a picker round every edge of the selected feature or of the
whole part.

The **Inspect** tab and Python read the part: mass (volume, surface, center, bounds, mass for a
density), the distance between two parallel flat faces and the angle between two faces, and the
volume two parts overlap when one is placed against the other:

```python
b = afx.cad_model()                                  # the default plate
b.cad.edges()                                        # [{'index': 0, 'point': [...], 'length': 30.0,
                                                     #   'faces': ['Base bottom', 'Base side 1'], ...}, ...]
b.cad.fillet_edges(("Base z+", "Base side 2"), size=2)   # the edge between two named faces
b.cad.chamfer_edges([0, 3, 5], size=1)               # edges by index (or a point on each)
b.cad.shell_faces(["Base z+"], thickness=2)          # an open box
b.cad.mass(7.85)                                     # steel
b.cad.measure_faces("Base x-", "Base x+")            # {'parallel': True, 'distance': 40.0, ...}
b.cad.interference(other, at=(12, 6, 0))             # overlap volume with another CAD Model placed there
```

## From a CAD part to a mesh and a solver case

A CAD Model feeds the meshing tools directly, and **its faces come along as named side sets**:

- **Overlay Mesh** of a CAD Model fills it with hexahedra; the mesh has one side set per CAD face
  (`Bore side`, `Base z+`, or `plate_with_hole face 3` for a solid read from a STEP file), so a
  boundary condition can name the wall of a hole. **Extract Set** shows one.
- In geometry commands, **`import 'part.step'`** (also `.stp`, `.igs`, `.3mf`, `.sc`, `.afxcad`)
  brings the part in as a body whose faces keep their names, and `mesh body 1 size 0.5 scheme
  overlay` meshes it; the Hex mesh output carries the same side sets.
- Assign Grains and the solver cases follow as for any mesh: give the set to hold and the set to
  pull by name (`fixed='plate_with_hole face 1'`, `loaded='plate_with_hole face 2'`).

![A STEP file's plate with its hole, meshed with hexahedra; the highlighted faces are the side set of one of the file's faces](images/cad_mesh.png)

```python
part = afx.load("bracket.step")                # a CAD Model
mesh = part.overlay_mesh(cell=1.0)             # hexahedra; side sets named after the faces
grains = mesh.assign_grains(grains=40, blocks=True, boundaries=True)
afx.simulation_case(grains, "moose", "case", fixed="bracket face 1", loaded="bracket face 2")
```

## Python

```python
m = afx.cad_model()                                   # a plate; or afx.cad_model("part.afxcad"),
                                                      # afx.cad_model(template="flange")
m.cad.parameter("width", "60 mm")                     # a named dimension
m.cad.hole(diameter="width / 10", depth=20, name="Bore", at=(0, 0, 12))   # cuts down from z = 12
m.cad.fillet(0.8)                                     # round the whole part
print(m.cad.volume(), [f["name"] for f in m.cad.faces()])
m.set(width=80)                                       # a dimension, without editing the part
```

Any number may be an expression of the dimensions (`"width / 2"`): the field then follows the
dimension. The calls:

| Call | What it does |
|---|---|
| `m.cad.parameter(name, expr)` | names a dimension (units allowed: `"1.5 in"`) |
| `m.cad.box`, `sphere`, `cylinder`, `cone`, `torus` | add (`op="add"`), cut (`"cut"`) or intersect (`"intersect"`) a solid; `at=(x, y, z)` and `rotate=(rx, ry, rz)` place it |
| `m.cad.hole(diameter, depth, kind=...)` | a hole cut downward from `at`: `simple`, `drilled`, `counterbore`, `countersink`, `tapped` |
| `m.cad.thread(major_diameter, pitch, length)` | a helical thread |
| `s = m.cad.sketch(plane="XY")`, then `.rectangle(w, h, center=...)`, `.circle(r)`, `.polygon(points)`, `.regular(sides, r)` | a sketch with its dimensions |
| `m.cad.extrude(s, depth, end="blind")` | extrude a sketch (`"midplane"`, `"through_all"`; `draft`, `top={"fillet": 1}`) |
| `m.cad.revolve(s, angle=360)` | revolve a sketch about its vertical axis (x is the radius) |
| `m.cad.fillet(r)`, `shell(t)`, `linear_pattern`, `grid_pattern`, `circular_pattern`, `mirror`, `split`, `scale` | modifiers on a feature (`feature="Base"`) or the whole part |
| `m.cad.suppress(name)`, `remove`, `rename`, `move` | edit the history |
| `m.cad.configuration("small", width=20)`, `activate("small")` | named variants |
| `m.cad.faces()`, `volume()`, `features()`, `parameters()` | read the result |
| `m.cad.save("part.afxcad")`, `m.cad.load(path)` | files |

Every call is one undo step, and is recorded in the Python trace.

## Files

A part is saved as an **`.afxcad`** file: readable JSON with a format version, so a later
auraFX opens an earlier file, and an earlier one says plainly that a file is from a newer
version. The state files (`.afx`) hold the part inside, so a session reopens with it.

## Meshing a part

A CAD model feeds the same tools as Geometry: **Overlay Mesh** turns its surface into
hexahedra, **Tet Mesh** into tetrahedra, **Assign Grains** and the solver cases follow.
Face names carry through: an extracted set or a boundary layer can name `Bore side`.

## Limits

- The kernel used here describes the part as a distance field. It never fails on a hard
  combination of features, but edges are not exact lines: the faces it reports are labelled
  regions of the surface, and the same labels come back after every rebuild.
- A part is one body. Sketches start on the three principal planes or on a flat face whose
  normal is along x, y or z; a face at another angle can not be sketched on yet.
