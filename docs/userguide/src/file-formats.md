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
| CGNS | `.cgns` | HDF5 storage; structured and unstructured zones, MIXED and NGON/NFACE sections, solutions, time steps; ADF storage is not read |
| Xdmf 2/3 | `.xmf .xdmf` | inline, HDF5 and raw binary data; collections; `xi:include` and XPath references (FEniCS/Dolfinx); HyperSlab and JOIN items |
| NetCDF (CF) | `.nc .nc4 .cdf` | classic and NetCDF-4; lon/lat grids on a sphere with cell bounds, rectilinear otherwise; time steps; packing; fill values become NaN (grey) |
| Tecplot | `.dat .plt .tec .tp` | ASCII (ordered and FE zones, BLOCK/POINT, cell-centered variables) and binary `.plt`; `.szplt` is not read |
| PLOT3D | `.xyz .x .p3d .bin` + `.q` | layout detected from the file (Fortran/C binary, byte order, precision, multi-grid, 2-D/3-D, IBLANK), ASCII |
| LAMMPS | `.dump .lammpstrj .atom` (+`.gz`), `.data .lmp` | multi-frame dumps with any columns, triclinic boxes; data files with bonds |
| XYZ / extended XYZ | `.xyz .extxyz` | multi-frame; properties and lattice from the comment line |
| PDB | `.pdb .ent` | all atoms (hydrogens kept), MODELs as time steps, CONECT and distance bonds, residues and chains |
| Gaussian cube | `.cube` | the density as a volume in Å, plus the atoms with bonds |
| VASP | `POSCAR`, `CONTCAR`, `CHGCAR`, `LOCPOT`, `ELFCAR`, `PARCHG` (by name), `.vasp` | atoms with bonds and the lattice; volumes on the cell's axes (charge per Å³) |
| Images | `.png .jpg .bmp .tif` | as image data; multi-page TIFFs and `img_*.png` wildcards as volumes |
| Surfaces | `.stl .ply .obj` | |
| CSV / TSV | `.csv .tsv .txt` | delimiter and header detection; tables with x, y, z columns become points |

Numbered files (`run_0001.vtu`…) open as a series when one of them is opened.

![Sea surface temperature from a NetCDF CF file, on the globe](images/netcdf_globe_view.png)

## Writers

| Format | Extensions | Notes |
|---|---|---|
| VTK XML | `.vtu .vtp .vti .vtr .vts .vtm` | appended binary; multiblocks as a folder of pieces |
| VTK legacy | `.vtk` | |
| VTKHDF | `.vtkhdf` | all dataset kinds; multiblocks as a collection; time series with `all_times=True`; written with auraFX's own HDF5 encoder (read by h5py and VTK) |
| Exodus II | `.exo` | blocks, nodal and element variables, every time step with `all_times=True` |
| EnSight Gold | `.case` | C binary; one part per block (image, rectilinear and structured grids as blocks, the rest as elements of every standard type, polygons and polyhedra); every time step with `all_times=True`, the geometry written once when it does not change |
| Xdmf 3 | `.xmf` + `.h5` | the description in the `.xmf`, the arrays in an HDF5 file beside it; block trees as spatial collections, time steps as a temporal collection that shares unchanged points and cells |
| CSV, STL, PLY | | |
| Scenes | `.glb .obj .x3d .wrl` | see [Exporting](exporting.md) |
| Pictures | `.png .svg`, GIF/MP4 animations | see [Exporting](exporting.md) |

Files written by the VTK XML, VTKHDF, Exodus, EnSight and Xdmf writers open in ParaView with
the same points, cells and values. EnSight groups the cells of each part by type, so cells come
back in a different order (each keeps its values); other cell types that EnSight or Xdmf do not
have (Lagrange and Bezier cells, for example) are written by their corner points.

![The can crash saved as EnSight (left) and Xdmf (right) with all 44 time steps, opened again](images/writers.png)

## Curved cells

VTK's Lagrange and Bezier cells of any order, quadratic cells and serendipity cells are read
with their node numbering, drawn curved and cut by the filters along their curvature; see
[Volumes, point clouds and curved cells](representations.md#curved-cells).

## Your own formats

A [plugin](plugins.md) reader is a Python function with `@afx.reader(extensions=[...])`; it
takes precedence over the built-in reader for its extensions and appears in the Open dialog.
