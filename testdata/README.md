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

Tests that need a missing file skip themselves.
