# CAM-SE output (a CAM-SE-style test file and its connectivity file, found automatically):
# surface pressure on the lon/lat map, the cubed-sphere quads drawn; quads crossing the 0/360
# seam stick out on the left and right as in ParaView.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/cam_se.nc")
d.set(variables="Surface [time, ncol]")
d.set(color_by="PS", cmap="Viridis", representation="Surface With Edges")
d.info()
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT, width=900)
