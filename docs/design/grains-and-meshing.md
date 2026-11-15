# Grain workbench and mesh generator: design

The design of ROADMAP.md sections 11 and 12, as built. One window takes a user from a geometry
to a mesh, from a mesh to a polycrystal on it, from the polycrystal to a solver run (MOOSE,
SPPARKS, PINNACLE, JAX-FEM, DAMASK, LAMMPS; local or over SSH), and from the run's output back
to grain statistics, texture plots and pictures:

1. a **grain workbench**: EBSD and synthetic microstructures, grain segmentation and
   statistics, texture, boundaries and mechanics as filters, sources, readers, writers and
   charts, for FCC, BCC and HCP crystals;
2. a **geometry and mesh generator**: implicit geometry with structured and overlay-grid
   all-hex meshing, tetrahedra, quality control and sets, as sources and filters with a small
   command language.

Both use what the rest of auraFX uses: the `FilterDef` registry (one definition drives the
menus, the Properties panel, Python and the AI assistant), the ordinary dataset types, the
Exodus and VTK writers, the job templates and the charts. The window has two menu categories
for them (**Filters › Grains** and **Filters › Mesh**) and their sources under **Sources**. The
user-facing description is in the guide chapters *Grains and crystals*
and *Geometry and meshing*; how each part was checked is in `docs/reference/verification.md`.

## 1. Crates

```
crates/
  aurafx-crystal   orientation mathematics: quaternions, Euler/Rodrigues/axis-angle/matrix and
                   homochoric vectors, the 11 Laue groups' operators (built by closure from
                   their generators), disorientation, fundamental zones, IPF and other colour
                   keys, pole figures, ODF/MDF kernels, slip systems, Schmid and Taylor factors,
                   elastic constants and a materials table for FCC, BCC and HCP (no dependency
                   on the rest of auraFX)
  aurafx-mesh      geometry kernel (implicit shapes, CSG, 2-D outlines, extrude/revolve/sweep,
                   closed surfaces as signed distance) and the meshers (structured and mapped,
                   O-grid macro blocks, overlay grid, tetrahedra), quality metrics, sets and
                   blocks, the command language
  aurafx-filters   the Grains and Mesh filters and their sources, registered like every
                   other one (crystal.rs, features.rs, crystal_texture.rs, synthetic.rs,
                   grain_surface.rs, mechanics.rs, am.rs, mesh_gen.rs, geometry_model.rs)
  aurafx-io        readers and writers: .ang, .ctf, .h5ebsd, EDAX/Bruker .h5, .dream3d, .tex,
                   SPPARKS sites, Abaqus .inp, Gmsh .msh, Nastran, Fluent
  aurafx-pipeline  pipeline_json.rs (the chain of steps that made a node as JSON, and pipelines
                   rebuilt from such files) and cases.rs (solver cases written from a node's data)
  aurafx-jobs      the case templates, with the mesh and orientation files filled in
```

## 2. Data conventions

**Microstructures.** A microstructure is an ordinary `ImageData` (or any grid) whose cell data
holds `FeatureIds`, `Phases` and the orientations (`EulerAngles`, `Quats`; radians, Bunge,
passive, vector-scalar order). Per-phase (ensemble) arrays travel as **field data** of the grid:
`CrystalStructures` (a Laue code per phase, entry 0 = not indexed, 999 unknown),
`MaterialName` and `LatticeConstants` (6 per phase). Per-feature arrays are field data too: one
row per feature id, row 0 for id 0, recognised by that length. The Feature Table filter turns
them into a `Table` for the Spreadsheet, the charts and CSV. Field data passes through every
filter, the state file and the server protocol, so no new dataset type or protocol message is
needed, and every other filter, the Spreadsheet and Color by work unchanged on these datasets.
The `.dream3d` writer maps them to that format's groups.

EBSD readers keep the Euler angles as stored (no reference-frame change), apart from the
optional +30° on φ2 of hexagonal `.ctf` phases; Rotate Orientations re-expresses them in
another frame. 8-bit arrays with 3 or 4 components (orientation colors) are drawn as their own
colors, with a Display switch to use the color map instead.

