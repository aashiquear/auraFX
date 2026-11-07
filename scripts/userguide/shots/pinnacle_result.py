# A PINNACLE crystal-plasticity result read back (testdata/solver/pinnacle_plate: the plate with
# a hole of Plate Mesh, 30 Voronoi grains, pulled along x to 2 % strain; made with
# scripts/cases/pinnacle_plate.py). The grains come back by nearest point from the case's point
# file; the plate is colored by the equivalent plastic strain, with the grain boundaries on top.
import aurafx as afx
d = ROOT + "/testdata/solver/pinnacle_plate/"
res = afx.load(d + "fields_final.vtk")
res.hide()
pts = afx.load(d + "orientations_points.csv")
pts.hide()
g = res.assign_grains(pts, method="Nearest point", boundaries=True)
g.update()
g.set(color_by="equivalent_plastic_strain", cmap="Inferno (matplotlib)")
b = g.extract_set(set="grain boundaries")
b.set(color_by=None, representation="Wireframe", solid_color=(0.85, 0.85, 0.85), line_width=2)
g.activate()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)
