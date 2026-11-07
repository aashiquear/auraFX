# Grains on a mesh (no file needed): the dog-bone specimen mesh with 80 Voronoi grains
# assigned to its elements (one block per grain, a side set on the grain boundaries), colored
# by grain id.
import aurafx as afx
d = afx.dogbone_mesh(cell=0.1, grip_cell=0.2, layers=3)
d.hide()
g = d.assign_grains(grains=80, blocks=True, boundaries=True)
g.set(color_by="grain_id", cmap="Turbo", representation="Surface With Edges", edge_color=(0.15, 0.15, 0.15))
afx.view([0.2, -0.9, 1.0], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
