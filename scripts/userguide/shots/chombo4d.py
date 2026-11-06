# 4-D Chombo: the space grid of testdata/amr/chombo4d.hdf5 (two boxes) colored by f summed
# over velocity space, with its cell edges.
import aurafx as afx
c = afx.load(ROOT + "/testdata/amr/chombo4d.hdf5")
c.set(color_by="f_sum", cmap="Viridis (matplotlib)", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12), lighting=False)
c.show(); c.activate(); c.info()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
