# Hyper tree grids: the Random Hyper Tree Grid source in 2-D (a quadtree), drawn with
# its cell edges and colored by depth; Properties shows the source's settings.
import aurafx as afx
h = afx.random_hyper_tree_grid(dimensions=(7, 5, 1), max_depth=5, split_fraction=0.45, seed=3)
h.set(color_by="Depth", cmap="Viridis", representation="Surface With Edges", edge_color=(0.02, 0.02, 0.04), lighting=False)
h.activate(); h.info()
afx.view("+z"); afx.reset_camera()
