# File formats

Every reader was checked against ParaView 6.1.1: the same points, cells, bounds and value
ranges on the same files. Where auraFX differs on purpose, the table says so.

## Readers

| Format | Extensions | Notes |
|---|---|---|
| Exodus II | `.e .exo .ex2 .g .gen` | NetCDF-3 and NetCDF-4; blocks, side and node sets, nodal/element/global variables, time steps, displacements |
| VTK legacy | `.vtk` | ASCII and binary, all dataset types |
| VTK XML | `.vtu .vtp .vti .vtr .vts .vtt`, parallel `.pvtu …`, `.vtm`, `.pvd` | inline, appended, base64; zlib and LZ4; curved (Lagrange/Bezier) cells |
| AMR (adaptive mesh refinement) | VTK `.vthb .vth`, AMReX/BoxLib plotfiles (the folder, or its `Header`) | levels of boxes with cell data; coarse cells under finer boxes are left out; see below |
| Hyper tree grids | VTK `.htg` (XML version 2) | trees of refined cells; drawn as their leaves; see below |
| VTKHDF | `.vtkhdf .hdf` | image data, unstructured grids, poly data, partitioned collections with their assembly, time steps; chunked and gzip storage; no HDF5 library needed |
| OpenFOAM | `.foam` | ASCII/binary/gzip, decomposed and reconstructed cases, time directories, boundary patches |
| EnSight Gold | `.case .encas` | ASCII and C binary, all element types, structured blocks, time sets, wildcard file names |
| CGNS | `.cgns` | HDF5 and ADF storage; structured and unstructured zones, MIXED and NGON/NFACE sections, solutions, time steps |
| Xdmf 2/3 | `.xmf .xdmf` | inline, HDF5 and raw binary data; collections; `xi:include` and XPath references (FEniCS/Dolfinx); HyperSlab and JOIN items |
| NetCDF (CF) | `.nc .nc4 .cdf` | classic and NetCDF-4; lon/lat grids on a sphere with cell bounds, curvilinear grids with 2-D longitude/latitude (POP, CICE, ROMS), rectilinear otherwise; time steps; packing; fill values become NaN (grey); see below |
| CAM-SE | `.nc` + connectivity file (found from the contents) | CESM atmosphere on its cubed sphere: lon/lat map or sphere, surface, midpoint or interface variables, one level or all; see below |
| MPAS | `.nc` (found from the file's contents) | MPAS ocean, atmosphere and land-ice output: cell centers joined into triangles, any vertical level or all of them, sphere or lon/lat map, time steps; see below |
| Tecplot | `.dat .plt .tec .tp` | ASCII (ordered and FE zones, BLOCK/POINT, cell-centered variables) and binary `.plt`; `.szplt` is not read |
| PLOT3D | `.xyz .x .p3d .bin` + `.q` | layout detected from the file (Fortran/C binary, byte order, precision, multi-grid, 2-D/3-D, IBLANK), ASCII |
| LAMMPS | `.dump .lammpstrj .atom` (+`.gz`), `.data .lmp` | multi-frame dumps with any columns, triclinic boxes; data files with bonds |
| XYZ / extended XYZ | `.xyz .extxyz` | multi-frame; properties and lattice from the comment line |
| PDB | `.pdb .ent` | all atoms (hydrogens kept), MODELs as time steps, CONECT and distance bonds, residues and chains |
| Gaussian cube | `.cube` | the density as a volume in Å, plus the atoms with bonds |
| VASP | `POSCAR`, `CONTCAR`, `CHGCAR`, `LOCPOT`, `ELFCAR`, `PARCHG` (by name), `.vasp` | atoms with bonds and the lattice; volumes on the cell's axes (charge per Å³) |
| Images | `.png .jpg .bmp .tif` | as image data; multi-page TIFFs and `img_*.png` wildcards as volumes |
| GIS rasters | GeoTIFF `.tif`, Esri ASCII grid `.asc` | at their map coordinates, a cell per pixel, every band; no-data values become NaN (grey); see below |
| GIS vectors | Shapefile `.shp`, GeoJSON `.geojson .json`, KML `.kml`, GeoPackage `.gpkg` | points, lines and polygon outlines (or filled areas) with their attributes as cell data, a block per layer; see below |
| Surfaces | `.stl .ply .obj` | |
| CSV / TSV | `.csv .tsv .txt` | delimiter and header detection; tables with x, y, z columns become points |

Numbered files (`run_0001.vtu`…) open as a series when one of them is opened.

CGNS files come in two storage formats with the same `.cgns` extension: HDF5, the default
since CGNS 3.4, and ADF, which older solvers and older CGNS libraries write. auraFX looks at
the file and reads either one the same way, with **File › Open** or `afx.load("wing.cgns")`.
Both the 64-bit ADF layout and the older 32-bit one are read, in either byte order. Links
inside the file and links to other ADF files are followed. A link to an HDF5 file from an ADF
file is not.

![A CGNS file in ADF storage, clipped and colored by RTData](images/cgns_adf.png)

### NetCDF climate and ocean files

Files that follow the CF conventions are placed by their coordinates:

- Longitude and latitude axes put the grid on a sphere of radius 1. A third (vertical)
  axis becomes the radius, as in ParaView. Depths marked `positive="down"` are turned around,
  and all levels are shifted so that none is below zero, so deeper levels lie further in
  (the deepest at the center).
- Ocean and sea-ice models such as POP (CESM's ocean), CICE and ROMS use index dimensions
  (`nlat`, `nlon`) and give each point's longitude and latitude in 2-D variables (`TLONG`,
  `TLAT`) named in the `coordinates` attribute. These become curvilinear grids on the
  sphere. The coordinate variables themselves are not shown as data.
- When the variables live on different grids (say `TEMP` on depth, latitude and longitude,
  and `SSH` on the surface only), **Reader options › Variables on** picks the grid. Python:
  `ds.set(variables="(nlat, nlon)")`. The grid with the most dimensions is read first.

![A POP-style ocean file: sea surface height on its curvilinear TLONG/TLAT grid; the island has no data (grey)](images/pop_ocean.png)

ParaView's dedicated POP reader only reads variables with exactly three dimensions, so it
skips POP history output (time, depth, latitude, longitude). Its general NetCDF reader places
those files as auraFX does.

CAM-SE output (the spectral-element core of CESM's atmosphere model) comes as two files:
the data, with columns `ncol` at `lat`/`lon`, and a connectivity file whose
`element_corners` joins the columns into quads. Open the data file. auraFX finds the
connectivity file in the same folder, or you can name it under **Reader options ›
Connectivity file** (Python: `ds.set(connectivity="ne30np4_connectivity.nc")`). Like
ParaView's CAM reader, it draws the grid on a flat lon/lat map:

- Quads that cross the 0/360° seam are moved to the side where they stick out least, on
  copies of their points, so the map runs a little past 0 and 360.
- **Variables** picks the surface variables `[time, ncol]`, or those on midpoint
  (`lev`) or interface (`ilev`) levels. With **All levels** on, the default, each quad
  becomes a column of hexahedra, the top level highest, and the `lev` (or `ilev`) array
  holds each point's level. With it off, **Level** chooses one level.
- **Layout › Sphere** puts the columns on a sphere instead. This is not in ParaView; there is
  no seam on a sphere.
- A single level lies at height 0 (ParaView puts it at 1 or at the number of levels).

![CAM-SE surface pressure on the lon/lat map; quads crossing the 0/360 seam stick out at both edges](images/cam_se_map.png)

MPAS output (from the ocean, atmosphere or land-ice models of the Model for Prediction
Across Scales) is NetCDF too. When a `.nc` file has MPAS's mesh variables (`xCell`,
`cellsOnVertex`), auraFX reads it as MPAS instead of as a CF grid, the way ParaView's MPAS
reader does:

- Each MPAS cell center becomes a point, and the cells around each MPAS vertex are joined
  into a triangle (or a quad, on quad meshes). On a sphere, the points lie at the planet's
  radius in meters.
- Variables on cells (`temperature`, `ssh`…) become point data, and variables on vertices
  (`relativeVorticity`…) become cell data. Time steps are numbered 0, 1, 2…, and the date
  of each step (`xtime`) is kept with the data.
- For other dimensions than time and position, the first index is used, except the vertical
  levels, which you choose with the reader options below.

#### Reader options

Some readers have settings for the file they read. They appear under **Properties ›
Reader options** when the file is selected, and from Python they are set like any other
property, with `node.set(...)`. Changing one reads the file again; the choice is saved with
the state file. **Reset to defaults** goes back to the file as first opened.

An MPAS file offers:

| Option | Python | What it does |
|---|---|---|
| Vertical level | `vertical_level=2` | which level of 3-D variables is shown (0 is the top of the ocean, the bottom of the atmosphere) |
| All levels (multilayer) | `multilayer=True` | stacks every level: each triangle becomes a column of wedges, and points carry each level's values |
| Layer thickness | `layer_thickness=10000` | the distance between levels in the multilayer view, in meters on the sphere and degrees on the map; 0 picks one so that the whole stack is easy to see |
| Layers go up (atmosphere) | `atmosphere=True` | stacks levels outwards (or upwards on the map) instead of downwards |
| Layout | `layout="Lat/Lon map"` | `"Sphere"` (default) or a flat map with longitude and latitude in degrees |
| Map center longitude | `center_lon=0` | the longitude in the middle of the map (180 by default) |

```python
ocean = afx.load("output.nc")
ocean.set(vertical_level=10)                         # the 11th level down
ocean.set(layout="Lat/Lon map", multilayer=True)     # all levels, as a flat 3-D map
```

On the map, cells that cross its left or right edge are drawn on both sides, so the map
reaches a little beyond 360°, as in ParaView.

Some things differ from ParaView:

- ParaView adds a point at the planet's center. It collapses onto that point the triangles at
  a mesh boundary and, when the file has `maxLevelCell`, the triangles that touch land. auraFX
  leaves those triangles out, so land shows as holes. In the multilayer view, the levels
  below the sea floor are left out too.
- The default layer thickness of 0 picks a visible spacing; ParaView uses 10000 everywhere.
- ParaView 6.1.1 fills point values in the multilayer view with garbage numbers (around
  10<sup>35</sup>); auraFX gives each point its level's value.
- Reader options do not reach files opened on another machine through a server yet.

![MPAS ocean output: temperature at the top level on the sphere, with land left out](images/mpas_ocean.png)

![The same file as a lon/lat map with all four levels stacked; the reader options are under Properties](images/mpas_options.png)

### GIS rasters

A GeoTIFF is a TIFF that also records where it lies on the map. auraFX notices these tags and
reads the file as a map raster instead of as a picture. Esri ASCII grids (`.asc`), a common
text format for elevation models, are read the same way. Open them with **File › Open** or
`afx.load("dem.tif")`.

- The raster becomes image data with one cell per pixel, placed at its map coordinates
  (meters for UTM, degrees for longitude/latitude), with north up.
- Values are cell data. A single band, such as an elevation model, is named `Elevation`, as
  in ParaView. In a file with several bands, the first is `Elevation` and the others are
  `Band 2`, `Band 3`… An RGB(A) image becomes one `Elevation` array with 3 or 4 components.
- Pixels equal to the file's no-data value become NaN and are drawn grey (after Warp By
  Scalar they leave a hole, since a NaN height has no place). ParaView hides
  those cells instead.
