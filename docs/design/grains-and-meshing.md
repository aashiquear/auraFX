# Grain workbench and mesh generator: design and development stages

Status: design, 2026-11-06. Nothing in this document is built yet; it sets the stages that
ROADMAP.md sections 11 and 12 (phases P8 and P9) track.

auraFX today is a full visualization suite with a Python console, an AI assistant and a
simulation runner. This design adds two more things to the same window, so that one tool
takes a user from a geometry to a mesh, from a mesh to a polycrystal on it, from the
polycrystal to a solver run (MOOSE, SPPARKS, PINNACLE, LAMMPS, local or over SSH), and from
the run's output back to grain statistics, texture plots and pictures:

1. a **grain workbench**: EBSD and synthetic microstructures, grain segmentation and
   statistics, texture, boundaries and mechanics as filters, sources, readers, writers and
   charts, for FCC, BCC and HCP crystals;
2. a **geometry and mesh generator**: implicit geometry with structured and overlay-grid
   all-hex meshing, quality control and sets, as sources and filters with a small command
   language.

The reference material behind both (section 1) is the one place in the repository, with the
README's sentence, that names the programs this work is inspired by; the features themselves
are described in auraFX's own terms everywhere else.

Both reuse what exists: the `FilterDef` registry (one definition drives the menus, the
Properties panel, Python and the AI assistant), the dataset types, the Exodus/VTK writers,
the job templates, the charts and the user-guide rule. The UI gains two menu categories
(**Filters › Grains**, **Sources › Mesh**) and nothing else is moved.

## 1. Reference material (what the surveys found)

