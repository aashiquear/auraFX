# A POP-style ocean file: sea surface height on the curvilinear grid given by the 2-D
# TLONG/TLAT coordinates (the surface grid picked under Reader options > Variables on), on
# the unit sphere as ParaView's NetCDF reader places it; the island has no data (grey).
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/pop_ocean.nc")
d.set(variables="(nlat, nlon)")
d.set(color_by="SSH", cmap="Cool to Warm", representation="Surface With Edges")
d.info()
afx.view([-0.9, 1.0, 0.1], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=900)
