# How the readers, writers and filters were verified

These are the comparisons behind the test suite, made with ParaView 6.1.1's pvpython on the
files in `testdata/`; the scripts are in `scripts/`. They are test documentation and name the
reference program where the user guide does not.

## Readers in general

Every reader was checked against ParaView 6.1.1: the same points, cells, bounds and value
ranges on the same files. Where auraFX differs on purpose, the notes below say so. The CML
reader gives the atoms and the file's bonds with their orders (`bond_order`), as ParaView's
CML reader does.

## NetCDF climate and ocean files

- A third (vertical) axis of a CF file becomes the radius, as in ParaView.
- ParaView's dedicated POP reader only reads variables with exactly three dimensions, so it
  skips POP history output (time, depth, latitude, longitude). Its general NetCDF reader places
  those files as auraFX does.
- CAM-SE: like ParaView's CAM reader, auraFX draws the grid on a flat lon/lat map. The
  **Layout › Sphere** option is not in ParaView. A single level lies at height 0 in auraFX;
  ParaView puts it at 1 or at the number of levels.
- MPAS files are recognized by their mesh variables (`xCell`, `cellsOnVertex`) and read as
  MPAS instead of as a CF grid, the way ParaView's MPAS reader does. On the map, cells that
  cross its left or right edge are drawn on both sides, so the map reaches a little beyond
  360°, as in ParaView.

Where auraFX's MPAS reader differs from ParaView:

- ParaView adds a point at the planet's center. It collapses onto that point the triangles at
  a mesh boundary and, when the file has `maxLevelCell`, the triangles that touch land. auraFX
  leaves those triangles out, so land shows as holes. In the multilayer view, the levels
  below the sea floor are left out too.
- The default layer thickness of 0 picks a visible spacing; ParaView uses 10000 everywhere.
- ParaView 6.1.1 fills point values in the multilayer view with garbage numbers (around
  10<sup>35</sup>); auraFX gives each point its level's value.

## GIS rasters

- A single band is named `Elevation`, as in ParaView.
- Pixels equal to the file's no-data value become NaN in auraFX; ParaView hides those cells
  instead.
- `MAP_PROJECTION` for a GeoTIFF is the EPSG code (`EPSG:32633`); ParaView writes a PROJ
  string instead.

Checked against ParaView 6.1.1's GDAL raster reader and against the GeoTIFFs the `.img`
files were copied from: a compressed float elevation model with no-data, RGB and four-band
images, and run-length compressed byte, 1-bit and 4-bit layers give the same values in every
cell, the same placement and the same coordinate system. ParaView itself places `.img` files
at the origin with zero spacing (it mistakes GDAL's empty list of control points for corner
points). It also leaves its `Elevation` array empty and lists every layer as `Band 1`, `Band 2`…

## GIS vectors

- Points become vertices, lines become polylines, and polygon rings become closed lines, as
  in ParaView.
- 64-bit integer attributes stay numbers in auraFX; ParaView turns them into text.
- A file geodatabase's `Shape_Length` and `Shape_Area` attributes come through as stored, as
  in ParaView.
- MapInfo coordinates are converted as GDAL does, rounding to the file's grid, so the values
  are those ParaView shows.
- The **Blocks › One per feature (ParaView)** option is ParaView's layout; the **Polygons ›
  Outlines (ParaView)** option draws polygons as ParaView does.
- Shapefile multipatches (3-D surfaces) are skipped, as ParaView does.

