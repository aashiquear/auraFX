import aurafx as afx
p = afx.load(ROOT + "/testdata/fcc_melt.dump")
p.set(representation="Points", point_size=5, points_as_spheres=False, color_by="type")
afx.lighting(edl=True)
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT, width=1000)
