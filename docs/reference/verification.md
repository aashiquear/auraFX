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
