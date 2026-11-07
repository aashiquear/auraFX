# Geometry and meshing

auraFX builds solid geometry from short commands: primitives, booleans, outlines extruded,
revolved or swept, and imported surfaces. Each body keeps named faces, so later steps can
refer to the top of a plate or the side of a hole. It also makes structured hexahedral meshes:
boxes, and boxes of matrix holding inclusions whose surfaces the elements follow exactly. The
meshes carry element blocks, side sets and node sets, check their quality, and write Exodus,
Abaqus and Gmsh files for solvers. Meshing the Geometry source's bodies themselves comes in a
later release.

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

## Meshes, blocks and sets

A generated mesh is an unstructured grid of hexahedra with the cell array `ElementBlock` (the
block each element belongs to) and field arrays for the rest: `ElementBlockIds` and
`ElementBlockNames`, one `SideSet <name>` per side set (element index and Exodus side number
1 to 6 per face) and one `NodeSet <name>` per node set (node indices). Exodus files keep all
of them, as do Abaqus files (element sets, node sets and surfaces). Gmsh files keep the blocks
and side sets as physical groups. Exodus files written elsewhere read back the same way, with
their side sets and node sets.

**Filters › Mesh › Extract Set** shows one set: a side set's faces as a surface (with the
owning element's block and element variables) or a node set's nodes as points.

```python
walls = mesh.extract_set(set="x-")
mesh.save("part.e")          # or part.inp, part.msh
```

## Box Mesh

**Sources › Box Mesh** makes a box of hexahedra: its **Size**, **Origin** and **Cells** along
x, y and z, optionally **Graded**. The grading is the ratio of the largest to the smallest
interval along each axis, with the small intervals at the **Start**, the **End** or **Both**
ends. It has one block, `block_1`, and side sets and node sets `x-`, `x+`, `y-`, `y+`,
`z-` and `z+` on its faces. **Element** gives HEX8, HEX20 or HEX27 elements.

```python
b = afx.box_mesh(size=(2, 1, 1), cells=(40, 20, 20), grading=(3, 1, 1), cluster="Start")
```

## Inclusion Mesh

**Sources › Inclusion Mesh** makes a box of matrix with randomly placed inclusions, meshed with
conforming hexahedra. This is the method of hexaform, a structured mesher written by the
author of auraFX:

- **Background grid.** A grid of cubic cells covers the box. **Cells along the shortest side**
  sets the cell size.
- **Packing.** Each inclusion gets a box of whole grid cells. The boxes are placed at random,
  largest first, at least **Cells between boxes** apart and **Cells from the walls** away from
  the box faces. So anything that is placed can be meshed.
- **Blocks.** Each inclusion box is filled with a block:
  - a core filled from its six sides;
  - layers out to the exact inclusion surface, where every node lies on the surface;
  - layers out to the flat box faces, whose nodes are the grid's own.
- **Welding.** The pieces are welded exactly, so there are no hanging nodes and the elements
  fill the box exactly.

**Shape** chooses spheres, ellipsoids or superquadrics (with **Semi-axes / radius** and the
superquadric **Exponents**), or harmonic grains with a random surface of the given
**Roughness**. **Radius** and **Radius spread** set the size distribution, and **Random
orientation** turns the non-spherical shapes. **Seed** fixes the placement.

The mesh has:
- block 1 `matrix` (the grid and the outer layers) and block 100 `inclusions`, or one block per
  inclusion with **One block per inclusion**;
- side sets `x-` … `z+` on the box faces and `interface` (the matrix side of every inclusion
  surface), and node sets `x-` … `z+`;
- the element variable `particle_id`: the inclusion's number inside it, minus that number in
  its outer layers, 0 elsewhere;
- field arrays `InclusionsPlaced`, `InclusionsRequested`, `VolumeFraction` and `CellSize`.

With **Element** HEX20 or HEX27, the new nodes on the inclusion surfaces are placed on the
exact surface too. **Periodic** pairs every node on a box face with the node opposite it
(matched by their other two coordinates). It adds node sets `x-_node_set`/`x+_node_set` (and
y, z) listing the pairs in the same order, and the field arrays `PeriodicPairs x` (and y, z),
for periodic boundary conditions.