- The field data holds `NO_DATA_VALUE` per band and `MAP_PROJECTION`. For a GeoTIFF that is
  the EPSG code (`EPSG:32633`); ParaView writes a PROJ string instead. For an ASCII grid it is
  the text of the `.prj` file beside it.
- 8- to 64-bit integer and floating-point bands are read, compressed or not, in strips or
  tiles, with the bands stored together or one after another. A pixel-is-point GeoTIFF is
  moved half a pixel to its corner, as GDAL does.

To see an elevation model as terrain, move the values to the points with **Cell Data to
Point Data**, then raise them with **Warp By Scalar**:

```python
dem = afx.load("dem.tif")
terrain = dem.cell_data_to_point_data().warp_by_scalar(array="Elevation", scale=1.0)
terrain.set(color_by="Elevation")
```

Other GDAL raster formats (Erdas `.img`, NetCDF through GDAL, JPEG 2000…) are not read yet.

![A GeoTIFF elevation model raised into terrain (heights doubled); the missing far corner has no data](images/geotiff_terrain.png)

### GIS vectors

Maps of points, lines and areas, such as countries, roads, rivers, wells or survey sites, open
like any other file:

| Format | Files |
|---|---|
| ESRI Shapefile | `name.shp` with `name.dbf` (attributes) and `name.prj` (coordinate system) beside it; open the `.shp` |
| GeoJSON | `.geojson`, or a `.json` file that holds GeoJSON |
| KML (Google Earth) | `.kml`; each folder is a layer |
| GeoPackage | `.gpkg`; each feature table is a layer |

