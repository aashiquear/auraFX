# MapInfo: the detail layer as a MapInfo table (testdata/vectors/detail.tab), areas filled and
# colored by size (lots, some with holes; roads, a fork and wells as lines and points), and
# the same layer from the interchange file (detail.mif) as dark outlines on top.
import aurafx as afx
t = afx.load(ROOT + "/testdata/vectors/detail.tab")
t.set(polygons="Filled")
t.set(color_by="size", cmap="Viridis (matplotlib)", point_size=8, line_width=2, lighting=False)
m = afx.load(ROOT + "/testdata/vectors/detail.mif")
m.set(color_by=None, solid_color=(0.08, 0.08, 0.1), line_width=2, point_size=5, lighting=False)
t.show(); m.show(); t.activate()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
