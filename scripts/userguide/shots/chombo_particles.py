# Chombo particles: the 2-D test file with particles (testdata/amr/chombo2d_particles.hdf5) read
# as its grid (cell edges, colored by density) and with reader option data = Particles (spheres
# colored by mass).
import aurafx as afx
path = ROOT + "/testdata/amr/chombo2d_particles.hdf5"
g = afx.load(path)
g.set(color_by="density", cmap="Viridis", representation="Surface With Edges", edge_color=(0.05, 0.05, 0.08), lighting=False, scalar_bar=False)
p = afx.load(path)
p.set(data="Particles")
p.set(color_by="mass", cmap="Inferno", representation="Points", point_size=18, points_as_spheres=True)
g.show(); p.show(); p.activate(); p.info()
afx.view("+z"); afx.reset_camera()
