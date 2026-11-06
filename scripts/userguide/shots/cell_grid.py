# Cell grids: VTK's two-hexahedron DG test file (testdata/cellgrid/dgHexahedra.dg) colored by a
# discontinuous field, with Cell Grid Centers as spheres colored by the edge (HCURL) field.
import aurafx as afx
g = afx.load(ROOT + "/testdata/cellgrid/dgHexahedra.dg")
g.set(color_by="scalar2", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.05, 0.05, 0.08), opacity=0.45)
c = g.cell_grid_centers()
c.set(color_by="curl1", cmap="Viridis", representation="Points", point_size=44, points_as_spheres=True)
g.show(); g.activate(); g.info(); c.info()
afx.view([0.6, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
