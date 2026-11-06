# Test data

Small files are committed. Larger ones are ignored by git and can be recreated:

| File | How to get it |
|---|---|
| `can.ex2`, `bake.e` | Copy from a ParaView install: `share/paraview-*/examples/` |
| `can_nc4.exo`, `disk_out_ref_nc4.exo` | NetCDF-4 (HDF5, zlib) copies of the ParaView Exodus samples, made with the netCDF4 Python module (`disk_out_ref_nc4.exo` is committed) |
| `fcc_melt.dump` | `python3 scripts/gen_lammps_dump.py` |
| VTK samples (`*.vt?`, `*.vtk`, `series/`, `group/`) | Written by ParaView 6.1.1 (`pvpython`), committed |
| `lagrange_annulus.vtu`, `bezier_annulus.vtu`, `lagrange_shapes.vtu`, `quadratic_shapes.vtu` | `python3 scripts/gen_higher_order.py` (curved Lagrange/Bezier cells; ParaView reference values in `crates/aurafx-filters/tests/filters.rs`), committed |
| `rotating.pvd`, `rotating/` | `python3 scripts/gen_rotating_flow.py` (rotation with speed 1 + t; exact particle paths known), committed |
| `cgns/mixed_adf.cgns`, `cgns/tets_adf.cgns` | `cgns/mixed.cgns` and `cgns/tets.cgns` copied into ADF storage by the CGNS library in ParaView: `pvpython scripts/gen_cgns_adf.py testdata/cgns/mixed.cgns testdata/cgns/mixed_adf.cgns` (the same for tets), committed |
| `cgns/mixed_adf_old.cgns` | `python3 scripts/gen_adf_old.py testdata/cgns/mixed_adf.cgns testdata/cgns/mixed_adf_old.cgns`: the old ADF layout (hex-text pointers), big-endian data, arrays split into two data chunks; the CGNS library reads it back to the same values, committed |
| `netcdf/mpas_ocean.nc` | `python3 scripts/gen_mpas.py`: MPAS-style ocean output on an icosahedral sphere (642 cells, 3 time steps, land in `maxLevelCell`); ParaView reference values in `mpas_matches_paraview`, committed |
| `rasters/*.tif`, `rasters/dem.asc` | `pvpython scripts/gen_rasters.py`: GeoTIFFs written by the GDAL library inside ParaView (elevation with no-data, RGB, 4 planar tiled bands, pixel-is-point) and an Esri ASCII grid of the same elevations; ParaView reference values in `rasters_match_paraview`, committed |
| `netcdf/pop_ocean.nc` | `python3 scripts/gen_pop.py`: POP-style ocean history (2-D TLONG/TLAT, depth in cm positive down, an island as fill values), committed |
| `netcdf/cam_se.nc`, `netcdf/cam_se_connectivity.nc` | `python3 scripts/gen_cam.py`: CAM-SE-style output on a cubed sphere (turned 10 degrees so no column sits on the seam) and its connectivity file; ParaView reference values for both in `cam_and_pop_match_paraview`, committed |
| `images/planet.jpg` | `python3 scripts/gen_texture.py`: a made-up planet map (equirectangular, 1024 × 512, wraps without a seam) for texture tests and the user guide, committed |
| `vectors/*.geojson`, `*.shp/.shx/.dbf/.prj`, `world.kml`, `world.gpkg` | `pvpython scripts/gen_vectors.py`: three small layers (countries with a hole and a multipolygon, rivers, cities) written as GeoJSON and converted by the GDAL library inside ParaView to shapefiles, KML and a GeoPackage; `vectors/paraview_reference.json` from `pvpython scripts/vectors_reference.py` holds ParaView's reading of them, committed |
| `vectors/*.fgb`, `points.fgb`, `world.gml` (+ `.xsd`), `world2.gml` (+ `.xsd`), `world.kmz` | `pvpython scripts/gen_vectors.py --only fgb,gml,kmz`: the three layers converted by ParaView's GDAL to FlatGeobuf (one file each) and GML 3.2 and GML 2 (all layers), 300 points as FlatGeobuf (a three-level spatial index), and `world.kml` zipped as KMZ; ParaView's reading of the FlatGeobuf files is in `vectors/paraview_reference.json`, committed |
| `vectors/pages.sqlite` | `python3 scripts/gen_sqlite_pages.py`: an SQLite file with 512-byte pages, interior pages and overflow blobs, for the SQLite reader under GeoPackage, committed |
| `amr/pulse.vthb`, `amr/pulse2d.vthb` (+ folders) | ParaView 6.1.1's AMR Gaussian Pulse source (3-D and 2-D) saved with its XMLUniformGridAMRWriter, committed |
| `amr/plt_3d`, `amr/plt_2d`, `amr/plt_3d_f32` | `python3 scripts/gen_amrex.py`: AMReX plotfiles with two levels (two coarse boxes, one finer), scalar and `amrexvec_` vector variables, double and single precision; ParaView reference values in `amr_files_match_paraview`, committed |
| `amr/flash3d_hdf5_plt_cnt_0007`, `amr/flash3d_ffv7_hdf5_chk_0007`, `amr/flash2d_hdf5_chk_0003`, `amr/enzo/DD0003/` | `pvpython scripts/gen_amr_files.py` (h5py): a 3-D FLASH grid (two root blocks, one refined) in the FLASH 3 and FLASH 2 layouts, a 2-D FLASH 2 file with three levels, and an Enzo dump with three levels (parameter file, hierarchy, boundary, HDF5 grid file); `amr/flash_enzo_reference.json` from `pvpython scripts/amr_files_reference.py` holds ParaView 6.1.1's reading, committed |
| `htg/tree3d.htg`, `htg/tree2d.htg`, `htg/tree3d_mask.htg` | ParaView 6.1.1's Hyper Tree Grid source (3-D branch factor 2; 2-D branch factor 3; 3-D with a mask) saved with its HyperTreeGridWriter (XML version 2); the descriptors are in `hyper_tree_grids_match_paraview`, committed |
| `htg/dual_reference.json` | `pvpython scripts/htg_dual_reference.py`: ParaView 6.1.1's Hyper Tree Grid To Dual Grid on the three `.htg` files and on eight source grids (1-D, 2-D, 3-D, masked descriptors, masked random grids): every cell's type, point places and Depth values, committed |
| `cellgrid/*.dg` | VTK's DG test files (VTK ExternalData, Testing/Data/dg*.dg; BSD): edges, hexahedra, mixed types, 19-node pyramid, pyramids, quadratic quadrilaterals, quadrilaterals, tetrahedra, triangles, wedges; `cellgrid/paraview_reference.json` from `pvpython scripts/cellgrid_reference.py` holds ParaView 6.1.1's To Unstructured Grid and Cell Grid Centers output, committed |
| `molecules/test.cml` | `python3 scripts/gen_cml.py`: a 20-atom test molecule (benzene ring with Cl, NH2, SH; water with a sodium ion; a lone bromine) with bonds and orders; ParaView reference bonds in `molecule_bonds_match_paraview`, committed |

Tests that need a missing file skip themselves.
