# File geodatabase: three layers of testdata/vectors/world.gdb (lots filled and colored by
# size, roads 3 pixels wide colored by id, wells as points).
import aurafx as afx
gdb = ROOT + "/testdata/vectors/world.gdb"
lots = afx.load(gdb)
lots.set(layer="lots", polygons="Filled")
lots.set(color_by="size", cmap="Viridis (matplotlib)", lighting=False)
roads = afx.load(gdb)
roads.set(layer="roads")
roads.set(color_by="id", cmap="Cool to Warm", line_width=3, lighting=False)
wells = afx.load(gdb)
wells.set(layer="wells")
wells.set(color_by=None, solid_color=(0.9, 0.2, 0.1), point_size=10, points_as_spheres=True)
lots.show(); roads.show(); wells.show(); lots.activate()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
