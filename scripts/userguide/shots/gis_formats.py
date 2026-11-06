# GIS vector formats: countries (filled, colored by area) and rivers from a GML file
# (testdata/vectors/world.gml, one layer each) and cities from a FlatGeobuf file (cities.fgb),
# drawn as spheres colored by population.
import aurafx as afx
c = afx.load(ROOT + "/testdata/vectors/world.gml")
c.set(layer="countries", polygons="Filled")
c.set(color_by="area", cmap="Viridis", scalar_bar=False)
r = afx.load(ROOT + "/testdata/vectors/world.gml")
r.set(layer="rivers")
r.set(color_by=None, solid_color=(0.1, 0.35, 0.95), line_width=3)
p = afx.load(ROOT + "/testdata/vectors/cities.fgb")
p.set(color_by="pop", cmap="Inferno", representation="Points", point_size=26, points_as_spheres=True)
c.show(); r.show(); p.show(); p.activate(); p.info()
afx.view("+z"); afx.reset_camera()
