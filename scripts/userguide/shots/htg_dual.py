# Hyper Tree Grid To Dual Grid: a random 2-D hyper tree grid (its leaves as thin gray lines)
# with its dual grid on top: a point at every leaf center, joined across the leaves, colored by
# depth.
import aurafx as afx
h = afx.random_hyper_tree_grid(dimensions=(5, 4, 1), max_depth=4, split_fraction=0.45, seed=3)
h.set(representation="Wireframe", solid_color=(0.45, 0.45, 0.5), color_by=None)
d = h.hyper_tree_grid_to_dual_grid()
d.set(color_by="Depth", cmap="Viridis", representation="Surface With Edges", edge_color=(0.02, 0.02, 0.04), lighting=False, opacity=0.8)
d.activate(); d.info()
afx.view("+z"); afx.reset_camera()