What you get:
- The coordinates stay as stored: longitude and latitude in degrees for most GeoJSON and KML
  files, or meters for projected shapefiles. Heights (the third coordinate) are kept, so a
  file with heights in meters on a map in degrees looks very tall. Flatten it with
  **Transform** (scale z by 0) if that gets in the way.
- Points become vertices, lines become polylines, and polygon rings become closed lines, as
  in ParaView.
- Each feature's attributes become cell data on its cells: whole numbers as integers,
  decimals as doubles, and anything else (names, dates) as text. 64-bit integers stay
  numbers; ParaView turns them into text.
- `MAP_PROJECTION` in the field data holds the coordinate system: the `.prj` text, the EPSG
  code of a GeoPackage layer, or `EPSG:4326` (longitude/latitude) for GeoJSON and KML.

**Reader options** (Properties › Reader options, or `node.set(...)`):

| Option | Choices |
|---|---|
| **Layer** (files with several layers) | **All** or one layer by name |
| **Blocks** | **One per layer** (default: one block per layer, a row of attributes per feature), or **One per feature (ParaView)**, which is ParaView's layout and slow for thousands of features |
| **Polygons** | **Outlines (ParaView)**, or **Filled**: triangles covering each area, with holes left open, to color areas by an attribute |
| **Feature ids** | adds the feature number as `_vtkPedigreeIds` |