The advanced settings shape the blocks:
- the number of layers inside and outside the surface, and their grading;
- the core size and roundness;
- the box size relative to the inclusion;
- the spacing of the nodes on the surface.

The default blocks have a smallest scaled Jacobian of 1/√3 = 0.577, at the corners of the
inclusion boxes.

```python
m = afx.inclusion_mesh(size=(1, 1, 1), cells=24, count=8, radius=0.075, shape="Sphere", seed=7)
m.save("rve.e")
```

![Eight spheres meshed in a cube: the inclusion elements, and three walls of the box showing the background grid](images/inclusion_mesh.png)

## Elevate

**Filters › Mesh › Elevate** turns 8-node hexahedra into 20-node (edge midpoints) or 27-node
(also face and cell centres) ones. Neighbouring elements share their new nodes, and the
blocks, sets and element variables are kept. The new nodes lie at the midpoints. Inclusion
Mesh's own **Element** setting also puts those on the inclusion surfaces onto the surface.

## Mesh Quality

**Filters › Data › Mesh Quality** gives every cell its edge ratio (longest over shortest edge)
and smallest corner angle. For hexahedra it also adds the quality metrics as cell arrays:
`Volume`, `Jacobian`, `ScaledJacobian`, `MaxEdgeRatio`, `MaxAspectFrobenius`,
`MeanAspectFrobenius`, `Skew`, `Taper`, `Stretch`, `Diagonal`, `Dimension`, `Oddy`, `Shear`,
`Shape`, `RelativeSizeSquared`, `ShapeAndSize`, `ShearAndSize` and `Distortion`. They follow
the definitions of the Verdict library (Sandia National Laboratories), which most mesh tools
share. **Output › Summary table** gives instead one row per metric: its minimum, maximum and
mean over the hexahedra, its acceptable range and how many cells fall outside it.

| Metric | Acceptable | Perfect cube |
|---|---|---|
| ScaledJacobian | 0.5 to 1 | 1 |
| MaxAspectFrobenius, MeanAspectFrobenius | 1 to 3 | 1 |
| MaxEdgeRatio | 1 to 1.3 | 1 |
| Skew, Taper, Oddy | 0 to 0.5 | 0 |
| Stretch | 0.25 to 1 | 1 |
| Diagonal | 0.65 to 1 | 1 |
| Shear, Shape | 0.3 to 1 | 1 |
| RelativeSizeSquared, Distortion | 0.5 to 1 | 1 |
| ShapeAndSize, ShearAndSize | 0.2 to 1 | 1 |
| Volume, Jacobian, Dimension | above 0 | |

A negative Jacobian or scaled Jacobian marks an inverted element. HEX20 and HEX27 elements are
measured by their corners.

```python
q = m.mesh_quality()                              # cell arrays
t = m.mesh_quality(output="Summary table")        # one row per metric
```

![The quality summary of an inclusion mesh with harmonic grains in the Spreadsheet, and the mesh colored by scaled Jacobian](images/mesh_quality_table.png)

## Limits

- Surfaces are drawn by contouring each body's distance field, so sharp edges are rounded
  over about one cell. Raise **Resolution** for crisper edges. The `Volume` and
  `SurfaceArea` of smooth bodies are within about 0.5 % at a resolution of 96.
- The distances of superquadrics, harmonic shapes, partial revolutions, sweeps and
  non-uniform scaling are approximate away from the surface. The surface itself is exact.
- Sweeps along sharply bent paths join the pieces with a mitre, not a rounded bend.
- Imported surfaces must be closed. Their inside is found from the direction the triangles
  face.
- Inclusions must be star-shaped (one surface point along every direction from their
  centre) and cannot touch: each lies in its own box of grid cells. Very dense packings
  (beyond roughly 20 % of the volume for equal spheres) do not fit; placing collectively,
  plates with holes, dog-bone specimens and sweeping 2-D meshes come in the next release.
- The Geometry source's bodies are not meshed yet.
- Gmsh files carry no node sets.
