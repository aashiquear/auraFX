# Boundary layers (no file needed): a box of 10 x 10 x 10 hexahedra with four layers grown under
# its top face (z+, first layer 0.01 thick, each next 1.6 times thicker) and under its x- face,
# drawn with edges.
import aurafx as afx
b = afx.box_mesh()
b.hide()
l = b.boundary_layers(side_set="z+", layers=4, first=0.01, growth=1.6)
l.hide()
l2 = l.boundary_layers(side_set="x-", layers=4, first=0.01, growth=1.6)
l2.set(color_by=None, solid_color=(0.55, 0.7, 0.9), representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0.5, -1.0, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