**DREAM.3D** (develop branch): 190 filters in ten plugins. The ones that matter here are
EBSD import (`.ang`, `.ctf`, `.h5ebsd`, EDAX and Bruker `.h5`), orientation conversions
among seven representations with passive rotations and vector-scalar quaternions, eleven
Laue groups, segmentation by misorientation (a 6-neighbour burn), feature statistics (sizes,
neighbours, moment-based ellipsoid shapes with Omega3, average orientations, misorientation
lists, Schmid factors, kernel average misorientation, reference-orientation deviation,
Euclidean distance maps), twin boundaries and the 5-D GBCD, IPF/Rodrigues/Euler colouring,
the synthetic builder (StatsGenerator's size/shape/neighbour/ODF/MDF/axis-ODF statistics,
packing of ellipsoids and superellipsoids with periodic boundaries, matrix and precipitate
phases, MatchCrystallography's swap search), Quick Surface Mesh with Laplacian/Taubin
smoothing and STL export, cropping/resampling/section alignment, pole figure images, and the
`.dream3d` HDF5 layout (`DataContainers/<name>/_SIMPL_GEOMETRY`, attribute matrices as
groups, arrays as flat datasets with a component-dimensions attribute, `/Pipeline` JSON).
DREAM.3D has no GND filter; its PackPrimaryPhases page is a copy of the precipitate page.

**AM-Orientation-Shift-TOOLKIT**: FCC only, NumPy/SciPy. It reads `.dream3d`, `.ctf`, its
own `.npz` volumes, Exodus, MOOSE CSV, `.tex` (MOOSE `EulerAngleFileReader`) and SPPARKS
sites files, and implements exactly the analyses a build-direction study needs: cubic
disorientation (exact near zero, and the fast Heinz-Neumann form), IPF folding/projection
and colour key, 6-connected segmentation, per-grain reference orientation and SROD
(deviation from the starting orientation) binned by build height, KAM per direction, Nye
tensor GND from least-squares lattice curvature, kernel-density pole figures in MRD, ODF
sections and texture index, texture fractions, chord lengths, Taylor factor (exact 384-basis
enumeration), Schmid factor, cubic directional modulus, a Hall-Petch + Taylor yield estimate,
a columnar power-diagram grain generator, ECDF/area/Kolmogorov validation metrics, and
drivers for MOOSE (Docker or local), SPPARKS and a JAX pipeline, each rewriting orientations
layer by layer with a surrogate.

**hexaform** (Python, ~10k lines, 209 tests): all-hex structured meshing of a cube with
star-shaped inclusions (spheres, ellipsoids, superquadrics, spherical-harmonic grains)
packed by random sequential addition on a background grid, each inclusion a cubed-sphere
O-grid macro-block with transfinite core and graded shells, welded into the grid; a plate
with a circular or elliptical hole (mapped blocks around an O-grid ring with automatic
geometric grading); a dog-bone bar with fillets and a spline thickness profile; HEX8/20/27;
scaled-Jacobian, edge-ratio, volume-error and conformity checks; named blocks, side sets and
node sets; Exodus and legacy VTK output. Its octree backend, pillowing, geometric notch and
periodic node pairing are unfinished and should be redesigned rather than ported. The
benchmark project uses the plate and dog-bone meshes in PINNACLE decks (`[Mesh] type = file`,
set names referenced directly) and builds grains by Voronoi seeding, phase-field growth,
component labelling and nearest-centroid assignment of a point file to elements.

**Cubit 15.8**: the reference for a command language (`<entity> <ids> scheme sweep`,
`mesh volume all`, journals, Python `cubit.cmd`), for the catalog of geometry operations
(primitives, sweep/revolve/loft, booleans, webcuts, imprint/merge, healing, tweaks), for
mesh schemes (map, submap, pave, sweep, tet, sphere, hole, circle, THex, HTet), sizing and
biasing, smoothing schemes, the acceptable-quality tables (hex scaled Jacobian 0.5-1, tet
0.2-1, ...), refinement, blocks/node sets/side sets with element types per block, and the
exporters. Most relevant is **Sculpt**, its overlay-grid hex mesher: a Cartesian grid, a
volume fraction per cell and material from STL, CSG or voxel images, interface
reconstruction, node projection, pillow layers at material interfaces, tiered smoothing
(Laplacian, then optimisation below SJ 0.6, then spot optimisation below 0.2), defeaturing,
adaptive refinement in 8x steps, void meshing, generated side sets and periodic RVE sets.
That is the method that meshes arbitrary geometry and voxel microstructures without a CAD
kernel.

## 2. Architecture

```
crates/
  aurafx-crystal   orientation mathematics: quaternions, Euler/Rodrigues/axis-angle/matrix,
                   the 11 Laue groups' operators, disorientation, fundamental zones, IPF and
                   other colour keys, pole figures, ODF/MDF kernels, slip systems and Schmid /
                   Taylor factors for FCC, BCC, HCP (no dependency on the rest of auraFX)
  aurafx-mesh      geometry kernel (implicit shapes, CSG, 2-D outlines, extrude/revolve,
                   STL as signed distance) and the meshers (structured/mapped, O-grid macro
                   blocks, overlay grid), quality metrics, sets and blocks
  aurafx-filters   the Grains and Mesh filters and sources, registered like every other one
  aurafx-io        readers/writers: .ang, .ctf, .h5ebsd, EDAX/Bruker .h5, .dream3d, .tex,
                   SPPARKS sites, Abaqus .inp, Gmsh .msh
  aurafx-jobs      templates: MOOSE polycrystal, SPPARKS, PINNACLE, with the mesh and
                   orientation files the pipeline produced filled in
```

**Microstructure data.** A microstructure is kept as an ordinary `ImageData` (or any grid) whose cell data holds
`FeatureIds`, `Phases` and the orientations (`EulerAngles`, `Quats`, radians, Bunge, passive,
vector-scalar order), plus two `Table`s in a `MultiBlock` beside it,
**Features** (one row per feature id, id 0 reserved) and **Ensembles** (one row per phase:
crystal structure as a Laue-group code, lattice constants, name). A filter that needs
feature arrays takes the multi-block; one that only needs cell arrays also accepts the grid
alone. Every existing filter, the Spreadsheet and Color by work unchanged on these datasets,
and the `.dream3d` writer maps them to that format's groups.

As built in G1, the per-phase (ensemble) arrays travel as **field data** of the grid:
`CrystalStructures` (Laue code per phase, entry 0 = not indexed, 999 unknown), `MaterialName`
and `LatticeConstants` (6 per phase). Field data already passes through every filter, the
state file and the server protocol, so no new table or protocol change was needed for
ensembles; the Features table (G2) still comes as a `Table` beside the grid. EBSD readers keep
the Euler angles as stored (no reference-frame change), apart from the optional +30° on φ2 of
hexagonal `.ctf` phases; Rotate Orientations re-expresses them in another frame.

**Mesh data.** A generated mesh is an `UnstructuredGrid` with element blocks (an `ElementBlock`
cell array plus block names in field data, as the Exodus reader and writer already use),
side sets and node sets (as the Exodus writer takes them), element variables (`grain_id`,
`particle_id`, `phase_field`) and a `vtkOriginalCellIds`-style provenance array. The Mesh
Quality filter gains the full metric set with an acceptable-range table.

**Pipeline.** Geometry and mesh steps are sources and filters: *Geometry* source (a CSG
tree edited in Properties or by commands), *Hex Mesh* filter (scheme, size, grading,
smoothing, pillowing), *Blocks and Sets* filter, *Elevate* (HEX8 to HEX20/27). A
microstructure can be built on a mesh (*Assign Grains*: Voronoi/Laguerre or nearest voxel of
a synthetic volume) or a mesh can be built from a microstructure (*Mesh Grains*: the overlay-grid
path, one block per grain or `grain_id` per element). The *Run Simulation* dialog's
templates take the pipeline's mesh and orientation files as inputs and the existing
auto-load brings the results back under the same pipeline, where the Grains filters run on
Exodus output as well as on EBSD.

**Command language.** `afx.mesh` in Python uses a noun-verb style (name the entity, set an
attribute or act on it) and is what the AI assistant and journals use:

```python
m = afx.mesh.geometry()
m.cmd("brick x 10 y 5 z 2")
m.cmd("cylinder radius 1 height 2; subtract body 2 from body 1")
m.cmd("volume 1 size 0.25; volume 1 scheme overlay pillow surfaces")
m.cmd("mesh volume 1; block 1 add volume 1; sideset 1 add surface 3 name 'top'")
mesh = m.result()                       # a pipeline node like any filter output
mesh.save("part.e")
```

The same commands come out of the Properties panel through the trace, as filter parameters
do today, so the journal is always reproducible.

As built in M1, `afx.mesh.geometry()` returns a builder (`cmd`, `bodies`, `journal`,
`result`) and the Geometry source takes the whole command list as one parameter edited in a
command box; edits of command boxes are traced as `node.set(commands=...)`. Bodies are
implicit (signed distance) solids, so the source draws them by contouring the distance field
per body and snapping smooth parts onto the exact surface; sharp edges are rounded over a
cell in the drawing (the meshers of M2/M3 work on the exact geometry). Faces are tagged by
the primitive feature nearest each point and keep their names through booleans. Mesh
commands (`mesh volume`, `block`, `sideset`) come with M2.

## 3. Development stages

Each stage follows the existing rule: a user-guide entry with a screenshot, tests against a
reference, the release binaries rebuilt, one commit. The references are named per stage
because no reference program for either side is installed here.

### Grain workbench (P8)

| Stage | Deliverable | Reference for the tests |
|---|---|---|
| **G1 Crystal core and EBSD** | `aurafx-crystal` (conversions, Laue operators, disorientation, FZ, IPF/Rodrigues/Euler colours, cubic and hexagonal keys); readers for `.ang`, `.ctf`, `.h5ebsd`, EDAX/Bruker `.h5`, `.dream3d` (cell arrays and geometry); Orientation Colors (IPF and Euler keys), IPF Color Key source, Convert Orientations, Rotate Orientations, Change Resolution, Crop, Align Sections | the toolkit's NumPy functions run with `python3` on the same inputs (FCC); hand-built cases for HCP and the other groups (every operator applied to a known orientation); public sample `.dream3d` and `.ang` files |
| **G2 Features** | Segment Features (misorientation, scalar, C-axis), Find Sizes / Neighbors / Shapes / Centroids / Boundary Cells / Surface Features / Average Orientations / Misorientations / Kernel Average Misorientation / Reference Misorientations / Euclidean Distance Map, Min Size, Fill Bad Data, Merge Twins, Find Twin Boundaries; the Features and Ensembles tables; feature-coloured and boundary pictures | the toolkit's `segment`, `analyse`, `neighbour_misorientation` on its EBSD volumes; the published formulas (moment invariants, Omega3) on synthetic ellipsoids of known axes |
| **G3 Texture** | pole figures (kernel density, MRD, stereographic and equal-area), inverse pole figures, ODF sections and texture index, MDF, texture fractions, chord lengths; shown in the Charts panel and saved as pictures and CSV | `am_texture.pole_figure`, `odf_values`, `texture_index`, `fractions`, `chords_from_map` |
| **G4 Synthetic builder** | Ensemble Statistics (size, shape, neighbour, ODF, MDF, axis ODF, as a Properties form with lognormal/beta fits), Initialize Volume, Pack Primary Phases (ellipsoid, superellipsoid, cube-octahedron, cylinder; periodic), Insert Precipitates, Matrix Phase, Match Crystallography, Add Orientation Noise, Jumble Orientations; plus Voronoi / Laguerre tessellation and the toolkit's columnar AM grains as sources; Generate Ensemble Statistics from measured data | the goal statistics are recovered from the generated volume within tolerance (the builder is its own oracle); the toolkit's `generate_columns` |
| **G5 Surface meshing and GBCD** | Quick Surface Mesh, Laplacian / Taubin smoothing with node types, Triangle Geometry filters (areas, normals, dihedral angles, face labels), STL per feature, Find GBCD with pole-figure sections, Face IPF / misorientation colouring | watertightness and feature volumes preserved; GBCD of a synthetic Σ3-rich volume |
| **G6 Mechanics and AM** | Schmid factors (FCC {111}<110>; BCC {110}, {112}, {123} <111>; HCP basal, prismatic, pyramidal a and c+a with c/a), Taylor factor, directional modulus from C11/C12/C44 (and the five HCP constants), yield estimate, SROD by build height with classes, Nye GND and KAM-based GND, layer and section plots; a materials table (lattice, elastic constants, slip systems, Burgers vector) with the toolkit's CoNi and IN718 entries | `taylor_factor`, `schmid_max`, `youngs_modulus`, `nye_gnd`, `analyse` from the toolkit |
| **G7 Files and runs** | `.dream3d` writer (with the `/Pipeline` JSON of the auraFX pipeline), `.tex`, SPPARKS sites reader/writer, import of `.dream3d` pipeline JSON (the subset of steps above mapped to auraFX filters), job templates for MOOSE polycrystal (mesh + `EulerAngleFileReader`), SPPARKS and PINNACLE, results reopened under the pipeline | files read back by h5py in `pvpython`; a MOOSE deck generated and run on the test image when MOOSE is present |

### Geometry and mesh generator (P9)

| Stage | Deliverable | Reference for the tests |
|---|---|---|
| **M1 Geometry kernel** | implicit primitives (brick, cylinder, sphere, cone, torus, prism, superquadric, spherical-harmonic shape), transforms, CSG (unite, subtract, intersect), 2-D outlines (polyline, arc, spline, circle, ellipse, fillets) extruded, revolved or swept, STL and the existing surfaces as signed distance fields, named faces; the *Geometry* source with a Properties tree and the `afx.mesh` commands; drawn through the existing contour of the distance field | volumes and surface areas of primitives and booleans against closed forms; STL round trip |
| **M2 Structured hex** | box, mapped blocks, hexaform's cube-with-inclusions (RSA and collective packing, cubed-sphere O-grid macro-blocks, transfinite cores, graded shells, weld), plate with hole, dog-bone, sweep of a 2-D quad mesh, Elevate to HEX20/27, blocks/side sets/node sets (hexaform's names), periodic pairing done after the weld; Mesh Quality with the full metric set and acceptable ranges; Exodus, VTK, Abaqus `.inp`, Gmsh `.msh` writers | hexaform's own output on its examples (node for node after sorting), its 209 tests re-expressed where they test algorithms, the benchmark's `results/pfd_study/mesh/*/mesh.e` and Ni dog-bones |
| **M3 Overlay-grid mesher** | Cartesian overlay grid with cell size or counts, volume fractions per material from the implicit geometry, STL or a voxel image (including a microstructure's FeatureIds), interface reconstruction, node projection, pillowing at interfaces, curves and domain faces, tiered smoothing with scaled-Jacobian thresholds, defeaturing (minimum cells, protrusions), void meshing, generated side sets (domain, model boundary, material pairs), periodic RVE node sets, stair-step mode as a fallback | quality floors (minimum SJ, no inverted elements, conformity, volume error per material below a tolerance) on a test set of CSG parts, STL parts and synthetic microstructures; the documented behaviour of the reference overlay-grid mesher (section 1) as the specification |
| **M4 Refinement and tets** | conformal 8x refinement with transition templates, local refinement by region or size field, Laplacian / smart Laplacian / Winslow / condition-number smoothing, Delaunay triangle and tetrahedral meshing of implicit and STL surfaces (sizing by curvature and gradation), THex and HTet conversions, boundary layers | quality tables; tet volumes sum to the geometry's volume |
| **M5 Grains on meshes** | Assign Grains (Voronoi, Laguerre, nearest voxel, from a `.dream3d` or synthetic volume) writing `grain_id`, per-grain blocks or side sets at grain boundaries, orientation files; Mesh Grains (M3 on FeatureIds with one block per grain); the simulation templates of G7 wired to these outputs; a worked example: geometry → mesh → grains → MOOSE run over SSH → grain statistics on the result | the benchmark's `elem_grain.csv` rule (nearest centroid); a solver's own Voronoi polycrystal initial condition on a small case for the Voronoi assignment |

Order: G1, M1, G2, M2, G3, M3, G4, G5, M4, G6, M5, G7. Interleaving keeps each side usable
early (G1-G3 already make auraFX an EBSD viewer; M1-M2 already replace hexaform).

## 4. Implications

- **Scope.** Each side is about as large as the whole of P5 (33 iterations). The stages above
  are twelve units of work of the size of two to four recent iterations each. The grain side
  has solid references (the toolkit, published formulas); the mesh side's hardest part, M3,
  has none here beyond a manual, so its tests are quality guarantees, not a comparison.
- **Crystal structures.** The toolkit is FCC-only; the hexagonal operators, HCP slip systems
  (with c/a) and the five HCP elastic constants are new work with no local oracle. `orix`
  or MTEX can serve as test oracles in a separate Python environment, but they are GPL, so
  only their results, never their code, may be used.
- **Licences.** Of the reference material in section 1, the BSD-3 code may be re-implemented
  with attribution, hexaform is the user's own code, and the proprietary manual only informs
  the command style and the documented behaviour; no code is taken from it. Everything stays
  pure Rust with no GPL dependency, and the documentation describes auraFX's features without
  presenting them as another program's.
- **CAD.** Overlay-grid meshing needs no B-rep kernel, which is why it is the main path.
  STEP/IGES import (ROADMAP 1.2) would need OpenCASCADE; it stays out of scope, with external
  CAD meshers for CAD-derived meshes (their output opens in auraFX already).
- **Data model.** Feature and ensemble tables beside a grid are a convention, not a new
  dataset type; filters must find them by name in the multi-block, and state files and the
  server protocol must carry them (another protocol bump). The `.dream3d` writer needs the
  existing HDF5 encoder to write compound attributes.
- **Size.** 3-D EBSD volumes reach 10^8 voxels; segmentation, KAM and Nye GND must be the
  rayon-chunked kind of filter (as Slice and Contour are), and overlay meshes of that size
  need the server path. GPU versions are a later item under section 8.
- **UI.** Two menu categories and the `afx.mesh` command box in the Python panel are the only
  visible additions. StatsGenerator-sized forms rely on the grouped and advanced parameters
  the Properties panel already has; a dedicated Grains panel is deliberately not planned until
  the filters exist.
- **Simulation coupling.** MOOSE polycrystal decks need a mesh file, `grain_id` as blocks or
  an element variable, and a `.tex` orientation file; SPPARKS needs a sites file; PINNACLE
  reads Exodus with hexaform's set names. The templates fill those in from the pipeline, and
  the auto-load already brings results back; what is new is that the Grains filters then run
  on the results, closing the loop the toolkit runs by hand today.
- **Prototyping.** The toolkit's NumPy functions can be dropped into Python plugins
  immediately, so a user can run them in auraFX before the Rust ports land; the Rust ports
  replace them filter by filter with the plugin as the oracle.
- **Unfinished hexaform parts** (octree, pillow, geometric notch, periodic sets) are not
  ported; M3 and M4 redo them.
