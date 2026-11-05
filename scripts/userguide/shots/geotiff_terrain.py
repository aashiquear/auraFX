# A GeoTIFF elevation model (written by GDAL, in UTM meters) as 3D terrain: the pixel values
# moved to the points, then raised by twice their height. The missing corner has no data.
import aurafx as afx
d = afx.load(ROOT + "/testdata/rasters/dem.tif")
p = d.cell_data_to_point_data()
w = p.warp_by_scalar(array="Elevation", scale=2.0)
w.set(color_by="Elevation", cmap="Viridis")
d.hide(); p.hide(); w.info()
afx.view([0.55, -1.0, 0.75], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=900)
