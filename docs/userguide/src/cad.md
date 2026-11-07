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
- A part is one body, and sketches are made from the shapes the ribbon and Python offer
  (rectangle, circle, polygon, hexagon, profile); drawing a sketch line by line, with
  constraints, comes with the sketch canvas in a later version.
