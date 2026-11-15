# In-plane orientation gradient (no file needed): 40 Voronoi grains in a 96 x 96 x 12 volume,
# no gradient along z and 0.3 degrees per unit length across every grain (the same rate for
# all); the top seen from above, colored by each cell's misorientation from its grain's
# average orientation (degrees): zero along a line through each grain's centroid and growing
# to both sides, in a direction that differs from grain to grain.
import aurafx as afx
g = afx.voronoi_grains(cells=[96, 96, 12], grains=40, seed=4)
g.hide()
t = g.orientation_gradient(rate=0.0, amplitude=0.0, in_plane_rate=0.3, rate_spread=0.0)
t.hide()
m = t.reference_misorientations()
m.set(color_by="FeatureReferenceMisorientations", cmap="Inferno")
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)
