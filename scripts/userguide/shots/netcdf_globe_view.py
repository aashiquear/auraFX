# NetCDF CF on the globe: the synthetic sea-surface temperature file
# (testdata/netcdf/globe_sst.nc, from scripts/gen_netcdf_globe.py) drawn on the sphere, land
# left out as fill values.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/globe_sst.nc")
d.set(color_by="sst", cmap="Cool to Warm")
afx.view([1.0, 0.6, 0.5], up=[0, 0, 1]); afx.reset_camera()
