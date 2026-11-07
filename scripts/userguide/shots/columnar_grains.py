# Columnar grains of an additive build (no file needed): a 300 x 300 x 450 box of 5-unit
# voxels, 50-wide columns with a median height of 150, tilted by about 6 degrees, a <100>
# fibre along the build direction Z within 10 degrees; colored by the inverse pole figure key
# for X, which shows the grains' different rotations about Z.
import aurafx as afx
g = afx.columnar_grains()
g.hide()
c = g.orientation_colors(direction=(1, 0, 0))
c.set(color_by="IPFColor")
afx.view([0.7, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
