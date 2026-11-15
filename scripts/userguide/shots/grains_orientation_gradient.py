# Orientation Gradient (no file needed): the default columnar grains (300 x 300 x 450 box of
# 5-unit voxels, <100> near the build direction Z), every grain turning by 0.03 degrees per
# unit of height about its own axis from its centroid, with a smooth spread of 0.8 degrees
# and sub-grain cells 25 across misoriented by 1 degree on average; colored by each cell's
# misorientation from its grain's average orientation (degrees).
import aurafx as afx
g = afx.columnar_grains()
g.hide()
t = g.orientation_gradient(rate=0.03, amplitude=0.8, cell_size=25, cell_angle=1)
t.hide()
m = t.reference_misorientations()
m.set(color_by="FeatureReferenceMisorientations", cmap="Inferno")
afx.view([0.7, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
