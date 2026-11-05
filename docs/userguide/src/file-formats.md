# File formats

Every reader was checked against ParaView 6.1.1: the same points, cells, bounds and value
ranges on the same files. Where auraFX differs on purpose, the table says so.

## Readers

| Format | Extensions | Notes |
|---|---|---|
| Exodus II | `.e .exo .ex2 .g .gen` | NetCDF-3 and NetCDF-4; blocks, side and node sets, nodal/element/global variables, time steps, displacements |
| VTK legacy | `.vtk` | ASCII and binary, all dataset types |
| VTK XML | `.vtu .vtp .vti .vtr .vts .vtt`, parallel `.pvtu …`, `.vtm`, `.pvd` | inline, appended, base64; zlib and LZ4; curved (Lagrange/Bezier) cells |
| VTKHDF | `.vtkhdf .hdf` | image data, unstructured grids, poly data, partitioned collections with their assembly, time steps; chunked and gzip storage; no HDF5 library needed |
| OpenFOAM | `.foam` | ASCII/binary/gzip, decomposed and reconstructed cases, time directories, boundary patches |
| EnSight Gold | `.case .encas` | ASCII and C binary, all element types, structured blocks, time sets, wildcard file names |
| CGNS | `.cgns` | HDF5 and ADF storage; structured and unstructured zones, MIXED and NGON/NFACE sections, solutions, time steps |
| Xdmf 2/3 | `.xmf .xdmf` | inline, HDF5 and raw binary data; collections; `xi:include` and XPath references (FEniCS/Dolfinx); HyperSlab and JOIN items |
| NetCDF (CF) | `.nc .nc4 .cdf` | classic and NetCDF-4; lon/lat grids on a sphere with cell bounds, rectilinear otherwise; time steps; packing; fill values become NaN (grey) |
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

Other GDAL formats (Erdas `.img`, NetCDF through GDAL, JPEG 2000…) and vector files
(shapefiles, GeoJSON) are not read yet.

![A GeoTIFF elevation model raised into terrain (heights doubled); the missing far corner has no data](images/geotiff_terrain.png)

![Sea surface temperature from a NetCDF CF file, on the globe](images/netcdf_globe_view.png)

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
