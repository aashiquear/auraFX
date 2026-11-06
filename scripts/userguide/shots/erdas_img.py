# ERDAS Imagine: the run-length compressed elevation model (testdata/rasters/dem.img) raised
# into terrain (heights doubled) and colored by height; the missing corner has no data.
import aurafx as afx
dem = afx.load(ROOT + "/testdata/rasters/dem.img")
dem.hide()
terrain = dem.cell_data_to_point_data().warp_by_scalar(array="Elevation", scale=2.0)
terrain.set(color_by="Elevation", cmap="Viridis (matplotlib)")
terrain.show(); terrain.activate()
afx.view([-0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
