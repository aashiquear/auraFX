# Cell Grid Compute Sides: the wavelet clipped by a slanted plane (hexahedra, wedges,
# pyramids and tetrahedra), made a cell grid and reduced to the faces on its outside, drawn with
# their edges and colored by RTData.
import aurafx as afx
w = afx.load(ROOT + "/testdata/wavelet.vti")
c = w.clip(normal=(1, 1, 0.4), origin=(0, 0, 0))
g = c.convert_to_cell_grid()
s = g.cell_grid_compute_sides()
s.set(color_by="RTData", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.05, 0.05, 0.08))
w.hide(); c.hide(); g.hide()
s.show(); s.activate(); s.info()
afx.view([-0.9, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
