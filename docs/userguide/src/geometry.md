# Geometry and meshing

auraFX builds solid geometry from short commands: primitives, booleans, outlines extruded,
revolved or swept, and imported surfaces. Each body keeps named faces, so later steps can
refer to the top of a plate or the side of a hole. This chapter covers the geometry. Meshing
it into hexahedra and other elements comes in the following releases.

## The Geometry source

Add it with **Sources › Geometry**. Its Properties show:

- **Commands**: the command list, edited in the box and applied with **Apply**.
- **Bodies**: a tree of the bodies the commands make. It shows how each body was built and
  lists its faces with their numbers and names. A mistake in a command shows here, with the
  line it is on.
- **Resolution**: cells along the longest side of each body when its surface is drawn.
- **Output**: *Surface* gives the bodies' surfaces. *Distance field* gives the signed
  distance to the nearest body on a grid (negative inside), with the body and face nearest
  each point.

The surface output carries the cell arrays `BodyId` and `FaceId`. Its field arrays are
`FaceNames` (indexed by face number), `BodyIds`, `BodyNames`, and each body's `Volume` and
`SurfaceArea`. Color by `FaceId` to see the faces.

![A plate with two holes, a revolved and rounded boss, a swept pipe and a torus, built with commands and colored by face](images/geometry_part.png)

![The Geometry source in the window: its commands, the Bodies tree with how each body was made, and the result colored by face](images/geometry_panel.png)

## Commands

Write one command per line, or separate commands with `;`. Text after `#` is a comment.
Bodies, outlines and faces are numbered from 1 in the order they are made. Sizes are in
model units and angles in degrees.

### Primitives

All primitives are centred on the origin, and round ones stand along z.

| Command | Makes |
|---|---|
| `brick x 4 y 2 z 1` (or `brick 2` for a cube) | a box; faces `x-`, `x+`, `y-`, `y+`, `z-`, `z+` |
| `sphere radius 1` | a sphere |
| `cylinder radius 0.5 height 2` | a cylinder; faces `side`, `bottom`, `top` |
| `cone radius 1 top 0.4 height 2` | a cone or frustum (top 0 for a point) |
| `torus major 2 minor 0.5` | a torus in the xy plane |
| `prism sides 6 radius 1 height 2` | a regular prism; faces `side 1` … , `bottom`, `top` |
| `superquadric x 1 y 0.8 z 0.6 e1 0.5 e2 0.7` | a superellipsoid (exponents 1 give an ellipsoid; smaller ones square it off) |
| `harmonic radius 1 coefficient 2 0 0.3 coefficient 3 2 0.1` | a star-shaped body with radius r (1 + Σ c Y<sub>lm</sub>), real spherical harmonics, for particle and grain shapes |
| `import 'part.stl' scale 0.001` | a closed surface from a file (STL, PLY, OBJ, VTK polygonal data) |

### Outlines and profile solids

An outline is a closed 2-D curve. Build it, then extrude, revolve or sweep it.

| Command | Does |
|---|---|
| `outline 1 polyline 0 0 2 0 2 1 0 1` | straight edges through the points, closed back to the start |
| `outline 1 rectangle x 2 y 1 center 0 0` | a rectangle |
| `outline 1 circle radius 1 center 0 0` | a circle |
| `outline 1 ellipse x 2 y 1` | an ellipse |
| `outline 1 regular sides 6 radius 1` | a regular polygon |
| `outline 1 spline 0 0 2 0 2 1 0 1` | a smooth closed curve through the points |
| `outline 1 add line 3 1 3 2` | straight edges appended to the outline |
| `outline 1 add arc center 0 0 radius 1 start 90 end -90` | an arc appended (counterclockwise when end > start) |
| `outline 1 fillet radius 0.2` | every corner between straight edges rounded |
| `extrude outline 1 height 2` | the outline from z = 0 to the height |
| `revolve outline 1 angle 360` | the outline turned about z; x is the distance from the axis and y the height |
| `sweep outline 1 path 0 0 0 4 0 0 4 3 0` | the outline carried along a path of points, kept normal to it |

Faces of profile solids are named after the outline's curves: `edge 1`, `edge 2`, … for
straight edges, then `arc 1`, `fillet 1`, `circle`, `spline 1`. Extrusions add `bottom` and
`top`, partial revolutions `start` and `end`, and sweeps `start` and `end`.

### Moving and combining bodies

| Command | Does |
|---|---|
| `move body 2 x 1 y 0 z 0.5` | translates |
| `rotate body 2 angle 30 about z center 0 0 0` | rotates (`about x`, `y`, `z` or a direction `1 1 0`) |
| `scale body 2 1.5` or `scale body 2 x 2 y 1 z 1` | scales about the origin |
| `unite body 1 2 3` | one body (numbered as the first) from several |
| `subtract body 2 3 from body 1` | removes bodies 2 and 3 from body 1 (add `keep` to keep them) |
| `intersect body 1 2` | the common part |
| `copy body 1` | a copy as a new body |
| `delete body 2` | removes a body |
| `body 1 name 'bracket'` | names a body |
| `face 6 name 'inlet'` | names a face |

A face keeps its name through booleans, so the faces of a hole are still `cylinder 2 side`
after the cylinder is subtracted, unless you rename them.

## Python

`afx.mesh.geometry()` builds geometry command by command. Each command is checked at once,
so a mistake raises an error naming the line.

```python
g = afx.mesh.geometry()
g.cmd("brick x 10 y 5 z 2")
g.cmd("cylinder radius 1 height 4; subtract body 2 from body 1")
for b in g.bodies():                  # id, name, history, faces, bounds
    print(b["id"], b["name"], b["faces"])
part = g.result(resolution=96)        # adds the Geometry source to the pipeline
print(g.journal())                    # every command so far
```

The Geometry source is also an ordinary source: `afx.geometry(commands="sphere radius 1")`.
Edits made in the command box are recorded in the trace (**Trace** in the Python panel), so
the journal can be replayed.

## Signed Distance Field

**Filters › Mesh › Signed Distance Field** turns a closed surface, such as an STL file or the
Geometry output, into an image grid with a `Distance` point array. The array is negative
inside. **Resolution** sets the cells along the surface's longest side, and **Margin** adds
space around it as a fraction of its size. Contour it at 0 to get the surface back, or
threshold it to keep the inside.

```python
d = afx.load("part.stl").signed_distance(resolution=128, margin=0.1)
```

## Limits

- Surfaces are drawn by contouring each body's distance field, so sharp edges are rounded
  over about one cell. Raise **Resolution** for crisper edges. The `Volume` and
  `SurfaceArea` of smooth bodies are within about 0.5 % at a resolution of 96.
- The distances of superquadrics, harmonic shapes, partial revolutions, sweeps and
  non-uniform scaling are approximate away from the surface. The surface itself is exact.
- Sweeps along sharply bent paths join the pieces with a mitre, not a rounded bend.
- Imported surfaces must be closed. Their inside is found from the direction the triangles
  face.