**Meshes.** A generated mesh is an `UnstructuredGrid` with element blocks (an `ElementBlock`
cell array plus block names in field data, as the Exodus reader and writer use them), side sets
and node sets, and element variables (`grain_id`, `particle_id`, ...). Hexahedral and
tetrahedral meshes share the model; HEX20 and HEX27 elevation shares mid nodes and projects
them onto the surface. Mesh Quality has the full hexahedron and tetrahedron metric sets with a
summary table of acceptable ranges.

**Geometry.** Bodies are implicit (signed distance) solids. The Geometry source draws each
body by contouring its distance field and snapping the points onto the exact surface; sharp
edges are rounded over a cell in the drawing only (the meshers work on the exact geometry).
Faces are tagged by the primitive feature nearest each point and keep their names through
booleans, so a face name becomes a side set name.

## 3. The pipeline

Geometry and mesh steps are sources and filters:

- **Geometry** (source): the whole command list is one parameter, edited in a command box with
  menus that insert commands and a list of the steps from bodies to a mesh; outputs the
  surface, a hexahedral mesh or a tetrahedral mesh.
- **Box Mesh, Plate Mesh, Dog-bone Mesh, Inclusion Mesh** (sources): structured hexahedral
  meshes of standard specimens and of a cube with packed inclusions.
- **Overlay Mesh, Tet Mesh, Face Mesh, Remesh Surface, Sweep Quads, Refine Mesh, Refine Region,
  Smooth Mesh, Boundary Layers, Elevate, Convert Elements, Tetrahedra in a Region, Define Mesh
  Set, Merge Nodes, Extract Set, Mesh Quality** (filters).
- **Assign Grains** puts a microstructure on a mesh (Voronoi or Laguerre seeds, the nearest
  point of a point set or table, or the feature of a microstructure volume; `grain_id` and
  orientations per element, a block per grain, side sets at grain boundaries). **Overlay Mesh**
  on `FeatureIds` goes the other way and meshes a microstructure, one block per grain.
- The simulation templates take the pipeline's mesh and orientation files as inputs; results
  are reopened under the same pipeline, where the Grains filters run on solver output as well
  as on EBSD.

## 4. The command language

`afx.mesh.geometry()` returns a builder (`cmd`, `bodies`, `journal`, `result`); the same
commands are the Geometry source's `commands` parameter. The style is noun-verb: name the
entity, then set an attribute or act on it.

```python
m = afx.mesh.geometry()
m.cmd("brick x 10 y 5 z 2")
m.cmd("cylinder radius 1 height 2; subtract body 2 from body 1")
m.cmd("mesh body 1 size 0.25 scheme overlay")
m.cmd("block 1 add body 1; sideset 1 add face 3 name 'top'")
mesh = m.result()                       # a pipeline node like any filter output
mesh.save("part.e")
```

Edits of the command box are traced as `node.set(commands=...)`, so the journal of a session is
always reproducible. The commands cover primitives, moves, rotations and scaling, booleans,
2-D outlines extruded, revolved or swept, `import` of closed surfaces and CAD parts, webcuts
(plane, cylinder, body, sheet, swept curve), `imprint` and `merge`, `thicken`, `loft`, `sew`,
groups, meshing (`mesh body ... scheme overlay|mapped`, intervals and bias), blocks, side sets
and node sets.

## 5. The meshers

- **Structured.** Boxes and mapped blocks with grading; a plate with a circular or elliptical
  hole (mapped blocks around an O-grid ring); a dog-bone bar with fillets and a spline
  thickness profile; bricks and cylinders from the geometry commands (O-grid disks swept along
  the axis); sweeps of a 2-D quad mesh.
- **Inclusions.** A background grid, packing of whole-cell boxes (one by one or collectively),
  and for each inclusion a cubed-sphere O-grid macro block with a transfinite core and graded
  shells, welded exactly into the grid; spheres, ellipsoids, superquadrics and
  spherical-harmonic grains.
- **Overlay grid** (`aurafx-mesh/src/overlay.rs`) for geometry bodies, closed surfaces and
  labelled images: a Cartesian grid classified by sampling, clean-up of small pieces,
  protrusions, one-cell layers, saddles and pinches by single-cell changes that only lower the
  problem count, pillow layers at interfaces, projection along stair-step normals onto exact
  distances, guarded Laplacian and tiered optimisation with an untangling energy (in parallel
  over colour classes of nodes), convex sharp edges and corners kept, void meshing, side sets
  for domain faces, model surfaces and material interfaces, periodic pairs, and a stair-step
  mode. This is the path for arbitrary geometry and voxel microstructures; it needs no
  boundary representation.
