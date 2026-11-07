# Geoscience: a GeoTIFF elevation model (testdata/rasters/dem.tif) raised into terrain; a turntable.
import aurafx as afx
d = afx.load(ROOT + "/testdata/rasters/dem.tif"); d.hide()
p = d.cell_data_to_point_data(); p.hide()
w = p.warp_by_scalar(array="Elevation", scale=2.0)
w.set(color_by="Elevation", cmap="Viridis")
afx.view([0.55, -1.0, 0.75], up=[0, 0, 1]); afx.reset_camera()
afx.save_animation(OUT, fps=20, orbit=80)