![Countries from a shapefile, filled and colored by population (log scale), rivers from GeoJSON and cities as points](images/gis_vectors.png)

```python
countries = afx.load("countries.shp")
countries.set(polygons="Filled")
countries.set(color_by="pop", log_scale=True)
roads = afx.load("world.gpkg")
roads.set(layer="roads")                      # one layer of a GeoPackage
```

Checked against ParaView 6.1.1's GDAL vector reader on shapefiles, GeoJSON and GeoPackage
files written by GDAL: with one block per feature, every point, cell and attribute is the
same. ParaView reads only the first layer of a file unless told otherwise; auraFX reads all of
them. ParaView's GDAL cannot read KML, so the KML reader was checked against the same data
saved as GeoJSON.

Not read yet: KMZ (zipped KML), GML, FlatGeobuf, MapInfo, file geodatabases, and curved
geometry (arcs). Shapefile multipatches (3-D surfaces) are skipped, as ParaView does.

![Sea surface temperature from a NetCDF CF file, on the globe](images/netcdf_globe_view.png)

### AMR data (adaptive mesh refinement)

AMR simulations (AMReX, BoxLib, Castro, MAESTRO, Nyx, WarpX…) store their domain as levels of
boxes: a coarse level everywhere and finer boxes where more detail was needed. auraFX reads:

- **AMReX / BoxLib plotfiles**: a folder like `plt00100` with a `Header` file. Open the
  `Header` file in **File › Open** (choose "All files"), drop the folder on the window, or
  `afx.load("plt00100")`.
- **VTK AMR files** (`.vthb`, `.vth`), as ParaView writes them.

What you get:
- One block per level (**Level 0**, **Level 1**, ...), each holding that level's boxes as image
  data with the variables as cell data. The **Information** panel lists them.
- Coarse cells that a finer box covers are marked as hidden in `vtkGhostType`, as in ParaView.
  Surfaces, slices, clips, contours, thresholds, Integrate Variables, Resample To Image and
  probing skip them, so every place is shown and counted once, at its finest level.
- AMReX vector variables written as `amrexvec_<name>_x`, `_y`, `_z` become one vector
  `<name>`. In 2-D files the missing z is 0. ParaView puts the file's first variable there
  instead.
- The plotfile's time is in the field data (`TimeValue`). Single- and double-precision
  plotfiles are read.
- **Outline** draws the outline of every box, which shows where the refinement is.
- Contours of cell data are stitched across levels (see below).

![An AMReX plotfile: the outlines of its two coarse boxes and one finer box, and a slice through both levels colored by density](images/amr_view.png)

```python
amr = afx.load("plt00100")                 # the plotfile folder
amr.set(representation="Outline")          # where the refinement is
s = amr.slice(normal=(0, 0, 1))            # finest data on the cut
s.set(color_by="density")
lev = amr.overlapping_amr_level_ids()      # LevelIdScalars: each cell's level
```

**Sources › AMR Gaussian Pulse** makes ParaView's small AMR test dataset: a coarse box and
two finer boxes holding a Gaussian pulse, in 2-D or 3-D. **Filters › Overlapping AMR Level
Ids** adds the cell array `LevelIdScalars` with each cell's level.

