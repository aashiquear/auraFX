# FLASH and Enzo AMR: the 3-D FLASH test file (testdata/amr/flash3d_hdf5_plt_cnt_0007) as block
# outlines with a slice colored by density, and the Enzo test dump (three levels) beside it,
# sliced across its finest grid and colored by Density.
import aurafx as afx
f = afx.load(ROOT + "/testdata/amr/flash3d_hdf5_plt_cnt_0007")
f.set(representation="Outline", color_by=None, solid_color=(0.15, 0.15, 0.2))
fs = f.slice(origin=(0.5, 0.25, 0.2), normal=(0, 0, 1))
fs.set(color_by="dens", cmap="Viridis", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.1))
e = afx.load(ROOT + "/testdata/amr/enzo/DD0003/data0003.hierarchy")
e = e.transform(translate=(1.4, 0, 0))  # beside the FLASH data
e.set(representation="Outline", color_by=None, solid_color=(0.15, 0.15, 0.2))
es = e.slice(origin=(1.9, 0.4, 0.4), normal=(0, 0, 1))
es.set(color_by="Density", cmap="Inferno", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.1))
f.show(); e.show(); e.activate(); e.info()
afx.view([0.3, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()
