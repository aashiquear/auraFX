# Geoscience: an MPAS ocean file (testdata/netcdf/mpas_ocean.nc) on the sphere, the top-level
# temperature; a turntable.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/mpas_ocean.nc")
d.set(color_by="temperature", cmap="Cool to Warm")
afx.time(2)
afx.view([-0.3, -1.0, 0.25], up=[0, 0, 1]); afx.reset_camera()
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=20, orbit=80)
