# GIS vectors: the test countries (a shapefile) filled and colored by population, the rivers
# (GeoJSON) as blue lines and the cities (a shapefile, flat) as points; Properties shows the reader options.
import aurafx as afx
V = ROOT + "/testdata/vectors/"
r = afx.load(V + "rivers.geojson")
r.set(color_by=None, solid_color=(0.05, 0.25, 0.95), lighting=False)
t = afx.load(V + "cities.shp")
t.set(color_by=None, representation="Points", point_size=16, points_as_spheres=True, solid_color=(0.85, 0.15, 0.1))
c = afx.load(V + "countries.shp")
c.set(polygons="Filled")
c.set(color_by="pop", cmap="Viridis", range=(10000, 2000000), log_scale=True)
c.activate()
r.info(); t.info(); c.info()
afx.view("+z"); afx.reset_camera()