Checked against ParaView 6.1.1: the AMR Gaussian Pulse source (cells, values, covered cells),
the `.vthb` reader on files ParaView wrote, the AMReX reader on test plotfiles
(`scripts/gen_amrex.py`; every box's origin, size, sums and covered cells), and on the AMR data
Integrate Variables (the volume counts each place once), slices, Threshold, Extract Surface,
Cell Data to Point Data and Resample To Image.

#### Contours of AMR data

Values of AMR data sit at cell centers, and cells change size where levels meet. Averaging
them to points box by box, which is what ParaView's Contour does, leaves gaps and overlaps
along every level boundary. **Filters › AMR Contour** joins the cell centers of all levels into
one mesh, the *dual grid*. Where a finer box meets coarser cells, its border cells reach out
to the coarse cell centers, so the isosurface continues across the boundary without a crack.
The other cell arrays are interpolated onto the surface. **Contour** on a cell array of AMR
data does the same automatically. In 2-D data the result is closed isolines.

![The density isosurface of an AMReX plotfile: one closed surface across the fine box (smaller triangles) and the coarse boxes, colored by temperature](images/amr_contour.png)

```python
c = amr.amr_contour(array="density", values=[0.3])
c = amr.contour(array="density", values=[0.3])     # the same, for AMR cell data
dual = amr.amr_dual_grid()                          # cell values on one stitched mesh
cut = dual.clip(type="Scalar", array="density", value=0.3)   # a crack-free "dual clip"
```

**Filters › AMR Dual Grid** gives that mesh itself: one point per cell center, carrying all the
cell arrays as point data. Clip, Slice, Stream Tracer and other filters that interpolate point
data work on it across levels without seams; ParaView's AMR Dual Clip works this way too.

How it was checked: on a test plotfile whose density isosurface is a sphere crossing from the
fine box into the coarse ones, the contour is closed (every edge shared by exactly two
triangles, the topology of a sphere), its area is within 0.2 % of the sphere's, and its center
is within 0.001 of the sphere's center. The box-by-box contour had 346 open edges there.
ParaView's own AMR Contour and AMR Dual Clip are material-interface tools for CTH files and
refuse this kind of AMR data, so there is no ParaView result to compare with.

Not read yet: plotfiles without FAB headers (the "NoFabHeader" variants), AMReX particles,
Enzo, FLASH and Chombo files. Not available yet: ParaView's CTH material filters (AMR
Fragments, Material Interface) and Resample AMR (Resample To Image does the same job).

### Hyper tree grids

A *hyper tree grid* (HTG) starts from a grid of root cells. Each root cell is the top of a
tree: a refined cell splits into 2×2×2 children (3×3×3 with branch factor 3; 2×2 or 3×3 in
2-D), and so on down to the leaves. Every cell of every tree, coarse or leaf, can hold values.
A *mask* can hide cells. It is VTK's tree-based format for adaptive meshes. auraFX reads VTK's
`.htg` files (XML version 2, as ParaView 5.10 and later write them).

What you get:
- The grid is drawn as its visible leaves. Every filter that works on cells (Slice, Clip,
  Threshold, Contour after Cell Data to Point Data, Cell Centers, Integrate Variables...) sees
  those leaves as voxels (pixels in 2-D) with their values.
- **Information** shows the dimension, branch factor, number of visible leaves and depth.
  The cell count is the number of all cells (coarse ones included), as in ParaView.
- Saving writes the leaves as an unstructured grid (`.vtu` and the other formats). `.htg`
  files cannot be written yet.

**Sources:**
- **Hyper Tree Grid** builds a grid from a text descriptor, as ParaView's source does.
  Levels are separated by `|`. Each level lists `R` (refined) or `.` (leaf) for every cell
  of that level, all trees in order; spaces are ignored. **Dimensions** (points per axis; 1
  makes an axis flat), **Branch factor**, **Max depth**, **Grid scale**, and **Use mask** with
  a **Mask** string of the same layout (`0` hides a leaf).
- **Random Hyper Tree Grid** refines cells at random. **Split fraction** is each cell's
  chance of being refined, down to **Max depth**. It uses ParaView's random sequence, so the
  same **Seed** gives the same grid as in ParaView. ParaView's masking option is not
  available yet.

**Filters** (Filters › Data):

