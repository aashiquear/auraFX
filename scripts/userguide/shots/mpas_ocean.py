# MPAS ocean output (an MPAS-style test file): the dual grid on the sphere, colored by the
# top-level temperature at the last time step; triangles over land are left out.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/mpas_ocean.nc")
d.set(color_by="temperature", cmap="Cool to Warm")
afx.time(2); d.info()
afx.view([-0.3, -1.0, 0.25], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=900)
