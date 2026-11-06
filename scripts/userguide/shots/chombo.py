# Chombo AMR: the 2-D test file (testdata/amr/chombo2d.hdf5, three levels, ghost cells dropped)
# drawn with its cell edges, colored by density; the finer boxes show as smaller cells.
import aurafx as afx
c = afx.load(ROOT + "/testdata/amr/chombo2d.hdf5")
c.set(color_by="density", cmap="Viridis", representation="Surface With Edges", edge_color=(0.05, 0.05, 0.08), lighting=False)
c.activate(); c.info()
afx.view("+z"); afx.reset_camera()
