# The crystal-plasticity example of the Sample pipelines chapter as it stands in the window: the
# solver's result and the case's point file, Assign Grains (nearest point, with the boundaries as
# a set), Warp By Vector and the grain boundaries, the plate colored by the plastic strain.
# Uses testdata/solver/pinnacle_plate_fine.
import aurafx as afx
d = ROOT + "/testdata/solver/pinnacle_plate_fine/"
res = afx.load(d + "fields_final.vtu"); res.hide()
pts = afx.load(d + "orientations_points.csv"); pts.hide()
g = res.assign_grains(pts, method="Nearest point", boundaries=True); g.hide()
w = g.warp_by_vector(vectors="displacement", scale=9.0)
w.set(color_by="equivalent_plastic_strain", cmap="Inferno")
b = w.extract_set(set="grain boundaries"); w.show()
b.set(color_by=None, representation="Wireframe", solid_color=(0.92, 0.92, 0.92), line_width=2)
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
