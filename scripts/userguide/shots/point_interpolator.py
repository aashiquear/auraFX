import aurafx as afx
# Temperature at 200 scattered points of the disk sample, carried onto a plane (Gaussian kernel).
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
pts = d.mask_points(stride=37, max_points=200)
pts.set(representation="Points", point_size=7, color_by="Temp")
plane = pts.point_interpolator(kernel="Gaussian", output="Plane", axis="X", at=0.5, nx=160, ny=160)
plane.set(color_by="Temp")
pts.show()
afx.view("+x"); afx.reset_camera()
afx.screenshot(OUT)