| Filter | What it does |
|---|---|
| **Hyper Tree Grid To Unstructured Grid** | the visible leaves as an unstructured grid, with `OriginalIds` (each leaf's cell index) |
| **Hyper Tree Grid Depth Limiter** | cuts every tree at a depth: cells at that depth become leaves (Depth 0 keeps only the root cells) |
| **Hyper Tree Grid Generate Fields** | `CellSize` (volume, area or length of every cell), `ValidCell` (1 on visible leaves), `CellCenter`, and `TotalVisibleVolume` in the field data |
| **Hyper Tree Grid Evaluate Coarse** | fills the values of refined cells from their children: Min, Max, Sum, Average, Unmasked Average, Elder Child (the first child) or Splatting Average |

![A random 2-D hyper tree grid (a quadtree) drawn with its cell edges, colored by depth](images/hyper_tree_grid.png)

```python
h = afx.random_hyper_tree_grid(dimensions=(7, 5, 1), max_depth=5, split_fraction=0.45, seed=3)
h.set(color_by="Depth", representation="Surface With Edges")
t = afx.hyper_tree_grid(dimensions=(3, 3, 1), max_depth=3, descriptor="R... | .R.. | ....")
coarse = h.hyper_tree_grid_evaluate_coarse(operator="Max")
top = h.hyper_tree_grid_depth_limiter(depth=2)
leaves = afx.load("tree.htg").hyper_tree_grid_to_unstructured_grid()
```

Checked against ParaView 6.1.1 on `.htg` files it wrote (3-D, 2-D with branch factor 3,
masked):
- the cell and visible-leaf counts, and the leaves in the same order (their `OriginalIds`
  are ParaView's);
- Depth Limiter, Generate Fields and all seven Evaluate Coarse operators;
- both sources, which build the same grids as ParaView from the same settings.

ParaView's To Unstructured Grid leaves the `OriginalIds` of masked files partly unfilled (zeros
at the end); auraFX gives every leaf its index. ParaView deprecates its HTG Cell Centers and
Axis Reflection filters in 6.0: Cell Centers works on hyper tree grids directly, and Axis
Reflection is not provided.

Not available yet: writing `.htg`, version 1 `.htg` files, HTG To Dual Grid, HTG Geometry
(drawing gives the same picture), Ghost Cells and Feature Edges.

## Writers

| Format | Extensions | Notes |
|---|---|---|
| VTK XML | `.vtu .vtp .vti .vtr .vts .vtm` | appended binary; multiblocks as a folder of pieces |
| VTK legacy | `.vtk` | |
| VTKHDF | `.vtkhdf` | all dataset kinds; multiblocks as a collection; time series with `all_times=True`; written with auraFX's own HDF5 encoder (read by h5py and VTK) |
| Exodus II | `.exo` | blocks, nodal and element variables, every time step with `all_times=True` |
| EnSight Gold | `.case` | C binary; one part per block (image, rectilinear and structured grids as blocks, the rest as elements of every standard type, polygons and polyhedra); every time step with `all_times=True`, the geometry written once when it does not change |
| CGNS | `.cgns` | HDF5 storage (CGNS 4): a zone per block, image/rectilinear/structured grids as structured zones, the rest as one element section (MIXED when the cell types differ) plus NGON/NFACE for polygons and polyhedra; vectors as `NameX/NameY/NameZ`; with `all_times=True` one solution per step (and coordinates per step when the points move), but the cells must stay the same |
| Xdmf 3 | `.xmf` + `.h5` | the description in the `.xmf`, the arrays in an HDF5 file beside it; block trees as spatial collections, time steps as a temporal collection that shares unchanged points and cells |
| CSV, STL, PLY | | |
| Scenes | `.glb .obj .x3d .wrl` | see [Exporting](exporting.md) |
| Pictures | `.png .svg`, GIF/MP4 animations | see [Exporting](exporting.md) |

Files written by the VTK XML, VTKHDF, Exodus, EnSight, Xdmf and CGNS writers open in ParaView
with the same points, cells and values. EnSight groups the cells of each part by type, and
CGNS puts polyhedra after the other cells, so cells can come back in a different order (each
keeps its values). Cell types a format does not have (Lagrange and Bezier cells, for example)
are written by their corner points. A CGNS zone holds cells of one dimension: in a volume
mesh, boundary faces and lines are left out.

![The can crash saved with all 44 time steps as EnSight (left), Xdmf (top right) and CGNS (bottom right), opened again](images/writers.png)

## Curved cells

VTK's Lagrange and Bezier cells of any order, quadratic cells and serendipity cells are read
with their node numbering, drawn curved and cut by the filters along their curvature; see
[Volumes, point clouds and curved cells](representations.md#curved-cells).

## Your own formats

A [plugin](plugins.md) reader is a Python function with `@afx.reader(extensions=[...])`; it
takes precedence over the built-in reader for its extensions and appears in the Open dialog.
