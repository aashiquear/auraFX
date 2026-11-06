# Cell Grid Probe: VTK's two-hexahedron cell grid (testdata/cellgrid/dgHexahedra.dg) as a
# wireframe, probed with a cloud of random points; the points found in the cells are drawn as
# spheres colored by the quadratic field there (grey: near a cell but outside its curved
# faces).
import aurafx as afx
g = afx.load(ROOT + "/testdata/cellgrid/dgHexahedra.dg")
g.set(representation="Wireframe", color_by=None, solid_color=(0.2, 0.2, 0.25))
cloud = afx.point_source(center=(1.0, 0.625, 0.5), radius=0.75, count=4000)
cloud.hide()
p = g.cell_grid_probe(cloud, attribute="quadratic")
p.set(color_by="quadratic", cmap="Viridis (matplotlib)", points_as_spheres=True, point_size=6)
g.show(); p.show(); p.activate(); p.info()
afx.view([0.5, -1.0, 0.55], up=[0, 0, 1]); afx.reset_camera()