Checked against ParaView 6.1.1's GDAL vector reader on shapefiles, GeoJSON, GeoPackage,
FlatGeobuf, MapInfo files and a file geodatabase written by GDAL (FlatGeobuf also with 300 features, whose index
has three levels; MapInfo `.tab` and `.mif` versions of the three layers and of a 63-feature
layer of lots with holes, 300-point roads spread over several blocks of the file, a two-point
line, multi-lines, points and a feature without geometry): with one block per feature, every
point, cell and attribute is the same. A feature without geometry gives an empty block there,
as in ParaView. The file geodatabase holds six layers (the three above, with heights on the
cities, and the detail layer's lots, roads and wells); each was checked alone and all six
together. ParaView cannot pick a file's last layer on its own (choosing it reads every layer);
auraFX can. ParaView
reads only the first layer of a file unless told otherwise; auraFX reads all of them.
ParaView's GDAL cannot read KML, KMZ or GML (it is built without an XML parser), so those were
checked against the same data saved as GeoJSON: GML 3.2 and GML 2 files written by GDAL, and
the KML file zipped as KMZ.

## AMR data

- VTK AMR files (`.vthb`, `.vth`) are read as ParaView writes them.
- Enzo's **Convert to CGS** is off by default, as in ParaView. Chombo particle properties are
  read as doubles, as in ParaView.
- Coarse cells that a finer box covers are marked as hidden in `vtkGhostType`, as in ParaView.
- AMReX vector variables in 2-D files get z = 0; ParaView puts the file's first variable there
  instead.
- The 4-D Chombo reader follows VisIt: ParaView's VisIt reader builds the same 2-D boxes but
  drops the data.
- **Sources › AMR Gaussian Pulse** is ParaView's small AMR test dataset.
- Resample AMR clips the requested box to the data, dropping the samples that fall outside, as
  ParaView does. ParaView's Resample AMR has a demand-driven mode, on by default, that reads
  only the levels fine enough for the requested sampling. It needs a reader that supports it
  and gives nothing on a source. auraFX always uses every level, which is ParaView's result
  with that mode off.

Checked against ParaView 6.1.1: the AMR Gaussian Pulse source (cells, values, covered cells),
the `.vthb` reader on files ParaView wrote, the AMReX reader on test plotfiles
(`scripts/gen_amrex.py`; every box's origin, size, sums and covered cells), the FLASH reader
(FLASH 3 and FLASH 2 layouts, grid and particles) and the Enzo reader (grid and particles; the grid with and without Convert to CGS) on test
files (`scripts/gen_amr_files.py`; every block's origin, spacing, size, array types and sums
and covered cells), the Chombo reader against ParaView's VisIt Chombo reader (3-D, node-centered
3-D, a 2-D file with ghost cells and three levels, and particles), Resample AMR (every value at the points, on the 2-D
and 3-D Gaussian pulse, a region reaching past the data and a FLASH file), and on the AMR data
Integrate Variables (the volume counts each place once), slices, Threshold, Extract Surface,
Cell Data to Point Data and Resample To Image. ParaView 6.1.1 cannot read 2-D FLASH files
(it reports "Inconsistent types of vtkCartesianGrid", and crashes with several levels); auraFX
reads them, checked against the values the test file was written with. ParaView's Chombo
reader keeps the ghost cells of level 0 as ordinary cells, so its level 0 reaches one ghost
layer past the problem domain, while it drops them on the finer levels; auraFX drops them on
every level. ParaView gives FLASH particles once per grid block (the same particles in every
block) and reads every FLASH 3 particle property from the first column; auraFX gives each
particle once with its own properties, checked against the same particles in FLASH 2's layout.
ParaView's particle readers list no arrays to load in its Python interface, so they were driven
directly. ParaView's Resample AMR with Transfer to nodes off copies values from the wrong
cells (in the pulse, cells inside a level-1 box get another cell's values); auraFX takes each
cell center's own cell, which the tests check against the cell positions.

### Contours of AMR data

Averaging AMR cell values to points box by box is what ParaView's Contour does; it leaves gaps
and overlaps along every level boundary. auraFX's AMR Contour and AMR Dual Grid stitch the
levels through the dual grid; ParaView's AMR Dual Clip works this way too.

How it was checked: on a test plotfile whose density isosurface is a sphere crossing from the
fine box into the coarse ones, the contour is closed (every edge shared by exactly two
triangles, the topology of a sphere), its area is within 0.2 % of the sphere's, and its center
is within 0.001 of the sphere's center. The box-by-box contour had 346 open edges there.
ParaView's own AMR Contour and AMR Dual Clip are material-interface tools for CTH files and
refuse this kind of AMR data, so there is no ParaView result to compare with.

Not available in auraFX: ParaView's CTH material filters (AMR Fragments, Material Interface).

## Hyper tree grids

- auraFX reads and writes `.htg` files in XML version 2, as ParaView 5.10 and later write them.
- The cell count in **Information** is the number of all cells (coarse ones included), as in
  ParaView. A grid with material interfaces is drawn cut to its material, as ParaView draws it.
- The **Hyper Tree Grid** source builds a grid from a text descriptor, as ParaView's source
  does. ParaView builds nothing when the mask hides a refined cell; auraFX ignores the mask on
  refined cells.
- **Random Hyper Tree Grid** uses ParaView's random sequence, so the same **Seed** gives the
  same grid as in ParaView. **Masked fraction** hides whole subtrees with ParaView's random
  choices (the same settings hide the same cells). Its masking can hide a cell while leaving
  some cells below it unmasked; those stay hidden, as in ParaView.
- In To Dual Grid, points next to masked leaves move as ParaView moves them.
- Hyper Tree Grid Geometry cuts each mixed cell by its interface planes as ParaView does.
  Interfaces are used by Geometry only, as in ParaView.

Checked against ParaView 6.1.1 on `.htg` files it wrote (3-D, 2-D with branch factor 3,
masked):
- the cell and visible-leaf counts, and the leaves in the same order (their `OriginalIds`
  are ParaView's);
- Depth Limiter, Generate Fields and all seven Evaluate Coarse operators;
- both sources, which build the same grids as ParaView from the same settings, masks
  included;
- To Dual Grid on eleven grids (ParaView's files, both sources in 1-D, 2-D and 3-D, with and
  without masks): every cell in ParaView's order, its type, where its points are and their
  values;
- `.htg` files auraFX writes open in ParaView with the same cells, leaves and values, and read
  back into auraFX unchanged;
- Geometry and Feature Edges on eight grids (1-D, 2-D and 3-D, files and sources, masked and
  not): every polygon and line in ParaView's order, its points and its values;
- Geometry with material interfaces on six grids (1-D, 2-D, 2-D masked, 2-D with branch
  factor 3, 3-D, 3-D masked) with one-plane and two-plane interfaces, Fill material on and
  off: every output cell in order, its points (shared exactly as in ParaView) and the cell it
  comes from. ParaView keeps the output points in single precision, so cut points agree to
  about 7 digits.

ParaView's To Unstructured Grid leaves the `OriginalIds` of masked files partly unfilled (zeros
at the end); auraFX gives every leaf its index. ParaView's To Dual Grid also keeps a point for
every coarse and masked cell, left at the origin and used by no cell; auraFX leaves them out. ParaView deprecates its HTG Cell Centers and
Axis Reflection filters in 6.0: Cell Centers works on hyper tree grids directly, and Axis
Reflection is not provided.

## Cell grids

- Cell grids are VTK's newest data type (ParaView 5.13 and later). `.dg` files auraFX writes
  open in ParaView, side lists included (ParaView draws the sides).
- The **Cell Grid** source makes its test fields as ParaView's source does.
- Convert To Cell Grid renames a constant field "(cell)" when point data has the same name;
  ParaView drops one of the two.
- Cell Grid Probe works like ParaView's: a cell is searched with ParaView's quick test (the
  point must lie behind each of the cell's faces, taken as flat planes through the corners),
  and a point just outside a curved face that passes the test is kept with NaN coordinates and
  values, as in ParaView.
- Cell Grid Compute Sides works like ParaView's Compute Sides.

Checked against ParaView 6.1.1 on VTK's ten DG test files (hexahedra, tetrahedra, wedges,
pyramids including 19-node ones, quadrilaterals, quadratic quadrilaterals, triangles, edges,
mixed types):
- every corner point and every field there after To Unstructured Grid, edge and face fields
  included;
- Cell Grid Centers (positions and values);
- Elevation, and the source's edge and face fields;
- Cell Grid Transform (the edge and face fields follow the transformed cells), Cell Grid Warp
  and Convert To Cell Grid, every corner point and value;
- `.dg` files auraFX writes open in ParaView with the same values, and read back unchanged;
- Cell Grid Probe on a lattice of points around each file and the cell centers: the same
  cells for every point, and the coordinates and every field's value (edge and face fields
  too) wherever ParaView gives them;
- Cell Grid Compute Sides with eight settings (each choice of sides, omitting and keeping
  surfaces, the three Keep rules) and run twice: the same sides of every cell, the same cells
  hidden, the same lists (shape, side type, numbering, the `.dg` entries ParaView writes), and
  the same cells and corners after To Unstructured Grid.

Where auraFX differs from ParaView:
- ParaView keeps the extra nodes of quadratic cells as points, but evaluates the fields there
  at one fixed point of the cell (the center of a hexahedron, a corner of a tetrahedron)
  instead of at the node, and draws the cell as if it were linear.
  auraFX makes linear cells through the corners only.
- On quadrilaterals and triangles, ParaView's edge fields read undefined memory and its face
  fields come out NaN, because the flat cell's Jacobian is singular. auraFX completes it with
  the cell's unit normal, so both are in the cell's plane.
- ParaView's Cell Grid Centers gives one point for a file with several cell types; auraFX
  gives one per cell.
- Cell Grid Probe:
  - On quadrilaterals, triangles and edges, ParaView's search for a point's coordinates
    divides by zero (the same flat-cell Jacobian), so it gives NaN unless the point is at the
    cell's parametric origin. auraFX gives the coordinates.
  - For edges, ParaView's quick test reads undefined memory, so its results change from run
    to run. auraFX keeps points between the two end planes, within the sphere around the
    edge, and then keeps only the points on the edge.
  - ParaView's quick test for a flat cell uses the edges' directions rather than their
    normals. auraFX does the same, so the same points are listed (with NaN coordinates when
    outside).
  - ParaView samples the field in parallel with a shared cache, which mixes values between
    points (the reference was made with ParaView's threads off). It also crashes on cells
    with a quadratic shape. Its `CellType` array is always 0; auraFX gives the cell's VTK
    type number.
  - Points come out in single precision, as in ParaView.
  - With several cell types, ParaView lists them in an order of its own; auraFX lists them
    in the file's order.
- Compute Sides:
  - ParaView lists the sides in the order of a hash table, so some lists come out shuffled.
    auraFX lists them by cell, then side.
  - ParaView's To Unstructured Grid evaluates a side's fields on the wrong cell: the one
    numbered like the side's place in its list, not the side's own cell. It also crashes on
    the end points of edges. auraFX uses the side's cell, so a field that is continuous
    between cells has the same values on the sides as on the cells.
  - VTK's markings of ghost and hidden points (which skip sides) are not read.

Not available in auraFX: the cell grid filters ParaView 6.1.1 does not offer either (range,
copy).

## Grains and crystals

No reference program reads the EBSD maps here, so the checks use independent
implementations (`testdata/crystal/reference.json`, `scripts/crystal_reference.py`):
- Bunge angles to quaternions and orientation matrices agree with SciPy 1.17's Rotation
  ('ZXZ' intrinsic, transposed for the passive sample-to-crystal matrix) to 1e-12.
- Disorientation angles for all 11 Laue classes agree to 1e-9 rad with a brute force over
  both sides of SciPy's symmetry groups (C1, C2 about y, D2, C4, D4, C3, D3, C6, D6, T, O),
  whose axes match the generators auraFX uses. The AM orientation toolkit's cubic
  disorientation (`am_validation_metrics.disorientation_deg`) agrees with that brute force to
  2e-15 rad.
- Cubic m-3m and hexagonal 6/mmm IPF colors agree within one level of 255 with a separate
  implementation that reduces the direction by sorting its components (cubic) or folding its
  azimuth (hexagonal) instead of searching the group.
- On a window of NIST's AM Bench 2022 IN718 volume (`testdata/dream3d/am_bench_subset.dream3d`,
  written by DREAM.3D 6.6), Orientation Colors agrees with the `IPFColorZ` and `IPFColorX`
  arrays DREAM.3D stored, within one level of 255 on every cell.
- The full 2 µm AM Bench volume (525 × 424 × 367 cells, 3.5 GB) reads in about 3 s with three
  arrays; Align Sections by misorientation runs over its 525 slices in about 7 s.
- The HDF5 readers (`.h5ebsd` TSL and HKL stacks, EDAX OIM and Bruker Esprit `.h5`) are checked
  against the text maps the test files were written from (`scripts/gen_ebsd_h5.py`, in the
  layouts of EbsdLib's readers and importers); no real files in these layouts were at hand.
- Align Sections recovers the known window shifts of `ni_stack.h5ebsd` exactly.
- The `.ang` and `.ctf` readers are checked against the angles, phases and quality values
  `scripts/gen_ebsd.py` wrote, including the +30° on φ2 of hexagonal Oxford phases.

## Features (grains)

Checked against DREAM.3D 6.6's results stored in a window of NIST's AM Bench 2022 IN718 1 µm
volume (`testdata/dream3d/am_bench_features.dream3d`; DREAM.3D's pipeline: Segment Features
(misorientation 2°, within SampleVoxels), Fill Bad Data 7, Minimum Size 7, then the
statistics). Of the 225 features in the window, the 44 wholly inside it are compared:
- Segmentation, Fill Bad Data and Minimum Size give the same cells for 43 features. The
  44th is joined to its neighbour because the stored orientations meet across one face at
  0.82° (below the 2° tolerance): the reference segmented before its clean-up copied
  orientations into the joining cells.
- NumElements, Volumes and EquivalentDiameters agree exactly; Centroids to float precision
  (both use cell centres).
- Feature Shapes: with the reference's moment scheme (cells placed at their lower corner,
  half a cell from their centres, and sampled at quarter-cell points), axis lengths and
  Omega3 agree within 0.2 % and the shortest axis's direction within 0.8°. The stored
  directions of the two longer axes are turned within their plane for about half the
  features (the reference keeps its eigenvectors in the solver's order). auraFX uses exact
  moments (cell centres plus each cell's own inertia) and recovers the semi-axes (within 2 %),
  Omega3 = 1 and all three directions of a voxelised, rotated ellipsoid.
- Average orientations: auraFX's averages fit the cells better (smaller mean squared
  misorientation) for every feature and lie within 1.6° of the stored ones. The stored
  reference misorientations are consistent with the stored averages, and auraFX reproduces
  them (within 0.05° per cell) from those averages.
- Kernel average misorientation with the reference's kernel (2, 2, 1, the cell itself
  counted): within 0.01° on all cells whose kernel lies in the window, except at most 0.1 %.
- SurfaceFeatures agree for every feature.
Merge Twins, Feature Boundaries' twin flags, scalar and c-axis segmentation, Fill Bad Data and
Minimum Size are also checked on synthetic grids with known answers, and Distance to
Boundaries against a brute-force distance.

## Hexahedral meshes

- Inclusion Mesh reproduces hexaform's meshes node for node: on four hand-placed inclusions
  (sphere, rotated ellipsoid, superquadric, spherical-harmonic grain) in a 12³ grid, the
  points (to 1e-12), connectivity, blocks, side sets, node sets and `particle_id` are the same
  for HEX8 and HEX27 (`testdata/hexaform`, `scripts/hexaform_reference.py`). hexaform's own
  Exodus file reads back as the same mesh.
- Plate Mesh and Dog-bone Mesh reproduce the meshes the pinn-cp-pfd-benchmark study made
  with hexaform node for node: a plate with a circular hole, one with a 45-degree elliptical
  hole and the nickel dog-bone P.1 with five thickness stations (`testdata/benchmark`), with
  the same blocks, side sets and node sets.
- Texture (scripts/texture_reference.py, testdata/crystal/texture_reference.json), on the 11 520
  indexed cells of the AM Bench window, against the AM orientation toolkit (`am_texture`):
  - pole figures {100}, {110} and {111} on the toolkit's 41 × 41 stereographic grid agree
    within 1e-9;
  - the ODF kernel means on the φ2 = 0, 45 and 65° sections agree within 1e-9 relative, and
    the closed-form cubic disorientation agrees with the search over the 24 operators within
    1e-12;
  - the uniform ODF value is computed exactly from the distribution of random disorientation
    angles (0.0106917 for 7.5°); the toolkit's Monte Carlo estimate is 0.0106425 (0.46 % lower);
  - texture index: the toolkit's 2.652 comes from a 10° grid that counts φ1 = 0 and 360° twice
    and Φ = 90° in full. With its grid and uniform value, auraFX reproduces it within 1e-9. The
    midpoint grid auraFX uses gives 2.4692 at 5° and 2.4696 at 2.5°, so the toolkit's value is
    7 % high;
  - the random-disorientation distribution integrates to 1 for all eleven classes; its cubic
    maximum is 62.8° and its mean 40.74°;
  - component fractions (<100> along X, Y and Z, <110> and <111> along Z, 15° cones) are equal
    to the toolkit's;
  - chord lengths of the first slice (10° boundaries, gaps of up to 3 points skipped), 474
    along x and 347 along y, are the toolkit's `chords_from_map` list, chord for chord.
- Overlay meshing (crates/aurafx-mesh/tests/overlay.rs) is checked by quality floors on a set
  of parts: no inverted element, conformity (no face shared by more than two elements),
  smallest scaled Jacobian, and volume per material against the exact one: a sphere (0.53,
  -0.6 %), a plate with a hole (0.40, -0.15 %), a sphere inside a block (0.46, -1.1 % and
  +0.1 %), two crossed cylinders (0.21, -0.9 %), a cube surface turned 30 degrees about z
  (0.53, -1.1 %), a voxel sphere in a periodic cube and four voxel grains (0.30 and 0.32,
  volumes summing to the cube). On the measured AM Bench feature window (178 grains, 82 000
  voxels) every element is valid, smallest scaled Jacobian 0.15, once saddles are removed
  in the clean-up.
- Sharp edges: a brick turned 25 and 10 degrees off the grid has its volume within -0.36 %
  (-0.95 % without edge capture), its eight corners within 0.021 of a mesh node (six of them
  exactly) at cell size 0.1; the turned cube surface (STL) within -0.33 %; the plate with a
  hole within +0.03 %.
- Columnar Grains replays the random numbers recorded from the AM orientation toolkit's
  `generate_columns` (testdata/synthetic/columns_reference.json) and gives the same label
  volume voxel for voxel on two builds (34 and 56 grains, 76 800 voxels each), with the same
  grain bases.
- Voronoi Grains (200 periodic grains on 60³ cells) has 15.0 neighbours per grain on average
  against 15.5 for the exact Poisson-Voronoi tessellation (the voxels hide the smallest
  faces), and a log-sigma of the diameters of 0.14 (volume coefficient of variation 0.42).
  Add Orientation Noise of 3 degrees turns orientations by 2.39 degrees on average
  (3 sqrt(2/pi)).
- Pack Grains recovers its goals from the volume it builds (80³ cells, periodic, 489
  grains): interior grains measure size mu 2.486 (goal 2.485), b/a 0.756 (0.75) and c/a
  0.623 (0.625) after calibration; without it 2.463, 0.765 and 0.596 (space-filling grains
  come out a little smaller and flatter). Grains cut by the faces of the periodic box,
  measured as pieces, would give b/a about 0.55. A 0.6 grain fraction leaves 0.405 as
  matrix; Insert Precipitates at 5 % places 5.00 %; Pack Like of a Laguerre volume
  reproduces its size mu (2.344 against 2.342). Match Crystallography against a reference
  whose neighbouring grains are close in orientation lowers the boundary misorientation
  error from 0.99 to 0.03.
- Grain boundary surfaces (crates/aurafx-filters/tests/grain_surface.rs): on 40 Voronoi grains
  the volume each feature's boundary encloses equals its voxel volume to 1e-9, each feature
  surface is closed and consistently oriented, Taubin smoothing (20 passes) changes the
  volumes by 0.41 % on average against 1.66 % for plain Laplacian smoothing, and no node
  leaves the box. GBCD normalisation: 40 000 boundaries with random orientations and random
  planes give 1.03, 0.93 and 1.03 multiples of random at 45 degrees about [100], 60 about
  [111] and 30 about [110] (sampling noise; 60 000 give 1.007 and 0.974 for two seeds). A
  stack of coherent Sigma 3 twin layers gives about 39 000 multiples of random at (111) and
  none at (100) and (110); every twin face measures 60.000 degrees.
- Tetrahedron quality: 20 measures (all ParaView offers but equivolume skew) agree with
  ParaView 6.1.1's Mesh Quality on 24 tetrahedra within its 32-bit precision
  (testdata/mesh/tet_quality_reference.json).
- Refinement and element conversion (crates/aurafx-mesh/tests/refine.rs): hexahedra refined,
  split into tetrahedra, tetrahedra refined and turned back into hexahedra keep the volume to
  1e-12, every element positive, faces conforming, and every boundary face in one of the six
  side sets. Boundary layers keep the volume, give the asked first-layer thickness exactly and
  add the layers' side faces to the neighbouring side sets; smoothing raises the worst scaled
  Jacobian of a jiggled box from 0.25 to 0.97-1.
- Tetrahedral meshing (crates/aurafx-mesh/tests/stuffing.rs): a sphere and a torus give
  positive, conforming tetrahedra whose volumes come within -1.0 % and -0.26 % (sphere, sizes
  0.2 and 0.1 of the radius) and -3.0 % and -0.7 % (torus, tube radius 0.35) of the exact
  volume; the smallest dihedral angle is 21-30 degrees. Refine Region on the upper half of the
  sphere keeps the mesh positive and conforming (smallest angle 13.6 degrees).
- Crystal mechanics against the AM orientation toolkit (am_build_campaign) on 2000 AM Bench
  orientations, along z and x: full-constraint Taylor factor, largest Schmid factor and
  Young's modulus agree within 1e-14 (testdata/crystal/mechanics_reference.json). A random FCC
  texture gives a mean Taylor factor of 3.067 (Kocks: 3.06) and a mean largest Schmid factor
  of 0.452.
- SROD and Nye GND against the toolkit's am_ebsd3d_analysis on the 1 um AM Bench feature
  window: the same 79 grains with the same classes and numbers of bins, the same 580 bins
  with the same cell counts, every SROD (grain, bin, cell, rate) within 3e-5 degrees; the Nye
  GND density within 2.3e-6 relative with the same empty cells
  (testdata/crystal/am_reference.json).
- hexaform's periodic node sets use background-grid node numbers after the weld renumbered
  the nodes, and sort the pair list; auraFX pairs the welded nodes by their coordinates.
- Exodus files with blocks, side sets, node sets and HEX27 elements written by auraFX open in
  ParaView 6.1.1 with the same blocks and sets, and HEX27 nodes in the right places. ParaView's
  Cell Size gives triquadratic hexahedra wrong (negative) volumes even for straight cubes, so
  it cannot check them.
- The hexahedron quality metrics match ParaView 6.1.1's Mesh Quality (Verdict) on 24 test
  hexahedra to 32-bit precision for all 19 measures (`testdata/mesh/hex_quality_reference.json`).
  Like Verdict, the hexahedron edge ratio takes its maximum over six of the twelve edges only;
  auraFX shows the generic edge ratio (all edges) instead.

## Geometry

The geometry kernel is checked against closed forms (`crates/aurafx-mesh/tests/geometry.rs`,
volumes and areas integrated from the distance field on cells 1/100 of the body's size):
- sphere, brick, cylinder, frustum, torus, hexagonal prism, ellipsoid and superellipsoid
  (volume from the Beta-function formula), and a spherical-harmonic shape (volume by
  quadrature of r³/3 over the sphere): volumes within 0.5 % (1 % for the superellipsoid and
  the harmonic shape), areas within 1 %;
- a brick with a through hole, two overlapping spheres (union and lens intersection),
  rotated and scaled bricks;
- extruded, filleted, revolved (full and partial), swept and arc-built outlines.
The Geometry source's drawn surfaces give the volume within 0.5 % and the area within 1 % of
the closed forms at a resolution of 96. An STL file written from a drawn sphere and imported
back encloses the same volume within 0.5 %. The Signed Distance Field of that file is within
0.01 of the exact distance to the sphere.

## Writers

The `.htg` writer produces XML version 2, as ParaView 5.10 and later write it. Files written
by the VTK XML, VTKHDF, Exodus, EnSight, Xdmf and CGNS writers open in ParaView with the same
points, cells and values.
