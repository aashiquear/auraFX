# AMR particles: the Enzo test dump with particles (testdata/amr/enzo/DD0004) read twice, as its
# grid (box outlines) and with reader option data = Particles (spheres colored by mass).
import aurafx as afx
path = ROOT + "/testdata/amr/enzo/DD0004/data0004.hierarchy"
g = afx.load(path)
g.set(representation="Outline", color_by=None, solid_color=(0.15, 0.15, 0.2))
p = afx.load(path)
p.set(data="Particles")
p.set(color_by="particle_mass", cmap="Plasma", representation="Points", point_size=16, points_as_spheres=True)
g.show(); p.show(); p.activate(); p.info()
afx.view([0.8, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
