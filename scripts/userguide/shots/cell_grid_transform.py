# Cell Grid Transform: VTK's two-hexahedron cell grid (testdata/cellgrid/dgHexahedra.dg) and a
# copy turned 30 degrees about z, stretched in z and moved aside, both colored by scalar2.
import aurafx as afx
g = afx.load(ROOT + "/testdata/cellgrid/dgHexahedra.dg")
t = g.cell_grid_transform(translate=(3.2, 0.5, 0), rotate=(0, 0, 30), scale=(1, 1, 2))
for n in (g, t):
    n.set(color_by="scalar2", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.05, 0.05, 0.08), range=(0, 3))
g.show(); t.show(); t.activate(); t.info()
afx.view([0.4, -1.0, 0.6], up=[0, 0, 1]); afx.reset_camera()
