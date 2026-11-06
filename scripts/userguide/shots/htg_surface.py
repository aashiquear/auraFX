# Hyper Tree Grid Geometry and Feature Edges: a random 3-D hyper tree grid with 40 % masked,
# its surface (the walls of the visible leaves) colored by depth, and its feature edges as dark
# lines on top.
import aurafx as afx
h = afx.random_hyper_tree_grid(dimensions=(4, 4, 3), max_depth=3, split_fraction=0.5, seed=5, masked_fraction=0.4)
s = h.hyper_tree_grid_geometry()
s.set(color_by="Depth", cmap="Viridis", opacity=0.7)
e = h.hyper_tree_grid_feature_edges()
e.set(color_by=None, solid_color=(0.05, 0.05, 0.08))
h.hide(); s.show(); e.show(); s.activate(); s.info()
afx.view([1.0, -1.3, 0.9], up=[0, 0, 1]); afx.reset_camera()
