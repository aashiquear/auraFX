# Hyper tree grid material interfaces: a 2-D grid whose cells crossing a ring carry interface
# planes (testdata/htg/interface_plane.htg), drawn cut to its material with edges and colored
# by the interface type, and the interface lines alone (Hyper Tree Grid Geometry with Fill
# material off) 4 pixels wide in red on top.
import aurafx as afx
h = afx.load(ROOT + "/testdata/htg/interface_plane.htg")
h.set(representation="Surface With Edges", color_by="Intercepts", component=2, cmap="Viridis (matplotlib)", edge_color=(0.35, 0.35, 0.4))
l = h.hyper_tree_grid_geometry(fill_material=False)
l.set(color_by=None, solid_color=(0.95, 0.25, 0.1), line_width=4, lighting=False)
h.show(); l.show(); h.activate()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