- **Tetrahedra.** Isosurface stuffing (a BCC lattice warped onto the surface) for geometry
  bodies and closed surfaces, local refinement with centre-point closure, conversions between
  hexahedra and tetrahedra, pyramid transitions for a region.
- **Surfaces.** Face Mesh (triangles or all quads on planar faces with holes), Remesh Surface
  (a fixed size, a size array or curvature).
- **Improvement.** Uniform refinement, Laplacian, smart Laplacian, condition-number and Winslow
  smoothing, boundary layers on side sets.

## 6. The grain workbench

- **Reading.** `.ang` (square and hexagonal grids), `.ctf`, `.h5ebsd` stacks as one volume,
  EDAX and Bruker `.h5`, `.dream3d` (image, rectilinear and shared-list geometries, with reader
  options for the data container and for reading only some arrays of big volumes), SPPARKS
  sites and dumps.
- **Clean-up and geometry.** Crop, Change Resolution, Align Sections, Rotate Sample Reference
  Frame, Apply Transformation to Geometry, the bad-data filters, image filters on volumes.
- **Features.** Segment Features (misorientation, c-axis, scalar), Fill Bad Data, Minimum Size,
  sizes, centroids, shapes from exact moments, average and reference orientations, kernel
  average misorientation, neighbours and boundaries, distance to boundaries, twins, and the
  further per-feature metrics of the filter reference.
- **Texture.** Pole figures and inverse pole figures as kernel densities in multiples of
  random, ODF sections with texture index, misorientation distributions with the exact uniform
  distribution of every class, texture component fractions, chord lengths; shown in the Charts
  panel, saved as pictures and CSV.
- **Synthetic microstructures.** Voronoi and Laguerre grains, columnar grains, Pack Grains
  (goal statistics, four shapes, matrix phase, calibration against the measured result), Pack
  Like, Insert Precipitates, Insert Transformation Phases, Match Crystallography, orientation
  noise, Orientation Gradient (along the build direction and in plane) and Grain Realism
  (boundary relaxation, coarsening, roughness, competitive columnar growth and layers).
- **Boundaries.** Quick Surface Mesh, Smooth Boundaries by node type, Triangle Geometry,
  Feature Surfaces, Face Orientation Colors, GBCD Section.
- **Mechanics.** Schmid factors for the FCC, BCC and HCP slip families, exact full-constraint
  Taylor factors, directional Young's modulus, a yield estimate, SROD by build height, GND
  density from the Nye tensor or from KAM.
- **Files and runs.** The `.dream3d` writer records the auraFX pipeline as JSON; File › Import
  Pipeline rebuilds auraFX pipelines exactly and maps numbered-step pipeline files to auraFX
  filters where it has them; the case templates write their files from the selected data and
  reopen the results named after the run.

## 7. Constraints that shaped the design

- **Size.** 3-D EBSD volumes reach 10^8 voxels, so segmentation, KAM, the Nye tensor and the
  overlay mesher's smoothing are chunked over all cores, like Slice and Contour.
- **Licences.** Everything is pure Rust with no GPL dependency. Reference programs and Python
  packages are used only to produce reference results for tests (`testdata/README.md`,
  `docs/reference/verification.md`), never as code.
- **No new dataset type.** Feature and ensemble arrays as field data, and meshes as
  unstructured grids with block and set arrays, keep every existing filter, the state file and
  the server protocol working on these datasets.
- **Boundary representation.** The mesh generator itself needs none; exact CAD geometry (STEP,
  IGES) comes in through the CAD environment (`docs/design/cad-environment.md`), whose parts
  the geometry commands `import` with their face names.

## 8. Open

Listed in ROADMAP.md under Status: transition templates for local refinement of hexahedra,
sharp edges in tetrahedral meshes, conforming tetrahedra between touching bodies, fillets on
picked curves and healing of implicit bodies, sweeps with several sources or targets,
submapping, indexing of EBSD patterns, and the limits of the synthetic grain models.
