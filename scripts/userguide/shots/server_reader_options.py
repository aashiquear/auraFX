# Reader options for a file read by a server ("This computer" runs one locally): the MPAS
# test file on the server at vertical level 2; Properties lists the options the server reported.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/mpas_ocean.nc", host="local")
d.info()                      # the server answers with the file's options
d.set(vertical_level=2)
d.set(color_by="temperature", cmap="Cool to Warm")
d.info()
afx.view([-0.3, -1.0, 0.25], up=[0, 0, 1]); afx.reset_camera()
