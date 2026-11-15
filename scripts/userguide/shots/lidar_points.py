import aurafx as afx
# 20 000 lidar points (LAS) colored by classification, with Eye-Dome Lighting.
p = afx.load(ROOT + "/testdata/lidar/terrain.las")
p.set(representation="Points", point_size=3, color_by="Classification", cmap="Turbo")
afx.lighting(edl=True)
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)
