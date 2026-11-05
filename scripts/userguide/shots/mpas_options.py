# Reader options: the MPAS test file on a lon/lat map with all four levels stacked (multilayer),
# colored by temperature; the Properties panel shows the reader's options.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/mpas_ocean.nc")
d.set(layout="Lat/Lon map", multilayer=True, layer_thickness=8)
d.set(color_by="temperature", cmap="Cool to Warm")
afx.time(2); d.info()
afx.view([0.35, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
